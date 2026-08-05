//! Owner-only, size-bounded JSONL audit storage for Host Exec metadata.

use std::{
    env,
    error::Error,
    ffi::OsStr,
    fmt, fs, io,
    io::Write,
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tokio::sync::oneshot;

use super::{
    AuthorizedHostCommand, HostExecOutput, HostExecRequest, HostExecutionState,
    execution::MAX_RETAINED_OUTPUT_BYTES,
};

pub const AUDIT_SCHEMA_VERSION: u32 = 1;
pub const AUDIT_SEGMENT_BYTES: u64 = 10 * 1024 * 1024;
pub const AUDIT_TOTAL_BYTES: u64 = 2 * AUDIT_SEGMENT_BYTES;

const AUDIT_RELATIVE_PATH: &str = "cloister/audit/host-exec.jsonl";
const AUDIT_DIRECTORY_MODE: u32 = 0o700;
const AUDIT_FILE_MODE: u32 = 0o600;

/// Resolves the fixed Host Exec audit path without creating filesystem state.
pub fn default_audit_log_path() -> Result<PathBuf, AuditLogPathError> {
    resolve_audit_log_path(env::var_os("XDG_STATE_HOME"), env::var_os("HOME"))
}

fn resolve_audit_log_path(
    xdg_state_home: Option<impl Into<PathBuf>>,
    home: Option<impl Into<PathBuf>>,
) -> Result<PathBuf, AuditLogPathError> {
    let base = xdg_state_home
        .map(Into::into)
        .filter(|path: &PathBuf| !path.as_os_str().is_empty())
        .or_else(|| {
            home.map(Into::into)
                .filter(|path: &PathBuf| !path.as_os_str().is_empty())
                .map(|home| home.join(".local/state"))
        })
        .ok_or(AuditLogPathError::HomeDirectoryMissing)?;
    if !base.is_absolute() {
        return Err(AuditLogPathError::RelativeStateDirectory { path: base });
    }
    Ok(base.join(AUDIT_RELATIVE_PATH))
}

/// Read-only inspection of the configured audit destination.
pub fn inspect_audit_log_path(path: &Path) -> Result<AuditLogInspection, AuditLogError> {
    let audit_directory = audit_directory(path)?;
    let cloister_directory = audit_directory
        .parent()
        .expect("audit directory should have a Cloister parent");

    if !path_exists(cloister_directory)? {
        return Ok(AuditLogInspection::NotCreated);
    }
    validate_directory(cloister_directory)?;
    if !path_exists(audit_directory)? {
        return Ok(AuditLogInspection::NotCreated);
    }
    validate_directory(audit_directory)?;

    for candidate in [path.to_owned(), rotated_path(path), lock_path(path)] {
        if path_exists(&candidate)? {
            validate_file_path(&candidate)?;
        }
    }

    Ok(AuditLogInspection::Ready)
}

/// Result of inspecting an audit destination without creating it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditLogInspection {
    NotCreated,
    Ready,
}

#[derive(Clone, Debug)]
pub(super) struct AuditLog {
    sender: mpsc::Sender<WriterMessage>,
    context: Arc<AuditContext>,
}

#[derive(Debug)]
pub(super) struct AuditController {
    log: AuditLog,
    worker: Option<thread::JoinHandle<()>>,
}

impl AuditController {
    pub(super) fn start(
        path: PathBuf,
        profile_name: String,
        agent_name: String,
        workspace: PathBuf,
    ) -> Result<Self, AuditLogError> {
        prepare_destination(&path)?;
        let (sender, receiver) = mpsc::channel();
        let worker_path = path.clone();
        let worker = thread::Builder::new()
            .name("cloister-audit".to_owned())
            .spawn(move || writer_loop(&worker_path, receiver))
            .map_err(|source| AuditLogError::StartWorker { source })?;
        Ok(Self {
            log: AuditLog {
                sender,
                context: Arc::new(AuditContext {
                    profile_name,
                    agent_name,
                    workspace,
                }),
            },
            worker: Some(worker),
        })
    }

    pub(super) fn log(&self) -> AuditLog {
        self.log.clone()
    }

