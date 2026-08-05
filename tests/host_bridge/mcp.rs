use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::{BufRead, BufReader, Write},
    net::TcpStream,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::Duration,
};

use cloister::{
    host_bridge::{
        BridgeToken, HOST_EXEC_DSL_VERSION, HostBridgeContext, HostExecCancelRequest,
        HostExecOutput, HostExecPolicy, HostExecRequest, HostExecStatusRequest, HostExecutionState,
        HostOutputStream, call_host_exec, call_host_exec_cancel, call_host_exec_status,
        call_host_list_commands, serve,
    },
    profile::{
        HostExecAllowProfile, HostExecArguments, HostExecEnvironmentMode,
        HostExecEnvironmentProfile, HostExecProfile,
    },
};
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

async fn start(
    token: BridgeToken,
    policy: HostExecPolicy,
    working_directory: &Path,
) -> (
    String,
    CancellationToken,
    tokio::task::JoinHandle<Result<(), cloister::host_bridge::HostBridgeServerError>>,
) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("loopback listener should bind");
    let address = listener
        .local_addr()
        .expect("listener address should be available");
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let working_directory = working_directory.to_owned();
    let handle = tokio::spawn(async move {
        serve(
            listener,
            token,
            policy,
            context(&working_directory),
            server_cancellation,
        )
        .await
    });

    (format!("http://{address}/mcp"), cancellation, handle)
}

fn context(working_directory: &Path) -> HostBridgeContext {
    HostBridgeContext::new(
        "test-profile",
        "test-agent",
        working_directory,
        audit_path(working_directory),
    )
}

fn audit_path(working_directory: &Path) -> std::path::PathBuf {
    working_directory.join("state/cloister/audit/host-exec.jsonl")
}

fn empty_policy() -> HostExecPolicy {
    HostExecPolicy::new([], BTreeMap::new()).expect("empty test policy should build")
}

fn profile_policy(
    name: &str,
    executable: &Path,
    environment: BTreeMap<OsString, OsString>,
) -> HostExecPolicy {
    HostExecPolicy::from_profile(
        &HostExecProfile {
            enabled: true,
            environment: HostExecEnvironmentProfile {
                mode: HostExecEnvironmentMode::InheritAll,
            },
            allow: vec![HostExecAllowProfile {
                name: name.to_owned(),
                executable: executable.to_owned(),
                description: "Test an allowed host command".to_owned(),
                arguments: HostExecArguments::Any,
            }],
        },
        environment,
    )
    .expect("test policy should build")
    .expect("test policy should be enabled")
}

fn request(command: &str, args: &[&str]) -> HostExecRequest {
    HostExecRequest {
        version: HOST_EXEC_DSL_VERSION,
        command: command.to_owned(),
        args: args.iter().map(|argument| (*argument).to_owned()).collect(),
    }
}

async fn wait_for_terminal(
    endpoint: &str,
    token: &BridgeToken,
    mut output: HostExecOutput,
) -> HostExecOutput {
    let mut chunks = std::mem::take(&mut output.chunks);
    let mut cursor = output.next_cursor;
    for _ in 0..200 {
        if output.state.is_terminal() {
            output.chunks = chunks;
            return output;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        output = call_host_exec_status(
            endpoint,
            token,
            &HostExecStatusRequest {
                execution_id: output.execution_id.clone(),
                cursor: Some(cursor),
            },
        )
        .await
        .expect("host.exec_status should return execution state");
        cursor = output.next_cursor;
        chunks.append(&mut output.chunks);
    }
    panic!("Host Exec did not reach a terminal state")
}

fn stream_text(output: &HostExecOutput, stream: HostOutputStream) -> String {
    output
        .chunks
        .iter()
        .filter(|chunk| chunk.stream == stream)
        .map(|chunk| chunk.text.as_str())
        .collect()
}

fn create_token(path: &Path) -> BridgeToken {
    BridgeToken::load_or_create(path).expect("bridge token should be created")
}

fn address_from_endpoint(endpoint: &str) -> String {
    endpoint
        .strip_prefix("http://")
        .and_then(|endpoint| endpoint.strip_suffix("/mcp"))
        .expect("test endpoint should contain an HTTP authority")
        .to_owned()
}

fn initialize_status(address: &str, host: &str, bearer: &str) -> String {
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"cloister-test","version":"0.1"}}}"#;
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {bearer}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(address).expect("test client should connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("test client should set a read timeout");
    stream
        .write_all(request.as_bytes())
        .expect("test client should write the request");

    let mut status = String::new();
    BufReader::new(stream)
        .read_line(&mut status)
        .expect("test client should read the response status");
    status
}

