use std::{
    fs,
    io::Write,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    process::{Command, Output, Stdio},
};

use cloister::profile::{
    AgentState, HostExecArguments, HostExecEnvironmentMode, NetworkProxyMode, load_profile,
};
use tempfile::tempdir;

const RELEASE_IMAGE: &str = concat!("ghcr.io/yi427/cloister:", env!("CARGO_PKG_VERSION"));
const OLD_IMAGE: &str = "ghcr.io/yi427/cloister:0.0.0";

const FAKE_CONTAINER: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$CLOISTER_TEST_COMMAND_LOG"

if [ "$1:$2:$3:$4" = "system:status:--format:json" ]; then
  if [ -f "$CLOISTER_TEST_RUNTIME_STATE" ]; then
    printf '%s\n' '{"apiServerVersion":"container-apiserver version 1.2.0","status":"running"}'
  else
    printf '%s\n' '{"apiServerVersion":"container-apiserver version 1.2.0","status":"stopped"}'
  fi
  exit 0
fi

if [ "$1:$2" = "system:start" ]; then
  : > "$CLOISTER_TEST_RUNTIME_STATE"
  exit 0
fi

if [ "$1:$2" = "image:inspect" ]; then
  if [ -f "$CLOISTER_TEST_IMAGE_STATE" ]; then
    printf '%s\n' '[{"variants":[{"platform":{"architecture":"arm64","os":"linux"}}]}]'
    exit 0
  fi
  printf '%s\n' 'image not found' >&2
  exit 1
fi

if [ "$1:$2:$3:$4" = "image:pull:--arch:arm64" ]; then
  : > "$CLOISTER_TEST_IMAGE_STATE"
  exit 0
fi

if [ "$1:$2:$3:$4:$5" = "system:dns:list:--format:json" ]; then
  if [ -f "$CLOISTER_TEST_DNS_STATE" ]; then
    printf '%s\n' '["host.container.internal"]'
  else
    printf '%s\n' '[]'
  fi
  exit 0
fi

exit 90
"#;

const FAKE_SUDO: &str = r#"#!/bin/sh
printf 'sudo %s\n' "$*" >> "$CLOISTER_TEST_COMMAND_LOG"
if [ "$1:$2:$3:$4:$5:$6:$7" = "container:system:dns:create:host.container.internal:--localhost:203.0.113.113" ]; then
  : > "$CLOISTER_TEST_DNS_STATE"
  exit 0
fi
exit 90
"#;

