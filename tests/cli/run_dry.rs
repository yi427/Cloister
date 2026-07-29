use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn fixture(relative_path: impl AsRef<Path>) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/profiles")
        .join(relative_path)
}

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cloister"))
        .args(arguments)
        .output()
        .expect("Cloister binary should start")
}

#[test]
fn prints_an_inspectable_plan_without_running_container() {
    let path = fixture("valid/default.toml");
    let output = run(&[
        "run",
        "--profile",
        path.to_str().expect("UTF-8 path"),
        "--dry-run",
    ]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("Profile: rust-default"));
    assert!(stdout.contains("Network: default (outbound internet enabled)"));
    assert!(stdout.contains("SSH agent forwarding: disabled"));
    assert!(stdout.contains("Host credential mounts: none"));
    assert!(stdout.contains("Agent state: isolated policy (storage provisioning deferred)"));
    assert!(stdout.contains("Execution: dry-run only"));
    assert!(stdout.contains("program: \"container\""));
    assert!(stdout.contains("\"create\""));
    assert!(stdout.contains("\"--read-only\""));
    assert!(stdout.contains("\"--mount\""));
}

#[test]
fn refuses_execution_until_an_executor_exists() {
    let path = fixture("valid/default.toml");
    let output = run(&["run", "--profile", path.to_str().expect("UTF-8 path")]);
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("container execution is not implemented"));
    assert!(stderr.contains("--dry-run"));
}

#[test]
fn reports_a_missing_workspace_during_preflight() {
    let path = fixture("preflight/missing-workspace.toml");

    let output = run(&[
        "run",
        "--profile",
        path.to_str().expect("UTF-8 path"),
        "--dry-run",
    ]);
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("failed to resolve workspace path"));
    assert!(stderr.contains("does-not-exist"));
}