async fn request_with_host(endpoint: &str, host: &str, bearer: &str) -> String {
    let address = address_from_endpoint(endpoint);
    let host = host.to_owned();
    let bearer = bearer.to_owned();

    tokio::task::spawn_blocking(move || initialize_status(&address, &host, &bearer))
        .await
        .expect("test client task should join")
}

async fn stop(
    cancellation: CancellationToken,
    handle: tokio::task::JoinHandle<Result<(), cloister::host_bridge::HostBridgeServerError>>,
) {
    cancellation.cancel();
    handle
        .await
        .expect("server task should join")
        .expect("server should stop cleanly");
}

#[tokio::test]
async fn rejects_a_client_with_the_wrong_token() {
    let directory = tempdir().expect("temporary directory should exist");
    let server_token = create_token(&directory.path().join("server.token"));
    let client_token = create_token(&directory.path().join("client.token"));
    let (endpoint, cancellation, handle) =
        start(server_token, empty_policy(), directory.path()).await;

    let error = call_host_exec(&endpoint, &client_token, &request("missing", &[]))
        .await
        .expect_err("wrong token should fail");

    assert!(error.to_string().contains("host bridge request failed"));
    stop(cancellation, handle).await;
}

#[tokio::test]
async fn server_api_rejects_a_non_loopback_listener() {
    let directory = tempdir().expect("temporary directory should exist");
    let token = create_token(&directory.path().join("bridge.token"));
    let listener = TcpListener::bind("0.0.0.0:0")
        .await
        .expect("wildcard listener should bind");
    let cancellation = CancellationToken::new();

    let error = serve(
        listener,
        token,
        empty_policy(),
        context(directory.path()),
        cancellation,
    )
    .await
    .expect_err("non-loopback server should fail");

    assert!(error.to_string().contains("only listen on a loopback"));
}

#[tokio::test]
async fn server_rejects_a_missing_working_directory() {
    let directory = tempdir().expect("temporary directory should exist");
    let token = create_token(&directory.path().join("bridge.token"));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("loopback listener should bind");
    let cancellation = CancellationToken::new();

    let error = serve(
        listener,
        token,
        empty_policy(),
        HostBridgeContext::new(
            "test-profile",
            "test-agent",
            directory.path().join("missing-workspace"),
            directory
                .path()
                .join("state/cloister/audit/host-exec.jsonl"),
        ),
        cancellation,
    )
    .await
    .expect_err("missing working directory should fail");

    assert!(
        error
            .to_string()
            .contains("failed to resolve Host Exec working directory")
    );
}

#[tokio::test]
async fn discovers_only_profile_allowed_commands_without_environment_values() {
    let directory = tempdir().expect("temporary directory should exist");
    let token = create_token(&directory.path().join("bridge.token"));
    let executable = directory.path().join("allowed-tool");
    fs::write(&executable, "#!/bin/sh\nexit 0\n").expect("test executable should be written");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
        .expect("test executable should be executable");
    let policy = profile_policy(
        "allowed-tool",
        &executable,
        BTreeMap::from([(
            OsString::from("CLOISTER_TEST_SECRET"),
            OsString::from("secret-value"),
        )]),
    );
    let (endpoint, cancellation, handle) = start(token.clone(), policy, directory.path()).await;

    let output = call_host_list_commands(&endpoint, &token)
        .await
        .expect("host.list_commands should return policy metadata");
    let rendered = serde_json::to_string(&output).expect("discovery should serialize");

    assert_eq!(output.version, HOST_EXEC_DSL_VERSION);
    assert_eq!(output.commands.len(), 1);
    assert_eq!(output.commands[0].name, "allowed-tool");
    assert_eq!(output.commands[0].arguments, "any");
    assert_eq!(output.environment.variable_names, ["CLOISTER_TEST_SECRET"]);
    assert!(output.audit_logging);
    assert!(!rendered.contains("secret-value"));
    stop(cancellation, handle).await;
}

