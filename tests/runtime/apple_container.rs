use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
};

use cloister::{
    agent::{AgentAdapter, ClaudeAgent, CodexAgent},
    preflight::{resolve_guest_proxy, resolve_launch},
    profile::{AgentState, NetworkProxyMode, load_profile},
    runtime::{
        GuestProxyPlan, HOST_BRIDGE_GUEST_NAME, HOST_BRIDGE_LOCALHOST_ADDRESS, HostBridgeLaunch,
        NetworkExposure, dns_create_command, dns_list_command, image_inspect_command,
        image_pull_command, plan_agent_container as plan_with_software, system_start_command,
        system_status_command,
    },
};
use tempfile::tempdir;

const RELEASE_IMAGE: &str = concat!("ghcr.io/yi427/cloister:", env!("CARGO_PKG_VERSION"));

#[test]
fn connectivity_probe_has_bounded_resources_and_no_host_mounts_or_secrets() {
    let mut profile = load_profile(fixture("valid/default.toml")).unwrap();
    profile.network.proxy = NetworkProxyMode::Inherit;
    let endpoint = "http://host.container.internal:12345/cloister-check/test-nonce";
    let command =
        cloister::runtime::host_connectivity_command(&profile, "cloister-check-test", endpoint);
    let args: Vec<_> = command
        .arguments()
        .iter()
        .map(|s| s.to_str().unwrap())
        .collect();
    assert_eq!(command.program(), "container");
    assert!(command.secret_environment_names().next().is_none());
    for forbidden in [
        "--mount",
        "--volume",
        "--ssh",
        "--env",
        "--interactive",
        "--tty",
    ] {
        assert!(!args.contains(&forbidden));
    }
    assert!(args.contains(&"--rm"));
    assert!(args.contains(&"--read-only"));
    assert!(args.contains(&"--ipv4"));
    for pair in [
        ["--cpus", "1"],
        ["--memory", "256M"],
        ["--entrypoint", "/usr/bin/curl"],
        ["--noproxy", "*"],
        ["--connect-timeout", "8"],
        ["--max-time", "10"],
    ] {
        assert!(args.windows(2).any(|window| window == pair));
    }
    assert_eq!(args.last(), Some(&endpoint));
    assert!(
        args.windows(3)
            .any(|window| window == ["--", profile.image.reference.as_str(), "--disable"])
    );
}

fn fixture(relative_path: impl AsRef<Path>) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/profiles")
        .join(relative_path)
}

fn default_plan() -> cloister::runtime::RuntimePlan {
    let path = fixture("valid/default.toml");
    let profile = load_profile(&path).expect("default profile should load");
    let resolved =
        resolve_launch(profile, fixture("valid")).expect("default workspace should resolve");

    plan_agent_container(&resolved, &CodexAgent, None, None, None, &[])
        .expect("default profile should produce a plan")
}

fn argument_after<'a>(arguments: &'a [OsString], option: &str) -> &'a OsStr {
    let position = arguments
        .iter()
        .position(|argument| argument == option)
        .unwrap_or_else(|| panic!("{option} should be present"));
    arguments
        .get(position + 1)
        .expect("option should have a value")
}

#[test]
fn constructs_explicit_setup_and_probe_commands() {
    let status = system_status_command();
    assert_eq!(status.program(), OsStr::new("container"));
    assert_eq!(status.arguments(), ["system", "status", "--format", "json"]);

    let start = system_start_command();
    assert_eq!(start.program(), OsStr::new("container"));
    assert_eq!(start.arguments(), ["system", "start"]);

    let image = RELEASE_IMAGE;
    assert_eq!(
        image_inspect_command(image).arguments(),
        ["image", "inspect", image]
    );
    assert_eq!(
        image_pull_command(image).arguments(),
        ["image", "pull", "--arch", "arm64", image]
    );
    assert_eq!(
        dns_list_command().arguments(),
        ["system", "dns", "list", "--format", "json"]
    );

    let dns = dns_create_command();
    assert_eq!(dns.program(), OsStr::new("sudo"));
    assert_eq!(
        dns.arguments(),
        [
            "container",
            "system",
            "dns",
            "create",
            HOST_BRIDGE_GUEST_NAME,
            "--localhost",
            HOST_BRIDGE_LOCALHOST_ADDRESS,
        ]
    );
}