#[test]
fn creates_a_versioned_profile_and_prepares_every_missing_component() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let bin = directory.path().join("bin");
    let state = directory.path().join("state");
    let runtime_state = state.join("runtime");
    let image_state = state.join("image");
    let dns_state = state.join("dns");
    let command_log = state.join("commands.log");
    fs::create_dir_all(&state).expect("state directory should be created");
    write_executable(&bin.join("container"), FAKE_CONTAINER);
    write_executable(&bin.join("sudo"), FAKE_SUDO);

    let output = run(
        &home,
        &bin,
        "\n\n\n\n\n\n\n\n\ny\n",
        &[
            ("CLOISTER_TEST_RUNTIME_STATE", runtime_state.as_path()),
            ("CLOISTER_TEST_IMAGE_STATE", image_state.as_path()),
            ("CLOISTER_TEST_DNS_STATE", dns_state.as_path()),
            ("CLOISTER_TEST_COMMAND_LOG", command_log.as_path()),
        ],
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    let profile_path = home.join(".config/cloister/profile.toml");
    let profile = load_profile(&profile_path).expect("generated Profile should load");
    let profile_source = fs::read_to_string(&profile_path).expect("Profile should be readable");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(profile.name, "default");
    assert_eq!(profile.image.reference, RELEASE_IMAGE);
    assert_eq!(profile.guest.cpus.get(), 4);
    assert_eq!(profile.guest.memory.to_string(), "8G");
    assert_eq!(profile.agent.state, AgentState::Shared);
    assert_eq!(profile.network.proxy, NetworkProxyMode::Disabled);
    assert!(profile.host.exec.enabled);
    assert_eq!(
        profile.host.exec.environment.mode,
        HostExecEnvironmentMode::InheritAll
    );
    assert!(profile.host.exec.allow.is_empty());
    assert!(profile_source.contains("schema_version = 6"));
    assert!(profile_source.contains("proxy = \"disabled\""));
    assert!(profile_source.contains("[agent]"));
    assert!(profile_source.contains("[host.exec]"));
    assert!(profile_source.contains("mode = \"inherit-all\""));
    assert!(!profile_source.contains("[codex]"));
    assert_eq!(
        fs::metadata(&profile_path)
            .expect("Profile metadata should exist")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(stdout.contains("Created Profile at"));
    assert!(stdout.contains(
        "Persist agent credentials, settings, and session history across projects? [Y/n]:"
    ));
    assert!(stdout.contains("Host HTTP proxy: not detected; guest proxy inheritance is disabled."));
    assert!(
        stdout.contains(
            "Agent state: shared (separate per agent; may contain credentials and history)"
        )
    );
    assert!(
        stdout.contains("Host exec policy: enabled, inherit-all environment, 0 allowed command(s)")
    );
    assert!(stdout.contains("Running: container system start"));
    assert!(stdout.contains(&format!(
        "Running: container image pull --arch arm64 {RELEASE_IMAGE}"
    )));
    assert!(stdout.contains(
        "Running: sudo container system dns create host.container.internal --localhost 203.0.113.113"
    ));
    assert!(stdout.contains("[PASS] Profile: 'default'"));
    assert!(stdout.contains(&format!("[PASS] Image: '{RELEASE_IMAGE}' (linux/arm64)")));
    assert!(stdout.ends_with("All checks passed.\n"));

    let commands = fs::read_to_string(command_log).expect("command log should be readable");
    assert!(commands.contains("system start"));
    assert!(commands.contains(&format!("image pull --arch arm64 {RELEASE_IMAGE}")));
    assert!(commands.contains(
        "sudo container system dns create host.container.internal --localhost 203.0.113.113"
    ));
}

#[test]
fn defaults_to_inheriting_a_detected_host_proxy() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let empty_bin = directory.path().join("empty-bin");
    fs::create_dir_all(&empty_bin).expect("empty bin should be created");
    let mut command = init_command(&home, &empty_bin);
    command
        .env("HTTPS_PROXY", "http://127.0.0.1:3080")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("Cloister binary should start");
    child
        .stdin
        .take()
        .expect("stdin pipe should exist")
        .write_all(b"\n\n\n\n\n\n\ny\n")
        .expect("interactive input should be written");
    let output = child.wait_with_output().expect("Cloister should exit");
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    let profile = load_profile(home.join(".config/cloister/profile.toml"))
        .expect("generated Profile should load");

    assert_eq!(profile.network.proxy, NetworkProxyMode::Inherit);
    assert!(stdout.contains("Host HTTP proxy detected via HTTPS_PROXY."));
    assert!(stdout.contains("Inherit this proxy inside Apple containers? [Y/n]:"));
    assert!(stdout.contains("Guest proxy: inherit host HTTP proxy at launch (values redacted)"));
    assert!(stdout.contains(
        "[PASS] Guest proxy: inherit from HTTPS_PROXY (loopback mapped to host.container.internal; value redacted)"
    ));
}

