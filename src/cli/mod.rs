//! Command-line interface for Cloister.

mod profile;
mod run;

use std::{error::Error, fmt, process::ExitCode};

use clap::{Parser, Subcommand};

use crate::error::message;

use self::profile::ProfileArgs;
use self::run::RunArgs;

#[derive(Debug, Parser)]
#[command(
    name = "cloister",
    version,
    about = "Privacy-oriented development environments for AI coding agents",
    long_about = None,
    propagate_version = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Inspect and manage environment profiles.
    Profile(ProfileArgs),
    /// Plan or start a development environment.
    Run(RunArgs),
}

/// Parses process arguments and executes the requested command.
pub fn run() -> ExitCode {
    match Cli::parse().command.execute() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}: {error}", message::CLI_ERROR_PREFIX);
            ExitCode::FAILURE
        }
    }
}

impl Command {
    fn execute(self) -> Result<(), CliError> {
        match self {
            Self::Profile(arguments) => arguments.execute().map_err(CliError::Profile),
            Self::Run(arguments) => arguments.execute().map_err(CliError::Run),
        }
    }
}

#[derive(Debug)]
enum CliError {
    Profile(profile::ProfileCommandError),
    Run(run::RunCommandError),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Profile(error) => error.fmt(formatter),
            Self::Run(error) => error.fmt(formatter),
        }
    }
}

impl Error for CliError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Profile(error) => Some(error),
            Self::Run(error) => Some(error),
        }
    }
}
