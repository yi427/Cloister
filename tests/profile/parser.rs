//! Profile TOML parser integration tests.

use cloister::profile::{Architecture, NetworkMode, parse_profile};

const DEFAULT_PROFILE: &str = include_str!("../fixtures/profiles/valid/default.toml");
const INVALID_MEMORY: &str = include_str!("../fixtures/profiles/invalid/invalid-memory.toml");
const UNKNOWN_FIELD: &str = include_str!("../fixtures/profiles/invalid/unknown-field.toml");
const ZERO_CPUS: &str = include_str!("../fixtures/profiles/invalid/zero-cpus.toml");
const EXAMPLE_PROFILE: &str = include_str!("../../examples/codex.toml");

#[test]
fn parses_a_complete_profile_v3() {
    let profile = parse_profile(DEFAULT_PROFILE).expect("default profile should parse");

    assert_eq!(profile.schema_version, 3);
    assert_eq!(profile.name, "rust-default");
    assert_eq!(profile.image.architecture, Architecture::Arm64);
    assert_eq!(profile.guest.cpus.get(), 4);
    assert_eq!(profile.guest.memory.as_mebibytes(), 8192);
    assert_eq!(profile.network.mode, NetworkMode::Default);
    assert_eq!(profile.codex.state, cloister::profile::AgentState::Isolated);
}

#[test]
fn parses_the_user_facing_codex_example() {
    let example = parse_profile(EXAMPLE_PROFILE).expect("example profile should parse");

    assert_eq!(example.name, "codex-default");
    assert_eq!(example.codex.state, cloister::profile::AgentState::Shared);
}

#[test]
fn rejects_the_removed_proxy_setting() {
    let source = DEFAULT_PROFILE.replace(
        "mode = \"default\"",
        "mode = \"default\"\nproxy = \"http://host.container.internal:7890\"",
    );

    let error = parse_profile(&source).expect_err("proxy setting should no longer be accepted");

    assert!(error.message().contains("unknown field"));
    assert!(error.span().is_some());
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
