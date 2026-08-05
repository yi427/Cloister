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
        BridgeToken, HOST_EXEC_DSL_VERSION, HostExecPolicy, HostExecRequest, call_host_exec,
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
            working_directory,
            server_cancellation,
        )
        .await
    });

    (format!("http://{address}/mcp"), cancellation, handle)
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
        directory.path().to_owned(),
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
        directory.path().join("missing-workspace"),
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

    assert_eq!(output.stdout, "$(uname)\n; exit 0\n");
    assert_eq!(
        output.stderr,
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
