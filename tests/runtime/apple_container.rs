use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
};

use cloister::{
    preflight::resolve_launch,
    profile::{AgentState, load_profile},
    runtime::{NetworkExposure, plan_codex_container},
};
use tempfile::tempdir;

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

    plan_codex_container(&resolved, None, &[]).expect("default profile should produce a plan")
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
fn translates_every_supported_codex_setting() {
    let plan = default_plan();
    let arguments = plan.command().arguments();

    assert_eq!(plan.profile_name(), "rust-default");
    assert_eq!(plan.network(), NetworkExposure::DefaultWithInternetEgress);
    assert!(plan.codex_state().is_none());
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
    assert!(arguments.ends_with(&[
        OsString::from("cloister/rust-node:dev"),
        OsString::from("codex"),
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
    assert!(mount.contains(",target=/workspace"));
    assert!(!mount.ends_with(",readonly"));
    assert_eq!(
        plan.workspace().host(),
        fixture("valid").canonicalize().unwrap()
    );
}

#[test]
fn rejects_a_mount_path_that_container_cannot_represent() {
    let path = fixture("valid/default.toml");
    let profile = load_profile(&path).expect("default profile should load");
    let directory = tempdir().expect("temporary directory should exist");
    let workspace = directory.path().join("work,space");
    fs::create_dir(&workspace).expect("workspace should be created");
    let resolved = resolve_launch(profile, workspace).expect("host workspace should resolve");

    let error =
        plan_codex_container(&resolved, None, &[]).expect_err("comma mount path should fail");

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

    let plan = plan_codex_container(&resolved, None, &arguments)
        .expect("Codex launch should produce plan");

    assert!(plan.command().arguments().ends_with(&[
        OsString::from("cloister/rust-node:dev"),
        OsString::from("codex"),
        OsString::from("--config"),
        OsString::from("model_reasoning_effort=high"),
    ]));
}

#[test]
fn mounts_shared_codex_state_and_sets_codex_home() {
    let path = fixture("valid/default.toml");
    let mut profile = load_profile(&path).expect("default profile should load");
    profile.codex.state = AgentState::Shared;
    let resolved =
        resolve_launch(profile, fixture("valid")).expect("default workspace should resolve");
    let state = fixture("valid");

    let plan = plan_codex_container(&resolved, Some(&state), &[])
        .expect("shared Codex state should produce a plan");
    let arguments = plan.command().arguments();
    let mount = plan.codex_state().expect("Codex state should be mounted");
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
    profile.codex.state = AgentState::Shared;
    let resolved =
        resolve_launch(profile, fixture("valid")).expect("default workspace should resolve");

    let error = plan_codex_container(&resolved, None, &[])
        .expect_err("shared state without a directory should fail");

    assert!(error.to_string().contains("shared Codex state requires"));
}