    pub(super) async fn shutdown(mut self) -> Result<(), AuditLogError> {
        let (completion, completed) = oneshot::channel();
        self.log
            .sender
            .send(WriterMessage::Shutdown { completion })
            .map_err(|_| AuditLogError::WorkerUnavailable)?;
        completed
            .await
            .map_err(|_| AuditLogError::WorkerUnavailable)?;
        let worker = self.worker.take().expect("audit worker should exist");
        tokio::task::spawn_blocking(move || worker.join())
            .await
            .map_err(|source| AuditLogError::JoinWorker {
                detail: source.to_string(),
            })?
            .map_err(|_| AuditLogError::WorkerPanicked)
    }
}

impl AuditLog {
    pub(super) async fn denied(
        &self,
        request_id: String,
        request: &HostExecRequest,
        failure_kind: &'static str,
    ) -> Result<(), AuditLogError> {
        self.record(AuditEvent::attempt(
            &self.context,
            "execution_denied",
            request_id,
            None,
            request,
            None,
            "denied",
            Some(failure_kind),
        ))
        .await
    }

    pub(super) async fn failed(
        &self,
        request_id: String,
        execution_id: Option<String>,
        request: &HostExecRequest,
        command: Option<&AuthorizedHostCommand>,
        failure_kind: &'static str,
    ) -> Result<(), AuditLogError> {
        self.record(AuditEvent::attempt(
            &self.context,
            "execution_failed",
            request_id,
            execution_id,
            request,
            command,
            "failed",
            Some(failure_kind),
        ))
        .await
    }

    pub(super) async fn started(
        &self,
        request_id: String,
        execution_id: String,
        request: &HostExecRequest,
        command: &AuthorizedHostCommand,
    ) -> Result<AuditExecutionMetadata, AuditLogError> {
        let metadata = AuditExecutionMetadata::new(request_id, execution_id, request, command);
        self.record(AuditEvent::started(&self.context, &metadata))
            .await?;
        Ok(metadata)
    }

    pub(super) async fn finished(
        &self,
        metadata: &AuditExecutionMetadata,
        output: &HostExecOutput,
    ) -> Result<(), AuditLogError> {
        self.record(AuditEvent::finished(&self.context, metadata, output))
            .await
    }

    async fn record(&self, event: AuditEvent) -> Result<(), AuditLogError> {
        let (completion, completed) = oneshot::channel();
        self.sender
            .send(WriterMessage::Record {
                event: Box::new(event),
                completion,
            })
            .map_err(|_| AuditLogError::WorkerUnavailable)?;
        completed
            .await
            .map_err(|_| AuditLogError::WorkerUnavailable)?
    }
}

#[derive(Clone, Debug)]
struct AuditContext {
    profile_name: String,
    agent_name: String,
    workspace: PathBuf,
}

#[derive(Clone, Debug)]
pub(super) struct AuditExecutionMetadata {
    request_id: String,
    execution_id: String,
    command: String,
    request_version: u32,
    declared_executable: PathBuf,
    resolved_executable: PathBuf,
    argument_count: usize,
    environment_variable_names: Vec<String>,
}

impl AuditExecutionMetadata {
    fn new(
        request_id: String,
        execution_id: String,
        request: &HostExecRequest,
        command: &AuthorizedHostCommand,
    ) -> Self {
        Self {
            request_id,
            execution_id,
            command: command.command_name().to_owned(),
            request_version: request.version,
            declared_executable: command.executable().to_owned(),
            resolved_executable: fs::canonicalize(command.executable())
                .unwrap_or_else(|_| command.executable().to_owned()),
            argument_count: request.args.len(),
            environment_variable_names: command
                .environment()
                .keys()
                .map(|name| name.to_string_lossy().into_owned())
                .collect(),
        }
    }

