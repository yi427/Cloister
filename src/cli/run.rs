//! Planning and execution of a development environment.

use std::{
    error::Error,
    ffi::OsString,
    fmt,
    path::PathBuf,
    process::{ExitCode, ExitStatus},
};

use clap::{Args, ValueHint};

use crate::{
    preflight::{PreflightError, resolve_profile_workspace},
    profile::{LoadProfileError, load_profile},
    runtime::{RuntimeExecutionError, RuntimePlanError, execute, plan_apple_container},
};

#[derive(Debug, Args)]
pub(super) struct RunArgs {
    /// Path to a Profile V2 TOML file.
    #[arg(long, value_name = "PROFILE", value_hint = ValueHint::FilePath)]
    profile: PathBuf,

    /// Host project directory mounted at /workspace.
    ///
    /// Defaults to the current directory.
    #[arg(long, value_name = "DIRECTORY", value_hint = ValueHint::DirPath)]
    workspace: Option<PathBuf>,

    /// Print the runtime plan without starting a container.
    #[arg(long)]
    dry_run: bool,

    /// Command and arguments to run inside the environment.
    #[arg(last = true, value_name = "COMMAND")]
    command: Vec<OsString>,
}

impl RunArgs {
    pub(super) async fn execute(self) -> Result<ExitCode, RunCommandError> {
        let profile = load_profile(&self.profile)?;
        let workspace = match self.workspace {
            Some(workspace) => workspace,
            None => std::env::current_dir().map_err(RunCommandError::CurrentDirectory)?,
        };
        let resolved = resolve_profile_workspace(profile, &self.profile, workspace)?;
        let plan = plan_apple_container(&resolved, &self.command)?;

        if self.dry_run {
            print!("{plan}");
            return Ok(ExitCode::SUCCESS);
        }

        let status = execute(plan.command()).await?;
        Ok(exit_code(status))
    }
}

pub(super) fn exit_code(status: ExitStatus) -> ExitCode {
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map(ExitCode::from)
        .unwrap_or(ExitCode::FAILURE)
}

#[derive(Debug)]
pub(super) enum RunCommandError {
    CurrentDirectory(std::io::Error),
    Execution(RuntimeExecutionError),
    Load(LoadProfileError),
    Preflight(PreflightError),
    Plan(RuntimePlanError),
}

impl fmt::Display for RunCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CurrentDirectory(error) => {
                write!(
                    formatter,
                    "{}: {error}",
                    crate::error::message::CURRENT_DIRECTORY_FAILED
                )
            }
            Self::Execution(error) => error.fmt(formatter),
            Self::Load(error) => error.fmt(formatter),
            Self::Preflight(error) => error.fmt(formatter),
            Self::Plan(error) => error.fmt(formatter),
        }
    }
}

impl Error for RunCommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CurrentDirectory(error) => Some(error),
            Self::Execution(error) => Some(error),
            Self::Load(error) => Some(error),
            Self::Preflight(error) => Some(error),
            Self::Plan(error) => Some(error),
        }
    }
}

impl From<LoadProfileError> for RunCommandError {
    fn from(error: LoadProfileError) -> Self {
        Self::Load(error)
    }
}

impl From<PreflightError> for RunCommandError {
    fn from(error: PreflightError) -> Self {
        Self::Preflight(error)
    }
}

impl From<RuntimePlanError> for RunCommandError {
    fn from(error: RuntimePlanError) -> Self {
        Self::Plan(error)
    }
}

impl From<RuntimeExecutionError> for RunCommandError {
    fn from(error: RuntimeExecutionError) -> Self {
        Self::Execution(error)
    }
}