#[test]
fn can_disable_an_invalid_detected_host_proxy_without_printing_its_value() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let empty_bin = directory.path().join("empty-bin");
    fs::create_dir_all(&empty_bin).expect("empty bin should be created");
    let secret = "private-proxy-host";
    let mut command = init_command(&home, &empty_bin);
    command
        .env("HTTPS_PROXY", format!("socks5://{secret}:1080"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("Cloister binary should start");
    child
        .stdin
        .take()
        .expect("stdin pipe should exist")
        .write_all(b"\n\n\n\n\n\n\ny\n")
        .expect("interactive input should be written");
    let output = child.wait_with_output().expect("Cloister should exit");
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    let profile = load_profile(home.join(".config/cloister/profile.toml"))
        .expect("generated Profile should load");

    assert_eq!(profile.network.proxy, NetworkProxyMode::Disabled);
    assert!(stdout.contains("A host HTTP proxy variable was detected but cannot be inherited"));
    assert!(stdout.contains("Continue with guest proxy inheritance disabled? [Y/n]:"));
    assert!(!stdout.contains(secret));
}

#[test]
fn resolves_and_writes_explicit_host_commands() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let bin = directory.path().join("bin");
    let cargo = bin.join("cargo");
    let xcodebuild = bin.join("xcodebuild");
    write_executable(&cargo, "test executable\n");
    write_executable(&xcodebuild, "test executable\n");

    let output = run(&home, &bin, "\n\n\n\n\ncargo, xcodebuild\ny\n", &[]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    let profile_path = home.join(".config/cloister/profile.toml");
    let profile = load_profile(&profile_path).expect("generated Profile should load");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    assert_eq!(profile.host.exec.allow.len(), 2);
    assert_eq!(profile.host.exec.allow[0].name, "cargo");
    assert_eq!(profile.host.exec.allow[0].executable, cargo);
    assert_eq!(
        profile.host.exec.allow[0].description,
        "Run cargo on the macOS host"
    );
    assert_eq!(profile.host.exec.allow[0].arguments, HostExecArguments::Any);
    assert_eq!(profile.host.exec.allow[1].name, "xcodebuild");
    assert_eq!(profile.host.exec.allow[1].executable, xcodebuild);
    assert!(stdout.contains("Resolved host commands:"));
    assert!(stdout.contains(&format!("cargo: declared '{}'", cargo.display())));
    assert!(stdout.contains(&format!("xcodebuild: declared '{}'", xcodebuild.display())));
    assert!(
        stdout.contains("Host exec policy: enabled, inherit-all environment, 2 allowed command(s)")
    );
    assert!(stdout.contains("[PASS] Host command: 'cargo'"));
    assert!(stdout.contains("[PASS] Host command: 'xcodebuild'"));
    assert!(stdout.contains("[FAIL] Runtime: failed to start 'container'"));
}

#[test]
fn reprompts_for_duplicate_or_unresolved_host_commands() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let bin = directory.path().join("bin");
    write_executable(&bin.join("cargo"), "test executable\n");

    let output = run(
        &home,
        &bin,
        "\n\n\n\n\ncargo, cargo\nmissing\ncargo\nn\n",
        &[],
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        stdout
            .matches("Allowed host commands, comma-separated [none]:")
            .count(),
        3
    );
    assert!(
        stdout.contains("Could not add host commands: command 'cargo' was listed more than once")
    );
    assert!(stdout.contains(
        "Could not add host commands: host command 'missing' was not found in an absolute PATH directory"
    ));
    assert!(stdout.ends_with("No changes made.\n"));
    assert!(!home.join(".config/cloister/profile.toml").exists());
}

#[test]
fn can_create_only_the_profile_when_container_is_missing() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let empty_bin = directory.path().join("empty-bin");
    fs::create_dir_all(&empty_bin).expect("empty bin should be created");

    let output = run(&home, &empty_bin, "\n\n\n\n\n\ny\n", &[]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    let profile = home.join(".config/cloister/profile.toml");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    assert!(profile.exists());
    assert!(stdout.contains("Apple container was not found on PATH."));
    assert!(stdout.contains("Create the Profile without runtime setup? [y/N]:"));
    assert!(stdout.contains("[PASS] Profile: 'default'"));
    assert!(stdout.contains("[FAIL] Runtime: failed to start 'container'"));
    assert!(stdout.ends_with("1 check(s) failed; 2 skipped.\n"));
}