#[test]
fn translates_every_supported_codex_setting() {
    let plan = default_plan();
    let arguments = plan.command().arguments();

    assert_eq!(plan.profile_name(), "rust-default");
    assert_eq!(plan.network(), NetworkExposure::DefaultWithInternetEgress);
    assert_eq!(plan.agent_name(), "Codex");
    assert!(plan.agent_state().is_none());
    assert_eq!(plan.command().program(), OsStr::new("container"));
    assert_eq!(
        arguments.first().map(OsString::as_os_str),
        Some(OsStr::new("run"))
    );
    assert!(arguments.iter().any(|argument| argument == "--rm"));
    assert!(!arguments.iter().any(|argument| argument == "--name"));
    assert_eq!(argument_after(arguments, "--arch"), "arm64");
    assert_eq!(argument_after(arguments, "--cpus"), "4");
    assert_eq!(argument_after(arguments, "--memory"), "8G");
    assert_eq!(argument_after(arguments, "--user"), "cloister");
    assert_eq!(
        argument_after(arguments, "--workdir"),
        plan.workspace().guest()
    );
    assert_eq!(argument_after(arguments, "--network"), "default");
    assert_eq!(
        argument_after(arguments, "--label"),
        "org.cloister.profile=rust-default"
    );
    assert!(arguments.iter().any(|argument| argument == "--interactive"));
    assert!(arguments.iter().any(|argument| argument == "--tty"));
    assert!(arguments.iter().any(|argument| argument == "--read-only"));
    assert!(!arguments.iter().any(|argument| argument == "--ssh"));
    assert_eq!(argument_after(arguments, "--tmpfs"), "/tmp");
    assert!(
        arguments
            .windows(2)
            .any(|p| p[0] == "--tmpfs" && p[1] == "/cloister/session:mode=1777")
    );
    assert!(arguments.ends_with(&[
        OsString::from("--"),
        OsString::from("cloister:dev"),
        OsString::from("codex"),
        OsString::from("--config"),
        OsString::from("check_for_update_on_startup=false"),
    ]));

    let environments = arguments
        .windows(2)
        .filter(|pair| pair[0] == "--env")
        .map(|pair| pair[1].to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        environments,
        [
            "LANG=en_US.UTF-8",
            "LC_ALL=en_US.UTF-8",
            "TZ=America/New_York"
        ]
    );
}

#[test]
fn creates_the_read_write_workspace_mount() {
    let plan = default_plan();
    let mount = argument_after(plan.command().arguments(), "--mount")
        .to_string_lossy()
        .into_owned();

    assert!(mount.starts_with("type=bind,source="));
    let host = fixture("valid").canonicalize().unwrap();
    let guest = Path::new("/workspace").join(host.strip_prefix("/").unwrap());
    assert_eq!(plan.workspace().guest(), guest);
    assert_eq!(
        mount,
        format!(
            "type=bind,source={},target={}",
            host.display(),
            guest.display()
        )
    );
    assert!(!mount.ends_with(",readonly"));
    assert_eq!(
        plan.workspace().host(),
        fixture("valid").canonicalize().unwrap()
    );
}

