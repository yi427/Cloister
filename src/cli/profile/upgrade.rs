//! Explicit upgrade of a current-schema Profile to the running CLI release image.

use std::{
    error::Error,
    ffi::OsString,
    fmt, fs,
    fs::OpenOptions,
    io::{self, BufRead, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use clap::{Args, ValueHint};
use semver::Version;
use serde::Deserialize;
use tempfile::NamedTempFile;

use crate::{
    profile::{LoadProfileError, Profile, load_profile, parse_profile, validate_profile},
    release::image::{
        ImageCompatibility, ImageCompatibilityError, classify_image, official_release_reference,
    },
    runtime::{CommandSpec, execute_output, image_pull_command},
};

use super::super::{
    check::{command_description, inspect_image},
    config::default_profile_path,
};

#[derive(Debug, Args)]
pub(super) struct ProfileUpgradeArgs {
    /// Profile V6 TOML file to upgrade.
    ///
    /// Defaults to ~/.config/cloister/profile.toml.
    #[arg(long, value_name = "PROFILE", value_hint = ValueHint::FilePath)]
    profile: Option<PathBuf>,

    /// Print the upgrade plan without pulling an image or changing the Profile.
    #[arg(long)]
    dry_run: bool,
}

impl ProfileUpgradeArgs {
    pub(super) async fn execute(self) -> Result<(), ProfileUpgradeError> {
        let path = self
            .profile
            .or_else(default_profile_path)
            .ok_or(ProfileUpgradeError::HomeDirectoryMissing)?;
        let metadata = inspect_profile_target(&path)?;
        let source = fs::read_to_string(&path).map_err(|source| ProfileUpgradeError::Read {
            path: path.clone(),
            source,
        })?;
        let profile = load_profile(&path).map_err(ProfileUpgradeError::Load)?;

        let (found, expected) = match classify_image(&profile.image.reference) {
            Ok(ImageCompatibility::PairedRelease { version }) => {
                println!(
                    "Profile '{}' already uses the paired official image {}.",
                    profile.name, version
                );
                println!("No changes made.");
                return Ok(());
            }
            Err(ImageCompatibilityError::OfficialReleaseMismatch { expected, found })
                if found < expected =>
            {
                (found, expected)
            }
            Err(error) => return Err(ProfileUpgradeError::Compatibility(error)),
            Ok(compatibility) => {
                return Err(ProfileUpgradeError::UnmanagedImage {
                    detail: compatibility.detail(&profile.image.reference),
                });
            }
        };

        let target_reference = official_release_reference(&expected);
        let updated_source = replace_image_reference(&source, &profile, &target_reference)?;
        let backup = backup_path(&path, &found)?;
        reject_existing_backup(&backup)?;
        print_plan(
            &path,
            &backup,
            &profile,
            &found,
            &expected,
            &target_reference,
        );

        if self.dry_run {
            println!("Dry run: no image pulled and no files changed.");
            return Ok(());
        }

        let stdin = io::stdin();
        let mut input = stdin.lock();
        let stdout = io::stdout();
        let mut output = stdout.lock();
        let mut pulled = false;

        match inspect_image(&target_reference, profile.image.architecture).await {
            Ok(detail) => writeln!(output, "Target image is available: {detail}")?,
            Err(detail) => {
                writeln!(output, "Target image is not available: {detail}")?;
                if !prompt_yes_no(
                    &mut input,
                    &mut output,
                    "Pull the exact target ARM64 image now?",
                    true,
                )? {
                    writeln!(output, "Image pull skipped. No Profile changes made.")?;
                    return Ok(());
                }
                run_required_command(&image_pull_command(&target_reference), &mut output).await?;
                inspect_image(&target_reference, profile.image.architecture)
                    .await
                    .map_err(ProfileUpgradeError::TargetImage)?;
                pulled = true;
            }
        }

        if !prompt_yes_no(
            &mut input,
            &mut output,
            "Create the backup and update this Profile?",
            false,
        )? {
            if pulled {
                writeln!(
                    output,
                    "Profile update skipped. The pulled target image remains available."
                )?;
            } else {
                writeln!(output, "Profile update skipped. No changes made.")?;
            }
            return Ok(());
        }

        write_backup(&backup, source.as_bytes())?;
        replace_profile_atomically(&path, &updated_source, metadata.permissions().mode())?;
        writeln!(output, "Updated Profile at {}.", path.display())?;
        writeln!(output, "Backup saved at {}.", backup.display())?;
        writeln!(
            output,
            "Run 'cloister check' to verify the upgraded environment."
        )?;
        Ok(())
    }
}

fn inspect_profile_target(path: &Path) -> Result<fs::Metadata, ProfileUpgradeError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| ProfileUpgradeError::Inspect {
        path: path.to_owned(),
        source,
    })?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(ProfileUpgradeError::UnsafeTarget {
            path: path.to_owned(),
        });
    }
    Ok(metadata)
}

