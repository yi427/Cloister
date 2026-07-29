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
    preflight::{PreflightError, resolve_profile},
    profile::{LoadProfileError, load_profile},
    runtime::{RuntimeExecutionError, RuntimePlanError, execute, plan_apple_container},
};

#[derive(Debug, Args)]
pub(super) struct RunArgs {
    /// Path to a Profile V1 TOML file.
    #[arg(long, value_name = "PROFILE", value_hint = ValueHint::FilePath)]
    profile: PathBuf,

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
        let resolved = resolve_profile(profile, &self.profile)?;
        let plan = plan_apple_container(&resolved, &self.command)?;

        if self.dry_run {
            print!("{plan}");
            return Ok(ExitCode::SUCCESS);
        }

        let status = execute(plan.command()).await?;
        Ok(exit_code(status))
    }
}

fn exit_code(status: ExitStatus) -> ExitCode {
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map(ExitCode::from)
        .unwrap_or(ExitCode::FAILURE)
}

#[derive(Debug)]
pub(super) enum RunCommandError {
    Execution(RuntimeExecutionError),
    Load(LoadProfileError),
    Preflight(PreflightError),
    Plan(RuntimePlanError),
}

impl fmt::Display for RunCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
