//! Release metadata and user-facing version consistency contracts.

const README: &str = include_str!("../../README.md");
const RELEASING: &str = include_str!("../../docs/releasing.md");
const EXAMPLE_PROFILE: &str = include_str!("../../examples/profile.toml");
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
    assert!(LICENSE_APACHE.contains("Apache License"));
    assert!(LICENSE_MIT.contains("Copyright (c) 2026 Cloister contributors"));
}
