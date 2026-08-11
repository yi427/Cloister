use std::{
    fs,
    io::Write,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use cloister::profile::load_profile;
use tempfile::tempdir;

const RELEASE_IMAGE: &str = concat!("ghcr.io/yi427/cloister:", env!("CARGO_PKG_VERSION"));
const OLD_IMAGE: &str = "ghcr.io/yi427/cloister:0.0.0";

const FAKE_CONTAINER: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$CLOISTER_TEST_COMMAND_LOG"

if [ "$1:$2" = "image:inspect" ]; then
  if [ -f "$CLOISTER_TEST_IMAGE_STATE" ]; then
    printf '%s\n' '[{"variants":[{"platform":{"architecture":"arm64","os":"linux"}}]}]'
    exit 0
  fi
  printf '%s\n' 'image not found' >&2
  exit 1
fi

if [ "$1:$2:$3:$4" = "image:pull:--arch:arm64" ]; then
  : > "$CLOISTER_TEST_IMAGE_STATE"
  exit 0
fi

exit 90
"#;

const PULL_FAILURE_CONTAINER: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$CLOISTER_TEST_COMMAND_LOG"

if [ "$1:$2" = "image:inspect" ]; then
  printf '%s\n' 'image not found' >&2
  exit 1
fi

if [ "$1:$2:$3:$4" = "image:pull:--arch:arm64" ]; then
  printf '%s\n' 'registry unavailable' >&2
  exit 22
fi

exit 90
"#;

const WRONG_ARCH_CONTAINER: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$CLOISTER_TEST_COMMAND_LOG"

if [ "$1:$2" = "image:inspect" ]; then
  if [ -f "$CLOISTER_TEST_IMAGE_STATE" ]; then
    printf '%s\n' '[{"variants":[{"platform":{"architecture":"amd64","os":"linux"}}]}]'
    exit 0
  fi
  printf '%s\n' 'image not found' >&2
  exit 1
fi

if [ "$1:$2:$3:$4" = "image:pull:--arch:arm64" ]; then
  : > "$CLOISTER_TEST_IMAGE_STATE"
  exit 0
fi

exit 90
"#;

#[test]
fn dry_run_reports_the_release_image_upgrade_without_side_effects() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let profile = home.join(".config/cloister/profile.toml");
    let original = write_profile(&profile, OLD_IMAGE);

    let output = run(
        &home,
        directory.path(),
        None,
        "",
        &[
            "profile",
            "upgrade",
            "--profile",
            profile.to_str().expect("profile path should be UTF-8"),
            "--dry-run",
        ],
        &[],
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("Profile upgrade plan:"));
    assert!(stdout.contains(&format!("Image: {OLD_IMAGE} -> {RELEASE_IMAGE}")));
    assert!(stdout.contains("Profile schema: 6 (unchanged)"));
    assert!(stdout.contains("Dry run: no image pulled and no files changed."));
    assert_eq!(fs::read(&profile).expect("Profile should remain"), original);
    assert!(!backup_path(&profile).exists());
}

#[test]
fn pulls_verifies_backs_up_and_atomically_updates_the_profile() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let bin = directory.path().join("bin");
    let state = directory.path().join("state");
    let profile = home.join(".config/cloister/profile.toml");
    let image_state = state.join("image");
    let command_log = state.join("commands.log");
    fs::create_dir_all(&state).expect("state directory should exist");
    write_executable(&bin.join("container"), FAKE_CONTAINER);
    let original = write_profile(&profile, OLD_IMAGE);
    fs::set_permissions(&profile, fs::Permissions::from_mode(0o640))
        .expect("Profile mode should be set");

    let output = run(
        &home,
        directory.path(),
        Some(&bin),
        "\ny\n",
        &[
            "profile",
            "upgrade",
            "--profile",
            profile.to_str().expect("profile path should be UTF-8"),
        ],
        &[
            ("CLOISTER_TEST_IMAGE_STATE", image_state.as_path()),
            ("CLOISTER_TEST_COMMAND_LOG", command_log.as_path()),
        ],
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("Pull the exact target ARM64 image now? [Y/n]:"));
    assert!(stdout.contains("Create the backup and update this Profile? [y/N]:"));
    assert!(stdout.contains("Run 'cloister check'"));
    let updated = fs::read_to_string(&profile).expect("updated Profile should be readable");
    assert!(updated.contains(RELEASE_IMAGE));
    assert!(updated.contains("# Preserve this Profile comment."));
    assert_eq!(
        load_profile(&profile)
            .expect("updated Profile should load")
            .image
            .reference,
        RELEASE_IMAGE
    );
    assert_eq!(
        fs::metadata(&profile)
            .expect("updated Profile metadata should exist")
            .permissions()
            .mode()
            & 0o777,
        0o640
    );

    let backup = backup_path(&profile);
    assert_eq!(
        fs::read(&backup).expect("backup should be readable"),
        original
    );
    assert_eq!(
        fs::metadata(&backup)
            .expect("backup metadata should exist")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let commands = fs::read_to_string(command_log).expect("command log should be readable");
    assert_eq!(
        commands.lines().collect::<Vec<_>>(),
        [
            format!("image inspect {RELEASE_IMAGE}"),
            format!("image pull --arch arm64 {RELEASE_IMAGE}"),
            format!("image inspect {RELEASE_IMAGE}"),
        ]
    );
}

