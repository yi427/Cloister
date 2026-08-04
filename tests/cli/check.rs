use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
};

use tempfile::tempdir;

const HEALTHY_RUNTIME: &str = r#"#!/bin/sh
case "$1:$2:$3" in
  system:status:--format)
    printf '%s\n' '{"apiServerVersion":"container-apiserver version 1.2.0","status":"running"}'
    ;;
  image:inspect:cloister:dev)
    printf '%s\n' '[{"variants":[{"platform":{"architecture":"arm64","os":"linux"}}]}]'
    ;;
  system:dns:list)
    printf '%s\n' '["host.container.internal"]'
    ;;
  *)
    exit 90
    ;;
esac
"#;

#[test]
fn reports_a_ready_default_environment_without_writing_state() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let bin = directory.path().join("bin");
    fs::create_dir_all(&project).expect("project should be created");
    write_default_profile(&home);
    write_runtime(&bin, HEALTHY_RUNTIME);

    let output = run(&home, &project, Some(&bin), &["check"]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("[PASS] Profile: 'codex-default'"));
    assert!(stdout.contains("[PASS] Runtime: container-apiserver version 1.2.0"));
    assert!(stdout.contains("[PASS] Image: 'cloister:dev' (linux/arm64)"));
    assert!(stdout.contains("[PASS] DNS: 'host.container.internal' is configured"));
    assert!(stdout.ends_with("All checks passed.\n"));
    assert!(
        !home.join(".local/share/cloister/agents/codex").exists(),
        "check must not create agent state"
    );
}

#[test]
fn accepts_an_explicit_profile_path() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let bin = directory.path().join("bin");
    let profile = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/codex.toml");
    fs::create_dir_all(&project).expect("project should be created");
    write_runtime(&bin, HEALTHY_RUNTIME);

    let output = run(
        &home,
        &project,
        Some(&bin),
        &[
            "check",
            "--profile",
            profile.to_str().expect("profile path should be UTF-8"),
        ],
    );

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
}

#[test]
fn continues_with_independent_checks_when_the_profile_is_missing() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let bin = directory.path().join("bin");
    fs::create_dir_all(&project).expect("project should be created");
    write_runtime(&bin, HEALTHY_RUNTIME);

    let output = run(&home, &project, Some(&bin), &["check"]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("[FAIL] Profile: failed to read profile"));
    assert!(stdout.contains("[PASS] Runtime:"));
    assert!(stdout.contains("[SKIP] Image: Profile is unavailable"));
    assert!(stdout.contains("[PASS] DNS:"));
    assert!(stdout.ends_with("1 check(s) failed; 1 skipped.\n"));
}

#[test]
fn skips_runtime_dependent_checks_when_container_is_unavailable() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let empty_bin = directory.path().join("empty-bin");
    fs::create_dir_all(&project).expect("project should be created");
    fs::create_dir_all(&empty_bin).expect("empty bin should be created");
    write_default_profile(&home);

    let output = run(&home, &project, Some(&empty_bin), &["check"]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("[PASS] Profile:"));
    assert!(stdout.contains("[FAIL] Runtime: failed to start 'container'"));
    assert!(stdout.contains("[SKIP] Image: runtime is unavailable"));
    assert!(stdout.contains("[SKIP] DNS: runtime is unavailable"));
    assert!(stdout.ends_with("1 check(s) failed; 2 skipped.\n"));
}

#[test]
fn reports_a_missing_image_and_dns_mapping_together() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let bin = directory.path().join("bin");
    fs::create_dir_all(&project).expect("project should be created");
    write_default_profile(&home);
    write_runtime(
        &bin,
        r#"#!/bin/sh
case "$1:$2:$3" in
  system:status:--format)
    printf '%s\n' '{"apiServerVersion":"container-apiserver version 1.2.0","status":"running"}'
    ;;
  image:inspect:*)
    printf '%s\n' 'image not found' >&2
    exit 1
    ;;
  system:dns:list)
    printf '%s\n' '[]'
    ;;
  *)
    exit 90
    ;;
esac
"#,
    );

    let output = run(&home, &project, Some(&bin), &["check"]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("[FAIL] Image:"));
    assert!(stdout.contains("image not found"));
    assert!(stdout.contains("[FAIL] DNS: 'host.container.internal' is not configured"));
    assert!(stdout.ends_with("2 check(s) failed; 0 skipped.\n"));
}

fn write_runtime(bin: &Path, contents: &str) {
    fs::create_dir_all(bin).expect("bin directory should be created");
    let runtime = bin.join("container");
    fs::write(&runtime, contents).expect("fake container runtime should be written");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o755))
        .expect("fake runtime should be executable");
}

fn write_default_profile(home: &Path) {
    let config = home.join(".config/cloister/profile.toml");
    fs::create_dir_all(config.parent().expect("config should have a parent"))
        .expect("config directory should be created");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/codex.toml"),
        config,
    )
    .expect("default profile should be written");
}

fn run(home: &Path, current_directory: &Path, path: Option<&Path>, arguments: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cloister"));
    command
        .args(arguments)
        .current_dir(current_directory)
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME");
    if let Some(path) = path {
        command.env("PATH", path);
    }
    command.output().expect("Cloister binary should start")
}
