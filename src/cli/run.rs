//! Planning entry point for future environment execution.

use std::{error::Error, fmt, path::PathBuf};

use clap::{Args, ValueHint};

use crate::{
    error::message,
    preflight::{PreflightError, resolve_profile},
    profile::{LoadProfileError, load_profile},
    runtime::{RuntimePlanError, plan_apple_container},
};

#[derive(Debug, Args)]
pub(super) struct RunArgs {
    /// Path to a Profile V1 TOML file.
    #[arg(long, value_name = "PROFILE", value_hint = ValueHint::FilePath)]
    profile: PathBuf,

    /// Print the runtime plan without creating a container.
    #[arg(long)]
    dry_run: bool,
}

impl RunArgs {
    pub(super) fn execute(self) -> Result<(), RunCommandError> {
        if !self.dry_run {
            return Err(RunCommandError::ExecutionNotImplemented);
        }

        let profile = load_profile(&self.profile)?;
        let resolved = resolve_profile(profile, &self.profile)?;
        let plan = plan_apple_container(&resolved)?;

        print!("{plan}");
        Ok(())
    }
}

#[derive(Debug)]
pub(super) enum RunCommandError {
    ExecutionNotImplemented,
    Load(LoadProfileError),
    Preflight(PreflightError),
    Plan(RuntimePlanError),
}

impl fmt::Display for RunCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExecutionNotImplemented => {
                formatter.write_str(message::CONTAINER_EXECUTION_NOT_IMPLEMENTED)
            }
            Self::Load(error) => error.fmt(formatter),
            Self::Preflight(error) => error.fmt(formatter),
            Self::Plan(error) => error.fmt(formatter),
        }
    }
}

impl Error for RunCommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ExecutionNotImplemented => None,
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