    #[cfg(test)]
    pub(super) fn test(execution_id: &str) -> Self {
        Self {
            request_id: "req_test".to_owned(),
            execution_id: execution_id.to_owned(),
            command: "test".to_owned(),
            request_version: crate::host_bridge::HOST_EXEC_DSL_VERSION,
            declared_executable: "/usr/bin/true".into(),
            resolved_executable: "/usr/bin/true".into(),
            argument_count: 0,
            environment_variable_names: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize)]
struct AuditEvent {
    audit_schema_version: u32,
    timestamp_unix_ms: u64,
    event: &'static str,
    request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    execution_id: Option<String>,
    profile: String,
    agent: String,
    workspace: String,
    command: String,
    request_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    declared_executable: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_executable: Option<String>,
    argument_count: usize,
    argument_values_redacted: bool,
    environment_mode: &'static str,
    environment_variable_names: Vec<String>,
    outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<HostExecutionState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    duration_ms: u64,
    stdout_bytes: u64,
    stderr_bytes: u64,
    output_truncated: bool,
    retained_output_limit_bytes: u64,
}

impl AuditEvent {
    #[allow(clippy::too_many_arguments)]
    fn attempt(
        context: &AuditContext,
        event: &'static str,
        request_id: String,
        execution_id: Option<String>,
        request: &HostExecRequest,
        command: Option<&AuthorizedHostCommand>,
        outcome: &'static str,
        failure_kind: Option<&'static str>,
    ) -> Self {
        Self {
            audit_schema_version: AUDIT_SCHEMA_VERSION,
            timestamp_unix_ms: timestamp_unix_ms(),
            event,
            request_id,
            execution_id,
            profile: context.profile_name.clone(),
            agent: context.agent_name.clone(),
            workspace: context.workspace.to_string_lossy().into_owned(),
            command: request.command.clone(),
            request_version: request.version,
            declared_executable: command
                .map(AuthorizedHostCommand::executable)
                .map(path_text),
            resolved_executable: command.map(AuthorizedHostCommand::executable).map(|path| {
                fs::canonicalize(path)
                    .map(|resolved| path_text(&resolved))
                    .unwrap_or_else(|_| path_text(path))
            }),
            argument_count: request.args.len(),
            argument_values_redacted: true,
            environment_mode: "inherit-all",
            environment_variable_names: command
                .map(|command| {
                    command
                        .environment()
                        .keys()
                        .map(|name| name.to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default(),
            outcome,
            failure_kind,
            state: None,
            exit_code: None,
            duration_ms: 0,
            stdout_bytes: 0,
            stderr_bytes: 0,
            output_truncated: false,
            retained_output_limit_bytes: MAX_RETAINED_OUTPUT_BYTES as u64,
        }
    }

    fn started(context: &AuditContext, metadata: &AuditExecutionMetadata) -> Self {
        Self {
            audit_schema_version: AUDIT_SCHEMA_VERSION,
            timestamp_unix_ms: timestamp_unix_ms(),
            event: "execution_started",
            request_id: metadata.request_id.clone(),
            execution_id: Some(metadata.execution_id.clone()),
            profile: context.profile_name.clone(),
            agent: context.agent_name.clone(),
            workspace: path_text(&context.workspace),
            command: metadata.command.clone(),
            request_version: metadata.request_version,
            declared_executable: Some(path_text(&metadata.declared_executable)),
            resolved_executable: Some(path_text(&metadata.resolved_executable)),
            argument_count: metadata.argument_count,
            argument_values_redacted: true,
            environment_mode: "inherit-all",
            environment_variable_names: metadata.environment_variable_names.clone(),
            outcome: "started",
            failure_kind: None,
            state: Some(HostExecutionState::Running),
            exit_code: None,
            duration_ms: 0,
            stdout_bytes: 0,
            stderr_bytes: 0,
            output_truncated: false,
            retained_output_limit_bytes: MAX_RETAINED_OUTPUT_BYTES as u64,
        }
    }

    fn finished(
        context: &AuditContext,
        metadata: &AuditExecutionMetadata,
        output: &HostExecOutput,
    ) -> Self {
        Self {
            audit_schema_version: AUDIT_SCHEMA_VERSION,
            timestamp_unix_ms: timestamp_unix_ms(),
            event: "execution_finished",
            request_id: metadata.request_id.clone(),
            execution_id: Some(metadata.execution_id.clone()),
            profile: context.profile_name.clone(),
            agent: context.agent_name.clone(),
            workspace: path_text(&context.workspace),
            command: metadata.command.clone(),
            request_version: metadata.request_version,
            declared_executable: Some(path_text(&metadata.declared_executable)),
            resolved_executable: Some(path_text(&metadata.resolved_executable)),
            argument_count: metadata.argument_count,
            argument_values_redacted: true,
            environment_mode: "inherit-all",
            environment_variable_names: metadata.environment_variable_names.clone(),
            outcome: match output.state {
                HostExecutionState::Running => "running",
                HostExecutionState::Completed => "completed",
                HostExecutionState::Failed => "failed",
                HostExecutionState::Cancelled => "cancelled",
            },
            failure_kind: None,
            state: Some(output.state),
            exit_code: output.exit_code,
            duration_ms: output.duration_ms,
            stdout_bytes: output.stdout_bytes,
            stderr_bytes: output.stderr_bytes,
            output_truncated: output.output_truncated,
            retained_output_limit_bytes: MAX_RETAINED_OUTPUT_BYTES as u64,
        }
    }
}

enum WriterMessage {
    Record {
        event: Box<AuditEvent>,
        completion: oneshot::Sender<Result<(), AuditLogError>>,
    },
    Shutdown {
        completion: oneshot::Sender<()>,
    },
}

fn writer_loop(path: &Path, receiver: mpsc::Receiver<WriterMessage>) {
    for message in receiver {
        match message {
            WriterMessage::Record { event, completion } => {
                let _ = completion.send(write_event(path, &event, AUDIT_SEGMENT_BYTES));
            }
            WriterMessage::Shutdown { completion } => {
                let _ = completion.send(());
                return;
            }
        }
    }
}

fn write_event(path: &Path, event: &AuditEvent, segment_bytes: u64) -> Result<(), AuditLogError> {
    let mut line = serde_json::to_vec(event).map_err(AuditLogError::Serialize)?;
    line.push(b'\n');
    if u64::try_from(line.len()).unwrap_or(u64::MAX) > segment_bytes {
        return Err(AuditLogError::EventTooLarge {
            bytes: line.len(),
            limit: segment_bytes,
        });
    }

    let lock_file = open_secure_file(&lock_path(path), true)?;
    let _lock = FileLock::exclusive(lock_file, path)?;
    let mut active = open_secure_file(path, true)?;
    let current_bytes = active
        .metadata()
        .map_err(|source| AuditLogError::Inspect {
            path: path.to_owned(),
            source,
        })?
        .len();
    if current_bytes.saturating_add(line.len() as u64) > segment_bytes {
        drop(active);
        rotate(path)?;
        active = open_secure_file(path, true)?;
    }
    active
        .write_all(&line)
        .and_then(|()| active.sync_data())
        .map_err(|source| AuditLogError::Write {
            path: path.to_owned(),
            source,
        })
}

fn prepare_destination(path: &Path) -> Result<(), AuditLogError> {
    let audit_directory = audit_directory(path)?;
    let cloister_directory = audit_directory
        .parent()
        .expect("audit directory should have a Cloister parent");
    prepare_directory(cloister_directory)?;
    prepare_directory(audit_directory)?;
    let lock_file = open_secure_file(&lock_path(path), true)?;
    let _lock = FileLock::exclusive(lock_file, path)?;
    let _ = open_secure_file(path, true)?;
    if path_exists(&rotated_path(path))? {
        validate_file_path(&rotated_path(path))?;
    }
    Ok(())
}

fn prepare_directory(path: &Path) -> Result<(), AuditLogError> {
    match fs::symlink_metadata(path) {
        Ok(_) => validate_directory(path),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true).mode(AUDIT_DIRECTORY_MODE);
            builder
                .create(path)
                .map_err(|source| AuditLogError::CreateDirectory {
                    path: path.to_owned(),
                    source,
                })?;
            fs::set_permissions(path, fs::Permissions::from_mode(AUDIT_DIRECTORY_MODE)).map_err(
                |source| AuditLogError::SetPermissions {
                    path: path.to_owned(),
                    source,
                },
            )?;
            validate_directory(path)
        }
        Err(source) => Err(AuditLogError::Inspect {
            path: path.to_owned(),
            source,
        }),
    }
}

fn validate_directory(path: &Path) -> Result<(), AuditLogError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| AuditLogError::Inspect {
        path: path.to_owned(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(AuditLogError::UnsafeType {
            path: path.to_owned(),
            expected: "directory",
        });
    }
    validate_owner_and_mode(path, &metadata, AUDIT_DIRECTORY_MODE)
}

fn validate_file_path(path: &Path) -> Result<(), AuditLogError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| AuditLogError::Inspect {
        path: path.to_owned(),
        source,
    })?;
    validate_file_metadata(path, &metadata)
}

