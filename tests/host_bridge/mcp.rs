use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::TcpStream,
    path::Path,
    time::Duration,
};

use cloister::host_bridge::{BridgeToken, call_host_exec, serve};
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

async fn start(
    token: BridgeToken,
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
    let handle = tokio::spawn(async move { serve(listener, token, server_cancellation).await });

    (format!("http://{address}/mcp"), cancellation, handle)
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

#[tokio::test]
async fn rejects_a_client_with_the_wrong_token() {
    let directory = tempdir().expect("temporary directory should exist");
    let server_token = create_token(&directory.path().join("server.token"));
    let client_token = create_token(&directory.path().join("client.token"));
    let (endpoint, cancellation, handle) = start(server_token).await;

    let error = call_host_exec(&endpoint, &client_token, "printf hello")
        .await
        .expect_err("wrong token should fail");

    assert!(error.to_string().contains("host bridge request failed"));
    cancellation.cancel();
    handle
        .await
        .expect("server task should join")
        .expect("server should stop cleanly");
}

#[tokio::test]
async fn server_api_rejects_a_non_loopback_listener() {
    let directory = tempdir().expect("temporary directory should exist");
    let token = create_token(&directory.path().join("bridge.token"));
    let listener = TcpListener::bind("0.0.0.0:0")
        .await
        .expect("wildcard listener should bind");
    let cancellation = CancellationToken::new();

    let error = serve(listener, token, cancellation)
        .await
        .expect_err("non-loopback server should fail");

    assert!(error.to_string().contains("only listen on a loopback"));
}

#[tokio::test]
async fn executes_a_host_shell_command() {
    let directory = tempdir().expect("temporary directory should exist");
    let token = create_token(&directory.path().join("bridge.token"));
    let (endpoint, cancellation, handle) = start(token.clone()).await;

    let output = call_host_exec(
        &endpoint,
        &token,
        "printf stdout; printf stderr >&2; exit 7",
    )
    .await
    .expect("host.exec should return command output");

    assert_eq!(output.stdout, "stdout");
    assert_eq!(output.stderr, "stderr");
    assert_eq!(output.exit_code, Some(7));
    cancellation.cancel();
    handle
        .await
        .expect("server task should join")
        .expect("server should stop cleanly");
}

#[tokio::test]
async fn accepts_the_apple_container_host_name() {
    let directory = tempdir().expect("temporary directory should exist");
    let token_path = directory.path().join("bridge.token");
    let token = create_token(&token_path);
    let bearer = fs::read_to_string(&token_path).expect("bridge token should be readable");
    let (endpoint, cancellation, handle) = start(token).await;

    let status = request_with_host(&endpoint, "host.container.internal:17834", bearer.trim()).await;

    assert!(status.starts_with("HTTP/1.1 200"), "{status}");
    cancellation.cancel();
    handle
        .await
        .expect("server task should join")
        .expect("server should stop cleanly");
}

#[tokio::test]
async fn continues_to_reject_an_unrecognized_host_name() {
    let directory = tempdir().expect("temporary directory should exist");
    let token_path = directory.path().join("bridge.token");
    let token = create_token(&token_path);
    let bearer = fs::read_to_string(&token_path).expect("bridge token should be readable");
    let (endpoint, cancellation, handle) = start(token).await;

    let status = request_with_host(&endpoint, "attacker.example:17834", bearer.trim()).await;

    assert!(status.starts_with("HTTP/1.1 403"), "{status}");
    cancellation.cancel();
    handle
        .await
        .expect("server task should join")
        .expect("server should stop cleanly");
}
