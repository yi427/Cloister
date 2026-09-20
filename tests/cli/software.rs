use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;

const DEFAULT_IMAGE: &str = concat!("ghcr.io/yi427/cloister:", env!("CARGO_PKG_VERSION"));

struct Fixture {
    directory: TempDir,
    home: PathBuf,
    bin: PathBuf,
    events: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("home");
        let bin = directory.path().join("bin");
        let events = directory.path().join("events");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(directory.path().join("project")).unwrap();
        fs::create_dir_all(&bin).unwrap();
        fs::write(&events, "").unwrap();
        let runtime = bin.join("container");
        fs::write(&runtime, include_str!("../fixtures/software-runtime.sh")).unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o755)).unwrap();
        let profile = home.join(".config/cloister/profile.toml");
        fs::create_dir_all(profile.parent().unwrap()).unwrap();
        fs::write(
            profile,
            include_str!("../../examples/profile.toml").replace(
                "executable = \"/usr/bin/xcodebuild\"",
                "executable = \"/bin/echo\"",
            ),
        )
        .unwrap();
        Self {
            directory,
            home,
            bin,
            events,
        }
    }
    fn command(&self, agent: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cloister"));
        command
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", &self.bin)
            .env("TEST_EVENTS", &self.events)
            .env("TEST_RECORDS", self.directory.path())
            .current_dir(self.directory.path().join("project"))
            .arg(agent)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command("codex").args(args).output().unwrap()
    }
    fn root(&self) -> PathBuf {
        self.home
            .join(".local/share/cloister/software/linux-arm64/codex")
    }
    fn inventory(&self) -> serde_json::Value {
        serde_json::from_slice(&fs::read(self.root().join("inventory.json")).unwrap()).unwrap()
    }
    fn events(&self) -> String {
        fs::read_to_string(&self.events).unwrap()
    }
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn first_use_installs_only_selected_agent_and_reuses_it_with_isolated_state() {
    let f = Fixture::new();
    let profile = f.home.join(".config/cloister/profile.toml");
    fs::write(
        &profile,
        fs::read_to_string(&profile)
            .unwrap()
            .replace("state = \"shared\"", "state = \"isolated\""),
    )
    .unwrap();
    success(f.run(&["--no-host-bridge"]));
    success(f.run(&["--offline", "--no-host-bridge"]));
    assert_eq!(
        f.events()
            .lines()
            .filter(|l| l.starts_with("INSTALL"))
            .count(),
        1
    );
    assert_eq!(
        f.events()
            .lines()
            .filter(|l| l.starts_with("SESSION"))
            .count(),
        2
    );
    let events = f.events();
    let installer = events.lines().find(|l| l.starts_with("INSTALL")).unwrap();
    assert_eq!(installer.matches("--mount").count(), 1);
    assert!(!installer.contains("CLOISTER_HOST_BRIDGE_TOKEN"));
    assert!(!installer.contains("target=/workspace"));
    assert!(
        events
            .lines()
            .filter(|l| l.starts_with("SESSION"))
            .all(|l| l.contains("target=/cloister/software,readonly"))
    );
    assert!(!f.home.join(".local/share/cloister/agents").exists());
    assert!(!f.root().parent().unwrap().join("claude").exists());
}

#[test]
fn dry_run_and_offline_missing_never_install_or_create_software_state() {
    let f = Fixture::new();
    let output = success(f.run(&["--dry-run"]));
    assert!(output.contains("download on launch"));
    assert!(output.contains("Host bridge port: automatic"));
    let offline = f.run(&["--offline"]);
    assert!(!offline.status.success());
    assert!(String::from_utf8_lossy(&offline.stderr).contains("not installed"));
    assert!(!f.root().exists());
    assert!(f.events().is_empty());
}

#[test]
fn updates_atomically_retains_old_installations_and_supports_exact_rollback() {
    let f = Fixture::new();
    success(f.run(&["--no-host-bridge"]));
    let old = f.inventory()["latest"][DEFAULT_IMAGE]["directory"]
        .as_str()
        .unwrap()
        .to_owned();
    success(
        f.command("codex")
            .args(["--update-agent", "--no-host-bridge"])
            .env("TEST_AGENT_VERSION", "1.3.0")
            .output()
            .unwrap(),
    );
    let inventory = f.inventory();
    assert_ne!(
        inventory["latest"][DEFAULT_IMAGE]["directory"]
            .as_str()
            .unwrap(),
        old
    );
    assert!(f.root().join(&old).is_dir());
    let output = success(f.run(&["--agent-version", "1.2.3", "--offline", "--no-host-bridge"]));
    assert!(output.contains("Codex version: 1.2.3"));
    assert_eq!(
        f.inventory(),
        inventory,
        "a one-session rollback must not change latest"
    );
    assert_eq!(
        f.events()
            .lines()
            .filter(|l| l.starts_with("INSTALL"))
            .count(),
        2
    );
}