fn validate_file_metadata(path: &Path, metadata: &fs::Metadata) -> Result<(), AuditLogError> {
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AuditLogError::UnsafeType {
            path: path.to_owned(),
            expected: "regular file",
        });
    }
    if metadata.nlink() != 1 {
        return Err(AuditLogError::HardLinked {
            path: path.to_owned(),
            links: metadata.nlink(),
        });
    }
    if metadata.len() > AUDIT_SEGMENT_BYTES {
        return Err(AuditLogError::FileTooLarge {
            path: path.to_owned(),
            bytes: metadata.len(),
            limit: AUDIT_SEGMENT_BYTES,
        });
    }
    validate_owner_and_mode(path, metadata, AUDIT_FILE_MODE)
}

fn validate_owner_and_mode(
    path: &Path,
    metadata: &fs::Metadata,
    expected_mode: u32,
) -> Result<(), AuditLogError> {
    // SAFETY: `geteuid` takes no arguments and returns the current effective UID.
    let expected_uid = unsafe { libc::geteuid() };
    if metadata.uid() != expected_uid {
        return Err(AuditLogError::WrongOwner {
            path: path.to_owned(),
            expected: expected_uid,
            found: metadata.uid(),
        });
    }
    let mode = metadata.permissions().mode() & 0o777;
    if mode != expected_mode {
        return Err(AuditLogError::WrongMode {
            path: path.to_owned(),
            expected: expected_mode,
            found: mode,
        });
    }
    Ok(())
}