fn print_plan(
    path: &Path,
    backup: &Path,
    profile: &Profile,
    found: &Version,
    expected: &Version,
    target_reference: &str,
) {
    println!("Profile upgrade plan:");
    println!("  Path: {}", path.display());
    println!("  Profile: {}", profile.name);
    println!("  Profile schema: {} (unchanged)", profile.schema_version);
    println!("  CLI release: {expected}");
    println!("  Image: {} -> {target_reference}", profile.image.reference);
    println!("  Previous image version: {found}");
    println!("  Backup: {}", backup.display());
}

async fn run_required_command(
    command: &CommandSpec,
    output: &mut impl Write,
) -> Result<(), ProfileUpgradeError> {
    let description = command_description(command);
    writeln!(output, "Running: {description}")?;
    output.flush()?;
    let result = execute_output(command)
        .await
        .map_err(|error| ProfileUpgradeError::Runtime(error.to_string()))?;
    if !result.status.success() {
        let detail = String::from_utf8_lossy(&result.stderr).trim().to_owned();
        return Err(ProfileUpgradeError::Runtime(if detail.is_empty() {
            format!("'{description}' exited with {}", result.status)
        } else {
            format!("'{description}' failed: {detail}")
        }));
    }
    writeln!(output, "Completed: {description}")?;
    Ok(())
}

fn prompt_yes_no(
    input: &mut impl BufRead,
    output: &mut impl Write,
    question: &str,
    default: bool,
) -> Result<bool, ProfileUpgradeError> {
    let choices = if default { "Y/n" } else { "y/N" };
    loop {
        write!(output, "{question} [{choices}]: ")?;
        output.flush()?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Err(ProfileUpgradeError::InputClosed);
        }
        match line.trim().to_ascii_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => writeln!(output, "Please answer yes or no.")?,
        }
    }
}

#[derive(Deserialize)]
struct ReferenceDocument {
    image: ReferenceImage,
}

#[derive(Deserialize)]
struct ReferenceImage {
    reference: toml::Spanned<String>,
}

fn replace_image_reference(
    source: &str,
    profile: &Profile,
    target_reference: &str,
) -> Result<String, ProfileUpgradeError> {
    let document: ReferenceDocument =
        toml::from_str(source).map_err(|error| ProfileUpgradeError::Edit(error.to_string()))?;
    if document.image.reference.get_ref() != &profile.image.reference {
        return Err(ProfileUpgradeError::Edit(
            "parsed image reference did not match the validated Profile".to_owned(),
        ));
    }
    let span = document.image.reference.span();
    if source.get(span.clone()).is_none() {
        return Err(ProfileUpgradeError::Edit(
            "image reference span was outside the Profile source".to_owned(),
        ));
    }
    let replacement = toml::Value::String(target_reference.to_owned()).to_string();
    let mut updated = String::with_capacity(source.len() + replacement.len());
    updated.push_str(&source[..span.start]);
    updated.push_str(&replacement);
    updated.push_str(&source[span.end..]);

    let updated_profile =
        parse_profile(&updated).map_err(|error| ProfileUpgradeError::Edit(error.to_string()))?;
    validate_profile(&updated_profile)
        .map_err(|error| ProfileUpgradeError::Edit(error.to_string()))?;
    if updated_profile.image.reference != target_reference {
        return Err(ProfileUpgradeError::Edit(
            "updated Profile did not contain the target image reference".to_owned(),
        ));
    }
    Ok(updated)
}

fn backup_path(path: &Path, found: &Version) -> Result<PathBuf, ProfileUpgradeError> {
    let Some(file_name) = path.file_name() else {
        return Err(ProfileUpgradeError::UnsafeTarget {
            path: path.to_owned(),
        });
    };
    let mut backup_name = OsString::from(file_name);
    backup_name.push(format!(".bak-{found}"));
    Ok(path.with_file_name(backup_name))
}

fn reject_existing_backup(path: &Path) -> Result<(), ProfileUpgradeError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(ProfileUpgradeError::BackupExists {
            path: path.to_owned(),
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(ProfileUpgradeError::Inspect {
            path: path.to_owned(),
            source,
        }),
    }
}

fn write_backup(path: &Path, source: &[u8]) -> Result<(), ProfileUpgradeError> {
    let mut backup = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|source| ProfileUpgradeError::Write {
            path: path.to_owned(),
            source,
        })?;
    backup
        .write_all(source)
        .and_then(|()| backup.sync_all())
        .map_err(|source| ProfileUpgradeError::Write {
            path: path.to_owned(),
            source,
        })
}