#[test]
fn a_declined_pull_leaves_the_profile_and_backup_untouched() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let bin = directory.path().join("bin");
    let state = directory.path().join("state");
    let profile = home.join(".config/cloister/profile.toml");
    let image_state = state.join("image");
    let command_log = state.join("commands.log");
    fs::create_dir_all(&state).expect("state directory should exist");
    write_executable(&bin.join("container"), FAKE_CONTAINER);
    let original = write_profile(&profile, OLD_IMAGE);

    let output = run(
        &home,
        directory.path(),
        Some(&bin),
        "n\n",
        &[
            "profile",
            "upgrade",
            "--profile",
            profile.to_str().expect("profile path should be UTF-8"),
        ],
        &[
            ("CLOISTER_TEST_IMAGE_STATE", image_state.as_path()),
            ("CLOISTER_TEST_COMMAND_LOG", command_log.as_path()),
        ],
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("Image pull skipped. No Profile changes made."));
    assert_eq!(fs::read(&profile).expect("Profile should remain"), original);
    assert!(!backup_path(&profile).exists());
    let commands = fs::read_to_string(command_log).expect("command log should be readable");
    assert_eq!(commands, format!("image inspect {RELEASE_IMAGE}\n"));
}

#[test]
fn a_failed_pull_leaves_the_profile_and_backup_untouched() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let bin = directory.path().join("bin");
    let state = directory.path().join("state");
    let profile = home.join(".config/cloister/profile.toml");
    let command_log = state.join("commands.log");
    fs::create_dir_all(&state).expect("state directory should exist");
    write_executable(&bin.join("container"), PULL_FAILURE_CONTAINER);
    let original = write_profile(&profile, OLD_IMAGE);

    let output = run(
        &home,
        directory.path(),
        Some(&bin),
        "\n",
        &[
            "profile",
            "upgrade",
            "--profile",
            profile.to_str().expect("profile path should be UTF-8"),
        ],
        &[("CLOISTER_TEST_COMMAND_LOG", command_log.as_path())],
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr.contains("registry unavailable"),
        "unexpected error: {stderr}"
    );
    assert_eq!(fs::read(&profile).expect("Profile should remain"), original);
    assert!(!backup_path(&profile).exists());
    let commands = fs::read_to_string(command_log).expect("command log should be readable");
    assert_eq!(
        commands.lines().collect::<Vec<_>>(),
        [
            format!("image inspect {RELEASE_IMAGE}"),
            format!("image pull --arch arm64 {RELEASE_IMAGE}"),
        ]
    );
}

#[test]
fn a_wrong_architecture_after_pull_leaves_the_profile_untouched() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let bin = directory.path().join("bin");
    let state = directory.path().join("state");
    let profile = home.join(".config/cloister/profile.toml");
    let image_state = state.join("image");
    let command_log = state.join("commands.log");
    fs::create_dir_all(&state).expect("state directory should exist");
    write_executable(&bin.join("container"), WRONG_ARCH_CONTAINER);
    let original = write_profile(&profile, OLD_IMAGE);

    let output = run(
        &home,
        directory.path(),
        Some(&bin),
        "\n",
        &[
            "profile",
            "upgrade",
            "--profile",
            profile.to_str().expect("profile path should be UTF-8"),
        ],
        &[
            ("CLOISTER_TEST_IMAGE_STATE", image_state.as_path()),
            ("CLOISTER_TEST_COMMAND_LOG", command_log.as_path()),
        ],
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr.contains("has no linux/arm64 variant"),
        "unexpected error: {stderr}"
    );
    assert_eq!(fs::read(&profile).expect("Profile should remain"), original);
    assert!(!backup_path(&profile).exists());
}