fn open_secure_file(path: &Path, create: bool) -> Result<fs::File, AuditLogError> {
    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .append(true)
        .create(create)
        .mode(AUDIT_FILE_MODE)
        .custom_flags(libc::O_NOFOLLOW);
    let file = options.open(path).map_err(|source| AuditLogError::Open {
        path: path.to_owned(),
        source,
    })?;
    let metadata = file.metadata().map_err(|source| AuditLogError::Inspect {
        path: path.to_owned(),
        source,
    })?;
    validate_file_metadata(path, &metadata)?;
    Ok(file)
}

fn rotate(path: &Path) -> Result<(), AuditLogError> {
    let rotated = rotated_path(path);
    if path_exists(&rotated)? {
        validate_file_path(&rotated)?;
        fs::remove_file(&rotated).map_err(|source| AuditLogError::Rotate {
            path: rotated.clone(),
            source,
        })?;
    }
    validate_file_path(path)?;
    fs::rename(path, &rotated).map_err(|source| AuditLogError::Rotate {
        path: path.to_owned(),
        source,
    })?;
    Ok(())
}

fn path_exists(path: &Path) -> Result<bool, AuditLogError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(AuditLogError::Inspect {
            path: path.to_owned(),
            source,
        }),
    }
}

fn audit_directory(path: &Path) -> Result<&Path, AuditLogError> {
    path.parent().ok_or_else(|| AuditLogError::InvalidPath {
        path: path.to_owned(),
    })
}

fn rotated_path(path: &Path) -> PathBuf {
    append_suffix(path, ".1")
}

fn lock_path(path: &Path) -> PathBuf {
    append_suffix(path, ".lock")
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .unwrap_or_else(|| OsStr::new("host-exec.jsonl"))
        .to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

fn timestamp_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

struct FileLock {
    file: fs::File,
}

impl FileLock {
    fn exclusive(file: fs::File, audit_path: &Path) -> Result<Self, AuditLogError> {
        // SAFETY: `flock` receives a live file descriptor and a platform lock flag.
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if result != 0 {
            return Err(AuditLogError::Lock {
                path: audit_path.to_owned(),
                source: io::Error::last_os_error(),
            });
        }
        Ok(Self { file })
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // SAFETY: `flock` receives the still-live descriptor owned by this guard.
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

#[derive(Debug)]
pub enum AuditLogPathError {
    HomeDirectoryMissing,
    RelativeStateDirectory { path: PathBuf },
}

impl fmt::Display for AuditLogPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HomeDirectoryMissing => {
                formatter.write_str("HOME is unavailable and XDG_STATE_HOME is not set")
            }
            Self::RelativeStateDirectory { path } => write!(
                formatter,
                "Host Exec audit state directory must be absolute: '{}'",
                path.display()
            ),
        }
    }
}

impl Error for AuditLogPathError {}