#[test]
fn creates_an_explicit_custom_image_profile_with_visible_warnings() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let empty_bin = directory.path().join("empty-bin");
    fs::create_dir_all(&empty_bin).expect("empty bin should be created");

    let output = run(&home, &empty_bin, "\ncloister:dev\n\n\n\n\ny\n", &[]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    let profile_path = home.join(".config/cloister/profile.toml");
    let profile = load_profile(&profile_path).expect("generated Profile should load");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    assert_eq!(profile.image.reference, "cloister:dev");
    assert!(stdout.contains("Image compatibility warning:"));
    assert!(stdout.contains("custom image 'cloister:dev'"));
    assert!(stdout.contains("[WARN] Image compatibility:"));
}

#[test]
fn refuses_an_incompatible_official_image_before_runtime_or_profile_writes() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let empty_bin = directory.path().join("empty-bin");
    fs::create_dir_all(&empty_bin).expect("empty bin should be created");

    let output = run(&home, &empty_bin, &format!("\n{OLD_IMAGE}\n\n\n\n\n"), &[]);
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr.contains("official image version mismatch"),
        "unexpected error: {stderr}"
    );
    assert!(stderr.contains("cloister profile upgrade --dry-run"));
    assert!(!home.join(".config/cloister/profile.toml").exists());
}

#[test]
fn cancellation_before_missing_runtime_setup_writes_nothing() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let empty_bin = directory.path().join("empty-bin");
    fs::create_dir_all(&empty_bin).expect("empty bin should be created");

    let output = run(&home, &empty_bin, "\n\n\n\n\n\n\n", &[]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(stdout.ends_with("No changes made.\n"));
    assert!(!home.join(".config/cloister/profile.toml").exists());
}

#[test]
fn refuses_an_existing_profile_before_prompting_or_calling_runtime() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let bin = directory.path().join("bin");
    let profile = home.join(".config/cloister/profile.toml");
    let command_log = directory.path().join("commands.log");
    fs::create_dir_all(profile.parent().expect("Profile should have a parent"))
        .expect("Profile directory should be created");
    fs::create_dir_all(&bin).expect("bin directory should be created");
    fs::write(&profile, "preserve this content\n").expect("existing Profile should be written");
    write_executable(
        &bin.join("container"),
        "#!/bin/sh\nprintf called > \"$CLOISTER_TEST_COMMAND_LOG\"\n",
    );

    let output = run(
        &home,
        &bin,
        "",
        &[("CLOISTER_TEST_COMMAND_LOG", command_log.as_path())],
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("refusing to overwrite it"));
    assert_eq!(
        fs::read_to_string(profile).expect("existing Profile should remain readable"),
        "preserve this content\n"
    );
    assert!(!command_log.exists(), "runtime must not be called");
}

#[test]
fn refuses_a_broken_symbolic_link_as_an_existing_target() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let bin = directory.path().join("bin");
    let profile = home.join(".config/cloister/profile.toml");
    fs::create_dir_all(profile.parent().expect("Profile should have a parent"))
        .expect("Profile directory should be created");
    fs::create_dir_all(&bin).expect("bin directory should be created");
    symlink(directory.path().join("missing-target"), &profile)
        .expect("broken Profile symlink should be created");

    let output = run(&home, &bin, "", &[]);
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr.contains("refusing to overwrite it"));
    assert!(
        fs::symlink_metadata(profile)
            .expect("symlink should remain")
            .file_type()
            .is_symlink()
    );
}

fn write_executable(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("executable should have a parent"))
        .expect("bin directory should be created");
    fs::write(path, contents).expect("fake executable should be written");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .expect("fake executable should be executable");
}

fn run(home: &Path, path: &Path, input: &str, environment: &[(&str, &Path)]) -> Output {
    let mut command = init_command(home, path);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in environment {
        command.env(name, value);
    }
    let mut child = command.spawn().expect("Cloister binary should start");
    child
        .stdin
        .take()
        .expect("stdin pipe should exist")
        .write_all(input.as_bytes())
        .expect("interactive input should be written");
    child.wait_with_output().expect("Cloister should exit")
}

fn init_command(home: &Path, path: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cloister"));
    command
        .arg("init")
        .env("HOME", home)
        .env("PATH", path)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME");
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
    command
}