#[test]
fn refuses_to_overwrite_an_existing_backup_before_runtime_work() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let profile = home.join(".config/cloister/profile.toml");
    let original = write_profile(&profile, OLD_IMAGE);
    let backup = backup_path(&profile);
    fs::write(&backup, b"existing backup\n").expect("existing backup should be written");

    let output = run(
        &home,
        directory.path(),
        None,
        "",
        &[
            "profile",
            "upgrade",
            "--profile",
            profile.to_str().expect("profile path should be UTF-8"),
        ],
        &[],
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr.contains("refusing to overwrite existing Profile backup"));
    assert_eq!(fs::read(&profile).expect("Profile should remain"), original);
    assert_eq!(
        fs::read(backup).expect("existing backup should remain"),
        b"existing backup\n"
    );
}

#[test]
fn refuses_to_rewrite_testing_custom_or_newer_official_images() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let revision = "0123456789abcdef0123456789abcdef01234567";

    for (name, reference, expected) in [
        (
            "testing.toml",
            format!("ghcr.io/yi427/cloister:sha-{revision}"),
            "automatic Profile upgrade applies only",
        ),
        (
            "custom.toml",
            "cloister:dev".to_owned(),
            "automatic Profile upgrade applies only",
        ),
        (
            "newer.toml",
            "ghcr.io/yi427/cloister:999.0.0".to_owned(),
            "install the matching CLI",
        ),
    ] {
        let profile = home.join(name);
        let original = write_profile(&profile, &reference);
        let output = run(
            &home,
            directory.path(),
            None,
            "",
            &[
                "profile",
                "upgrade",
                "--profile",
                profile.to_str().expect("profile path should be UTF-8"),
            ],
            &[],
        );
        let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

        assert_eq!(output.status.code(), Some(1));
        assert!(stderr.contains(expected), "unexpected error: {stderr}");
        assert_eq!(fs::read(&profile).expect("Profile should remain"), original);
        assert!(!backup_path(&profile).exists());
    }
}

#[test]
fn refuses_a_symbolic_link_without_touching_its_target() {
    let directory = tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let target = home.join("target.toml");
    let link = home.join("profile.toml");
    let original = write_profile(&target, OLD_IMAGE);
    symlink(&target, &link).expect("Profile symlink should be created");

    let output = run(
        &home,
        directory.path(),
        None,
        "",
        &[
            "profile",
            "upgrade",
            "--profile",
            link.to_str().expect("profile path should be UTF-8"),
        ],
        &[],
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr.contains("must be a regular file and not a symbolic link"));
    assert_eq!(fs::read(target).expect("target should remain"), original);
    assert!(
        fs::symlink_metadata(link)
            .expect("symlink should remain")
            .file_type()
            .is_symlink()
    );
}

fn write_profile(path: &Path, reference: &str) -> Vec<u8> {
    fs::create_dir_all(path.parent().expect("Profile should have a parent"))
        .expect("Profile directory should be created");
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/profile.toml"))
            .expect("example Profile should be readable")
            .replace(RELEASE_IMAGE, reference)
            .replace("[image]\n", "[image]\n# Preserve this Profile comment.\n");
    fs::write(path, source.as_bytes()).expect("Profile should be written");
    source.into_bytes()
}

fn backup_path(profile: &Path) -> PathBuf {
    let mut file_name = profile
        .file_name()
        .expect("Profile should have a file name")
        .to_os_string();
    file_name.push(".bak-0.0.0");
    profile.with_file_name(file_name)
}

fn write_executable(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("executable should have a parent"))
        .expect("bin directory should be created");
    fs::write(path, contents).expect("fake executable should be written");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .expect("fake executable should be executable");
}

fn run(
    home: &Path,
    current_directory: &Path,
    path: Option<&Path>,
    input: &str,
    arguments: &[&str],
    environment: &[(&str, &Path)],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cloister"));
    command
        .args(arguments)
        .current_dir(current_directory)
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = path {
        command.env("PATH", path);
    }
    for (name, value) in environment {
        command.env(name, value);
    }
    let mut child = command.spawn().expect("Cloister binary should start");
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(input.as_bytes())
        .expect("input should be written");
    child.wait_with_output().expect("Cloister should finish")
}