#[test]
fn namesake_projects_have_distinct_workdirs_with_shared_state_for_both_agents() {
    let directory = tempdir().expect("temporary directory should exist");
    let first = directory.path().join("first/project");
    let second = directory.path().join("second/project");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    let mut profile = load_profile(fixture("valid/default.toml")).unwrap();
    profile.agent.state = AgentState::Shared;
    let first = resolve_launch(profile.clone(), first).unwrap();
    let second = resolve_launch(profile, second).unwrap();
    let state = directory.path().join("state");

    for agent in [&CodexAgent as &dyn AgentAdapter, &ClaudeAgent] {
        let first_plan =
            plan_agent_container(&first, agent, Some(&state), None, None, &[]).unwrap();
        let second_plan =
            plan_agent_container(&second, agent, Some(&state), None, None, &[]).unwrap();

        assert_ne!(
            first_plan.workspace().guest(),
            second_plan.workspace().guest()
        );
        assert_eq!(first_plan.agent_state(), second_plan.agent_state());
        for plan in [&first_plan, &second_plan] {
            assert_eq!(
                argument_after(plan.command().arguments(), "--workdir"),
                plan.workspace().guest()
            );
            assert_eq!(
                plan.workspace().guest().file_name(),
                Some(OsStr::new("project"))
            );
            assert_eq!(
                argument_after(plan.command().arguments(), "--mount"),
                OsString::from(format!(
                    "type=bind,source={},target={}",
                    plan.workspace().host().display(),
                    plan.workspace().guest().display()
                ))
            );
        }
    }
}

#[test]
fn repeated_launches_and_workspace_aliases_keep_the_same_guest_workdir() {
    let directory = tempdir().expect("temporary directory should exist");
    let project = directory.path().join("project");
    let alias = directory.path().join("alias");
    fs::create_dir(&project).unwrap();
    symlink(&project, &alias).unwrap();
    let profile = load_profile(fixture("valid/default.toml")).unwrap();
    let resolved = resolve_launch(profile.clone(), &project).unwrap();
    let expected = plan_agent_container(&resolved, &CodexAgent, None, None, None, &[]).unwrap();

    for workspace in [&project, &project.join("."), &alias] {
        let resolved = resolve_launch(profile.clone(), workspace).unwrap();
        let actual = plan_agent_container(&resolved, &CodexAgent, None, None, None, &[]).unwrap();
        assert_eq!(actual.workspace(), expected.workspace());
        assert_eq!(actual.command(), expected.command());
    }
}

#[test]
fn preserves_unicode_spaces_and_shell_metacharacters_in_workspace_arguments() {
    let directory = tempdir().expect("temporary directory should exist");
    let project = directory.path().join("项目 with spaces-$literal");
    fs::create_dir(&project).unwrap();
    let profile = load_profile(fixture("valid/default.toml")).unwrap();
    let resolved = resolve_launch(profile, &project).unwrap();
    let plan = plan_agent_container(&resolved, &CodexAgent, None, None, None, &[]).unwrap();
    let mut expected_guest = OsString::from("/workspace");
    expected_guest.push(resolved.workspace());
    assert_eq!(plan.workspace().guest(), Path::new(&expected_guest));
    assert_eq!(
        argument_after(plan.command().arguments(), "--workdir"),
        expected_guest
    );

    let mut expected_mount = OsString::from("type=bind,source=");
    expected_mount.push(resolved.workspace());
    expected_mount.push(",target=");
    expected_mount.push(expected_guest);
    assert_eq!(
        argument_after(plan.command().arguments(), "--mount"),
        expected_mount
    );
}

