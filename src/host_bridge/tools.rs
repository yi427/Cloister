//! Host shell execution exposed through the MCP bridge.

use std::{error::Error, fmt, io, process::Stdio, time::Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::error::message;

const HOST_SHELL: &str = "/bin/zsh";

/// Arguments accepted by the `host.exec` MCP tool.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HostExecInput {
    /// Shell command executed as the macOS user running the bridge.
    pub command: String,
}

/// Structured output returned by the `host.exec` MCP tool.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HostExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
}

pub(super) async fn host_exec(command: &str) -> Result<HostExecOutput, HostToolError> {
    let started = Instant::now();
    let output = Command::new(HOST_SHELL)
        .args(["-lc", command])
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(HostToolError::Spawn)?;

    Ok(HostExecOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code(),
        duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    })
}

#[derive(Debug)]
pub(super) enum HostToolError {
    Spawn(io::Error),
}

impl fmt::Display for HostToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(source) => {
                write!(formatter, "{}: {source}", message::HOST_EXEC_SPAWN_FAILED)
            }
        }
    }
}

impl Error for HostToolError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Spawn(source) => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::host_exec;

    #[tokio::test]
    async fn captures_stdout_stderr_and_exit_code() {
        let output = host_exec("printf stdout; printf stderr >&2; exit 7")
            .await
            .expect("host command should finish");

        assert_eq!(output.stdout, "stdout");
        assert_eq!(output.stderr, "stderr");
        assert_eq!(output.exit_code, Some(7));
    }
}
