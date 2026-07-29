//! Command-line interface for Cloister.

mod host;
mod profile;
mod run;

use std::{error::Error, fmt, process::ExitCode};

use clap::{Parser, Subcommand};

use crate::error::message;

use self::host::HostArgs;
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
    /// Serve or exercise the host shell MCP bridge.
    Host(HostArgs),
    /// Inspect and manage environment profiles.
    Profile(ProfileArgs),
    /// Plan or start a development environment.
    Run(RunArgs),
}

/// Parses process arguments and executes the requested command.
pub async fn run() -> ExitCode {
    match Cli::parse().command.execute().await {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("{}: {error}", message::CLI_ERROR_PREFIX);
            ExitCode::FAILURE
        }
    }
}

impl Command {
    async fn execute(self) -> Result<ExitCode, CliError> {
        match self {
            Self::Host(arguments) => arguments
                .execute()
                .await
                .map(|()| ExitCode::SUCCESS)
                .map_err(CliError::Host),
            Self::Profile(arguments) => arguments
                .execute()
                .map(|()| ExitCode::SUCCESS)
                .map_err(CliError::Profile),
            Self::Run(arguments) => arguments.execute().await.map_err(CliError::Run),
        }
    }
}

#[derive(Debug)]
enum CliError {
    Host(host::HostCommandError),
    Profile(profile::ProfileCommandError),
    Run(run::RunCommandError),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(error) => error.fmt(formatter),
            Self::Profile(error) => error.fmt(formatter),
            Self::Run(error) => error.fmt(formatter),
        }
    }
}

impl Error for CliError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Host(error) => Some(error),
            Self::Profile(error) => Some(error),
            Self::Run(error) => Some(error),
        }
    }
}
