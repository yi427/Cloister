//! In-memory supervision for asynchronous Profile-governed host processes.

use std::{
    collections::HashMap,
    error::Error,
    fmt, io,
    path::Path,
    process::{ExitStatus, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Child,
    sync::{Notify, OwnedSemaphorePermit, Semaphore},
    time::{sleep, timeout},
};
use tokio_util::sync::CancellationToken;

use crate::error::message;

use super::audit::{AuditExecutionMetadata, AuditLog};
use super::{HostExecAuthorizationError, HostExecPolicy, HostExecRequest, build_host_process};

const EXECUTION_ID_BYTES: usize = 16;
const INLINE_RESPONSE_WINDOW: Duration = Duration::from_millis(100);
const CANCELLATION_GRACE_PERIOD: Duration = Duration::from_secs(2);
const CANCELLATION_KILL_WAIT: Duration = Duration::from_secs(1);
const SHUTDOWN_WAIT: Duration = Duration::from_secs(4);
const MAX_CONCURRENT_EXECUTIONS: usize = 8;
const MAX_RETAINED_EXECUTIONS: usize = 128;
pub(super) const MAX_RETAINED_OUTPUT_BYTES: usize = 1024 * 1024;

/// Lifecycle state of one process registered with this bridge instance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostExecutionState {
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl HostExecutionState {
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }
}

/// Host process output stream associated with one retained chunk.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostOutputStream {
    Stdout,
    Stderr,
}

/// One ordered, cursor-addressable piece of retained host process output.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HostOutputChunk {
    pub cursor: u64,
    pub stream: HostOutputStream,
    pub text: String,
}

/// Result returned after starting a Profile-authorized host process.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HostExecOutput {
    pub execution_id: String,
    pub state: HostExecutionState,
    pub duration_ms: u64,
    pub exit_code: Option<i32>,
    pub chunks: Vec<HostOutputChunk>,
    pub next_cursor: u64,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub output_truncated: bool,
}

/// Model-supplied status request for one bridge-scoped execution.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostExecStatusRequest {
    pub execution_id: String,
    #[serde(default)]
    pub cursor: Option<u64>,
}

/// Status responses use the same complete snapshot shape as `host.exec`.
pub type HostExecStatusOutput = HostExecOutput;

/// Model-supplied cancellation request for one bridge-scoped execution.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostExecCancelRequest {
    pub execution_id: String,
}

/// Immediate response after requesting cancellation.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HostExecCancelOutput {
    pub execution_id: String,
    pub state: HostExecutionState,
}

#[derive(Debug)]
struct ExecutionRecord {
    execution_id: String,
    command_name: String,
    audit_metadata: AuditExecutionMetadata,
    started: Instant,
    cancellation: CancellationToken,
    terminal: Notify,
    inner: Mutex<ExecutionRecordState>,
}

#[derive(Debug)]
struct ExecutionRecordState {
    state: HostExecutionState,
    terminal_duration_ms: Option<u64>,
    exit_code: Option<i32>,
    chunks: Vec<HostOutputChunk>,
    next_cursor: u64,
    retained_output_bytes: usize,
    stdout_bytes: u64,
    stderr_bytes: u64,
    output_truncated: bool,
}

impl ExecutionRecord {
    fn new(
        execution_id: String,
        command_name: String,
        audit_metadata: AuditExecutionMetadata,
    ) -> Self {
        Self {
            execution_id,
            command_name,
            audit_metadata,
            started: Instant::now(),
            cancellation: CancellationToken::new(),
            terminal: Notify::new(),
            inner: Mutex::new(ExecutionRecordState {
                state: HostExecutionState::Running,
                terminal_duration_ms: None,
                exit_code: None,
                chunks: Vec::new(),
                next_cursor: 0,
                retained_output_bytes: 0,
                stdout_bytes: 0,
                stderr_bytes: 0,
                output_truncated: false,
            }),
        }
    }

    fn state(&self) -> HostExecutionState {
        self.inner.lock().expect("execution record poisoned").state
    }

    fn snapshot(&self, cursor: u64) -> HostExecOutput {
        let inner = self.inner.lock().expect("execution record poisoned");
        HostExecOutput {
            execution_id: self.execution_id.clone(),
            state: inner.state,
            duration_ms: inner
                .terminal_duration_ms
                .unwrap_or_else(|| elapsed_ms(self.started)),
            exit_code: inner.exit_code,
            chunks: inner
                .chunks
                .iter()
                .filter(|chunk| chunk.cursor > cursor)
                .cloned()
                .collect(),
            next_cursor: inner.next_cursor,
            stdout_bytes: inner.stdout_bytes,
            stderr_bytes: inner.stderr_bytes,
            output_truncated: inner.output_truncated,
        }
    }

