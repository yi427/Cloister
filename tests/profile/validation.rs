//! Static Profile validation integration tests.

use cloister::profile::{
    NetworkMode, PROFILE_SCHEMA_VERSION, Profile, ProfileValidationErrors, parse_profile,
    validate_profile,
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
    profile.network.mode = NetworkMode::Restricted;
    profile.agents.codex.enabled = false;
    profile.agents.claude.enabled = false;

    let errors =
        error_entries(validate_profile(&profile).expect_err("invalid profile should fail"));

    assert_eq!(errors.len(), 5);
    assert!(errors.iter().any(|(path, _)| path == "schema_version"));
    assert!(errors.iter().any(|(path, _)| path == "name"));
    assert!(errors.iter().any(|(path, _)| path == "image.reference"));
    assert!(errors.iter().any(|(path, _)| path == "network.mode"));
    assert!(errors.iter().any(|(path, _)| path == "agents"));
}
