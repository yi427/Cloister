//! Profile-related CLI commands.

use std::{error::Error, fmt, path::PathBuf};

use clap::{Args, Subcommand, ValueHint};

use crate::profile::{LoadProfileError, load_profile};

use self::upgrade::{ProfileUpgradeArgs, ProfileUpgradeError};

mod upgrade;

#[derive(Debug, Args)]
pub(super) struct ProfileArgs {
    #[command(subcommand)]
    command: ProfileCommand,
}

#[derive(Debug, Subcommand)]
enum ProfileCommand {
    /// Check that a profile can be parsed and passes static validation.
    Check {
        /// Path to a Profile V6 TOML file.
        #[arg(value_name = "PROFILE", value_hint = ValueHint::FilePath)]
        path: PathBuf,
    },
    /// Upgrade an official release image in an existing current-schema Profile.
    Upgrade(ProfileUpgradeArgs),
}

#[derive(Debug)]
pub(super) enum ProfileCommandError {
    Check(LoadProfileError),
    Upgrade(ProfileUpgradeError),
}

impl ProfileArgs {
    pub(super) async fn execute(self) -> Result<(), ProfileCommandError> {
        match self.command {
            ProfileCommand::Check { path } => {
                let profile = load_profile(&path).map_err(ProfileCommandError::Check)?;
                println!("Profile '{}' is valid.", profile.name);
                Ok(())
            }
            ProfileCommand::Upgrade(arguments) => arguments
                .execute()
                .await
                .map_err(ProfileCommandError::Upgrade),
        }
    }
}

impl fmt::Display for ProfileCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Check(error) => error.fmt(formatter),
            Self::Upgrade(error) => error.fmt(formatter),
        }
    }
}

impl Error for ProfileCommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Check(error) => Some(error),
            Self::Upgrade(error) => Some(error),
        }
    }
}