    fn append_output(&self, stream: HostOutputStream, bytes: &[u8]) {
        let mut inner = self.inner.lock().expect("execution record poisoned");
        let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        match stream {
            HostOutputStream::Stdout => {
                inner.stdout_bytes = inner.stdout_bytes.saturating_add(byte_count);
            }
            HostOutputStream::Stderr => {
                inner.stderr_bytes = inner.stderr_bytes.saturating_add(byte_count);
            }
        }

        let available = MAX_RETAINED_OUTPUT_BYTES.saturating_sub(inner.retained_output_bytes);
        let retained = available.min(bytes.len());
        if retained > 0 {
            inner.next_cursor = inner.next_cursor.saturating_add(1);
            let cursor = inner.next_cursor;
            inner.chunks.push(HostOutputChunk {
                cursor,
                stream,
                text: String::from_utf8_lossy(&bytes[..retained]).into_owned(),
            });
            inner.retained_output_bytes += retained;
        }
        if retained < bytes.len() {
            inner.output_truncated = true;
        }
    }

    fn finish(&self, state: HostExecutionState, exit_code: Option<i32>, duration_ms: u64) {
        {
            let mut inner = self.inner.lock().expect("execution record poisoned");
            inner.state = state;
            inner.terminal_duration_ms = Some(duration_ms);
            inner.exit_code = exit_code;
        }
        self.terminal.notify_waiters();
    }

    async fn wait_until_terminal(&self, wait: Duration) {
        let notified = self.terminal.notified();
        if self.state().is_terminal() {
            return;
        }
        let _ = timeout(wait, notified).await;
    }
}

/// One bridge-scoped registry and supervisor for all MCP service instances.
#[derive(Debug)]
pub(super) struct ExecutionManager {
    records: Mutex<HashMap<String, Arc<ExecutionRecord>>>,
    permits: Arc<Semaphore>,
    audit: AuditLog,
}

impl ExecutionManager {
    pub(super) fn new(audit: AuditLog) -> Arc<Self> {
        Arc::new(Self {
            records: Mutex::new(HashMap::new()),
            permits: Arc::new(Semaphore::new(MAX_CONCURRENT_EXECUTIONS)),
            audit,
        })
    }

