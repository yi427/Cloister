//! Profile TOML parser integration tests.

use cloister::profile::{Architecture, HostExecEnvironmentMode, NetworkMode, parse_profile};

const DEFAULT_PROFILE: &str = include_str!("../fixtures/profiles/valid/default.toml");
const INVALID_MEMORY: &str = include_str!("../fixtures/profiles/invalid/invalid-memory.toml");
const UNSUPPORTED_SCHEMA: &str =
    include_str!("../fixtures/profiles/invalid/unsupported-schema.toml");
const UNKNOWN_FIELD: &str = include_str!("../fixtures/profiles/invalid/unknown-field.toml");
const ZERO_CPUS: &str = include_str!("../fixtures/profiles/invalid/zero-cpus.toml");
const RELATIVE_HOST_EXECUTABLE: &str =
    include_str!("../fixtures/profiles/invalid/relative-host-executable.toml");
const EXAMPLE_PROFILE: &str = include_str!("../../examples/profile.toml");

#[test]
fn parses_a_complete_profile_v5() {
    let profile = parse_profile(DEFAULT_PROFILE).expect("default profile should parse");

    assert_eq!(profile.schema_version, 5);
    assert_eq!(profile.name, "rust-default");
    assert_eq!(profile.image.architecture, Architecture::Arm64);
    assert_eq!(profile.guest.cpus.get(), 4);
    assert_eq!(profile.guest.memory.as_mebibytes(), 8192);
    assert_eq!(profile.network.mode, NetworkMode::Default);
    assert_eq!(profile.agent.state, cloister::profile::AgentState::Isolated);
    assert!(profile.host.exec.enabled);
    assert_eq!(
        profile.host.exec.environment.mode,
        HostExecEnvironmentMode::InheritAll
    );
    assert_eq!(profile.host.exec.allow[0].name, "xcodebuild");
}

#[test]
fn parses_the_user_facing_profile_example() {
    let example = parse_profile(EXAMPLE_PROFILE).expect("example profile should parse");

    assert_eq!(example.name, "default");
    assert_eq!(example.agent.state, cloister::profile::AgentState::Shared);
}

#[test]
fn rejects_profile_v4_before_inspecting_its_fields() {
    let error = parse_profile(UNSUPPORTED_SCHEMA).expect_err("Profile V4 should be rejected");
    let rendered = error.to_string();

    assert_eq!(error.message(), "schema_version is not supported");
    assert!(error.span().is_some());
    assert!(rendered.contains("found 4; expected 5"));
    assert!(!rendered.contains("unknown field"));
}

#[test]
fn rejects_the_removed_codex_table_in_profile_v5() {
    let source = DEFAULT_PROFILE.replace("[agent]", "[codex]");

    let error = parse_profile(&source).expect_err("the removed Codex table should fail");

    assert!(error.message().contains("unknown field"));
    assert!(error.span().is_some());
}

#[test]
fn requires_an_explicit_host_policy() {
    let source = DEFAULT_PROFILE.split("\n[host.exec]\n").next().unwrap();

    let error = parse_profile(source).expect_err("the host policy should be required");

    assert!(error.message().contains("missing field"));
    assert!(error.span().is_some());
}

#[test]
fn parses_a_relative_host_executable_before_static_validation() {
    let profile = parse_profile(RELATIVE_HOST_EXECUTABLE)
        .expect("relative paths are a static validation concern");

    assert_eq!(
        profile.host.exec.allow[0].executable,
        std::path::Path::new("usr/bin/xcodebuild")
    );
}

#[test]
fn rejects_an_unknown_host_environment_mode() {
    let source = DEFAULT_PROFILE.replace("inherit-all", "filtered");

    let error = parse_profile(&source).expect_err("unknown environment mode should fail closed");

    assert!(error.message().contains("unknown variant"));
    assert!(error.span().is_some());
}

#[test]
fn rejects_an_unknown_host_argument_policy() {
    let source = DEFAULT_PROFILE.replace("arguments = \"any\"", "arguments = \"prefix\"");

    let error = parse_profile(&source).expect_err("unknown argument policy should fail closed");

    assert!(error.message().contains("unknown variant"));
    assert!(error.span().is_some());
}

#[test]
fn requires_an_explicit_agent_policy() {
    let source = DEFAULT_PROFILE.replace("\n[agent]\nstate = \"isolated\"\n", "\n");

    let error = parse_profile(&source).expect_err("the agent policy should be required");

    assert!(error.message().contains("missing field"));
    assert!(error.span().is_some());
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
