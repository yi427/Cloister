use std::{
    fs,
    net::TcpListener,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    process::{Command, Output},
};

use tempfile::tempdir;

fn write_default_profile(home: &Path) {
    let config = home.join(".config/cloister/profile.toml");
    fs::create_dir_all(config.parent().expect("config should have a parent"))
        .expect("config directory should be created");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/profile.toml"),
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

#[test]
fn keeps_the_shared_agent_command_help() {
    let directory = tempdir().expect("temporary directory should exist");

    let output = run(
        directory.path(),
        directory.path(),
        None,
        &["codex", "--help"],
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("Path to a Profile V5 TOML file"));
    assert!(stdout.contains("Print the runtime plan without starting the agent"));
    assert!(stdout.contains("Arguments passed directly to the agent"));
}

#[test]
fn default_profile_uses_current_directory_and_shared_codex_state() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    fs::create_dir_all(&project).expect("project should be created");
    write_default_profile(&home);

    let output = run(&home, &project, None, &["codex", "--dry-run"]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    let canonical_project = project.canonicalize().expect("project should resolve");
    let state = home.join(".local/share/cloister/agents/codex");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("Profile: default"));
    assert!(stdout.contains(&format!(
        "Workspace: {} -> /workspace (read-write)",
        canonical_project.display()
    )));
    assert!(stdout.contains(&format!(
        "Codex state: {} -> /cloister/agents/codex (shared across projects)",
        state.display()
    )));
    assert!(stdout.contains("\"CODEX_HOME=/cloister/agents/codex\""));
    assert!(stdout.contains("Host bridge: http://host.container.internal:17834/mcp"));
    assert!(stdout.contains("Host capabilities: host.list_commands, host.exec"));
    assert!(stdout.contains("Host policy: inherit-all environment, 1 allowed command(s)"));
    assert!(stdout.contains("xcodebuild: declared '/usr/bin/xcodebuild'"));
    assert!(stdout.contains("Host bridge token: ephemeral, forwarded, and redacted"));
    assert!(stdout.contains("\"CLOISTER_HOST_BRIDGE_TOKEN\""));
    assert!(stdout.contains("mcp_servers.cloister_host.required=true"));
    assert!(stdout.contains("enabled_tools=[\\\"host.list_commands\\\",\\\"host.exec\\\"]"));
    assert!(stdout.contains("default_tools_approval_mode=\\\"prompt\\\""));
    assert!(stdout.contains("\"cloister:dev\""));
    assert!(!state.exists(), "dry-run must not create agent state");
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
        &["codex", "--no-host-bridge", "--dry-run"],
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("Host bridge: disabled"));
    assert!(!stdout.contains("CLOISTER_HOST_BRIDGE_TOKEN"));
    assert!(!stdout.contains("mcp_servers.cloister_host"));
}

#[test]
fn profile_can_disable_the_host_bridge_without_inspecting_its_allowlist() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let profile_path = home.join("disabled-profile.toml");
    fs::create_dir_all(&project).expect("project should be created");
    fs::create_dir_all(&home).expect("home should be created");
    let profile =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/profile.toml"))
            .expect("example Profile should be readable")
            .replace("enabled = true", "enabled = false")
            .replace(
                "executable = \"/usr/bin/xcodebuild\"",
                "executable = \"/missing/xcodebuild\"",
            );
    fs::write(&profile_path, profile).expect("disabled Profile should be written");

    let output = run(
        &home,
        &project,
        None,
        &[
            "codex",
            "--profile",
            profile_path.to_str().expect("profile path should be UTF-8"),
            "--dry-run",
        ],
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("Host bridge: disabled"));
    assert!(!stdout.contains("CLOISTER_HOST_BRIDGE_TOKEN"));
}

#[test]
fn refuses_to_start_an_enabled_bridge_with_a_missing_executable() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let profile_path = home.join("missing-command.toml");
    fs::create_dir_all(&project).expect("project should be created");
    fs::create_dir_all(&home).expect("home should be created");
    let profile =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/profile.toml"))
            .expect("example Profile should be readable")
            .replace(
                "executable = \"/usr/bin/xcodebuild\"",
                "executable = \"/missing/xcodebuild\"",
            );
    fs::write(&profile_path, profile).expect("test Profile should be written");

    let output = run(
        &home,
        &project,
        None,
        &[
            "codex",
            "--profile",
            profile_path.to_str().expect("profile path should be UTF-8"),
            "--dry-run",
        ],
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("host command 'xcodebuild' is unavailable"));
    assert!(!home.join(".local/share/cloister/agents/codex").exists());
}