    pub(super) async fn start(
        self: &Arc<Self>,
        policy: &HostExecPolicy,
        request: &HostExecRequest,
        working_directory: &Path,
    ) -> Result<HostExecOutput, ExecutionError> {
        let request_id = generate_identifier("req_")?;
        let authorized = match policy.authorize(request) {
            Ok(authorized) => authorized,
            Err(source) => {
                self.audit
                    .denied(
                        request_id,
                        request,
                        match &source {
                            HostExecAuthorizationError::UnsupportedVersion { .. } => {
                                "unsupported_version"
                            }
                            HostExecAuthorizationError::CommandNotAllowed { .. } => {
                                "command_not_allowed"
                            }
                        },
                    )
                    .await
                    .map_err(ExecutionError::Audit)?;
                return Err(ExecutionError::Authorization(source));
            }
        };
        let permit = match Arc::clone(&self.permits).try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                self.audit
                    .failed(request_id, None, request, Some(&authorized), "capacity")
                    .await
                    .map_err(ExecutionError::Audit)?;
                return Err(ExecutionError::Capacity {
                    limit: MAX_CONCURRENT_EXECUTIONS,
                });
            }
        };
        let execution_id = match generate_identifier("exec_") {
            Ok(execution_id) => execution_id,
            Err(error) => {
                self.audit
                    .failed(request_id, None, request, Some(&authorized), "execution_id")
                    .await
                    .map_err(ExecutionError::Audit)?;
                return Err(error);
            }
        };
        let audit_metadata = self
            .audit
            .started(
                request_id.clone(),
                execution_id.clone(),
                request,
                &authorized,
            )
            .await
            .map_err(ExecutionError::Audit)?;
        let command_name = authorized.command_name().to_owned();
        let mut process = build_host_process(&authorized);
        process
            .current_dir(working_directory)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .process_group(0);
        let mut child = match process.spawn() {
            Ok(child) => child,
            Err(source) => {
                self.audit
                    .failed(
                        request_id,
                        Some(execution_id),
                        request,
                        Some(&authorized),
                        "spawn",
                    )
                    .await
                    .map_err(ExecutionError::Audit)?;
                return Err(ExecutionError::Spawn {
                    command: command_name,
                    source,
                });
            }
        };
        let process_id = match child.id() {
            Some(process_id) => process_id,
            None => {
                self.audit
                    .failed(
                        request_id,
                        Some(execution_id),
                        request,
                        Some(&authorized),
                        "missing_process_id",
                    )
                    .await
                    .map_err(ExecutionError::Audit)?;
                return Err(ExecutionError::MissingProcessId {
                    command: command_name,
                });
            }
        };
        let stdout = child
            .stdout
            .take()
            .expect("piped host stdout should be available");
        let stderr = child
            .stderr
            .take()
            .expect("piped host stderr should be available");
        let record = Arc::new(ExecutionRecord::new(
            execution_id.clone(),
            command_name,
            audit_metadata,
        ));
        self.register(Arc::clone(&record));

        tokio::spawn(supervise(
            child,
            process_id,
            stdout,
            stderr,
            Arc::clone(&record),
            permit,
            self.audit.clone(),
        ));

        record.wait_until_terminal(INLINE_RESPONSE_WINDOW).await;
        Ok(record.snapshot(0))
    }

    pub(super) fn status(
        &self,
        request: &HostExecStatusRequest,
    ) -> Result<HostExecStatusOutput, ExecutionError> {
        let record = self.find(&request.execution_id)?;
        Ok(record.snapshot(request.cursor.unwrap_or(0)))
    }

    pub(super) fn cancel(
        &self,
        request: &HostExecCancelRequest,
    ) -> Result<HostExecCancelOutput, ExecutionError> {
        let record = self.find(&request.execution_id)?;
        let state = record.state();
        if !state.is_terminal() {
            record.cancellation.cancel();
        }
        Ok(HostExecCancelOutput {
            execution_id: request.execution_id.clone(),
            state,
        })
    }

    pub(super) async fn shutdown(&self) {
        let records = self
            .records
            .lock()
            .expect("execution registry poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for record in &records {
            if !record.state().is_terminal() {
                record.cancellation.cancel();
            }
        }
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        for record in records {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            record.wait_until_terminal(remaining).await;
        }
    }

    fn register(&self, record: Arc<ExecutionRecord>) {
        let mut records = self.records.lock().expect("execution registry poisoned");
        if records.len() >= MAX_RETAINED_EXECUTIONS {
            let mut terminal = records
                .values()
                .filter(|record| record.state().is_terminal())
                .map(|record| (record.execution_id.clone(), record.started))
                .collect::<Vec<_>>();
            terminal.sort_by_key(|(_, started)| *started);
            let remove_count = records
                .len()
                .saturating_add(1)
                .saturating_sub(MAX_RETAINED_EXECUTIONS);
            for (execution_id, _) in terminal.into_iter().take(remove_count) {
                records.remove(&execution_id);
            }
        }
        records.insert(record.execution_id.clone(), record);
    }

    fn find(&self, execution_id: &str) -> Result<Arc<ExecutionRecord>, ExecutionError> {
        self.records
            .lock()
            .expect("execution registry poisoned")
            .get(execution_id)
            .cloned()
            .ok_or_else(|| ExecutionError::UnknownExecution {
                execution_id: execution_id.to_owned(),
            })
    }
}

