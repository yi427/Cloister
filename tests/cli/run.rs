use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use tempfile::tempdir;

fn fixture(relative_path: impl AsRef<Path>) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/profiles")
        .join(relative_path)
}

fn example(relative_path: impl AsRef<Path>) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
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
    assert!(stdout.contains("Lifecycle: run and remove after exit"));
    assert!(stdout.contains("program: \"container\""));
    assert!(stdout.contains("\"run\""));
    assert!(stdout.contains("\"--rm\""));
    assert!(stdout.contains("\"--read-only\""));
    assert!(stdout.contains("\"--mount\""));
}

#[cfg(unix)]
#[test]
fn executes_the_planned_runtime_and_returns_its_exit_code() {
    use std::os::unix::fs::PermissionsExt;

    let path = fixture("valid/default.toml");
    let directory = tempdir().expect("temporary runtime directory should exist");
    let runtime = directory.path().join("container");
    fs::write(&runtime, "#!/bin/sh\nprintf '%s\\n' \"$@\"\nexit 7\n")
        .expect("fake container runtime should be written");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o755))
        .expect("fake container runtime should be executable");

    let output = Command::new(env!("CARGO_BIN_EXE_cloister"))
        .args([
            "run",
            "--profile",
            path.to_str().expect("UTF-8 path"),
            "--",
            "/bin/sh",
            "-lc",
            "printf hello",
        ])
        .env("PATH", directory.path())
        .output()
        .expect("Cloister binary should start");
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert_eq!(output.status.code(), Some(7));
    assert!(output.stderr.is_empty());
    assert!(stdout.starts_with("run\n--rm\n"));
    assert!(stdout.contains("\ncloister/rust-node:dev\n/bin/sh\n-lc\nprintf hello\n"));
}

#[test]
fn reports_a_missing_container_runtime() {
    let path = fixture("valid/default.toml");
    let directory = tempdir().expect("empty PATH directory should exist");
    let output = Command::new(env!("CARGO_BIN_EXE_cloister"))
        .args(["run", "--profile", path.to_str().expect("UTF-8 path")])
        .env("PATH", directory.path())
        .output()
        .expect("Cloister binary should start");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("failed to start runtime"));
    assert!(stderr.contains("\"container\""));
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

#[test]
fn plans_the_directly_runnable_smoke_example() {
    let path = example("smoke.toml");
    let output = run(&[
        "run",
        "--profile",
        path.to_str().expect("UTF-8 path"),
        "--dry-run",
        "--",
        "/bin/sh",
    ]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .canonicalize()
        .expect("examples path should resolve");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("\"docker.io/library/debian:bookworm-slim\""));
    assert!(stdout.contains("\"root\""));
    assert!(stdout.contains(&format!(
        "\"type=bind,source={},target=/workspace\"",
        workspace.display()
    )));
}