#[test]
fn inherits_and_redacts_a_loopback_guest_proxy() {
    let path = fixture("valid/default.toml");
    let mut profile = load_profile(&path).expect("default profile should load");
    profile.network.proxy = NetworkProxyMode::Inherit;
    let resolved =
        resolve_launch(profile, fixture("valid")).expect("default workspace should resolve");
    let proxy_secret = "secret-proxy-password";
    let proxy = resolve_guest_proxy(
        NetworkProxyMode::Inherit,
        BTreeMap::from([
            (
                OsString::from("HTTPS_PROXY"),
                OsString::from(format!("http://user:{proxy_secret}@127.0.0.1:3080")),
            ),
            (OsString::from("NO_PROXY"), OsString::from("example.com")),
        ]),
    )
    .expect("host proxy should resolve")
    .expect("inherit mode should produce a proxy");

    let plan = plan_agent_container(&resolved, &CodexAgent, None, None, Some(&proxy), &[])
        .expect("inherited proxy should produce a plan");
    let forwarded_names = plan
        .command()
        .secret_environment_names()
        .collect::<Vec<_>>();

    assert_eq!(
        plan.proxy(),
        GuestProxyPlan::Inherited {
            source_variable: "HTTPS_PROXY",
            loopback_rewritten: true,
        }
    );
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
        assert!(forwarded_names.contains(&OsStr::new(name)));
        assert!(
            plan.command()
                .arguments()
                .windows(2)
                .any(|pair| { pair[0] == "--env" && pair[1] == name })
        );
    }

    let display = plan.to_string();
    let debug = format!("{plan:?}");
    assert!(display.contains(
        "Guest proxy: inherit from HTTPS_PROXY (loopback mapped to host.container.internal; value redacted)"
    ));
    assert!(!display.contains(proxy_secret));
    assert!(!debug.contains(proxy_secret));
}

#[test]
fn rejects_a_mount_path_that_container_cannot_represent() {
    let path = fixture("valid/default.toml");
    let profile = load_profile(&path).expect("default profile should load");
    let directory = tempdir().expect("temporary directory should exist");
    let workspace = directory.path().join("work,space");
    fs::create_dir(&workspace).expect("workspace should be created");
    let resolved = resolve_launch(profile, workspace).expect("host workspace should resolve");

    let error = plan_agent_container(&resolved, &CodexAgent, None, None, None, &[])
        .expect_err("comma mount path should fail");

    assert!(error.to_string().contains("must not contain ','"));
}

#[test]
fn appends_codex_arguments_without_shell_parsing() {
    let path = fixture("valid/default.toml");
    let profile = load_profile(&path).expect("default profile should load");
    let resolved =
        resolve_launch(profile, fixture("valid")).expect("default workspace should resolve");
    let arguments = [
        OsString::from("--config"),
        OsString::from("model_reasoning_effort=high"),
    ];

    let plan = plan_agent_container(&resolved, &CodexAgent, None, None, None, &arguments)
        .expect("Codex launch should produce plan");

    assert!(plan.command().arguments().ends_with(&[
        OsString::from("--"),
        OsString::from("cloister:dev"),
        OsString::from("codex"),
        OsString::from("--config"),
        OsString::from("check_for_update_on_startup=false"),
        OsString::from("--config"),
        OsString::from("model_reasoning_effort=high"),
    ]));
}

#[test]
fn mounts_shared_codex_state_and_sets_codex_home() {
    let path = fixture("valid/default.toml");
    let mut profile = load_profile(&path).expect("default profile should load");
    profile.agent.state = AgentState::Shared;
    let resolved =
        resolve_launch(profile, fixture("valid")).expect("default workspace should resolve");
    let state = fixture("valid");

    let plan = plan_agent_container(&resolved, &CodexAgent, Some(&state), None, None, &[])
        .expect("shared Codex state should produce a plan");
    let arguments = plan.command().arguments();
    let mount = plan.agent_state().expect("Codex state should be mounted");
    let expected_mount = OsString::from(format!(
        "type=bind,source={},target=/cloister/agents/codex",
        state.display()
    ));

    assert_eq!(mount.host(), state);
    assert_eq!(mount.guest(), Path::new("/cloister/agents/codex"));
    assert!(
        arguments
            .windows(2)
            .any(|pair| { pair[0] == "--env" && pair[1] == "CODEX_HOME=/cloister/agents/codex" })
    );
    assert!(
        arguments
            .windows(2)
            .any(|pair| { pair[0] == "--mount" && pair[1] == expected_mount })
    );
}

