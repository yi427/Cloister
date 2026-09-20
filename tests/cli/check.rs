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
fn accepts_the_container_1_4_nested_server_version() {
    let directory = tempdir().unwrap();
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let bin = directory.path().join("bin");
    fs::create_dir_all(&project).unwrap();
    write_default_profile(&home);
    write_runtime(
        &bin,
        &HEALTHY_RUNTIME.replace(
            "\"apiServerVersion\":\"container-apiserver version 1.2.0\"",
            "\"server\":{\"appName\":\"container-apiserver\",\"version\":\"1.4.1\"}",
        ),
    );
    let output = run(&home, &project, Some(&bin), &["check"]);
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("[PASS] Runtime: container-apiserver version 1.4.1")
    );
}

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
    assert!(stdout.contains("[PASS] PF configuration:"));
    assert!(stdout.contains("[PASS] Host connectivity:"));
    assert!(stdout.ends_with("All required checks passed; 2 warning(s).\n"));
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
    assert!(stdout.ends_with("1 check(s) failed; 2 warning(s); 1 skipped.\n"));
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
    assert!(stdout.ends_with("All required checks passed; 3 warning(s).\n"));
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
    assert!(stdout.ends_with("1 check(s) failed; 7 skipped.\n"));
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
    assert!(stdout.ends_with("1 check(s) failed; 2 warning(s); 4 skipped.\n"));
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
    assert!(stdout.ends_with("2 check(s) failed; 2 warning(s); 2 skipped.\n"));
}

