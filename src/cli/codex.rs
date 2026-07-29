//! Natural Codex entry point backed by the current project directory.

use std::{
    env,
    error::Error,
    ffi::OsString,
    fmt, fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{ExitCode, ExitStatus},
};

use clap::{Args, ValueHint};

use crate::{
    error::message,
    preflight::{PreflightError, resolve_launch},
    profile::{AgentState, LoadProfileError, Profile},
    runtime::{RuntimeExecutionError, RuntimePlanError, execute, plan_codex_container},
};

#[derive(Debug, Args)]
pub(super) struct CodexArgs {
    /// Path to a Profile V3 TOML file.
    ///
    /// Defaults to ~/.config/cloister/profile.toml.
    #[arg(long, value_name = "PROFILE", value_hint = ValueHint::FilePath)]
    profile: Option<PathBuf>,

    /// Host project directory mounted at /workspace.
    ///
    /// Defaults to the current directory.
    #[arg(long, value_name = "DIRECTORY", value_hint = ValueHint::DirPath)]
    workspace: Option<PathBuf>,

    /// Print the runtime plan without starting Codex.
    #[arg(long)]
    dry_run: bool,

    /// Arguments passed directly to Codex.
    #[arg(last = true, value_name = "ARGUMENT")]
    arguments: Vec<OsString>,
}

impl CodexArgs {
    pub(super) async fn execute(self) -> Result<ExitCode, CodexCommandError> {
        let profile = load_selected_profile(self.profile.as_deref())?;
        let workspace = match self.workspace {
            Some(workspace) => workspace,
            None => env::current_dir().map_err(CodexCommandError::CurrentDirectory)?,
        };
        let resolved = resolve_launch(profile, workspace)?;

        let shared_state = match resolved.profile().codex.state {
            AgentState::Isolated => None,
            AgentState::Shared if self.dry_run => Some(codex_state_directory_path()?),
            AgentState::Shared => Some(prepare_codex_state_directory()?),
        };
        let plan = plan_codex_container(&resolved, shared_state.as_deref(), &self.arguments)?;

        if self.dry_run {
            print!("{plan}");
            return Ok(ExitCode::SUCCESS);
        }

        let status = execute(plan.command()).await?;
        Ok(child_exit_code(status))
    }
}

fn load_selected_profile(path: Option<&Path>) -> Result<Profile, CodexCommandError> {
    let path = match path {
        Some(path) => path.to_owned(),
        None => default_profile_path()?,
    };
    crate::profile::load_profile(path).map_err(CodexCommandError::Load)
}

fn child_exit_code(status: ExitStatus) -> ExitCode {
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map(ExitCode::from)
        .unwrap_or(ExitCode::FAILURE)
}

fn default_profile_path() -> Result<PathBuf, CodexCommandError> {
    env::var_os("XDG_CONFIG_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|path| !path.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })
        .map(|directory| directory.join("cloister/profile.toml"))
        .ok_or(CodexCommandError::HomeDirectoryMissing)
}

fn codex_state_directory_path() -> Result<PathBuf, CodexCommandError> {
    let base = env::var_os("XDG_DATA_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|path| !path.is_empty())
                .map(|home| PathBuf::from(home).join(".local/share"))
        })
        .ok_or(CodexCommandError::HomeDirectoryMissing)?;
    Ok(base.join("cloister/agents/codex"))
}

fn prepare_codex_state_directory() -> Result<PathBuf, CodexCommandError> {
    let path = codex_state_directory_path()?;
    fs::create_dir_all(&path).map_err(|source| CodexCommandError::AgentState {
        path: path.clone(),
        kind: AgentStateDirectoryErrorKind::Create(source),
    })?;

    let metadata = fs::symlink_metadata(&path).map_err(|source| CodexCommandError::AgentState {
        path: path.clone(),
        kind: AgentStateDirectoryErrorKind::Metadata(source),
    })?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(CodexCommandError::AgentState {
            path,
            kind: AgentStateDirectoryErrorKind::Invalid,
        });
    }

    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(|source| {
        CodexCommandError::AgentState {
            path: path.clone(),
            kind: AgentStateDirectoryErrorKind::Permissions(source),
        }
    })?;

    path.canonicalize()
        .map_err(|source| CodexCommandError::AgentState {
            path,
            kind: AgentStateDirectoryErrorKind::Metadata(source),
        })
}

#[derive(Debug)]
pub(super) enum CodexCommandError {
    AgentState {
        path: PathBuf,
        kind: AgentStateDirectoryErrorKind,
    },
    CurrentDirectory(io::Error),
    Execution(RuntimeExecutionError),
    HomeDirectoryMissing,
    Load(LoadProfileError),
    Preflight(PreflightError),
    Plan(RuntimePlanError),
}

#[derive(Debug)]
pub(super) enum AgentStateDirectoryErrorKind {
    Create(io::Error),
    Metadata(io::Error),
    Invalid,
    Permissions(io::Error),
}

impl fmt::Display for CodexCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AgentState { path, kind } => {
                let prefix = match kind {
                    AgentStateDirectoryErrorKind::Create(_) => message::AGENT_STATE_CREATE_FAILED,
                    AgentStateDirectoryErrorKind::Metadata(_) => {
                        message::AGENT_STATE_METADATA_FAILED
                    }
                    AgentStateDirectoryErrorKind::Invalid => message::AGENT_STATE_INVALID,
                    AgentStateDirectoryErrorKind::Permissions(_) => {
                        message::AGENT_STATE_PERMISSIONS_FAILED
                    }
                };
                write!(formatter, "{prefix} '{}'", path.display())?;
                match kind {
                    AgentStateDirectoryErrorKind::Create(source)
                    | AgentStateDirectoryErrorKind::Metadata(source)
                    | AgentStateDirectoryErrorKind::Permissions(source) => {
                        write!(formatter, ": {source}")
                    }
                    AgentStateDirectoryErrorKind::Invalid => Ok(()),
                }
            }
            Self::CurrentDirectory(source) => {
                write!(formatter, "{}: {source}", message::CURRENT_DIRECTORY_FAILED)
            }
            Self::Execution(error) => error.fmt(formatter),
            Self::HomeDirectoryMissing => formatter.write_str(message::HOME_DIRECTORY_MISSING),
            Self::Load(error) => error.fmt(formatter),
            Self::Preflight(error) => error.fmt(formatter),
            Self::Plan(error) => error.fmt(formatter),
        }
    }
}

impl Error for CodexCommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::AgentState { kind, .. } => match kind {
                AgentStateDirectoryErrorKind::Create(source)
                | AgentStateDirectoryErrorKind::Metadata(source)
                | AgentStateDirectoryErrorKind::Permissions(source) => Some(source),
                AgentStateDirectoryErrorKind::Invalid => None,
            },
            Self::CurrentDirectory(source) => Some(source),
            Self::Execution(error) => Some(error),
            Self::HomeDirectoryMissing => None,
            Self::Load(error) => Some(error),
            Self::Preflight(error) => Some(error),
            Self::Plan(error) => Some(error),
        }
    }
}

impl From<PreflightError> for CodexCommandError {
    fn from(error: PreflightError) -> Self {
        Self::Preflight(error)
    }
}

impl From<RuntimePlanError> for CodexCommandError {
    fn from(error: RuntimePlanError) -> Self {
        Self::Plan(error)
    }
}

impl From<RuntimeExecutionError> for CodexCommandError {
    fn from(error: RuntimeExecutionError) -> Self {
        Self::Execution(error)
    }
}
