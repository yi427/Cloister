use std::path::{Path, PathBuf};

use cloister::{
    preflight::{PreflightError, resolve_launch},
    profile::load_profile,
};

fn fixture(relative_path: impl AsRef<Path>) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/profiles")
        .join(relative_path)
}

#[test]
fn resolves_the_workspace_selected_for_this_execution() {
    let path = fixture("valid/default.toml");
    let profile = load_profile(&path).expect("default profile should load");
    let workspace = fixture("valid");

    let resolved = resolve_launch(profile, &workspace).expect("explicit workspace should resolve");

    assert_eq!(
        resolved.workspace(),
        workspace
            .canonicalize()
            .expect("workspace should resolve")
            .as_path()
    );
}

#[test]
fn rejects_a_workspace_that_is_a_file() {
    let path = fixture("valid/default.toml");
    let profile = load_profile(&path).expect("default profile should load");

    let error = resolve_launch(profile, &path).expect_err("workspace file should fail");

    assert!(matches!(
        error,
        PreflightError::WorkspaceNotDirectory { .. }
    ));
}

#[test]
fn rejects_a_workspace_that_does_not_exist() {
    let path = fixture("valid/default.toml");
    let profile = load_profile(&path).expect("default profile should load");

    let error = resolve_launch(profile, fixture("does-not-exist"))
        .expect_err("missing workspace should fail");

    assert!(matches!(error, PreflightError::WorkspacePath { .. }));
}

#[test]
fn rejects_the_host_filesystem_root_as_a_workspace() {
    let path = fixture("valid/default.toml");
    let profile = load_profile(&path).expect("default profile should load");

    let error = resolve_launch(profile, Path::new("/")).expect_err("host root should fail");

    assert!(matches!(error, PreflightError::WorkspaceIsRoot { .. }));
}