#[test]
fn fails_when_dns_exists_but_pf_configuration_is_disconnected_or_incomplete() {
    for (state, expected) in [
        ("disconnected", "does not invoke rdr-anchor"),
        ("missing-rule", "does not load the expected redirect"),
        ("invalid", "could not parse /etc/pf.conf"),
    ] {
        let directory = tempdir().unwrap();
        let home = directory.path().join("home");
        let bin = directory.path().join("bin");
        write_default_profile(&home);
        write_runtime(&bin, HEALTHY_RUNTIME);
        let before = fs::read(default_profile_path(&home)).unwrap();
        let output = run_with_environment(
            &home,
            directory.path(),
            Some(&bin),
            &["check"],
            &[("TEST_PF_CONFIG", state)],
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert_eq!(output.status.code(), Some(1), "{stdout}");
        assert!(stdout.contains("[PASS] DNS:"));
        assert!(stdout.contains("[FAIL] PF configuration:"));
        assert!(stdout.contains(expected), "{stdout}");
        assert!(stdout.contains("[SKIP] Host connectivity:"));
        assert!(stdout.contains("sudo container system dns delete host.container.internal && sudo container system dns create host.container.internal --localhost 203.0.113.113"));
        assert_eq!(fs::read(default_profile_path(&home)).unwrap(), before);
        assert!(!home.join(".local").exists());
    }
}

#[test]
fn checks_forwarding_for_loopback_proxy_even_when_host_exec_is_disabled() {
    for (proxy, required) in [
        ("http://localhost:3080", true),
        ("http://proxy.example:8080", false),
    ] {
        let directory = tempdir().unwrap();
        let home = directory.path().join("home");
        let bin = directory.path().join("bin");
        write_default_profile(&home);
        let path = default_profile_path(&home);
        let source = fs::read_to_string(&path)
            .unwrap()
            .replace("enabled = true", "enabled = false")
            .replace("proxy = \"disabled\"", "proxy = \"inherit\"");
        fs::write(path, source).unwrap();
        write_runtime(&bin, HEALTHY_RUNTIME);
        let output = run_with_environment(
            &home,
            directory.path(),
            Some(&bin),
            &["check"],
            &[("HTTPS_PROXY", proxy), ("TEST_PF_CONFIG", "disconnected")],
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        if required {
            assert_eq!(output.status.code(), Some(1), "{stdout}");
            assert!(stdout.contains("[FAIL] PF configuration:"));
        } else {
            assert!(output.status.success(), "{stdout}");
            assert!(stdout.contains("[SKIP] PF configuration:"));
        }
    }
}

#[test]
fn connectivity_requires_a_matching_host_request_and_successful_container_exit() {
    for mode in ["ready", "fail", "no-contact", "wrong-path"] {
        let directory = tempdir().unwrap();
        let home = directory.path().join("home");
        let bin = directory.path().join("bin");
        let record = directory.path().join("probe.log");
        write_default_profile(&home);
        write_runtime(&bin, HEALTHY_RUNTIME);
        let output = run_with_environment(
            &home,
            directory.path(),
            Some(&bin),
            &["check"],
            &[
                ("TEST_PROBE_MODE", mode),
                ("TEST_PROBE_RECORD", record.to_str().unwrap()),
            ],
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("[PASS] PF configuration:"));
        if mode == "ready" {
            assert!(output.status.success(), "{stdout}");
            assert!(stdout.contains("[PASS] Host connectivity:"));
            assert!(!stdout.contains("[WARN] PF runtime:"));
        } else {
            assert_eq!(output.status.code(), Some(1), "{stdout}");
            assert!(stdout.contains("[FAIL] Host connectivity:"));
            assert!(!stdout.contains("[PASS] Host connectivity:"));
        }
        if mode == "no-contact" {
            assert!(stdout.contains("received no matching request"));
        }
        assert_probe_listener_released(&fs::read_to_string(record).unwrap());
    }
}

#[test]
fn connectivity_skips_launch_when_the_image_is_missing() {
    let directory = tempdir().unwrap();
    let home = directory.path().join("home");
    let bin = directory.path().join("bin");
    let record = directory.path().join("probe.log");
    write_default_profile(&home);
    let runtime = HEALTHY_RUNTIME.replace(
        "printf '%s\\n' '[{\"variants\":[{\"platform\":{\"architecture\":\"arm64\",\"os\":\"linux\"}}]}]'",
        "exit 1",
    );
    assert_ne!(runtime, HEALTHY_RUNTIME);
    write_runtime(&bin, &runtime);
    let output = run_with_environment(
        &home,
        directory.path(),
        Some(&bin),
        &["check"],
        &[("TEST_PROBE_RECORD", record.to_str().unwrap())],
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout.contains("[SKIP] Host connectivity: a compatible local image is required"));
    assert!(
        !record.exists(),
        "must not launch/pull a missing probe image"
    );
}

#[test]
fn interrupted_and_timed_out_probes_remove_the_named_container_and_release_listener() {
    use std::{
        process::Stdio,
        time::{Duration, Instant},
    };
    for interrupt in [true, false] {
        let directory = tempdir().unwrap();
        let home = directory.path().join("home");
        let bin = directory.path().join("bin");
        let record = directory.path().join("probe.log");
        write_default_profile(&home);
        write_runtime(&bin, HEALTHY_RUNTIME);
        let start = Instant::now();
        let mut child = Command::new(env!("CARGO_BIN_EXE_cloister"))
            .arg("check")
            .env_clear()
            .env("HOME", &home)
            .env("PATH", &bin)
            .env("TEST_PROBE_MODE", "stall")
            .env("TEST_PROBE_RECORD", &record)
            .current_dir(directory.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        while fs::read_to_string(&record).unwrap_or_default().is_empty() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "probe did not start"
            );
            assert!(
                child.try_wait().unwrap().is_none(),
                "check exited before probe launch"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        if interrupt {
            // SAFETY: signal only this test's live child.
            assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
        }
        let output = child.wait_with_output().unwrap();
        assert!(start.elapsed() < Duration::from_secs(35));
        assert!(!output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.contains(if interrupt {
                "interrupted"
            } else {
                "exceeded its 20s deadline"
            }),
            "{stdout}"
        );
        let log = fs::read_to_string(&record).unwrap();
        let args: Vec<_> = log.lines().next().unwrap().split_whitespace().collect();
        let name = args.windows(2).find(|pair| pair[0] == "--name").unwrap()[1];
        assert!(log.contains(&format!("delete --force {name}")));
        assert_probe_listener_released(&log);
    }
}

fn assert_probe_listener_released(log: &str) {
    let endpoint = log
        .split_whitespace()
        .find(|part| part.starts_with("http://host.container.internal:"))
        .unwrap();
    let port = url::Url::parse(endpoint).unwrap().port().unwrap();
    std::net::TcpListener::bind(("127.0.0.1", port)).expect("probe listener must be released");
}

fn write_runtime(bin: &Path, contents: &str) {
    fs::create_dir_all(bin).expect("bin directory should be created");
    super::write_fake_pfctl(bin);
    let runtime = bin.join("container");
    fs::write(&runtime, super::with_connectivity_probe(contents))
        .expect("fake container runtime should be written");
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
    command
        .env_remove("TEST_PF_CONFIG")
        .env_remove("TEST_PROBE_MODE")
        .env_remove("TEST_PROBE_RECORD");
    for (name, value) in environment {
        command.env(name, value);
    }
    command.output().expect("Cloister binary should start")
}
