//! Resolution of host workspace paths relative to their profile file.

use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use crate::{error::message, profile::Profile};

/// A validated profile whose host paths have been resolved and checked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedProfile {
    source: PathBuf,
    profile: Profile,
}

impl ResolvedProfile {
    /// Canonical path of the profile document.
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Profile with a canonical, existing workspace host directory.
    pub fn profile(&self) -> &Profile {
        &self.profile
    }
}

/// Resolves a profile's host workspace relative to the profile document.
pub fn resolve_profile(
    mut profile: Profile,
    profile_path: impl AsRef<Path>,
) -> Result<ResolvedProfile, PreflightError> {
    let profile_path = profile_path.as_ref();
    let source = fs::canonicalize(profile_path).map_err(|source| PreflightError::ProfilePath {
        path: profile_path.to_owned(),
        source,
    })?;
    let profile_directory = source
        .parent()
        .expect("a canonical file path always has a parent");
    let configured_workspace = profile.workspace.host.clone();
    let workspace_candidate = if configured_workspace.is_absolute() {
        configured_workspace.clone()
    } else {
        profile_directory.join(&configured_workspace)
    };
    let workspace =
        fs::canonicalize(&workspace_candidate).map_err(|source| PreflightError::WorkspacePath {
            configured: configured_workspace,
            resolved: workspace_candidate,
            source,
        })?;

    if !workspace.is_dir() {
        return Err(PreflightError::WorkspaceNotDirectory { path: workspace });
    }

    profile.workspace.host = workspace;

    Ok(ResolvedProfile { source, profile })
}

/// Host-dependent failure encountered before a runtime plan can be produced.
#[derive(Debug)]
pub enum PreflightError {
    ProfilePath {
        path: PathBuf,
        source: io::Error,
    },
    WorkspacePath {
        configured: PathBuf,
        resolved: PathBuf,
        source: io::Error,
    },
    WorkspaceNotDirectory {
        path: PathBuf,
    },
}

impl fmt::Display for PreflightError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProfilePath { path, source } => write!(
                formatter,
                "{} '{}': {source}",
                message::PROFILE_PATH_RESOLUTION_FAILED,
                path.display()
            ),
            Self::WorkspacePath {
                configured,
                resolved,
                source,
            } => write!(
                formatter,
                "{} '{}' (resolved as '{}'): {source}",
                message::WORKSPACE_PATH_RESOLUTION_FAILED,
                configured.display(),
                resolved.display()
            ),
            Self::WorkspaceNotDirectory { path } => write!(
                formatter,
                "{}: '{}'",
                message::WORKSPACE_PATH_NOT_DIRECTORY,
                path.display()
            ),
        }
    }
}

impl Error for PreflightError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ProfilePath { source, .. } | Self::WorkspacePath { source, .. } => Some(source),
            Self::WorkspaceNotDirectory { .. } => None,
        }
    }
}
