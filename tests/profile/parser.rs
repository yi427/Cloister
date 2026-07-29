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
    assert!(profile.network.proxy.is_none());
    assert_eq!(profile.codex.state, cloister::profile::AgentState::Isolated);
}

#[test]
fn parses_an_optional_http_proxy() {
    let source = DEFAULT_PROFILE.replace(
        "mode = \"default\"",
        "mode = \"default\"\nproxy = \"http://proxy.example:8080\"",
    );

    let profile = parse_profile(&source).expect("HTTP proxy should parse");

    assert_eq!(
        profile.network.proxy.as_ref().map(|proxy| proxy.as_str()),
        Some("http://proxy.example:8080/")
    );
}

#[test]
fn rejects_proxy_credentials_before_they_enter_the_model() {
    let source = DEFAULT_PROFILE.replace(
        "mode = \"default\"",
        "mode = \"default\"\nproxy = \"http://user:secret@proxy.example:8080\"",
    );

    let error = parse_profile(&source).expect_err("proxy credentials should fail");

    assert!(
        error
            .message()
            .contains("must not contain embedded credentials")
    );
    assert!(error.span().is_some());
}

#[test]
fn rejects_an_unsupported_proxy_scheme() {
    let source = DEFAULT_PROFILE.replace(
        "mode = \"default\"",
        "mode = \"default\"\nproxy = \"socks5://proxy.example:1080\"",
    );

    let error = parse_profile(&source).expect_err("SOCKS proxy should fail");

    assert!(error.message().contains("scheme must be http or https"));
    assert!(error.span().is_some());
}

#[test]
fn parses_the_user_facing_codex_example() {
    let example = parse_profile(EXAMPLE_PROFILE).expect("example profile should parse");

    assert_eq!(example.name, "codex-default");
    assert_eq!(example.codex.state, cloister::profile::AgentState::Shared);
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
