use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
};

use cloister::profile::{HostExecAllowProfile, HostExecArguments, Profile, load_profile};
use tempfile::tempdir;

const RELEASE_IMAGE: &str = concat!("ghcr.io/yi427/cloister:", env!("CARGO_PKG_VERSION"));
const HEALTHY_RUNTIME: &str = concat!(
    r#"#!/bin/sh
case "$1:$2:$3" in
  system:status:--format)
    printf '%s\n' '{"apiServerVersion":"container-apiserver version 1.2.0","status":"running"}'
    ;;
  image:inspect:ghcr.io/yi427/cloister:"#,
    env!("CARGO_PKG_VERSION"),
    r#")
    printf '%s\n' '[{"variants":[{"platform":{"architecture":"arm64","os":"linux"}}]}]'
    ;;
  system:dns:list)
    printf '%s\n' '["host.container.internal"]'
    ;;
  *)
    exit 90
    ;;
esac
"#,
);
const ANY_IMAGE_RUNTIME: &str = r#"#!/bin/sh
case "$1:$2:$3" in
  system:status:--format)
    printf '%s\n' '{"apiServerVersion":"container-apiserver version 1.2.0","status":"running"}'
    ;;
  image:inspect:*)
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
    assert!(stdout.contains("[PASS] Profile: 'default'"));
    assert!(stdout.contains("[PASS] Guest proxy: disabled by Profile"));
    assert!(stdout.contains(&format!(
        "[PASS] Host audit: {} (not created yet; JSONL, owner-only, 20 MiB total)",
        home.join(".local/state/cloister/audit/host-exec.jsonl")
            .display()
    )));
    assert!(
        stdout
            .contains("[PASS] Host policy: enabled, environment inherit-all, 0 allowed command(s)")
    );
    assert!(stdout.contains(&format!(
        "[PASS] Image compatibility: CLI {} matches official image {}",
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION")
    )));
    assert!(stdout.contains("[PASS] Runtime: container-apiserver version 1.2.0"));
    assert!(stdout.contains(&format!("[PASS] Image: '{RELEASE_IMAGE}' (linux/arm64)")));
    assert!(stdout.contains("[PASS] DNS: 'host.container.internal' is configured"));
    assert!(stdout.ends_with("All checks passed.\n"));
    assert!(
        !home.join(".local/share/cloister/agents/codex").exists(),
        "check must not create agent state"
    );
    assert!(
        !home.join(".local/state/cloister").exists(),
        "check must not create audit state"
    );
}

#[test]
fn rejects_an_official_release_image_that_does_not_match_the_cli() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let bin = directory.path().join("bin");
    let profile = default_profile_path(&home);
    let old_image = "ghcr.io/yi427/cloister:0.0.0";
    fs::create_dir_all(&project).expect("project should be created");
    write_profile(&profile, None);
    set_profile_image(&profile, old_image);
    write_runtime(&bin, ANY_IMAGE_RUNTIME);

    let output = run(&home, &project, Some(&bin), &["check"]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(stdout.contains("[FAIL] Image compatibility: official image version mismatch"));
    assert!(stdout.contains("run 'cloister profile upgrade --dry-run'"));
    assert!(stdout.contains(&format!("[PASS] Image: '{old_image}' (linux/arm64)")));
    assert!(stdout.ends_with("1 check(s) failed; 0 skipped.\n"));
}

#[test]
fn allows_an_immutable_testing_image_with_a_warning() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let bin = directory.path().join("bin");
    let profile = default_profile_path(&home);
    let revision = "0123456789abcdef0123456789abcdef01234567";
    let testing_image = format!("ghcr.io/yi427/cloister:sha-{revision}");
    fs::create_dir_all(&project).expect("project should be created");
    write_profile(&profile, None);
    set_profile_image(&profile, &testing_image);
    write_runtime(&bin, ANY_IMAGE_RUNTIME);

    let output = run(&home, &project, Some(&bin), &["check"]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("[WARN] Image compatibility: immutable testing image"));
    assert!(stdout.contains(revision));
    assert!(stdout.contains("release compatibility is not guaranteed"));
    assert!(stdout.ends_with("All required checks passed; 1 warning(s).\n"));
}

#[test]
fn rejects_a_moving_official_image_tag() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let bin = directory.path().join("bin");
    let profile = default_profile_path(&home);
    fs::create_dir_all(&project).expect("project should be created");
    write_profile(&profile, None);
    set_profile_image(&profile, "ghcr.io/yi427/cloister:main");
    write_runtime(&bin, ANY_IMAGE_RUNTIME);

    let output = run(&home, &project, Some(&bin), &["check"]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(stdout.contains("[FAIL] Image compatibility: official image tag 'main' is mutable"));
    assert!(stdout.contains("immutable sha-<full-commit> testing image"));
}

#[test]
fn rejects_unsafe_existing_audit_permissions_without_repairing_them() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let bin = directory.path().join("bin");
    let audit_root = home.join(".local/state/cloister");
    fs::create_dir_all(&project).expect("project should be created");
    fs::create_dir_all(&audit_root).expect("audit root should be created");
    fs::set_permissions(&audit_root, fs::Permissions::from_mode(0o755))
        .expect("unsafe permissions should be set");
    write_default_profile(&home);
    write_runtime(&bin, HEALTHY_RUNTIME);

    let output = run(&home, &project, Some(&bin), &["check"]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(stdout.contains("[FAIL] Host audit: unsafe Host Exec audit permissions"));
    assert!(stdout.contains("expected 0700, found 0755"));
    assert_eq!(
        fs::metadata(&audit_root)
            .expect("audit root should remain")
            .permissions()
            .mode()
            & 0o777,
        0o755,
        "check must not repair audit permissions"
    );
}