#[derive(Debug)]
pub enum AuditLogError {
    CreateDirectory {
        path: PathBuf,
        source: io::Error,
    },
    SetPermissions {
        path: PathBuf,
        source: io::Error,
    },
    Inspect {
        path: PathBuf,
        source: io::Error,
    },
    Open {
        path: PathBuf,
        source: io::Error,
    },
    Write {
        path: PathBuf,
        source: io::Error,
    },
    Rotate {
        path: PathBuf,
        source: io::Error,
    },
    Lock {
        path: PathBuf,
        source: io::Error,
    },
    UnsafeType {
        path: PathBuf,
        expected: &'static str,
    },
    HardLinked {
        path: PathBuf,
        links: u64,
    },
    FileTooLarge {
        path: PathBuf,
        bytes: u64,
        limit: u64,
    },
    WrongOwner {
        path: PathBuf,
        expected: u32,
        found: u32,
    },
    WrongMode {
        path: PathBuf,
        expected: u32,
        found: u32,
    },
    InvalidPath {
        path: PathBuf,
    },
    EventTooLarge {
        bytes: usize,
        limit: u64,
    },
    Serialize(serde_json::Error),
    StartWorker {
        source: io::Error,
    },
    JoinWorker {
        detail: String,
    },
    WorkerUnavailable,
    WorkerPanicked,
}

impl fmt::Display for AuditLogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateDirectory { path, source } => write!(
                formatter,
                "failed to create Host Exec audit directory '{}': {source}",
                path.display()
            ),
            Self::SetPermissions { path, source } => write!(
                formatter,
                "failed to secure Host Exec audit directory '{}': {source}",
                path.display()
            ),
            Self::Inspect { path, source } => write!(
                formatter,
                "failed to inspect Host Exec audit path '{}': {source}",
                path.display()
            ),
            Self::Open { path, source } => write!(
                formatter,
                "failed to open Host Exec audit file '{}': {source}",
                path.display()
            ),
            Self::Write { path, source } => write!(
                formatter,
                "failed to write Host Exec audit file '{}': {source}",
                path.display()
            ),
            Self::Rotate { path, source } => write!(
                formatter,
                "failed to rotate Host Exec audit file '{}': {source}",
                path.display()
            ),
            Self::Lock { path, source } => write!(
                formatter,
                "failed to lock Host Exec audit file '{}': {source}",
                path.display()
            ),
            Self::UnsafeType { path, expected } => write!(
                formatter,
                "unsafe Host Exec audit path '{}': expected {expected} without symbolic links",
                path.display()
            ),
            Self::HardLinked { path, links } => write!(
                formatter,
                "unsafe Host Exec audit file '{}': expected one hard link, found {links}",
                path.display()
            ),
            Self::FileTooLarge { path, bytes, limit } => write!(
                formatter,
                "unsafe Host Exec audit file size for '{}': found {bytes} bytes, limit {limit}",
                path.display()
            ),
            Self::WrongOwner {
                path,
                expected,
                found,
            } => write!(
                formatter,
                "unsafe Host Exec audit ownership for '{}': expected uid {expected}, found {found}",
                path.display()
            ),
            Self::WrongMode {
                path,
                expected,
                found,
            } => write!(
                formatter,
                "unsafe Host Exec audit permissions for '{}': expected {:04o}, found {:04o}",
                path.display(),
                expected,
                found
            ),
            Self::InvalidPath { path } => {
                write!(
                    formatter,
                    "invalid Host Exec audit path: '{}'",
                    path.display()
                )
            }
            Self::EventTooLarge { bytes, limit } => write!(
                formatter,
                "Host Exec audit event is too large ({bytes} bytes; limit {limit})"
            ),
            Self::Serialize(source) => {
                write!(
                    formatter,
                    "failed to serialize Host Exec audit event: {source}"
                )
            }
            Self::StartWorker { source } => {
                write!(
                    formatter,
                    "failed to start Host Exec audit worker: {source}"
                )
            }
            Self::JoinWorker { detail } => {
                write!(formatter, "failed to join Host Exec audit worker: {detail}")
            }
            Self::WorkerUnavailable => formatter.write_str("Host Exec audit worker is unavailable"),
            Self::WorkerPanicked => formatter.write_str("Host Exec audit worker panicked"),
        }
    }
}

