use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use cloister::{
    preflight::resolve_profile,
    profile::{NetworkMode, WorkspaceAccess, WorkspaceMode, load_profile},
    runtime::{AgentKind, NetworkExposure, WorkspaceMountAccess, plan_apple_container},
};

fn fixture(relative_path: impl AsRef<Path>) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/profiles")
        .join(relative_path)
}

fn default_plan() -> cloister::runtime::RuntimePlan {
    let path = fixture("valid/default.toml");
    let profile = load_profile(&path).expect("default profile should load");
    let resolved = resolve_profile(profile, path).expect("default workspace should resolve");

    plan_apple_container(&resolved, &[]).expect("default profile should produce a plan")
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
fn translates_every_supported_runtime_setting() {
    let plan = default_plan();
    let arguments = plan.command().arguments();

    assert_eq!(plan.profile_name(), "rust-default");
    assert_eq!(plan.guest_hostname(), "cloister");
    assert_eq!(plan.network(), NetworkExposure::DefaultWithInternetEgress);
    assert_eq!(plan.workspace().access(), WorkspaceMountAccess::ReadWrite);
    assert_eq!(
        plan.enabled_agents(),
        &[AgentKind::Codex, AgentKind::Claude]
    );
    assert_eq!(plan.command().program(), OsStr::new("container"));
    assert_eq!(
        arguments.first().map(OsString::as_os_str),
        Some(OsStr::new("run"))
    );
    assert!(arguments.iter().any(|argument| argument == "--rm"));
    assert_eq!(argument_after(arguments, "--name"), "cloister");
    assert_eq!(argument_after(arguments, "--arch"), "arm64");
    assert_eq!(argument_after(arguments, "--cpus"), "4");
    assert_eq!(argument_after(arguments, "--memory"), "8G");
    assert_eq!(argument_after(arguments, "--user"), "cloister");
    assert_eq!(argument_after(arguments, "--workdir"), "/workspace");
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
    assert_eq!(
        arguments.last().map(OsString::as_os_str),
        Some(OsStr::new("cloister/rust-node:dev"))
    );

    let environments = arguments
        .windows(2)
        .filter(|pair| pair[0] == "--env")
        .map(|pair| pair[1].to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        environments,
        ["LANG=zh_CN.UTF-8", "LC_ALL=zh_CN.UTF-8", "TZ=Asia/Shanghai"]
    );
}

#[test]
fn creates_an_explicit_read_write_bind_mount() {
    let plan = default_plan();
    let mount = argument_after(plan.command().arguments(), "--mount")
        .to_string_lossy()
        .into_owned();

    assert!(mount.starts_with("type=bind,source="));
    assert!(mount.contains(",target=/workspace"));
    assert!(!mount.ends_with(",readonly"));
    assert_eq!(
        plan.workspace().host(),
        fixture("valid").canonicalize().unwrap()
    );
}

#[test]
fn adds_readonly_to_a_read_only_workspace_mount() {
    let path = fixture("valid/default.toml");
    let mut profile = load_profile(&path).expect("default profile should load");
    profile.workspace.access = WorkspaceAccess::ReadOnly;
    let resolved = resolve_profile(profile, path).expect("workspace should resolve");
    let plan = plan_apple_container(&resolved, &[]).expect("read-only workspace should plan");
    let mount = argument_after(plan.command().arguments(), "--mount").to_string_lossy();

    assert_eq!(plan.workspace().access(), WorkspaceMountAccess::ReadOnly);
    assert!(mount.ends_with(",readonly"));
}

#[test]
fn rejects_a_mount_path_that_container_cannot_represent() {
    let path = fixture("valid/default.toml");
    let mut profile = load_profile(&path).expect("default profile should load");
    profile.workspace.guest = PathBuf::from("/work,space");
    let resolved = resolve_profile(profile, path).expect("host workspace should resolve");

    let error = plan_apple_container(&resolved, &[]).expect_err("comma mount path should fail");

    assert!(error.to_string().contains("must not contain ','"));
}

#[test]
fn fails_closed_for_unsupported_runtime_policies() {
    let path = fixture("valid/default.toml");
    let mut copy_profile = load_profile(&path).expect("default profile should load");
    copy_profile.workspace.mode = WorkspaceMode::Copy;
    let copy_resolved =
        resolve_profile(copy_profile, &path).expect("host workspace should resolve");

    let copy_error =
        plan_apple_container(&copy_resolved, &[]).expect_err("copy mode should not produce a plan");
    assert!(copy_error.to_string().contains("copy workspace mode"));

    let mut restricted_profile = load_profile(&path).expect("default profile should load");
    restricted_profile.network.mode = NetworkMode::Restricted;
    let restricted_resolved =
        resolve_profile(restricted_profile, path).expect("host workspace should resolve");

    let restricted_error = plan_apple_container(&restricted_resolved, &[])
        .expect_err("restricted network should not produce a plan");
    assert!(
        restricted_error
            .to_string()
            .contains("restricted network mode")
    );
}

#[test]
fn appends_the_container_command_without_shell_parsing() {
    let path = fixture("valid/default.toml");
    let profile = load_profile(&path).expect("default profile should load");
    let resolved = resolve_profile(profile, path).expect("default workspace should resolve");
    let command = [
        OsString::from("/bin/sh"),
        OsString::from("-lc"),
        OsString::from("printf 'hello world'"),
    ];

    let plan =
        plan_apple_container(&resolved, &command).expect("container command should produce a plan");
    let arguments = plan.command().arguments();
    let image_index = arguments.len() - command.len() - 1;

    assert_eq!(arguments[image_index], "cloister/rust-node:dev");
    assert_eq!(
        &arguments[arguments.len() - command.len()..],
        command.as_slice()
    );
}
