use std::process::{Command, Output};

use tempfile::tempdir;

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cloister"))
        .args(arguments)
        .output()
        .expect("Cloister binary should start")
}

#[test]
fn refuses_to_expose_the_bridge_on_a_non_loopback_address() {
    let directory = tempdir().expect("temporary directory should exist");
    let token = directory.path().join("bridge.token");
    let output = run(&[
        "host",
        "serve",
        "--listen",
        "0.0.0.0:17834",
        "--token-file",
        token.to_str().expect("UTF-8 path"),
    ]);
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("only listen on a loopback address"));
    assert!(!token.exists());
}

#[test]
fn reports_a_missing_client_token_before_connecting() {
    let directory = tempdir().expect("temporary directory should exist");
    let token = directory.path().join("missing.token");
    let output = run(&[
        "host",
        "exec",
        "--endpoint",
        "http://127.0.0.1:1/mcp",
        "--token-file",
        token.to_str().expect("UTF-8 path"),
        "printf hello",
    ]);
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("failed to read host bridge token"));
}