async fn supervise(
    mut child: Child,
    process_id: u32,
    stdout: impl AsyncRead + Unpin + Send + 'static,
    stderr: impl AsyncRead + Unpin + Send + 'static,
    record: Arc<ExecutionRecord>,
    _permit: OwnedSemaphorePermit,
    audit: AuditLog,
) {
    let stdout_task = tokio::spawn(read_output(
        stdout,
        Arc::clone(&record),
        HostOutputStream::Stdout,
    ));
    let stderr_task = tokio::spawn(read_output(
        stderr,
        Arc::clone(&record),
        HostOutputStream::Stderr,
    ));

    let mut cancelled = false;
    let status = tokio::select! {
        result = child.wait() => result,
        () = record.cancellation.cancelled() => {
            cancelled = true;
            terminate_process_group(&mut child, process_id).await
        }
    };
    let stdout_result = stdout_task.await;
    let stderr_result = stderr_task.await;
    let output_failed =
        !matches!(stdout_result, Ok(Ok(()))) || !matches!(stderr_result, Ok(Ok(())));
    let (state, exit_code) = match status {
        Ok(status) if cancelled => (HostExecutionState::Cancelled, status.code()),
        Ok(status) if output_failed => (HostExecutionState::Failed, status.code()),
        Ok(status) => (HostExecutionState::Completed, status.code()),
        Err(_) if cancelled => (HostExecutionState::Cancelled, None),
        Err(_) => (HostExecutionState::Failed, None),
    };
    let mut snapshot = record.snapshot(u64::MAX);
    snapshot.state = state;
    snapshot.exit_code = exit_code;
    if let Err(error) = audit.finished(&record.audit_metadata, &snapshot).await {
        eprintln!(
            "Host Exec audit failure execution_id={:?} command={:?}: {error}",
            record.execution_id, record.command_name
        );
        snapshot.state = HostExecutionState::Failed;
    }
    record.finish(snapshot.state, exit_code, snapshot.duration_ms);
    eprintln!(
        "audit capability=host.exec execution_id={:?} command={:?} outcome={:?} exit_code={:?} duration_ms={} stdout_bytes={} stderr_bytes={} output_truncated={}",
        record.execution_id,
        record.command_name,
        snapshot.state,
        exit_code,
        snapshot.duration_ms,
        snapshot.stdout_bytes,
        snapshot.stderr_bytes,
        snapshot.output_truncated,
    );
}

async fn read_output(
    mut reader: impl AsyncRead + Unpin,
    record: Arc<ExecutionRecord>,
    stream: HostOutputStream,
) -> io::Result<()> {
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            return Ok(());
        }
        record.append_output(stream, &buffer[..read]);
    }
}

async fn terminate_process_group(child: &mut Child, process_id: u32) -> io::Result<ExitStatus> {
    let term_result = signal_process_group(process_id, libc::SIGTERM);
    if let Err(error) = term_result {
        let _ = signal_process_group(process_id, libc::SIGKILL);
        let _ = child.wait().await;
        return Err(error);
    }

    let deadline = Instant::now() + CANCELLATION_GRACE_PERIOD;
    let mut leader_status = None;
    loop {
        if leader_status.is_none() {
            leader_status = child.try_wait()?;
        }
        if !process_group_exists(process_id)? {
            return match leader_status {
                Some(status) => Ok(status),
                None => child.wait().await,
            };
        }
        if Instant::now() >= deadline {
            signal_process_group(process_id, libc::SIGKILL)?;
            let kill_deadline = Instant::now() + CANCELLATION_KILL_WAIT;
            while process_group_exists(process_id)? {
                if Instant::now() >= kill_deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Host Exec process group survived SIGKILL",
                    ));
                }
                sleep(Duration::from_millis(10)).await;
            }
            return match leader_status {
                Some(status) => Ok(status),
                None => child.wait().await,
            };
        }
        sleep(Duration::from_millis(25)).await;
    }
}

fn process_group_exists(process_id: u32) -> io::Result<bool> {
    let process_group = i32::try_from(process_id)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "process ID exceeds i32"))?;
    // SAFETY: signal 0 performs an existence/permission check for the process
    // group and does not deliver a signal or access borrowed memory.
    let result = unsafe { libc::kill(-process_group, 0) };
    if result == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(false)
    } else if error.raw_os_error() == Some(libc::EPERM) {
        Ok(true)
    } else {
        Err(error)
    }
}

fn signal_process_group(process_id: u32, signal: i32) -> io::Result<()> {
    let process_group = i32::try_from(process_id)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "process ID exceeds i32"))?;
    // SAFETY: `kill` is called with a valid negative process-group identifier and
    // a platform signal constant. No pointer or borrowed memory crosses the FFI boundary.
    let result = unsafe { libc::kill(-process_group, signal) };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

