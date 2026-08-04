//! Command-line interface for Cloister.

mod check;
mod codex;
mod config;
mod host;
mod init;
mod profile;

use std::{error::Error, fmt, process::ExitCode};

use clap::{Parser, Subcommand};

use crate::error::message;

use self::check::CheckArgs;
use self::codex::CodexArgs;
use self::host::HostArgs;
use self::init::InitArgs;
use self::profile::ProfileArgs;

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
    /// Check whether Cloister is ready to launch an agent.
    Check(CheckArgs),
    /// Run Codex in the selected project.
    Codex(CodexArgs),
    /// Serve or exercise the host shell MCP bridge.
    Host(HostArgs),
    /// Interactively create a Profile and prepare Apple container.
    Init(InitArgs),
    /// Inspect and manage environment profiles.
    Profile(ProfileArgs),
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
            Self::Check(arguments) => Ok(arguments.execute().await),
            Self::Codex(arguments) => arguments.execute().await.map_err(CliError::Codex),
            Self::Host(arguments) => arguments
                .execute()
                .await
                .map(|()| ExitCode::SUCCESS)
                .map_err(CliError::Host),
            Self::Init(arguments) => arguments.execute().await.map_err(CliError::Init),
            Self::Profile(arguments) => arguments
                .execute()
                .map(|()| ExitCode::SUCCESS)
                .map_err(CliError::Profile),
        }
    }
}

#[derive(Debug)]
enum CliError {
    Codex(codex::CodexCommandError),
    Host(host::HostCommandError),
    Init(init::InitCommandError),
    Profile(profile::ProfileCommandError),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codex(error) => error.fmt(formatter),
            Self::Host(error) => error.fmt(formatter),
            Self::Init(error) => error.fmt(formatter),
            Self::Profile(error) => error.fmt(formatter),
        }
    }
}

impl Error for CliError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Codex(error) => Some(error),
            Self::Host(error) => Some(error),
            Self::Init(error) => Some(error),
            Self::Profile(error) => Some(error),
        }
    }
}