#[tokio::test]
async fn executes_an_allowed_command_without_shell_parsing() {
    let directory = tempdir().expect("temporary directory should exist");
    let token = create_token(&directory.path().join("bridge.token"));
    let executable = directory.path().join("argument-printer");
    fs::write(
        &executable,
        "#!/bin/sh\nprintf '%s\\n' \"$@\"\nprintf 'stderr\\n' >&2\npwd >&2\nexit 7\n",
    )
    .expect("test executable should be written");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
        .expect("test executable should be executable");
    let policy = profile_policy("printer", &executable, BTreeMap::new());
    let (endpoint, cancellation, handle) = start(token.clone(), policy, directory.path()).await;

    let output = call_host_exec(
        &endpoint,
        &token,
        &request("printer", &["$(uname)", "; exit 0"]),
    )
    .await
    .expect("host.exec should return command output");
    let output = wait_for_terminal(&endpoint, &token, output).await;

    assert_eq!(output.state, HostExecutionState::Completed);
    assert_eq!(
        stream_text(&output, HostOutputStream::Stdout),
        "$(uname)\n; exit 0\n"
    );
    assert_eq!(
        stream_text(&output, HostOutputStream::Stderr),
        format!(
            "stderr\n{}\n",
            fs::canonicalize(directory.path())
                .expect("working directory should canonicalize")
                .display()
        )
    );
    assert_eq!(output.exit_code, Some(7));
    stop(cancellation, handle).await;
}

#[tokio::test]
async fn terminal_execution_duration_stops_advancing() {
    let directory = tempdir().expect("temporary directory should exist");
    let token = create_token(&directory.path().join("bridge.token"));
    let executable = directory.path().join("quick-command");
    fs::write(&executable, "#!/bin/sh\nexit 0\n").expect("test executable should be written");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
        .expect("test executable should be executable");
    let policy = profile_policy("quick-command", &executable, BTreeMap::new());
    let (endpoint, cancellation, handle) = start(token.clone(), policy, directory.path()).await;

    let output = call_host_exec(&endpoint, &token, &request("quick-command", &[]))
        .await
        .expect("host.exec should start the command");
    let output = wait_for_terminal(&endpoint, &token, output).await;
    let terminal_duration_ms = output.duration_ms;

    tokio::time::sleep(Duration::from_millis(50)).await;
    let later = call_host_exec_status(
        &endpoint,
        &token,
        &HostExecStatusRequest {
            execution_id: output.execution_id,
            cursor: Some(output.next_cursor),
        },
    )
    .await
    .expect("host.exec_status should retain the terminal execution");

    assert_eq!(later.state, HostExecutionState::Completed);
    assert_eq!(later.duration_ms, terminal_duration_ms);
    stop(cancellation, handle).await;
}