#[test]
fn shared_codex_state_requires_a_host_directory() {
    let path = fixture("valid/default.toml");
    let mut profile = load_profile(&path).expect("default profile should load");
    profile.agent.state = AgentState::Shared;
    let resolved =
        resolve_launch(profile, fixture("valid")).expect("default workspace should resolve");

    let error = plan_agent_container(&resolved, &CodexAgent, None, None, None, &[])
        .expect_err("shared state without a directory should fail");

    assert!(error.to_string().contains("shared Codex state requires"));
}

#[test]
fn injects_the_authenticated_host_bridge_without_rendering_its_token() {
    let path = fixture("valid/default.toml");
    let directory = tempdir().expect("temporary directory should exist");
    let host_command = directory.path().join("xcodebuild");
    fs::write(&host_command, "#!/bin/sh\nexit 0\n").expect("fake host command should be written");
    fs::set_permissions(&host_command, fs::Permissions::from_mode(0o755))
        .expect("fake host command should be executable");
    let mut profile = load_profile(&path).expect("default profile should load");
    profile.host.exec.allow[0].executable = host_command.clone();
    let resolved =
        resolve_launch(profile, fixture("valid")).expect("default workspace should resolve");
    let endpoint = "http://host.container.internal:17834/mcp";
    let token = "sensitive-bridge-token";
    let audit_log_path = fixture("valid/audit/host-exec.jsonl");

    let plan = plan_agent_container(
        &resolved,
        &CodexAgent,
        None,
        Some(HostBridgeLaunch::new(endpoint, token, &audit_log_path)),
        None,
        &[],
    )
    .expect("host bridge plan should be created");
    let arguments = plan.command().arguments();

    assert_eq!(plan.host_bridge_endpoint(), Some(endpoint));
    assert_eq!(plan.host_audit_log_path(), Some(audit_log_path.as_path()));
    assert!(
        arguments
            .windows(2)
            .any(|pair| { pair[0] == "--env" && pair[1] == "CLOISTER_HOST_BRIDGE_TOKEN" })
    );
    for expected in [
        format!("mcp_servers.cloister_host.url=\"{endpoint}\""),
        "mcp_servers.cloister_host.bearer_token_env_var=\"CLOISTER_HOST_BRIDGE_TOKEN\"".to_owned(),
        "mcp_servers.cloister_host.required=true".to_owned(),
        "mcp_servers.cloister_host.enabled_tools=[\"host.list_commands\",\"host.exec\",\"host.exec_status\",\"host.exec_cancel\"]".to_owned(),
        "mcp_servers.cloister_host.default_tools_approval_mode=\"auto\"".to_owned(),
        "mcp_servers.cloister_host.tools.\"host.exec\".approval_mode=\"prompt\"".to_owned(),
    ] {
        assert!(
            arguments
                .iter()
                .any(|argument| argument == OsStr::new(&expected))
        );
    }
    assert_eq!(
        plan.command()
            .secret_environment_names()
            .collect::<Vec<_>>(),
        [OsStr::new("CLOISTER_HOST_BRIDGE_TOKEN")]
    );

    let display = plan.to_string();
    let debug = format!("{plan:?}");
    assert!(display.contains("Host capabilities: host.list_commands, host.exec"));
    assert!(display.contains(&format!(
        "Host audit log: {} (JSONL, owner-only, 20 MiB total)",
        audit_log_path.display()
    )));
    assert!(display.contains("Host policy: inherit-all environment, 1 allowed command(s)"));
    assert!(display.contains("Host Skill: host-exec (canonical read-only image source)"));
    assert!(display.contains(&format!(
        "xcodebuild: declared '{}'",
        host_command.display()
    )));
    assert!(display.contains("[REDACTED]"));
    assert!(!display.contains(token));
    assert!(!debug.contains(token));
}

