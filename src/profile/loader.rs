//! Filesystem boundary for loading complete profiles.

use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use crate::error::message;

use super::{ParseProfileError, Profile, ProfileValidationErrors, parse_profile, validate_profile};

/// Reads, parses, and statically validates one profile file.
pub fn load_profile(path: impl AsRef<Path>) -> Result<Profile, LoadProfileError> {
    let path = path.as_ref();
    let input = fs::read_to_string(path).map_err(|source| LoadProfileError::Read {
        path: path.to_owned(),
        source,
    })?;
    let profile = parse_profile(&input).map_err(|source| LoadProfileError::Parse {
        path: path.to_owned(),
        source,
    })?;

    validate_profile(&profile).map_err(|source| LoadProfileError::Validation {
        path: path.to_owned(),
        source,
    })?;

    Ok(profile)
}

/// Failure produced while loading a profile from the filesystem.
#[derive(Debug)]
pub enum LoadProfileError {
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        source: ParseProfileError,
    },
    Validation {
        path: PathBuf,
        source: ProfileValidationErrors,
    },
}

impl LoadProfileError {
    /// Profile path associated with this failure.
    pub fn path(&self) -> &Path {
        match self {
            Self::Read { path, .. } | Self::Parse { path, .. } | Self::Validation { path, .. } => {
                path
            }
        }
    }
}

impl fmt::Display for LoadProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => write!(
                formatter,
                "{} '{}': {source}",
                message::PROFILE_READ_FAILED,
                path.display()
            ),
            Self::Parse { path, source } => write!(
                formatter,
                "{} '{}': {}",
                message::PROFILE_PARSE_FAILED,
                path.display(),
                source.diagnostic()
            ),
            Self::Validation { path, source } => {
                let diagnostic = source.to_string();
                write!(
                    formatter,
                    "{} '{}':\n{}",
                    message::PROFILE_VALIDATION_FAILED,
                    path.display(),
                    diagnostic.trim_end()
                )
            }
        }
    }
}

impl Error for LoadProfileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::Validation { source, .. } => Some(source),
        }
    }
}
