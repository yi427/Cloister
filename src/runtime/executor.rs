//! Direct execution of an inspectable runtime command.

use std::{error::Error, ffi::OsString, fmt, io, process::ExitStatus};

use tokio::process::Command;

use crate::error::message;

use super::CommandSpec;

/// Executes a runtime command without shell parsing.
pub async fn execute(command: &CommandSpec) -> Result<ExitStatus, RuntimeExecutionError> {
    Command::new(command.program())
        .args(command.arguments())
        .status()
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
        };

        let error = execute(&command)
            .await
            .expect_err("missing runtime should fail to start");

        assert!(matches!(error, RuntimeExecutionError::Start { .. }));
        assert!(error.to_string().contains("failed to start runtime"));
    }
}