fn generate_identifier(prefix: &str) -> Result<String, ExecutionError> {
    let mut bytes = [0_u8; EXECUTION_ID_BYTES];
    getrandom::fill(&mut bytes).map_err(|source| ExecutionError::Random {
        detail: source.to_string(),
    })?;
    Ok(format!("{prefix}{}", URL_SAFE_NO_PAD.encode(bytes)))
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

#[derive(Debug)]
pub(super) enum ExecutionError {
    Audit(super::AuditLogError),
    Authorization(HostExecAuthorizationError),
    Capacity { limit: usize },
    MissingProcessId { command: String },
    Random { detail: String },
    Spawn { command: String, source: io::Error },
    UnknownExecution { execution_id: String },
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Audit(source) => source.fmt(formatter),
            Self::Authorization(source) => source.fmt(formatter),
            Self::Capacity { limit } => write!(
                formatter,
                "Host Exec concurrency limit reached ({limit} running executions)"
            ),
            Self::MissingProcessId { command } => {
                write!(
                    formatter,
                    "Host Exec did not receive a process ID for {command:?}"
                )
            }
            Self::Random { detail } => {
                write!(
                    formatter,
                    "failed to generate Host Exec execution ID: {detail}"
                )
            }
            Self::Spawn { command, source } => write!(
                formatter,
                "{} {command:?}: {source}",
                message::HOST_EXEC_SPAWN_FAILED
            ),
            Self::UnknownExecution { execution_id } => {
                write!(
                    formatter,
                    "unknown Host Exec execution ID: {execution_id:?}"
                )
            }
        }
    }
}

impl Error for ExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Audit(source) => Some(source),
            Self::Authorization(source) => Some(source),
            Self::Spawn { source, .. } => Some(source),
            Self::Capacity { .. }
            | Self::MissingProcessId { .. }
            | Self::Random { .. }
            | Self::UnknownExecution { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::{
        ExecutionManager, ExecutionRecord, HostExecutionState, HostOutputStream,
        MAX_RETAINED_EXECUTIONS, MAX_RETAINED_OUTPUT_BYTES,
    };
    use crate::host_bridge::audit::{AuditController, AuditExecutionMetadata};

    #[test]
    fn retains_bounded_output_while_counting_all_raw_bytes() {
        let record = ExecutionRecord::new(
            "exec_output".to_owned(),
            "test".to_owned(),
            AuditExecutionMetadata::test("exec_output"),
        );
        let output = vec![b'x'; MAX_RETAINED_OUTPUT_BYTES + 17];

        record.append_output(HostOutputStream::Stdout, &output);
        let snapshot = record.snapshot(0);

        assert_eq!(snapshot.stdout_bytes, output.len() as u64);
        assert_eq!(snapshot.stderr_bytes, 0);
        assert!(snapshot.output_truncated);
        assert_eq!(snapshot.chunks.len(), 1);
        assert_eq!(snapshot.chunks[0].text.len(), MAX_RETAINED_OUTPUT_BYTES);
    }

    #[tokio::test]
    async fn evicts_the_oldest_terminal_record_at_the_registry_limit() {
        let directory = tempdir().expect("temporary directory should exist");
        let audit = AuditController::start(
            directory
                .path()
                .join("state/cloister/audit/host-exec.jsonl"),
            "test".to_owned(),
            "test-agent".to_owned(),
            directory.path().to_owned(),
        )
        .expect("test audit should start");
        let manager = ExecutionManager::new(audit.log());
        for index in 0..=MAX_RETAINED_EXECUTIONS {
            let record = Arc::new(ExecutionRecord::new(
                format!("exec_{index}"),
                "test".to_owned(),
                AuditExecutionMetadata::test(&format!("exec_{index}")),
            ));
            record.finish(HostExecutionState::Completed, Some(0), 0);
            manager.register(record);
        }

        {
            let records = manager
                .records
                .lock()
                .expect("registry should not be poisoned");
            assert_eq!(records.len(), MAX_RETAINED_EXECUTIONS);
            assert!(!records.contains_key("exec_0"));
            assert!(records.contains_key(&format!("exec_{MAX_RETAINED_EXECUTIONS}")));
        }
        manager.shutdown().await;
        audit.shutdown().await.expect("test audit should stop");
    }

    #[test]
    fn status_and_cancel_requests_reject_extra_model_fields() {
        let status = serde_json::from_value::<super::HostExecStatusRequest>(serde_json::json!({
            "execution_id": "exec_test",
            "cursor": 0,
            "command": "not-allowed"
        }));
        let cancel = serde_json::from_value::<super::HostExecCancelRequest>(serde_json::json!({
            "execution_id": "exec_test",
            "signal": "KILL"
        }));

        assert!(status.is_err());
        assert!(cancel.is_err());
    }
}