#[tokio::test]
async fn persists_lifecycle_metadata_without_arguments_output_or_environment_values() {
    let directory = tempdir().expect("temporary directory should exist");
    let token = create_token(&directory.path().join("bridge.token"));
    let executable = directory.path().join("secret-printer");
    let secret = "do-not-persist-this-secret";
    fs::write(
        &executable,
        "#!/bin/sh\nprintf '%s\\n' \"$1\"\nprintf '%s\\n' \"$CLOISTER_AUDIT_SECRET\" >&2\n",
    )
    .expect("test executable should be written");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
        .expect("test executable should be executable");
    let policy = profile_policy(
        "secret-printer",
        &executable,
        BTreeMap::from([(
            OsString::from("CLOISTER_AUDIT_SECRET"),
            OsString::from(secret),
        )]),
    );
    let (endpoint, cancellation, handle) = start(token.clone(), policy, directory.path()).await;

    let output = call_host_exec(&endpoint, &token, &request("secret-printer", &[secret]))
        .await
        .expect("host.exec should start the command");
    let output = wait_for_terminal(&endpoint, &token, output).await;
    assert_eq!(output.state, HostExecutionState::Completed);
    assert!(stream_text(&output, HostOutputStream::Stdout).contains(secret));
    assert!(stream_text(&output, HostOutputStream::Stderr).contains(secret));
    stop(cancellation, handle).await;

    let source = fs::read_to_string(audit_path(directory.path()))
        .expect("Host Exec audit log should be readable");
    assert!(source.contains("\"event\":\"execution_started\""));
    assert!(source.contains("\"event\":\"execution_finished\""));
    assert!(source.contains("\"profile\":\"test-profile\""));
    assert!(source.contains("\"agent\":\"test-agent\""));
    assert!(source.contains("\"command\":\"secret-printer\""));
    assert!(source.contains("\"request_version\":1"));
    assert!(source.contains("\"argument_count\":1"));
    assert!(source.contains("\"argument_values_redacted\":true"));
    assert!(source.contains("\"retained_output_limit_bytes\":1048576"));
    assert!(source.contains("\"environment_variable_names\":[\"CLOISTER_AUDIT_SECRET\"]"));
    assert!(!source.contains(secret));
    assert!(!source.contains("\"args\""));
    assert!(!source.contains("\"chunks\""));
}

#[tokio::test]
async fn rejects_a_command_outside_the_profile_allowlist() {
    let directory = tempdir().expect("temporary directory should exist");
    let token = create_token(&directory.path().join("bridge.token"));
    let (endpoint, cancellation, handle) =
        start(token.clone(), empty_policy(), directory.path()).await;

    let error = call_host_exec(&endpoint, &token, &request("uname", &[]))
        .await
        .expect_err("unlisted command should be denied");

    assert!(error.to_string().contains("host command is not allowed"));
    stop(cancellation, handle).await;
    let source =
        fs::read_to_string(audit_path(directory.path())).expect("denied request should be audited");
    assert!(source.contains("\"event\":\"execution_denied\""));
    assert!(source.contains("\"failure_kind\":\"command_not_allowed\""));
}

#[tokio::test]
async fn status_returns_only_output_after_the_requested_cursor() {
    let directory = tempdir().expect("temporary directory should exist");
    let token = create_token(&directory.path().join("bridge.token"));
    let executable = directory.path().join("incremental-output");
    fs::write(
        &executable,
        "#!/bin/sh\nprintf 'first-out\\n'\nprintf 'first-err\\n' >&2\nsleep 0.3\nprintf 'second-out\\n'\nprintf 'second-err\\n' >&2\n",
    )
    .expect("test executable should be written");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
        .expect("test executable should be executable");
    let policy = profile_policy("incremental", &executable, BTreeMap::new());
    let (endpoint, cancellation, handle) = start(token.clone(), policy, directory.path()).await;

    let mut snapshot = call_host_exec(&endpoint, &token, &request("incremental", &[]))
        .await
        .expect("host.exec should start the command");
    assert_eq!(snapshot.state, HostExecutionState::Running);
    for _ in 0..20 {
        if stream_text(&snapshot, HostOutputStream::Stdout).contains("first-out\n")
            && stream_text(&snapshot, HostOutputStream::Stderr).contains("first-err\n")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        snapshot = call_host_exec_status(
            &endpoint,
            &token,
            &HostExecStatusRequest {
                execution_id: snapshot.execution_id.clone(),
                cursor: None,
            },
        )
        .await
        .expect("host.exec_status should expose first output");
    }
    assert_eq!(
        stream_text(&snapshot, HostOutputStream::Stdout),
        "first-out\n"
    );
    assert_eq!(
        stream_text(&snapshot, HostOutputStream::Stderr),
        "first-err\n"
    );
    let cursor = snapshot.next_cursor;

    let mut later = call_host_exec_status(
        &endpoint,
        &token,
        &HostExecStatusRequest {
            execution_id: snapshot.execution_id,
            cursor: Some(cursor),
        },
    )
    .await
    .expect("host.exec_status should accept a cursor");
    while later.state == HostExecutionState::Running {
        tokio::time::sleep(Duration::from_millis(20)).await;
        later = call_host_exec_status(
            &endpoint,
            &token,
            &HostExecStatusRequest {
                execution_id: later.execution_id.clone(),
                cursor: Some(cursor),
            },
        )
        .await
        .expect("host.exec_status should reach completion");
    }

    assert_eq!(later.state, HostExecutionState::Completed);
    assert_eq!(
        stream_text(&later, HostOutputStream::Stdout),
        "second-out\n"
    );
    assert_eq!(
        stream_text(&later, HostOutputStream::Stderr),
        "second-err\n"
    );
    stop(cancellation, handle).await;
}

