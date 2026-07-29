//! Resolution and validation of the workspace selected for a launch.

use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use crate::{error::message, profile::Profile};

/// A validated profile paired with the workspace selected for this launch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedLaunch {
    profile: Profile,
    workspace: PathBuf,
}

impl ResolvedLaunch {
    /// Validated environment and Codex policy.
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// Canonical, existing host workspace selected for this execution.
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }
}

/// Resolves the workspace selected for one launch.
pub fn resolve_launch(
    profile: Profile,
    workspace: impl AsRef<Path>,
) -> Result<ResolvedLaunch, PreflightError> {
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

    Ok(ResolvedLaunch {
        profile,
        workspace: resolved,
    })
}

/// Host-dependent failure encountered before a runtime plan can be produced.
#[derive(Debug)]
pub enum PreflightError {
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
            Self::WorkspacePath { source, .. } => Some(source),
            Self::WorkspaceNotDirectory { .. } | Self::WorkspaceIsRoot { .. } => None,
        }
    }
}
