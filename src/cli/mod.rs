//! Command-line interface for Cloister.

mod profile;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::error::message;

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
    /// Inspect and manage environment profiles.
    Profile(ProfileArgs),
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
    fn execute(self) -> Result<(), profile::ProfileCommandError> {
        match self {
            Self::Profile(arguments) => arguments.execute(),
        }
    }
}
