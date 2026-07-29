use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn fixture(relative_path: impl AsRef<Path>) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/profiles")
        .join(relative_path)
}

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cloister"))
        .args(arguments)
        .output()
        .expect("Cloister binary should start")
}

#[test]
fn checks_a_valid_profile() {
    let path = fixture("valid/default.toml");
    let output = run(&["profile", "check", path.to_str().expect("UTF-8 path")]);

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        "Profile 'rust-default' is valid.\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn reports_a_missing_profile() {
    let path = fixture("missing.toml");
    let output = run(&["profile", "check", path.to_str().expect("UTF-8 path")]);
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("failed to read profile"));
    assert!(stderr.contains("missing.toml"));
}

#[test]
fn reports_a_profile_parse_error() {
    let path = fixture("invalid/invalid-memory.toml");
    let output = run(&["profile", "check", path.to_str().expect("UTF-8 path")]);
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("invalid-memory.toml"));
    assert!(stderr.contains("memory must be a positive integer"));
}

#[test]
fn reports_a_profile_validation_error() {
    let path = fixture("invalid/unsupported-schema.toml");
    let output = run(&["profile", "check", path.to_str().expect("UTF-8 path")]);
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("profile validation failed"));
    assert!(stderr.contains("schema_version is not supported"));
}