#[test]
fn loads_the_default_global_profile_when_present() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let config = home.join(".config/cloister/profile.toml");
    fs::create_dir_all(&project).expect("project should be created");
    fs::create_dir_all(config.parent().expect("config should have a parent"))
        .expect("config directory should be created");
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/profiles/valid/default.toml");
    let profile = fs::read_to_string(fixture)
        .expect("fixture should be readable")
        .replace("name = \"rust-default\"", "name = \"global-profile\"");
    fs::write(config, profile).expect("global profile should be written");

    let output = run(&home, &project, None, &["codex", "--dry-run"]);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("Profile: global-profile"));
    assert!(stdout.contains("Codex state: ephemeral"));
    assert!(!home.join(".local/share/cloister/agents/codex").exists());
}

#[test]
fn passes_codex_arguments_directly_and_returns_the_runtime_exit_code() {
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
            "codex",
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
    assert!(stdout.ends_with("--\ncloister:dev\ncodex\n--version\n"));
    assert_eq!(
        fs::metadata(home.join(".local/share/cloister/agents/codex"))
            .expect("state metadata should be available")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn starts_the_default_bridge_and_forwards_only_the_token_name() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let bin = directory.path().join("bin");
    let runtime = bin.join("container");
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("temporary listener should bind")
        .local_addr()
        .expect("temporary address should exist")
        .port();
    fs::create_dir_all(&project).expect("project should be created");
    fs::create_dir_all(&bin).expect("bin directory should be created");
    fs::write(
        &runtime,
        "#!/bin/sh\ntest -n \"$CLOISTER_HOST_BRIDGE_TOKEN\" || exit 8\nprintf '%s\\n' \"$@\"\n",
    )
    .expect("fake container runtime should be written");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o755))
        .expect("fake runtime should be executable");
    let profile = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/profile.toml");

    let output = run(
        &home,
        &project,
        Some(&bin),
        &[
            "codex",
            "--profile",
            profile.to_str().expect("profile path should be UTF-8"),
            "--host-bridge-port",
            &port.to_string(),
            "--",
            "--version",
        ],
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(stdout.contains(&format!(
        "Host bridge: http://host.container.internal:{port}/mcp"
    )));
    assert!(stdout.contains("CLOISTER_HOST_BRIDGE_TOKEN"));
    assert!(stdout.contains("mcp_servers.cloister_host.required=true"));
    assert!(stdout.contains("default_tools_approval_mode=\"prompt\""));
    TcpListener::bind(("127.0.0.1", port)).expect("bridge port should be released after Codex");
}

#[test]
fn rejects_a_symbolic_link_as_the_shared_state_directory() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    let state = home.join(".local/share/cloister/agents/codex");
    let target = directory.path().join("outside");
    fs::create_dir_all(&project).expect("project should be created");
    fs::create_dir_all(&target).expect("target should be created");
    write_default_profile(&home);
    fs::create_dir_all(state.parent().expect("state should have a parent"))
        .expect("state parent should be created");
    symlink(&target, &state).expect("state symlink should be created");

    let output = run(&home, &project, None, &["codex"]);
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("must be a real directory"));
}

#[test]
fn explicit_workspace_overrides_the_current_directory() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let current = directory.path().join("current");
    let selected = directory.path().join("selected");
    fs::create_dir_all(&current).expect("current directory should be created");
    fs::create_dir_all(&selected).expect("selected workspace should be created");
    write_default_profile(&home);

    let output = run(
        &home,
        &current,
        None,
        &[
            "codex",
            "--workspace",
            selected.to_str().expect("workspace path should be UTF-8"),
            "--dry-run",
        ],
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(stdout.contains(&format!(
        "Workspace: {} -> /workspace (read-write)",
        selected.canonicalize().unwrap().display()
    )));
}

#[test]
fn reports_a_missing_default_profile() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let project = directory.path().join("project");
    fs::create_dir_all(&project).expect("project should be created");

    let output = run(&home, &project, None, &["codex", "--dry-run"]);
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("failed to read profile"));
    assert!(stderr.contains(".config/cloister/profile.toml"));
}