#[test]
fn image_switch_preserves_offline_selection_and_independent_update_intervals() {
    let f = Fixture::new();
    let profile = f.home.join(".config/cloister/profile.toml");
    let original = fs::read_to_string(&profile).unwrap();
    let other_image = "cloister:other-base";
    let other = original.replace(DEFAULT_IMAGE, other_image);
    assert_ne!(original, other);
    success(f.run(&["--no-host-bridge"]));
    fs::write(&profile, &other).unwrap();
    success(
        f.command("codex")
            .arg("--no-host-bridge")
            .env("TEST_AGENT_VERSION", "1.3.0")
            .output()
            .unwrap(),
    );
    let other_latest = f.inventory()["latest"][other_image].clone();
    fs::write(&profile, &original).unwrap();
    assert!(success(f.run(&["--offline", "--no-host-bridge"])).contains("Codex version: 1.2.3"));
    // A fresh check for the other image must not postpone this image's stale check.
    let mut inventory = f.inventory();
    inventory["latest"][DEFAULT_IMAGE]["checked_at"] = 0.into();
    fs::write(
        f.root().join("inventory.json"),
        serde_json::to_vec(&inventory).unwrap(),
    )
    .unwrap();
    let updated = success(
        f.command("codex")
            .arg("--no-host-bridge")
            .env("TEST_AGENT_VERSION", "1.4.0")
            .output()
            .unwrap(),
    );
    assert!(updated.contains("Codex version: 1.4.0"));
    assert_eq!(f.inventory()["latest"][other_image], other_latest);
    fs::write(&profile, &other).unwrap();
    assert!(success(f.run(&["--offline", "--no-host-bridge"])).contains("Codex version: 1.3.0"));
    // A normal launch of the still-fresh image also reuses its own cache.
    success(f.run(&["--no-host-bridge"]));
    assert_eq!(
        f.events()
            .lines()
            .filter(|l| l.starts_with("INSTALL"))
            .count(),
        3
    );
}

#[test]
fn failed_automatic_update_falls_back_but_explicit_update_reports_failure() {
    let f = Fixture::new();
    success(f.run(&["--no-host-bridge"]));
    let before = fs::read(f.root().join("inventory.json")).unwrap();
    let profile = f.home.join(".config/cloister/profile.toml");
    fs::write(
        &profile,
        fs::read_to_string(&profile)
            .unwrap()
            .replace("update_interval_hours = 24", "update_interval_hours = 0"),
    )
    .unwrap();
    let automatic = f
        .command("codex")
        .arg("--no-host-bridge")
        .env("TEST_INSTALL_FAIL", "1")
        .output()
        .unwrap();
    assert!(automatic.status.success());
    let stderr = String::from_utf8_lossy(&automatic.stderr);
    assert!(stderr.contains("using installed 1.2.3"));
    assert!(!stderr.contains("secret-proxy-password"));
    let explicit = f
        .command("codex")
        .args(["--no-host-bridge", "--update-agent"])
        .env("TEST_INSTALL_FAIL", "1")
        .output()
        .unwrap();
    assert!(!explicit.status.success());
    assert_eq!(fs::read(f.root().join("inventory.json")).unwrap(), before);
    assert_eq!(
        fs::read_dir(f.root())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("install-"))
            .count(),
        1
    );
}