#[test]
fn accepts_an_explicit_profile_path() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let bin = directory.path().join("bin");
    let profile = home.join("explicit.toml");
    fs::create_dir_all(&project).expect("project should be created");
    write_profile(&profile, None);
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
    assert!(stdout.contains("[SKIP] Host policy: Profile is unavailable"));
    assert!(stdout.contains("[SKIP] Image compatibility: Profile is unavailable"));
    assert!(stdout.contains("[PASS] Runtime:"));
    assert!(stdout.contains("[SKIP] Image: Profile is unavailable"));
    assert!(stdout.contains("[PASS] DNS:"));
    assert!(stdout.ends_with("1 check(s) failed; 5 skipped.\n"));
}

#[test]
fn reports_an_inherited_proxy_without_exposing_its_value() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let bin = directory.path().join("bin");
    let profile = home.join("proxy.toml");
    let secret = "secret-proxy-password";
    fs::create_dir_all(&project).expect("project should be created");
    write_profile(&profile, None);
    let source = fs::read_to_string(&profile)
        .expect("Profile should be readable")
        .replace("proxy = \"disabled\"", "proxy = \"inherit\"");
    fs::write(&profile, source).expect("proxy Profile should be written");
    write_runtime(&bin, HEALTHY_RUNTIME);

    let proxy_value = format!("http://user:{secret}@127.0.0.1:3080");
    let output = run_with_environment(
        &home,
        &project,
        Some(&bin),
        &[
            "check",
            "--profile",
            profile.to_str().expect("profile path should be UTF-8"),
        ],
        &[("HTTPS_PROXY", &proxy_value)],
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains(
        "[PASS] Guest proxy: inherit from HTTPS_PROXY (loopback mapped to host.container.internal; value redacted)"
    ));
    assert!(!stdout.contains(secret));
}

#[test]
fn reports_declared_and_resolved_host_command_paths() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let bin = directory.path().join("bin");
    let target = directory.path().join("tool-target");
    let declared = directory.path().join("tool");
    fs::create_dir_all(&project).expect("project should be created");
    write_executable(&target);
    symlink(&target, &declared).expect("tool symlink should be created");
    write_profile(
        &default_profile_path(&home),
        Some(("tool", declared.clone())),
    );
    write_runtime(&bin, HEALTHY_RUNTIME);

    let output = run(&home, &project, Some(&bin), &["check"]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert!(output.status.success());
    let resolved_target = fs::canonicalize(&target).expect("tool target should canonicalize");
    assert!(stdout.contains(&format!(
        "[PASS] Host command: 'tool': declared '{}', resolved '{}'",
        declared.display(),
        resolved_target.display()
    )));
}

#[test]
fn suggests_a_current_path_match_without_rewriting_the_profile() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let bin = directory.path().join("bin");
    let profile_path = default_profile_path(&home);
    let missing = directory.path().join("old-bin/tool");
    let replacement = bin.join("tool");
    fs::create_dir_all(&project).expect("project should be created");
    write_profile(&profile_path, Some(("tool", missing.clone())));
    write_runtime(&bin, HEALTHY_RUNTIME);
    write_executable(&replacement);
    let original = fs::read(&profile_path).expect("Profile should be readable");

    let output = run(&home, &project, Some(&bin), &["check"]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(stdout.contains("[FAIL] Host command: 'tool': failed to resolve host executable"));
    assert!(stdout.contains(&format!(
        "Current PATH finds 'tool' at '{}'",
        replacement.display()
    )));
    assert!(stdout.contains("update this Profile entry explicitly"));
    assert_eq!(
        fs::read(&profile_path).expect("Profile should remain readable"),
        original,
        "check must not rewrite stale command paths"
    );
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
    write_profile(&default_profile_path(home), None);
}

fn default_profile_path(home: &Path) -> PathBuf {
    home.join(".config/cloister/profile.toml")
}

fn write_profile(path: &Path, command: Option<(&str, PathBuf)>) {
    let mut profile: Profile =
        load_profile(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/profile.toml"))
            .expect("example Profile should load");
    profile.host.exec.allow.clear();
    if let Some((name, executable)) = command {
        profile.host.exec.allow.push(HostExecAllowProfile {
            name: name.to_owned(),
            executable,
            description: format!("Run {name}"),
            arguments: HostExecArguments::Any,
        });
    }

    fs::create_dir_all(path.parent().expect("Profile should have a parent"))
        .expect("config directory should be created");
    let source = toml::to_string_pretty(&profile).expect("Profile should serialize");
    fs::write(path, source).expect("Profile should be written");
}

fn set_profile_image(path: &Path, reference: &str) {
    let mut profile = load_profile(path).expect("Profile should load");
    profile.image.reference = reference.to_owned();
    fs::write(
        path,
        toml::to_string_pretty(&profile).expect("Profile should serialize"),
    )
    .expect("Profile should be updated");
}

fn write_executable(path: &Path) {
    fs::create_dir_all(path.parent().expect("executable should have a parent"))
        .expect("executable parent should be created");
    fs::write(path, "test executable\n").expect("executable should be written");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .expect("executable permissions should be set");
}

fn run(home: &Path, current_directory: &Path, path: Option<&Path>, arguments: &[&str]) -> Output {
    run_with_environment(home, current_directory, path, arguments, &[])
}

fn run_with_environment(
    home: &Path,
    current_directory: &Path,
    path: Option<&Path>,
    arguments: &[&str],
    environment: &[(&str, &str)],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cloister"));
    command
        .args(arguments)
        .current_dir(current_directory)
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_STATE_HOME");
    for name in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
    ] {
        command.env_remove(name);
    }
    if let Some(path) = path {
        command.env("PATH", path);
    }
    for (name, value) in environment {
        command.env(name, value);
    }
    command.output().expect("Cloister binary should start")
}
