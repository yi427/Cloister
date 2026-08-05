//! Resolution and inspection of host executables referenced by Profile policy.

use std::{
    error::Error,
    ffi::{OsStr, OsString},
    fmt, fs, io,
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
    path::{Path, PathBuf},
};

/// An absolute host executable path and its canonical filesystem target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedHostExecutable {
    declared: PathBuf,
    resolved: PathBuf,
}

impl ResolvedHostExecutable {
    /// Absolute path declared by the Profile or selected from `PATH`.
    pub fn declared(&self) -> &Path {
        &self.declared
    }

    /// Canonical regular-file target used for inspection.
    pub fn resolved(&self) -> &Path {
        &self.resolved
    }
}

/// Inspects one exact host executable path without running it.
pub fn inspect_host_executable(
    declared: impl AsRef<Path>,
) -> Result<ResolvedHostExecutable, HostExecutableCheckError> {
    let declared = declared.as_ref().to_owned();
    if !declared.is_absolute() {
        return Err(HostExecutableCheckError::RelativePath { path: declared });
    }

    let resolved =
        fs::canonicalize(&declared).map_err(|source| HostExecutableCheckError::Resolution {
            declared: declared.clone(),
            source,
        })?;
    let metadata =
        fs::metadata(&resolved).map_err(|source| HostExecutableCheckError::Metadata {
            declared: declared.clone(),
            resolved: resolved.clone(),
            source,
        })?;

    if !metadata.is_file() {
        return Err(HostExecutableCheckError::NotRegularFile { declared, resolved });
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(HostExecutableCheckError::NotExecutable { declared, resolved });
    }

    Ok(ResolvedHostExecutable { declared, resolved })
}

/// Resolves a bare command name from a supplied `PATH` without invoking a shell.
///
/// Empty and relative `PATH` entries are ignored so lookup cannot implicitly
/// select an executable beneath the current working directory.
pub fn resolve_host_command(
    command: impl AsRef<OsStr>,
    path: Option<&OsStr>,
) -> Result<ResolvedHostExecutable, HostCommandLookupError> {
    let command = command.as_ref();
    if command.is_empty()
        || command.as_bytes().contains(&b'/')
        || command == OsStr::new(".")
        || command == OsStr::new("..")
    {
        return Err(HostCommandLookupError::InvalidCommandName {
            command: command.to_owned(),
        });
    }

    if let Some(path) = path {
        for directory in std::env::split_paths(path).filter(|entry| entry.is_absolute()) {
            let candidate = directory.join(command);
            if let Ok(executable) = inspect_host_executable(candidate) {
                return Ok(executable);
            }
        }
    }

    Err(HostCommandLookupError::NotFound {
        command: command.to_owned(),
    })
}

/// Reason an exact host executable path failed inspection.
#[derive(Debug)]
pub enum HostExecutableCheckError {
    RelativePath {
        path: PathBuf,
    },
    Resolution {
        declared: PathBuf,
        source: io::Error,
    },
    Metadata {
        declared: PathBuf,
        resolved: PathBuf,
        source: io::Error,
    },
    NotRegularFile {
        declared: PathBuf,
        resolved: PathBuf,
    },
    NotExecutable {
        declared: PathBuf,
        resolved: PathBuf,
    },
}

impl fmt::Display for HostExecutableCheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RelativePath { path } => write!(
                formatter,
                "host executable path '{}' is not absolute",
                path.display()
            ),
            Self::Resolution { declared, source } => write!(
                formatter,
                "failed to resolve host executable '{}': {source}",
                declared.display()
            ),
            Self::Metadata {
                declared,
                resolved,
                source,
            } => write!(
                formatter,
                "failed to inspect host executable '{}' (resolved as '{}'): {source}",
                declared.display(),
                resolved.display()
            ),
            Self::NotRegularFile { declared, resolved } => write!(
                formatter,
                "host executable '{}' resolves to '{}', which is not a regular file",
                declared.display(),
                resolved.display()
            ),
            Self::NotExecutable { declared, resolved } => write!(
                formatter,
                "host executable '{}' resolves to '{}' without any execute permission bit",
                declared.display(),
                resolved.display()
            ),
        }
    }
}

impl Error for HostExecutableCheckError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Resolution { source, .. } | Self::Metadata { source, .. } => Some(source),
            Self::RelativePath { .. }
            | Self::NotRegularFile { .. }
            | Self::NotExecutable { .. } => None,
        }
    }
}

