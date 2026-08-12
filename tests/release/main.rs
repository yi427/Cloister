//! Release metadata and user-facing version consistency contracts.

const README: &str = include_str!("../../README.md");
const RELEASING: &str = include_str!("../../docs/releasing.md");
const EXAMPLE_PROFILE: &str = include_str!("../../examples/profile.toml");
const CONTAINERFILE: &str = include_str!("../../images/rust-node/Containerfile");
const CURRENT_RELEASE_NOTES: &str = include_str!("../../docs/releases/v0.2.0.md");
const LICENSE_APACHE: &str = include_str!("../../LICENSE-APACHE");
const LICENSE_MIT: &str = include_str!("../../LICENSE-MIT");

#[test]
fn release_surfaces_track_the_package_version_and_license() {
    let version = env!("CARGO_PKG_VERSION");
    let tag = format!("v{version}");
    let image = format!("ghcr.io/yi427/cloister:{version}");

    assert_eq!(env!("CARGO_PKG_LICENSE"), "MIT OR Apache-2.0");
    assert_eq!(
        env!("CARGO_PKG_REPOSITORY"),
        "https://github.com/yi427/Cloister"
    );
    assert!(README.contains(&format!("--tag {tag}")));
    assert!(README.contains(&image));
    assert!(RELEASING.contains(&format!("## {version} checklist")));
    assert!(RELEASING.contains(&tag));
    assert!(RELEASING.contains(&image));
    assert!(EXAMPLE_PROFILE.contains(&format!("reference = \"{image}\"")));
    assert!(CURRENT_RELEASE_NOTES.contains(&format!("# Cloister {version}")));
    assert!(CURRENT_RELEASE_NOTES.contains(&tag));
    assert!(CURRENT_RELEASE_NOTES.contains(&image));
    assert!(LICENSE_APACHE.contains("Apache License"));
    assert!(LICENSE_MIT.contains("Copyright (c) 2026 Cloister contributors"));
}

#[test]
fn release_notes_track_the_guest_toolchain_pins() {
    let node = pin_between("FROM docker.io/library/node:", "-bookworm-slim");
    let rust = pin_after("ARG RUST_VERSION=");
    let codex = pin_after("ARG CODEX_VERSION=");
    let claude = pin_after("ARG CLAUDE_CODE_VERSION=");

    assert!(CURRENT_RELEASE_NOTES.contains(&format!("| Node.js | {node} |")));
    assert!(CURRENT_RELEASE_NOTES.contains(&format!("| Rust | {rust} |")));
    assert!(CURRENT_RELEASE_NOTES.contains(&format!("| Codex | {codex} |")));
    assert!(CURRENT_RELEASE_NOTES.contains(&format!("| Claude Code | {claude} |")));
}

fn pin_after(prefix: &str) -> &'static str {
    CONTAINERFILE
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .unwrap_or_else(|| panic!("Containerfile must define {prefix}"))
}

fn pin_between(prefix: &str, suffix: &str) -> &'static str {
    pin_after(prefix)
        .strip_suffix(suffix)
        .unwrap_or_else(|| panic!("Containerfile pin after {prefix} must end with {suffix}"))
}
