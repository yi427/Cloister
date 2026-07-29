//! Static Profile validation integration tests.

use std::path::PathBuf;

use cloister::profile::{
    NetworkMode, PROFILE_SCHEMA_VERSION, Profile, ProfileValidationErrors, WorkspaceMode,
    parse_profile, validate_profile,
};

const DEFAULT_PROFILE: &str = include_str!("../fixtures/profiles/valid/default.toml");

fn valid_profile() -> Profile {
    parse_profile(DEFAULT_PROFILE).expect("default profile should parse")
}

fn error_entries(report: ProfileValidationErrors) -> Vec<(String, String)> {
    let mut entries = report
        .into_inner()
        .into_iter()
        .map(|(path, error)| (path.to_string(), error.message().to_owned()))
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

#[test]
fn accepts_the_default_profile() {
    validate_profile(&valid_profile()).expect("default profile should be valid");
}

#[test]
fn aggregates_independent_validation_failures() {
    let mut profile = valid_profile();
    profile.schema_version = PROFILE_SCHEMA_VERSION + 1;
    profile.name = "  ".to_owned();
    profile.image.reference.clear();
    profile.workspace.host = PathBuf::new();
    profile.workspace.guest = PathBuf::from("workspace");
    profile.workspace.mode = WorkspaceMode::Copy;
    profile.network.mode = NetworkMode::Restricted;
    profile.agents.codex.enabled = false;
    profile.agents.claude.enabled = false;

    let errors =
        error_entries(validate_profile(&profile).expect_err("invalid profile should fail"));

    assert_eq!(errors.len(), 8);
    assert!(errors.iter().any(|(path, _)| path == "schema_version"));
    assert!(errors.iter().any(|(path, _)| path == "name"));
    assert!(errors.iter().any(|(path, _)| path == "image.reference"));
    assert!(errors.iter().any(|(path, _)| path == "workspace.host"));
    assert!(errors.iter().any(|(path, _)| path == "workspace.guest"));
    assert!(errors.iter().any(|(path, _)| path == "workspace.mode"));
    assert!(errors.iter().any(|(path, _)| path == "network.mode"));
    assert!(errors.iter().any(|(path, _)| path == "agents"));
}

#[test]
fn rejects_host_or_guest_filesystem_root() {
    let mut profile = valid_profile();
    profile.workspace.host = PathBuf::from("/");
    profile.workspace.guest = PathBuf::from("/");

    let errors = error_entries(validate_profile(&profile).expect_err("root paths should fail"));

    assert_eq!(
        errors,
        vec![
            (
                "workspace.guest".to_owned(),
                "must not be the filesystem root".to_owned(),
            ),
            (
                "workspace.host".to_owned(),
                "must not be the filesystem root".to_owned(),
            ),
        ]
    );
}

#[test]
fn rejects_parent_directory_traversal() {
    let mut profile = valid_profile();
    profile.workspace.host = PathBuf::from("../project");
    profile.workspace.guest = PathBuf::from("/workspace/../secrets");

    let errors =
        error_entries(validate_profile(&profile).expect_err("parent traversal should fail"));

    assert_eq!(
        errors,
        vec![
            (
                "workspace.guest".to_owned(),
                "must not contain '..': /workspace/../secrets".to_owned(),
            ),
            (
                "workspace.host".to_owned(),
                "must not contain '..': ../project".to_owned(),
            ),
        ]
    );
}
