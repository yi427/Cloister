//! Profile TOML parser integration tests.

use cloister::profile::{Architecture, NetworkMode, parse_profile};

const DEFAULT_PROFILE: &str = include_str!("../fixtures/profiles/valid/default.toml");
const INVALID_MEMORY: &str = include_str!("../fixtures/profiles/invalid/invalid-memory.toml");
const UNKNOWN_FIELD: &str = include_str!("../fixtures/profiles/invalid/unknown-field.toml");
const ZERO_CPUS: &str = include_str!("../fixtures/profiles/invalid/zero-cpus.toml");
const EXAMPLE_PROFILE: &str = include_str!("../../examples/profile.toml");

#[test]
fn parses_a_complete_profile_v2() {
    let profile = parse_profile(DEFAULT_PROFILE).expect("default profile should parse");

    assert_eq!(profile.schema_version, 2);
    assert_eq!(profile.name, "rust-default");
    assert_eq!(profile.image.architecture, Architecture::Arm64);
    assert_eq!(profile.guest.cpus.get(), 4);
    assert_eq!(profile.guest.memory.as_mebibytes(), 8192);
    assert_eq!(profile.network.mode, NetworkMode::Default);
    assert!(profile.agents.codex.enabled);
    assert!(profile.agents.claude.enabled);
}

#[test]
fn user_facing_example_matches_the_valid_fixture() {
    let example = parse_profile(EXAMPLE_PROFILE).expect("example profile should parse");
    let fixture = parse_profile(DEFAULT_PROFILE).expect("default fixture should parse");

    assert_eq!(example, fixture);
}

#[test]
fn rejects_invalid_memory_before_it_enters_the_model() {
    let error = parse_profile(INVALID_MEMORY).expect_err("invalid memory should fail");

    assert!(error.message().contains("memory must be"));
    assert!(error.span().is_some());
}

#[test]
fn rejects_zero_cpu_count_before_it_enters_the_model() {
    let error = parse_profile(ZERO_CPUS).expect_err("zero CPUs should fail");

    assert!(error.message().contains("nonzero"));
    assert!(error.span().is_some());
}

#[test]
fn rejects_unknown_fields() {
    let error = parse_profile(UNKNOWN_FIELD).expect_err("unknown field should fail");

    assert!(error.message().contains("unknown field"));
    assert!(error.span().is_some());
}

#[test]
fn reports_a_span_for_invalid_toml_syntax() {
    let error = parse_profile("schema_version =").expect_err("invalid TOML should fail");

    assert!(error.span().is_some());
}
