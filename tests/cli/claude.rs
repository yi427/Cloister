use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use tempfile::tempdir;

const RELEASE_IMAGE: &str = concat!("ghcr.io/yi427/cloister:", env!("CARGO_PKG_VERSION"));

fn write_default_profile(home: &Path) -> PathBuf {
    let config = home.join(".config/cloister/profile.toml");
    let host_command = home.join("host-bin/xcodebuild");
    fs::create_dir_all(config.parent().expect("config should have a parent"))
        .expect("config directory should be created");
    fs::create_dir_all(
        host_command
            .parent()
            .expect("host command should have a parent"),
    )
    .expect("host command directory should be created");
    fs::write(&host_command, "#!/bin/sh\nexit 0\n").expect("fake host command should be written");
    fs::set_permissions(&host_command, fs::Permissions::from_mode(0o755))
        .expect("fake host command should be executable");
    let profile =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/profile.toml"))
            .expect("example Profile should be readable")
            .replace("/usr/bin/xcodebuild", &host_command.display().to_string());
    fs::write(config, profile).expect("default profile should be written");
    host_command
}

fn run(home: &Path, current_directory: &Path, path: Option<&Path>, arguments: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cloister"));
    command
        .args(arguments)
        .current_dir(current_directory)
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_STATE_HOME");
    if let Some(path) = path {
        command.env("PATH", path);
    }
    command.output().expect("Cloister binary should start")
}

#[test]
fn exposes_the_shared_agent_command_help() {
    let directory = tempdir().expect("temporary directory should exist");

    let output = run(
        directory.path(),
        directory.path(),
        None,
        &["claude", "--help"],
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("Path to a Profile V6 TOML file"));
    assert!(stdout.contains("Print the runtime plan without starting the agent"));
    assert!(stdout.contains("Arguments passed directly to the agent"));
}

#[test]
fn dry_run_uses_separate_claude_state_and_transient_host_bridge_config() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    fs::create_dir_all(&project).expect("project should be created");
    write_default_profile(&home);

    let output = run(&home, &project, None, &["claude", "--dry-run"]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    let state = home.join(".local/share/cloister/agents/claude");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(stdout.contains(&format!(
        "Claude state: {} -> /cloister/agents/claude (shared across projects)",
        state.display()
    )));
    assert!(stdout.contains("\"CLAUDE_CONFIG_DIR=/cloister/agents/claude\""));
    assert!(stdout.contains("\"--add-dir\""));
    assert!(stdout.contains("/usr/local/share/cloister/claude-skill-root"));
    assert!(stdout.contains("\"--mcp-config\""));
    assert!(stdout.contains("host.container.internal:17834/mcp"));
    assert!(stdout.contains("Bearer ${CLOISTER_HOST_BRIDGE_TOKEN}"));
    assert!(stdout.contains("alwaysLoad"));
    assert!(stdout.contains("Host capabilities: host.list_commands, host.exec"));
    assert!(stdout.contains(&format!(
        "Host audit log: {} (JSONL, owner-only, 20 MiB total)",
        home.join(".local/state/cloister/audit/host-exec.jsonl")
            .display()
    )));
    assert!(stdout.contains("Host policy: inherit-all environment, 1 allowed command(s)"));
    assert!(stdout.contains("Host Skill: host-exec (canonical read-only image source)"));
    assert!(!stdout.contains("--strict-mcp-config"));
    assert!(!state.exists(), "dry-run must not create Claude state");
    assert!(
        !home.join(".local/state/cloister").exists(),
        "dry-run must not create audit state"
    );
}

#[test]
fn can_disable_the_default_host_bridge() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    fs::create_dir_all(&project).expect("project should be created");
    write_default_profile(&home);

    let output = run(
        &home,
        &project,
        None,
        &["claude", "--no-host-bridge", "--dry-run"],
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("Host bridge: disabled"));
    assert!(!stdout.contains("CLOISTER_HOST_BRIDGE_TOKEN"));
    assert!(!stdout.contains("--mcp-config"));
    assert!(!stdout.contains("--add-dir"));
    assert!(!stdout.contains("Host Skill: host-exec"));
}

#[test]
fn refuses_to_shadow_an_existing_shared_claude_host_skill() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let skill = home.join(".local/share/cloister/agents/claude/skills/host-exec");
    fs::create_dir_all(&skill).expect("existing Skill directory should be created");
    fs::write(skill.join("SKILL.md"), "user-owned\n").expect("existing Skill should be written");
    fs::create_dir_all(&project).expect("project should be created");
    write_default_profile(&home);

    let output = run(&home, &project, None, &["claude", "--dry-run"]);
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("refusing to shadow or overwrite the existing Host Skill"));
    assert!(stderr.contains(&skill.display().to_string()));
    assert_eq!(
        fs::read_to_string(skill.join("SKILL.md")).expect("existing Skill should remain"),
        "user-owned\n"
    );

    let disabled = run(
        &home,
        &project,
        None,
        &["claude", "--no-host-bridge", "--dry-run"],
    );
    assert!(
        disabled.status.success(),
        "a disabled bridge must leave the user Skill usable"
    );
}

#[test]
fn passes_claude_arguments_directly_and_returns_the_runtime_exit_code() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let bin = directory.path().join("bin");
    let runtime = bin.join("container");
    fs::create_dir_all(&project).expect("project should be created");
    fs::create_dir_all(&bin).expect("bin directory should be created");
    fs::write(&runtime, "#!/bin/sh\nprintf '%s\\n' \"$@\"\nexit 9\n")
        .expect("fake container runtime should be written");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o755))
        .expect("fake runtime should be executable");
    let profile = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/profile.toml");

    let output = run(
        &home,
        &project,
        Some(&bin),
        &[
            "claude",
            "--profile",
            profile.to_str().expect("profile path should be UTF-8"),
            "--no-host-bridge",
            "--",
            "--version",
        ],
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert_eq!(output.status.code(), Some(9));
    assert!(output.stderr.is_empty());
    assert!(stdout.ends_with(&format!("--\n{RELEASE_IMAGE}\nclaude\n--version\n")));
    assert_eq!(
        fs::metadata(home.join(".local/share/cloister/agents/claude"))
            .expect("state metadata should be available")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}
