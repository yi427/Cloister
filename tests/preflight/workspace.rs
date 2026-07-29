use std::path::{Path, PathBuf};

use cloister::{
    preflight::{PreflightError, resolve_profile},
    profile::load_profile,
};

fn fixture(relative_path: impl AsRef<Path>) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/profiles")
        .join(relative_path)
}

#[test]
fn resolves_relative_workspace_from_the_profile_directory() {
    let path = fixture("valid/default.toml");
    let profile = load_profile(&path).expect("default profile should load");
    let resolved = resolve_profile(profile, &path).expect("workspace should resolve");

    assert_eq!(
        resolved.profile().workspace.host,
        path.parent()
            .expect("fixture should have a parent")
            .canonicalize()
            .expect("fixture directory should resolve")
    );
    assert_eq!(
        resolved.source(),
        path.canonicalize().expect("fixture should resolve")
    );
}

#[test]
fn rejects_a_workspace_that_is_a_file() {
    let path = fixture("valid/default.toml");
    let mut profile = load_profile(&path).expect("default profile should load");
    profile.workspace.host = PathBuf::from("default.toml");

    let error = resolve_profile(profile, path).expect_err("workspace file should fail");

    assert!(matches!(
        error,
        PreflightError::WorkspaceNotDirectory { .. }
    ));
}

#[test]
fn rejects_a_workspace_that_does_not_exist() {
    let path = fixture("valid/default.toml");
    let mut profile = load_profile(&path).expect("default profile should load");
    profile.workspace.host = PathBuf::from("does-not-exist");

    let error = resolve_profile(profile, path).expect_err("missing workspace should fail");

    assert!(matches!(error, PreflightError::WorkspacePath { .. }));
}
