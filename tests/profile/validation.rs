//! Static Profile validation integration tests.

use cloister::profile::{
    PROFILE_SCHEMA_VERSION, Profile, ProfileValidationErrors, parse_profile, validate_profile,
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
    profile.guest.user.clear();
    profile.guest.locale.clear();
    profile.guest.timezone.clear();

    let errors =
        error_entries(validate_profile(&profile).expect_err("invalid profile should fail"));

    assert_eq!(errors.len(), 6);
    assert!(errors.iter().any(|(path, _)| path == "schema_version"));
    assert!(errors.iter().any(|(path, _)| path == "name"));
    assert!(errors.iter().any(|(path, _)| path == "image.reference"));
    assert!(errors.iter().any(|(path, _)| path == "guest.user"));
    assert!(errors.iter().any(|(path, _)| path == "guest.locale"));
    assert!(errors.iter().any(|(path, _)| path == "guest.timezone"));
}

#[test]
fn rejects_relative_and_duplicate_host_command_entries() {
    let mut profile = valid_profile();
    profile.host.exec.allow[0].executable = "usr/bin/xcodebuild".into();
    profile
        .host
        .exec
        .allow
        .push(profile.host.exec.allow[0].clone());

    let errors =
        error_entries(validate_profile(&profile).expect_err("invalid host policy should fail"));

    assert!(errors.iter().any(|(path, message)| {
        path == "host.exec.allow[0].executable" && message.contains("absolute path")
    }));
    assert!(
        errors
            .iter()
            .any(|(path, message)| { path == "host.exec.allow" && message.contains("unique") })
    );
}

#[test]
fn rejects_blank_host_command_metadata() {
    let mut profile = valid_profile();
    profile.host.exec.allow[0].name = "  ".to_owned();
    profile.host.exec.allow[0].description.clear();

    let errors =
        error_entries(validate_profile(&profile).expect_err("blank command metadata should fail"));

    assert!(
        errors
            .iter()
            .any(|(path, _)| path == "host.exec.allow[0].name")
    );
    assert!(
        errors
            .iter()
            .any(|(path, _)| path == "host.exec.allow[0].description")
    );
}
