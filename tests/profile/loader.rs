//! Profile filesystem loading integration tests.

use std::path::{Path, PathBuf};

use cloister::profile::{LoadProfileError, load_profile};

fn fixture(relative_path: impl AsRef<Path>) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/profiles")
        .join(relative_path)
}

#[test]
fn loads_a_parsed_and_validated_profile() {
    let profile = load_profile(fixture("valid/default.toml")).expect("default profile should load");

    assert_eq!(profile.name, "rust-default");
}

#[test]
fn distinguishes_read_parse_and_validation_failures() {
    let read_error =
        load_profile(fixture("missing.toml")).expect_err("missing profile should fail");
    let parse_error = load_profile(fixture("invalid/invalid-memory.toml"))
        .expect_err("invalid memory should fail");
    let validation_error = load_profile(fixture("invalid/restricted-network.toml"))
        .expect_err("restricted network should fail");

    assert!(matches!(read_error, LoadProfileError::Read { .. }));
    assert!(matches!(parse_error, LoadProfileError::Parse { .. }));
    assert!(matches!(
        validation_error,
        LoadProfileError::Validation { .. }
    ));
}