#[tokio::test]
async fn rejects_unknown_execution_ids_for_status_and_cancel() {
    let directory = tempdir().expect("temporary directory should exist");
    let token = create_token(&directory.path().join("bridge.token"));
    let (endpoint, cancellation, handle) =
        start(token.clone(), empty_policy(), directory.path()).await;

    let status_error = call_host_exec_status(
        &endpoint,
        &token,
        &HostExecStatusRequest {
            execution_id: "exec_missing".to_owned(),
            cursor: None,
        },
    )
    .await
    .expect_err("unknown status execution should fail");
    let cancel_error = call_host_exec_cancel(
        &endpoint,
        &token,
        &HostExecCancelRequest {
            execution_id: "exec_missing".to_owned(),
        },
    )
    .await
    .expect_err("unknown cancellation execution should fail");

    assert!(
        status_error
            .to_string()
            .contains("unknown Host Exec execution ID")
    );
    assert!(
        cancel_error
            .to_string()
            .contains("unknown Host Exec execution ID")
    );
    stop(cancellation, handle).await;
}

#[tokio::test]
async fn rejects_execution_above_the_bridge_concurrency_limit() {
    let directory = tempdir().expect("temporary directory should exist");
    let token = create_token(&directory.path().join("bridge.token"));
    let executable = directory.path().join("long-running");
    fs::write(&executable, "#!/bin/sh\nsleep 10\n").expect("test executable should be written");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
        .expect("test executable should be executable");
    let policy = profile_policy("long-running", &executable, BTreeMap::new());
    let (endpoint, cancellation, handle) = start(token.clone(), policy, directory.path()).await;

    let mut executions = Vec::new();
    for _ in 0..8 {
        let output = call_host_exec(&endpoint, &token, &request("long-running", &[]))
            .await
            .expect("execution within the concurrency limit should start");
        assert_eq!(output.state, HostExecutionState::Running);
        executions.push(output);
    }
    let error = call_host_exec(&endpoint, &token, &request("long-running", &[]))
        .await
        .expect_err("execution above the concurrency limit should fail");
    assert!(error.to_string().contains("concurrency limit reached (8"));

    for output in &executions {
        call_host_exec_cancel(
            &endpoint,
            &token,
            &HostExecCancelRequest {
                execution_id: output.execution_id.clone(),
            },
        )
        .await
        .expect("test execution should accept cancellation");
    }
    for output in executions {
        let output = wait_for_terminal(&endpoint, &token, output).await;
        assert_eq!(output.state, HostExecutionState::Cancelled);
    }
    stop(cancellation, handle).await;
}

