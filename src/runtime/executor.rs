//! Direct execution of an inspectable runtime command.

use std::{
    error::Error,
    ffi::OsString,
    fmt, io,
    process::{ExitStatus, Output},
};

use tokio::process::Command;

use crate::error::message;

use super::CommandSpec;

/// Executes a runtime command without shell parsing.
pub async fn execute(command: &CommandSpec) -> Result<ExitStatus, RuntimeExecutionError> {
    let mut process = Command::new(command.program());
    process.args(command.arguments());
    for variable in command.secret_environment() {
        process.env(&variable.name, variable.value());
    }

    process
        .status()
        .await
        .map_err(|source| RuntimeExecutionError::Start {
            program: command.program().to_owned(),
            source,
        })
}

/// Executes a runtime query and captures its output without shell parsing.
pub async fn execute_output(command: &CommandSpec) -> Result<Output, RuntimeExecutionError> {
    let mut process = Command::new(command.program());
    process.args(command.arguments());
    for variable in command.secret_environment() {
        process.env(&variable.name, variable.value());
    }

    process
        .output()
        .await
        .map_err(|source| RuntimeExecutionError::Start {
            program: command.program().to_owned(),
            source,
        })
}

/// Failure to start the planned runtime process.
#[derive(Debug)]
pub enum RuntimeExecutionError {
    Start {
        program: OsString,
        source: io::Error,
    },
}

impl RuntimeExecutionError {
    /// Returns true when the runtime executable could not be found on PATH.
    pub fn is_not_found(&self) -> bool {
        match self {
            Self::Start { source, .. } => source.kind() == io::ErrorKind::NotFound,
        }
    }
}

impl fmt::Display for RuntimeExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start { program, source } => write!(
                formatter,
                "{} {:?}: {source}",
                message::RUNTIME_START_FAILED,
                program
            ),
        }
    }
}

impl Error for RuntimeExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Start { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{RuntimeExecutionError, execute};
    use crate::runtime::CommandSpec;

    #[tokio::test]
    async fn reports_a_missing_runtime_program() {
        let command = CommandSpec {
            program: OsString::from("/cloister/does-not-exist/container"),
            arguments: Vec::new(),
            secret_environment: Vec::new(),
        };

        let error = execute(&command)
            .await
            .expect_err("missing runtime should fail to start");

        assert!(matches!(error, RuntimeExecutionError::Start { .. }));
        assert!(error.to_string().contains("failed to start runtime"));
    }

    #[tokio::test]
    async fn forwards_a_secret_environment_without_shell_interpolation() {
        use crate::runtime::plan::SecretEnvironmentVariable;

        let command = CommandSpec {
            program: OsString::from("/bin/sh"),
            arguments: vec![
                OsString::from("-c"),
                OsString::from("test -n \"$CLOISTER_RUNTIME_TEST_TOKEN\""),
            ],
            secret_environment: vec![SecretEnvironmentVariable::new(
                "CLOISTER_RUNTIME_TEST_TOKEN",
                "sensitive-runtime-value",
            )],
        };

        let status = execute(&command).await.expect("test shell should start");

        assert!(status.success());
        assert!(!format!("{command:?}").contains("sensitive-runtime-value"));
    }
}
