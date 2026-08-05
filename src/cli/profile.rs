//! Profile-related CLI commands.

use std::path::PathBuf;

use clap::{Args, Subcommand, ValueHint};

use crate::profile::{LoadProfileError, load_profile};

#[derive(Debug, Args)]
pub(super) struct ProfileArgs {
    #[command(subcommand)]
    command: ProfileCommand,
}

#[derive(Debug, Subcommand)]
enum ProfileCommand {
    /// Check that a profile can be parsed and passes static validation.
    Check {
        /// Path to a Profile V5 TOML file.
        #[arg(value_name = "PROFILE", value_hint = ValueHint::FilePath)]
        path: PathBuf,
    },
}

pub(super) type ProfileCommandError = LoadProfileError;

impl ProfileArgs {
    pub(super) fn execute(self) -> Result<(), ProfileCommandError> {
        match self.command {
            ProfileCommand::Check { path } => {
                let profile = load_profile(&path)?;
                println!("Profile '{}' is valid.", profile.name);
                Ok(())
            }
        }
    }
}
