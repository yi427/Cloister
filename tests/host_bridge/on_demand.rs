use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
};

use cloister::{
    host_bridge::{
        HOST_EXEC_DSL_VERSION, HostExecAuthorizationError, HostExecPolicy, HostExecRequest,
    },
    profile::{HostExecMode, parse_profile},
};
use tempfile::tempdir;

const PROFILE: &str = include_str!("../fixtures/profiles/valid/on-demand.toml");
const TOOL: &str = include_str!("../fixtures/on-demand-tool.sh");

fn executable(path: &Path) {
    fs::write(path, TOOL).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn policy(path: &std::ffi::OsStr) -> HostExecPolicy {
    let profile = parse_profile(PROFILE).unwrap();
    HostExecPolicy::from_profile(
        &profile.host.exec,
        BTreeMap::from([(OsString::from("PATH"), path.to_owned())]),
    )
    .unwrap()
    .unwrap()
}

fn request(command: &str) -> HostExecRequest {
    HostExecRequest {
        version: HOST_EXEC_DSL_VERSION,
        command: command.to_owned(),
        args: vec!["literal; $(never-run)".to_owned()],
        reason: Some("Use the required host-only tool".to_owned()),
    }
}

#[test]
fn on_demand_resolves_bare_names_without_registration_and_keeps_literal_arguments() {
    let directory = tempdir().unwrap();
    let tool = directory.path().join("host-tool");
    executable(&tool);
    let policy = policy(directory.path().as_os_str());
    assert_eq!(policy.mode(), HostExecMode::OnDemand);
    assert_eq!(policy.command_count(), 0);
    let found = policy.find_command("host-tool").unwrap().unwrap();
    let authorized = policy.authorize(&request("host-tool")).unwrap();
    assert_eq!(found.declared(), tool);
    assert_eq!(authorized.executable(), tool);
    assert_eq!(authorized.arguments(), ["literal; $(never-run)"]);
    assert!(policy.find_command("absent").unwrap().is_none());
    for name in [
        tool.to_str().unwrap(),
        "../host-tool",
        "./host-tool",
        "",
        ".",
        "..",
    ] {
        assert!(policy.authorize(&request(name)).is_err(), "{name}");
    }
}

#[test]
fn on_demand_requires_a_bounded_printable_purpose_even_for_pinned_commands() {
    let directory = tempdir().unwrap();
    executable(&directory.path().join("host-tool"));
    let policy = policy(directory.path().as_os_str());
    for reason in [
        None,
        Some("".to_owned()),
        Some("  ".to_owned()),
        Some("x".repeat(241)),
        Some("line\nbreak".to_owned()),
        Some("escape\u{1b}".to_owned()),
    ] {
        let mut request = request("host-tool");
        request.reason = reason;
        assert_eq!(
            policy.authorize(&request).unwrap_err(),
            HostExecAuthorizationError::PurposeRequired
        );
    }
    let mut request = request("host-tool");
    request.reason = Some("需要宿主的 macOS 工具链".to_owned());
    assert!(policy.authorize(&request).is_ok());
    request.reason = Some("sensitive-purpose-value".to_owned());
    assert!(!format!("{request:?}").contains("sensitive-purpose-value"));
}

#[test]
fn discovery_is_cached_and_rejects_a_retargeted_symlink() {
    let directory = tempdir().unwrap();
    let first = directory.path().join("first");
    let second = directory.path().join("second");
    executable(&first);
    executable(&second);
    let shim = directory.path().join("host-tool");
    symlink(&first, &shim).unwrap();
    let policy = policy(directory.path().as_os_str());
    assert_eq!(
        policy
            .find_command("host-tool")
            .unwrap()
            .unwrap()
            .resolved(),
        fs::canonicalize(&first).unwrap()
    );
    fs::remove_file(&shim).unwrap();
    symlink(&second, &shim).unwrap();
    assert_eq!(
        policy.authorize(&request("host-tool")).unwrap_err(),
        HostExecAuthorizationError::CommandChanged {
            command: "host-tool".to_owned()
        }
    );
    assert!(policy.find_command("host-tool").is_err());
}

#[test]
fn absolute_path_directories_and_pinned_overrides_determine_selection() {
    let directory = tempdir().unwrap();
    let first = directory.path().join("first");
    let second = directory.path().join("second");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    executable(&first.join("host-tool"));
    executable(&second.join("host-tool"));
    let path = std::env::join_paths([
        Path::new(""),
        Path::new("relative"),
        first.as_path(),
        second.as_path(),
    ])
    .unwrap();
    let policy = policy(&path);
    assert_eq!(
        policy
            .authorize(&request("host-tool"))
            .unwrap()
            .executable(),
        first.join("host-tool")
    );
    let mut profile = parse_profile(PROFILE).unwrap();
    let mut pinned = parse_profile(include_str!("../fixtures/profiles/valid/default.toml"))
        .unwrap()
        .host
        .exec
        .allow
        .remove(0);
    pinned.name = "host-tool".to_owned();
    pinned.executable = second.join("host-tool");
    profile.host.exec.allow.push(pinned);
    let policy = HostExecPolicy::from_profile(
        &profile.host.exec,
        BTreeMap::from([(OsString::from("PATH"), path)]),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        policy
            .authorize(&request("host-tool"))
            .unwrap()
            .executable(),
        second.join("host-tool")
    );
    let mut missing_reason = request("host-tool");
    missing_reason.reason = None;
    assert!(policy.authorize(&missing_reason).is_err());
}

#[test]
fn allowlist_mode_does_not_grant_path_tools_and_on_demand_does_not_use_ambient_path() {
    let directory = tempdir().unwrap();
    executable(&directory.path().join("host-tool"));
    let profile = parse_profile(include_str!("../fixtures/profiles/valid/default.toml")).unwrap();
    let policy = HostExecPolicy::from_profile(
        &profile.host.exec,
        BTreeMap::from([(
            OsString::from("PATH"),
            directory.path().as_os_str().to_owned(),
        )]),
    )
    .unwrap()
    .unwrap();
    assert!(policy.find_command("host-tool").unwrap().is_none());
    assert!(matches!(
        policy.authorize(&request("host-tool")),
        Err(HostExecAuthorizationError::CommandNotAllowed { .. })
    ));
    let profile = parse_profile(PROFILE).unwrap();
    let policy = HostExecPolicy::from_profile(&profile.host.exec, BTreeMap::new())
        .unwrap()
        .unwrap();
    assert!(policy.find_command("host-tool").unwrap().is_none());
}