#[tokio::test]
async fn cancel_terminates_descendants_that_ignore_term() {
    let directory = tempdir().expect("temporary directory should exist");
    let token = create_token(&directory.path().join("bridge.token"));
    let executable = directory.path().join("process-tree");
    let child_pid_file = directory.path().join("child.pid");
    fs::write(
        &executable,
        "#!/bin/sh\nchild_file=$1\n/bin/sh -c 'trap \"\" TERM; printf \"%s\\n\" \"$$\" > \"$1\"; while :; do sleep 1; done' child \"$child_file\" &\nwait\n",
    )
    .expect("test executable should be written");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
        .expect("test executable should be executable");
    let policy = profile_policy("process-tree", &executable, BTreeMap::new());
    let (endpoint, cancellation, handle) = start(token.clone(), policy, directory.path()).await;

    let output = call_host_exec(
        &endpoint,
        &token,
        &request(
            "process-tree",
            &[child_pid_file
                .to_str()
                .expect("temporary path should be UTF-8")],
        ),
    )
    .await
    .expect("host.exec should start the process tree");
    for _ in 0..100 {
        if child_pid_file.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let child_pid: i32 = fs::read_to_string(&child_pid_file)
        .expect("child process should write its PID")
        .trim()
        .parse()
        .expect("child PID should parse");
    assert!(process_exists(child_pid));

    let cancellation_output = call_host_exec_cancel(
        &endpoint,
        &token,
        &HostExecCancelRequest {
            execution_id: output.execution_id.clone(),
        },
    )
    .await
    .expect("host.exec_cancel should accept the running execution");
    assert_eq!(cancellation_output.state, HostExecutionState::Running);
    let output = wait_for_terminal(&endpoint, &token, output).await;

    assert_eq!(output.state, HostExecutionState::Cancelled);
    for _ in 0..100 {
        if !process_exists(child_pid) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        !process_exists(child_pid),
        "descendant should be terminated"
    );
    stop(cancellation, handle).await;
}

#[tokio::test]
async fn bridge_shutdown_cleans_up_running_process_groups() {
    let directory = tempdir().expect("temporary directory should exist");
    let token = create_token(&directory.path().join("bridge.token"));
    let executable = directory.path().join("shutdown-tree");
    let child_pid_file = directory.path().join("shutdown-child.pid");
    fs::write(
        &executable,
        "#!/bin/sh\nchild_file=$1\n/bin/sh -c 'trap \"\" TERM; printf \"%s\\n\" \"$$\" > \"$1\"; while :; do sleep 1; done' child \"$child_file\" &\nwait\n",
    )
    .expect("test executable should be written");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
        .expect("test executable should be executable");
    let policy = profile_policy("shutdown-tree", &executable, BTreeMap::new());
    let (endpoint, cancellation, handle) = start(token.clone(), policy, directory.path()).await;

    let output = call_host_exec(
        &endpoint,
        &token,
        &request(
            "shutdown-tree",
            &[child_pid_file
                .to_str()
                .expect("temporary path should be UTF-8")],
        ),
    )
    .await
    .expect("host.exec should start the process tree");
    assert_eq!(output.state, HostExecutionState::Running);
    for _ in 0..100 {
        if child_pid_file.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let child_pid: i32 = fs::read_to_string(&child_pid_file)
        .expect("child process should write its PID")
        .trim()
        .parse()
        .expect("child PID should parse");
    assert!(process_exists(child_pid));

    stop(cancellation, handle).await;

    for _ in 0..100 {
        if !process_exists(child_pid) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        !process_exists(child_pid),
        "bridge shutdown should terminate descendants"
    );
}

fn process_exists(process_id: i32) -> bool {
    // SAFETY: signal 0 only checks whether this numeric process ID exists.
    let result = unsafe { libc::kill(process_id, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[tokio::test]
async fn accepts_the_apple_container_host_name() {
    let directory = tempdir().expect("temporary directory should exist");
    let token_path = directory.path().join("bridge.token");
    let token = create_token(&token_path);
    let bearer = fs::read_to_string(&token_path).expect("bridge token should be readable");
    let (endpoint, cancellation, handle) = start(token, empty_policy(), directory.path()).await;

    let status = request_with_host(&endpoint, "host.container.internal:17834", bearer.trim()).await;

    assert!(status.starts_with("HTTP/1.1 200"), "{status}");
    stop(cancellation, handle).await;
}

#[tokio::test]
async fn continues_to_reject_an_unrecognized_host_name() {
    let directory = tempdir().expect("temporary directory should exist");
    let token_path = directory.path().join("bridge.token");
    let token = create_token(&token_path);
    let bearer = fs::read_to_string(&token_path).expect("bridge token should be readable");
    let (endpoint, cancellation, handle) = start(token, empty_policy(), directory.path()).await;

    let status = request_with_host(&endpoint, "attacker.example:17834", bearer.trim()).await;

    assert!(status.starts_with("HTTP/1.1 403"), "{status}");
    stop(cancellation, handle).await;
}