#[test]
fn concurrent_first_launch_serializes_installation_and_uses_distinct_bridges() {
    let f = Fixture::new();
    let first = f
        .command("codex")
        .env("TEST_INSTALL_DELAY", "1")
        .env("TEST_SESSION_DELAY", "2")
        .spawn()
        .unwrap();
    let second = f
        .command("codex")
        .env("TEST_INSTALL_DELAY", "1")
        .env("TEST_SESSION_DELAY", "2")
        .spawn()
        .unwrap();
    let a = success(first.wait_with_output().unwrap());
    let b = success(second.wait_with_output().unwrap());
    let endpoint = |s: &str| {
        s.lines()
            .find(|l| l.starts_with("Host bridge: "))
            .unwrap()
            .to_owned()
    };
    assert_ne!(endpoint(&a), endpoint(&b));
    for output in [&a, &b] {
        let endpoint = endpoint(output);
        let port: u16 = endpoint
            .rsplit(':')
            .next()
            .unwrap()
            .trim_end_matches("/mcp")
            .parse()
            .unwrap();
        std::net::TcpListener::bind(("127.0.0.1", port)).expect("bridge released");
    }
    let tokens: Vec<_> = fs::read_dir(f.directory.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "token"))
        .map(|e| fs::read(e.path()).unwrap())
        .collect();
    assert_eq!(tokens.len(), 2);
    assert!(!tokens[0].is_empty());
    assert_ne!(tokens[0], tokens[1]);
    assert_eq!(
        f.events()
            .lines()
            .filter(|l| l.starts_with("INSTALL"))
            .count(),
        1
    );
}

fn wait_for_event(path: &Path, marker: &str, child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !fs::read_to_string(path).unwrap().contains(marker) {
        assert!(Instant::now() < deadline, "timed out waiting for {marker}");
        assert!(
            child.try_wait().unwrap().is_none(),
            "child exited before {marker}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn interrupting_install_removes_named_container_and_releases_install_lock() {
    let f = Fixture::new();
    let mut child = f
        .command("codex")
        .env("TEST_INSTALL_DELAY", "30")
        .spawn()
        .unwrap();
    wait_for_event(&f.events, "INSTALL", &mut child);
    // SAFETY: this is the live child created by this test.
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
    assert!(!child.wait_with_output().unwrap().status.success());
    assert!(f.events().contains("delete --force cloister-install-"));
    assert!(!f.root().join("inventory.json").exists());
    success(f.run(&["--no-host-bridge"]));
}

#[test]
fn rejects_unsafe_versions_before_creating_state() {
    let f = Fixture::new();
    for version in [
        "../other",
        "https://example.invalid/pkg",
        "^1.0.0",
        "--help",
    ] {
        assert!(!f.run(&["--agent-version", version]).status.success());
    }
    assert!(!f.root().exists());
}

#[test]
fn failed_first_install_never_launches_agent_or_publishes_inventory() {
    let f = Fixture::new();
    let output = f
        .command("codex")
        .env("TEST_INSTALL_FAIL", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!f.events().contains("SESSION"));
    assert!(!f.root().join("inventory.json").exists());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("secret-proxy-password"));
}

#[test]
fn unchanged_update_reuses_installation_and_claude_has_a_separate_cache() {
    let f = Fixture::new();
    success(f.run(&["--no-host-bridge"]));
    let selected = f.inventory()["latest"][DEFAULT_IMAGE]["directory"].clone();
    success(f.run(&["--update-agent", "--no-host-bridge"]));
    assert_eq!(
        f.inventory()["latest"][DEFAULT_IMAGE]["directory"],
        selected
    );
    assert_eq!(f.inventory()["installations"].as_array().unwrap().len(), 1);
    let claude = success(
        f.command("claude")
            .arg("--no-host-bridge")
            .output()
            .unwrap(),
    );
    assert!(claude.contains("Claude version: 1.2.3"));
    assert!(
        f.root()
            .parent()
            .unwrap()
            .join("claude/inventory.json")
            .is_file()
    );
}

#[test]
fn interrupted_session_releases_its_bridge_and_preserves_cached_software() {
    let f = Fixture::new();
    let mut child = f
        .command("codex")
        .env("TEST_SESSION_DELAY", "2")
        .spawn()
        .unwrap();
    wait_for_event(&f.events, "SESSION", &mut child);
    // SAFETY: signal only the live child started by this test.
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(f.events().contains("delete --force cloister-session-"));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let endpoint = stdout
        .lines()
        .find(|l| l.starts_with("Host bridge: "))
        .unwrap();
    let port: u16 = endpoint
        .rsplit(':')
        .next()
        .unwrap()
        .trim_end_matches("/mcp")
        .parse()
        .unwrap();
    std::net::TcpListener::bind(("127.0.0.1", port)).expect("interrupted bridge released");
    assert_eq!(f.inventory()["installations"].as_array().unwrap().len(), 1);
    success(f.run(&["--offline", "--no-host-bridge"]));
}

#[test]
fn rejects_workspace_that_would_expose_writable_software_cache() {
    let f = Fixture::new();
    let output = f
        .command("codex")
        .arg("--workspace")
        .arg(&f.home)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("overlaps Cloister's software cache"));
    assert!(f.events().is_empty());
    assert!(!f.root().exists());
}