impl Error for AuditLogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CreateDirectory { source, .. }
            | Self::SetPermissions { source, .. }
            | Self::Inspect { source, .. }
            | Self::Open { source, .. }
            | Self::Write { source, .. }
            | Self::Rotate { source, .. }
            | Self::Lock { source, .. }
            | Self::StartWorker { source } => Some(source),
            Self::Serialize(source) => Some(source),
            Self::UnsafeType { .. }
            | Self::HardLinked { .. }
            | Self::FileTooLarge { .. }
            | Self::WrongOwner { .. }
            | Self::WrongMode { .. }
            | Self::InvalidPath { .. }
            | Self::EventTooLarge { .. }
            | Self::JoinWorker { .. }
            | Self::WorkerUnavailable
            | Self::WorkerPanicked => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        ffi::OsString,
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt, symlink},
        path::{Path, PathBuf},
    };

    use tempfile::tempdir;

    use super::{
        AUDIT_DIRECTORY_MODE, AUDIT_FILE_MODE, AuditController, AuditEvent, AuditLogError,
        AuditLogInspection, MAX_RETAINED_OUTPUT_BYTES, inspect_audit_log_path, lock_path,
        resolve_audit_log_path, rotated_path, write_event,
    };
    use crate::host_bridge::{
        AllowedHostCommand, HOST_EXEC_DSL_VERSION, HostExecOutput, HostExecPolicy, HostExecRequest,
        HostExecutionState, HostOutputChunk, HostOutputStream,
    };

    #[test]
    fn resolves_xdg_state_before_the_home_fallback() {
        assert_eq!(
            resolve_audit_log_path(Some("/state"), Some("/home/test"))
                .expect("XDG state path should resolve"),
            Path::new("/state/cloister/audit/host-exec.jsonl")
        );
        assert_eq!(
            resolve_audit_log_path(None::<PathBuf>, Some("/home/test"))
                .expect("HOME fallback should resolve"),
            Path::new("/home/test/.local/state/cloister/audit/host-exec.jsonl")
        );
        assert!(resolve_audit_log_path(Some("relative"), Some("/home/test")).is_err());
    }

    #[tokio::test]
    async fn creates_owner_only_directories_and_files() {
        let directory = tempdir().expect("temporary directory should exist");
        let path = audit_path(directory.path());

        let audit = AuditController::start(
            path.clone(),
            "test".to_owned(),
            "test-agent".to_owned(),
            directory.path().to_owned(),
        )
        .expect("audit controller should start");

        assert_eq!(
            inspect_audit_log_path(&path).expect("audit path should inspect"),
            AuditLogInspection::Ready
        );
        assert_eq!(mode(path.parent().unwrap()), AUDIT_DIRECTORY_MODE);
        assert_eq!(
            mode(path.parent().unwrap().parent().unwrap()),
            AUDIT_DIRECTORY_MODE
        );
        assert_eq!(mode(&path), AUDIT_FILE_MODE);
        assert_eq!(mode(&lock_path(&path)), AUDIT_FILE_MODE);
        assert_eq!(fs::metadata(&path).unwrap().nlink(), 1);
        audit
            .shutdown()
            .await
            .expect("audit controller should stop");
    }

    #[tokio::test]
    async fn lifecycle_events_exclude_arguments_output_and_environment_values() {
        let directory = tempdir().expect("temporary directory should exist");
        let path = audit_path(directory.path());
        let secret = "super-secret-audit-value";
        let audit = AuditController::start(
            path.clone(),
            "test-profile".to_owned(),
            "test-agent".to_owned(),
            directory.path().to_owned(),
        )
        .expect("audit controller should start");
        let log = audit.log();
        let policy = HostExecPolicy::new(
            [AllowedHostCommand::new("tool", "/usr/bin/true")
                .expect("test command should be valid")],
            BTreeMap::from([(OsString::from("CLOISTER_SECRET"), OsString::from(secret))]),
        )
        .expect("test policy should build");
        let request = HostExecRequest {
            version: HOST_EXEC_DSL_VERSION,
            command: "tool".to_owned(),
            args: vec![secret.to_owned()],
        };
        let authorized = policy
            .authorize(&request)
            .expect("request should authorize");
        let metadata = log
            .started(
                "req_test".to_owned(),
                "exec_test".to_owned(),
                &request,
                &authorized,
            )
            .await
            .expect("started event should persist");
        log.finished(
            &metadata,
            &HostExecOutput {
                execution_id: "exec_test".to_owned(),
                state: HostExecutionState::Completed,
                duration_ms: 42,
                exit_code: Some(0),
                chunks: vec![HostOutputChunk {
                    cursor: 1,
                    stream: HostOutputStream::Stdout,
                    text: secret.to_owned(),
                }],
                next_cursor: 1,
                stdout_bytes: secret.len() as u64,
                stderr_bytes: 0,
                output_truncated: false,
            },
        )
        .await
        .expect("finished event should persist");
        audit
            .shutdown()
            .await
            .expect("audit controller should stop");

        let source = fs::read_to_string(&path).expect("audit log should be readable");
        assert!(!source.contains(secret));
        assert!(!source.contains("\"args\""));
        assert!(!source.contains("\"chunks\""));
        assert!(!source.contains("\"text\""));
        assert!(source.contains("\"environment_variable_names\":[\"CLOISTER_SECRET\"]"));
        assert!(source.contains("\"event\":\"execution_started\""));
        assert!(source.contains("\"event\":\"execution_finished\""));
        assert!(source.contains("\"stdout_bytes\":24"));
    }

    #[test]
    fn rotates_two_bounded_segments_before_appending() {
        let directory = tempdir().expect("temporary directory should exist");
        let path = audit_path(directory.path());
        super::prepare_destination(&path).expect("audit destination should prepare");
        let event = test_event();
        let segment_bytes = 2_048;

        for _ in 0..40 {
            write_event(&path, &event, segment_bytes).expect("audit event should append");
        }

        let active_bytes = fs::metadata(&path).expect("active log should exist").len();
        let rotated_bytes = fs::metadata(rotated_path(&path))
            .expect("rotated log should exist")
            .len();
        assert!(active_bytes <= segment_bytes);
        assert!(rotated_bytes <= segment_bytes);
        assert!(active_bytes + rotated_bytes <= segment_bytes * 2);
    }

    #[test]
    fn rejects_symbolic_link_and_broad_directory_permissions() {
        let directory = tempdir().expect("temporary directory should exist");
        let path = audit_path(directory.path());
        let cloister = path.parent().unwrap().parent().unwrap();
        fs::create_dir_all(cloister).expect("Cloister state directory should exist");
        fs::set_permissions(cloister, fs::Permissions::from_mode(0o700))
            .expect("Cloister state permissions should be set");
        let target = directory.path().join("outside");
        fs::create_dir(&target).expect("symlink target should exist");
        symlink(&target, path.parent().unwrap()).expect("audit symlink should be created");

        let error = AuditController::start(
            path.clone(),
            "test".to_owned(),
            "test-agent".to_owned(),
            directory.path().to_owned(),
        )
        .expect_err("audit symlink should be rejected");
        assert!(matches!(error, AuditLogError::UnsafeType { .. }));

        fs::remove_file(path.parent().unwrap()).expect("audit symlink should be removed");
        fs::create_dir(path.parent().unwrap()).expect("audit directory should exist");
        fs::set_permissions(path.parent().unwrap(), fs::Permissions::from_mode(0o755))
            .expect("unsafe audit permissions should be set");
        let error = inspect_audit_log_path(&path).expect_err("broad permissions should fail");
        assert!(matches!(error, AuditLogError::WrongMode { .. }));
    }

    #[test]
    fn rejects_an_oversized_existing_segment() {
        let directory = tempdir().expect("temporary directory should exist");
        let path = audit_path(directory.path());
        super::prepare_destination(&path).expect("audit destination should prepare");
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("audit segment should open")
            .set_len(super::AUDIT_SEGMENT_BYTES + 1)
            .expect("audit segment should be enlarged");

        let error = inspect_audit_log_path(&path).expect_err("oversized segment should fail");
        assert!(matches!(error, AuditLogError::FileTooLarge { .. }));
    }

    fn audit_path(root: &Path) -> PathBuf {
        root.join("state/cloister/audit/host-exec.jsonl")
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn test_event() -> AuditEvent {
        AuditEvent {
            audit_schema_version: 1,
            timestamp_unix_ms: 1,
            event: "execution_finished",
            request_id: "req_test".to_owned(),
            execution_id: Some("exec_test".to_owned()),
            profile: "test".to_owned(),
            agent: "test-agent".to_owned(),
            workspace: "/workspace".to_owned(),
            command: "tool".to_owned(),
            request_version: HOST_EXEC_DSL_VERSION,
            declared_executable: Some("/usr/bin/true".to_owned()),
            resolved_executable: Some("/usr/bin/true".to_owned()),
            argument_count: 0,
            argument_values_redacted: true,
            environment_mode: "inherit-all",
            environment_variable_names: Vec::new(),
            outcome: "completed",
            failure_kind: None,
            state: Some(HostExecutionState::Completed),
            exit_code: Some(0),
            duration_ms: 1,
            stdout_bytes: 0,
            stderr_bytes: 0,
            output_truncated: false,
            retained_output_limit_bytes: MAX_RETAINED_OUTPUT_BYTES as u64,
        }
    }
}