fn replace_profile_atomically(
    path: &Path,
    source: &str,
    permissions: u32,
) -> Result<(), ProfileUpgradeError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary =
        NamedTempFile::new_in(parent).map_err(|source| ProfileUpgradeError::Write {
            path: path.to_owned(),
            source,
        })?;
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(permissions & 0o777))
        .and_then(|()| temporary.write_all(source.as_bytes()))
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| ProfileUpgradeError::Write {
            path: path.to_owned(),
            source,
        })?;
    temporary
        .persist(path)
        .map_err(|error| ProfileUpgradeError::Write {
            path: path.to_owned(),
            source: error.error,
        })?;
    Ok(())
}

#[derive(Debug)]
pub(in crate::cli) enum ProfileUpgradeError {
    HomeDirectoryMissing,
    Inspect { path: PathBuf, source: io::Error },
    UnsafeTarget { path: PathBuf },
    Read { path: PathBuf, source: io::Error },
    Load(LoadProfileError),
    Compatibility(ImageCompatibilityError),
    UnmanagedImage { detail: String },
    BackupExists { path: PathBuf },
    TargetImage(String),
    Runtime(String),
    Edit(String),
    Write { path: PathBuf, source: io::Error },
    Input(io::Error),
    InputClosed,
}

impl fmt::Display for ProfileUpgradeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HomeDirectoryMissing => formatter
                .write_str("cannot resolve the default Profile because HOME is unavailable"),
            Self::Inspect { path, source } => {
                write!(
                    formatter,
                    "failed to inspect '{}': {source}",
                    path.display()
                )
            }
            Self::UnsafeTarget { path } => write!(
                formatter,
                "refusing to upgrade '{}': the Profile must be a regular file and not a symbolic link",
                path.display()
            ),
            Self::Read { path, source } => {
                write!(
                    formatter,
                    "failed to read Profile '{}': {source}",
                    path.display()
                )
            }
            Self::Load(error) => error.fmt(formatter),
            Self::Compatibility(error) => error.fmt(formatter),
            Self::UnmanagedImage { detail } => write!(
                formatter,
                "automatic Profile upgrade applies only to an older official X.Y.Z release image: {detail}"
            ),
            Self::BackupExists { path } => write!(
                formatter,
                "refusing to overwrite existing Profile backup '{}'",
                path.display()
            ),
            Self::TargetImage(detail) => {
                write!(formatter, "target image verification failed: {detail}")
            }
            Self::Runtime(detail) => formatter.write_str(detail),
            Self::Edit(detail) => write!(formatter, "failed to update Profile source: {detail}"),
            Self::Write { path, source } => {
                write!(formatter, "failed to write '{}': {source}", path.display())
            }
            Self::Input(source) => write!(formatter, "interactive input failed: {source}"),
            Self::InputClosed => {
                formatter.write_str("interactive input closed before upgrade completed")
            }
        }
    }
}

impl Error for ProfileUpgradeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Inspect { source, .. }
            | Self::Read { source, .. }
            | Self::Write { source, .. }
            | Self::Input(source) => Some(source),
            Self::Load(error) => Some(error),
            Self::Compatibility(error) => Some(error),
            Self::HomeDirectoryMissing
            | Self::UnsafeTarget { .. }
            | Self::UnmanagedImage { .. }
            | Self::BackupExists { .. }
            | Self::TargetImage(_)
            | Self::Runtime(_)
            | Self::Edit(_)
            | Self::InputClosed => None,
        }
    }
}

impl From<io::Error> for ProfileUpgradeError {
    fn from(source: io::Error) -> Self {
        Self::Input(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_only_the_spanned_image_reference() {
        let source = r#"schema_version = 6
name = "example"

[image]
# Keep this comment.
reference = "ghcr.io/yi427/cloister:0.1.0" # And this one.
architecture = "arm64"

[guest]
cpus = 4
memory = "8G"
user = "cloister"
locale = "en_US.UTF-8"
timezone = "America/New_York"

[network]
mode = "default"
proxy = "disabled"

[agent]
state = "isolated"

[host.exec]
enabled = false
allow = []

[host.exec.environment]
mode = "inherit-all"
"#;
        let profile = parse_profile(source).expect("source should parse");

        let updated = replace_image_reference(source, &profile, "ghcr.io/yi427/cloister:0.2.0")
            .expect("image reference should update");

        assert!(updated.contains("# Keep this comment."));
        assert!(updated.contains("# And this one."));
        assert!(updated.contains("reference = \"ghcr.io/yi427/cloister:0.2.0\""));
        assert_eq!(
            updated.replace("0.2.0", "0.1.0"),
            source,
            "only the image version should change"
        );
    }
}