#[test]
fn mounts_shared_claude_state_and_sets_claude_config_dir() {
    let path = fixture("valid/default.toml");
    let mut profile = load_profile(&path).expect("default profile should load");
    profile.agent.state = AgentState::Shared;
    let resolved =
        resolve_launch(profile, fixture("valid")).expect("default workspace should resolve");
    let state = fixture("valid");

    let plan = plan_agent_container(&resolved, &ClaudeAgent, Some(&state), None, None, &[])
        .expect("shared Claude state should produce a plan");
    let arguments = plan.command().arguments();
    let mount = plan.agent_state().expect("Claude state should be mounted");
    let expected_mount = OsString::from(format!(
        "type=bind,source={},target=/cloister/agents/claude",
        state.display()
    ));

    assert_eq!(plan.agent_name(), "Claude");
    assert_eq!(mount.host(), state);
    assert_eq!(mount.guest(), Path::new("/cloister/agents/claude"));
    assert!(arguments.windows(2).any(|pair| {
        pair[0] == "--env" && pair[1] == "CLAUDE_CONFIG_DIR=/cloister/agents/claude"
    }));
    assert!(
        arguments
            .windows(2)
            .any(|pair| { pair[0] == "--mount" && pair[1] == expected_mount })
    );
    assert!(arguments.ends_with(&[
        OsString::from("--"),
        OsString::from("cloister:dev"),
        OsString::from("claude"),
    ]));
}

#[test]
fn appends_claude_arguments_without_shell_parsing() {
    let path = fixture("valid/default.toml");
    let profile = load_profile(&path).expect("default profile should load");
    let resolved =
        resolve_launch(profile, fixture("valid")).expect("default workspace should resolve");
    let arguments = [OsString::from("--model"), OsString::from("sonnet")];

    let plan = plan_agent_container(&resolved, &ClaudeAgent, None, None, None, &arguments)
        .expect("Claude launch should produce plan");

    assert!(plan.command().arguments().ends_with(&[
        OsString::from("--"),
        OsString::from("cloister:dev"),
        OsString::from("claude"),
        OsString::from("--model"),
        OsString::from("sonnet"),
    ]));
}

fn plan_agent_container(
    resolved: &cloister::preflight::ResolvedLaunch,
    agent: &dyn cloister::agent::AgentAdapter,
    shared_state: Option<&Path>,
    host_bridge: Option<HostBridgeLaunch<'_>>,
    proxy: Option<&cloister::preflight::ResolvedGuestProxy>,
    arguments: &[OsString],
) -> Result<cloister::runtime::RuntimePlan, cloister::runtime::RuntimePlanError> {
    plan_with_software(
        resolved,
        agent,
        shared_state,
        host_bridge,
        proxy,
        arguments,
        Path::new("/test-agent-software"),
    )
}

#[test]
fn installer_mounts_only_staging_and_session_software_is_read_only() {
    let profile = load_profile(fixture("valid/default.toml")).unwrap();
    let selector = "latest".parse().unwrap();
    let command = cloister::runtime::agent_install_command(
        &profile,
        &CodexAgent,
        &selector,
        Some("1.2.3"),
        Path::new("/test-staging"),
        None,
    )
    .unwrap();
    let arguments = command.arguments();
    assert_eq!(arguments.iter().filter(|a| *a == "--mount").count(), 1);
    assert_eq!(
        argument_after(arguments, "--mount"),
        "type=bind,source=/test-staging,target=/cloister/install"
    );
    assert_eq!(
        argument_after(arguments, "--entrypoint"),
        "/usr/local/bin/cloister-install-agent"
    );
    assert!(
        !arguments
            .iter()
            .any(|a| a == "--tty" || a == "--interactive" || a == "CLOISTER_HOST_BRIDGE_TOKEN")
    );
    assert!(arguments.ends_with(&[
        "--".into(),
        "cloister:dev".into(),
        "codex".into(),
        "latest".into(),
        "1.2.3".into()
    ]));
    let session = default_plan();
    assert!(
        session.command().arguments().iter().any(
            |a| a == "type=bind,source=/test-agent-software,target=/cloister/software,readonly"
        )
    );
}