/// Reason a bare command name could not be resolved from `PATH`.
#[derive(Debug, Eq, PartialEq)]
pub enum HostCommandLookupError {
    InvalidCommandName { command: OsString },
    NotFound { command: OsString },
}

impl fmt::Display for HostCommandLookupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCommandName { command } => write!(
                formatter,
                "host command name '{}' must be a bare executable name",
                command.to_string_lossy()
            ),
            Self::NotFound { command } => write!(
                formatter,
                "host command '{}' was not found in an absolute PATH directory",
                command.to_string_lossy()
            ),
        }
    }
}

impl Error for HostCommandLookupError {}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        os::unix::fs::{PermissionsExt, symlink},
        path::Path,
    };

    use tempfile::tempdir;

    use super::{
        HostCommandLookupError, HostExecutableCheckError, inspect_host_executable,
        resolve_host_command,
    };

    #[test]
    fn inspects_an_executable_and_preserves_its_declared_symlink() {
        let directory = tempdir().expect("temporary directory should exist");
        let target = directory.path().join("tool-target");
        let declared = directory.path().join("tool");
        write_file(&target, 0o755);
        symlink(&target, &declared).expect("executable symlink should be created");

        let executable =
            inspect_host_executable(&declared).expect("executable symlink should be accepted");

        assert_eq!(executable.declared(), declared);
        assert_eq!(
            executable.resolved(),
            fs::canonicalize(target).expect("target should canonicalize")
        );
    }

    #[test]
    fn rejects_relative_missing_directory_and_non_executable_paths() {
        let directory = tempdir().expect("temporary directory should exist");
        let missing = directory.path().join("missing");
        let broken_symlink = directory.path().join("broken");
        let non_executable = directory.path().join("non-executable");
        symlink(&missing, &broken_symlink).expect("broken symlink should be created");
        write_file(&non_executable, 0o644);

        assert!(matches!(
            inspect_host_executable("relative/tool"),
            Err(HostExecutableCheckError::RelativePath { .. })
        ));
        assert!(matches!(
            inspect_host_executable(missing),
            Err(HostExecutableCheckError::Resolution { .. })
        ));
        assert!(matches!(
            inspect_host_executable(broken_symlink),
            Err(HostExecutableCheckError::Resolution { .. })
        ));
        assert!(matches!(
            inspect_host_executable(directory.path()),
            Err(HostExecutableCheckError::NotRegularFile { .. })
        ));
        assert!(matches!(
            inspect_host_executable(non_executable),
            Err(HostExecutableCheckError::NotExecutable { .. })
        ));
    }

    #[test]
    fn resolves_the_first_executable_in_absolute_path_entries() {
        let directory = tempdir().expect("temporary directory should exist");
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        fs::create_dir_all(&first).expect("first bin should exist");
        fs::create_dir_all(&second).expect("second bin should exist");
        write_file(&first.join("tool"), 0o755);
        write_file(&second.join("tool"), 0o755);
        let path = env::join_paths([&first, &second]).expect("PATH should join");

        let executable =
            resolve_host_command("tool", Some(&path)).expect("the first executable should resolve");

        assert_eq!(executable.declared(), first.join("tool"));
    }

    #[test]
    fn skips_relative_and_unusable_path_candidates() {
        let directory = tempdir().expect("temporary directory should exist");
        let unusable = directory.path().join("unusable");
        let usable = directory.path().join("usable");
        fs::create_dir_all(&unusable).expect("unusable bin should exist");
        fs::create_dir_all(&usable).expect("usable bin should exist");
        write_file(&unusable.join("tool"), 0o644);
        write_file(&usable.join("tool"), 0o755);
        let path = env::join_paths([Path::new("relative-bin"), &unusable, &usable])
            .expect("PATH should join");

        let executable =
            resolve_host_command("tool", Some(&path)).expect("usable candidate should resolve");

        assert_eq!(executable.declared(), usable.join("tool"));
    }

    #[test]
    fn rejects_paths_as_command_names_and_reports_missing_commands() {
        assert!(matches!(
            resolve_host_command("bin/tool", None),
            Err(HostCommandLookupError::InvalidCommandName { .. })
        ));
        assert_eq!(
            resolve_host_command("missing", None),
            Err(HostCommandLookupError::NotFound {
                command: "missing".into()
            })
        );
    }

    fn write_file(path: &Path, mode: u32) {
        fs::write(path, "test executable\n").expect("test file should be written");
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .expect("test permissions should be set");
    }
}
