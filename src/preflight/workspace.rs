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
    workspace: PathBuf,
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

    /// Canonical, existing host workspace selected for this execution.
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }
}

/// Resolves a profile document and the workspace selected for this execution.
pub fn resolve_profile_workspace(
    profile: Profile,
    profile_path: impl AsRef<Path>,
    workspace: impl AsRef<Path>,
) -> Result<ResolvedProfile, PreflightError> {
    let profile_path = profile_path.as_ref();
    let source = fs::canonicalize(profile_path).map_err(|source| PreflightError::ProfilePath {
        path: profile_path.to_owned(),
        source,
    })?;
    let configured = workspace.as_ref().to_owned();
    let resolved =
        fs::canonicalize(&configured).map_err(|source| PreflightError::WorkspacePath {
            configured: configured.clone(),
            resolved: configured,
            source,
        })?;

    if !resolved.is_dir() {
        return Err(PreflightError::WorkspaceNotDirectory { path: resolved });
    }
    if resolved.parent().is_none() {
        return Err(PreflightError::WorkspaceIsRoot { path: resolved });
    }

    Ok(ResolvedProfile {
        source,
        profile,
        workspace: resolved,
    })
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
    WorkspaceIsRoot {
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
            Self::WorkspaceIsRoot { path } => write!(
                formatter,
                "{}: '{}'",
                message::WORKSPACE_PATH_IS_ROOT,
                path.display()
            ),
        }
    }
}

impl Error for PreflightError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ProfilePath { source, .. } | Self::WorkspacePath { source, .. } => Some(source),
            Self::WorkspaceNotDirectory { .. } | Self::WorkspaceIsRoot { .. } => None,
        }
    }
}
