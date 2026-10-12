use std::{fs, os::unix::fs::PermissionsExt, process::Command};

use tempfile::tempdir;

#[test]
fn agent_host_execution_persists_audit_without_writing_to_the_terminal() {
    for agent in ["claude", "codex"] {
        for exit_code in [0, 7] {
            let directory = tempdir().expect("temporary directory should exist");
            let home = directory.path().join("home");
            let project = directory.path().join("project");
            let bin = directory.path().join("bin");
            fs::create_dir_all(&project).unwrap();
            fs::create_dir_all(&bin).unwrap();
            for (name, source) in [
                (
                    "container",
                    include_str!("../fixtures/host-audit-runtime.sh"),
                ),
                ("host-tool", include_str!("../fixtures/host-audit-tool.sh")),
            ] {
                let path = bin.join(name);
                fs::write(&path, source).unwrap();
                fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
            }
            let profile = directory.path().join("profile.toml");
            fs::write(
                &profile,
                include_str!("../fixtures/profiles/valid/on-demand.toml").replace(
                    "cloister:dev",
                    concat!("ghcr.io/yi427/cloister:", env!("CARGO_PKG_VERSION")),
                ),
            )
            .unwrap();
            super::seed_software(&home, agent);
            let client_stdout = directory.path().join("client.stdout");
            let client_stderr = directory.path().join("client.stderr");

            let output = Command::new(env!("CARGO_BIN_EXE_cloister"))
                .args([agent, "--offline", "--profile"])
                .arg(profile)
                .current_dir(&project)
                .env("HOME", &home)
                .env_remove("XDG_CONFIG_HOME")
                .env_remove("XDG_DATA_HOME")
                .env_remove("XDG_STATE_HOME")
                .env("PATH", &bin)
                .env("TEST_CLOISTER_BINARY", env!("CARGO_BIN_EXE_cloister"))
                .env(
                    "TEST_BRIDGE_TOKEN_FILE",
                    directory.path().join("bridge.token"),
                )
                .env("TEST_CLIENT_STDOUT", &client_stdout)
                .env("TEST_CLIENT_STDERR", &client_stderr)
                .env("TEST_HOST_EXIT_CODE", exit_code.to_string())
                .output()
                .expect("agent session should start");

            assert!(output.status.success(), "{agent}, exit code {exit_code}");
            assert!(
                output.stderr.is_empty(),
                "{agent} terminal received: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                String::from_utf8(output.stdout)
                    .unwrap()
                    .ends_with("test-agent-active\ntest-agent-finished\n")
            );
            assert_eq!(fs::read_to_string(client_stdout).unwrap(), "host stdout\n");
            assert!(
                fs::read_to_string(client_stderr)
                    .unwrap()
                    .starts_with("host stderr\n")
            );

            let audit =
                fs::read_to_string(home.join(".local/state/cloister/audit/host-exec.jsonl"))
                    .expect("file audit should remain available");
            let events = audit
                .lines()
                .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(events.len(), 2);
            assert_eq!(events[0]["event"], "execution_started");
            assert_eq!(events[1]["event"], "execution_finished");
            assert_eq!(events[1]["execution_id"], events[0]["execution_id"]);
            assert_eq!(events[1]["command"], "host-tool");
            assert_eq!(events[1]["outcome"], "completed");
            assert_eq!(events[1]["exit_code"], exit_code);
            assert_eq!(events[1]["stdout_bytes"], "host stdout\n".len());
            assert_eq!(events[1]["stderr_bytes"], "host stderr\n".len());
        }
    }
}
