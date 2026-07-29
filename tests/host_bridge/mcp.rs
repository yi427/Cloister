use std::path::Path;

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
