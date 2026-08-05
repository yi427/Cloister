//! Profile-governed tools exposed through the Host MCP bridge.

use std::{error::Error, fmt, io, path::Path, process::Stdio, time::Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{error::message, profile::HostExecArguments};

use super::{
    HOST_EXEC_DSL_VERSION, HostExecAuthorizationError, HostExecPolicy, HostExecRequest,
    build_host_process,
};

/// Read-only description of the immutable Host Exec policy.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HostListCommandsOutput {
    pub version: u32,
    pub commands: Vec<HostCommandInfo>,
    pub environment: HostEnvironmentInfo,
    pub audit_logging: bool,
}

/// One command visible to the model through policy discovery.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HostCommandInfo {
    pub name: String,
    pub description: String,
    pub arguments: String,
}

/// Non-secret environment metadata returned by policy discovery.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HostEnvironmentInfo {
    pub mode: String,
    pub variable_names: Vec<String>,
}

/// Structured output returned by the synchronous `host.exec` tool.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HostExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
}

pub(super) fn host_list_commands(policy: &HostExecPolicy) -> HostListCommandsOutput {
    HostListCommandsOutput {
        version: HOST_EXEC_DSL_VERSION,
        commands: policy
            .commands()
            .map(|command| HostCommandInfo {
                name: command.name().to_owned(),
                description: command.description().to_owned(),
                arguments: match command.arguments() {
                    HostExecArguments::Any => "any".to_owned(),
                },
            })
            .collect(),
        environment: HostEnvironmentInfo {
            mode: "inherit-all".to_owned(),
            variable_names: policy
                .environment_names()
                .map(|name| name.to_string_lossy().into_owned())
                .collect(),
        },
        audit_logging: false,
    }
}

pub(super) async fn host_exec(
    policy: &HostExecPolicy,
    request: &HostExecRequest,
    working_directory: &Path,
) -> Result<HostExecOutput, HostToolError> {
    let authorized = policy
        .authorize(request)
        .map_err(HostToolError::Authorization)?;
    let command_name = authorized.command_name().to_owned();
    let started = Instant::now();
    let mut process = build_host_process(&authorized);
    process
        .current_dir(working_directory)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let output = process
        .output()
        .await
        .map_err(|source| HostToolError::Spawn {
            command: command_name,
            source,
        })?;

    Ok(HostExecOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code(),
        duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    })
}

#[derive(Debug)]
pub(super) enum HostToolError {
    Authorization(HostExecAuthorizationError),
    Spawn { command: String, source: io::Error },
}

impl fmt::Display for HostToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authorization(source) => source.fmt(formatter),
            Self::Spawn { command, source } => write!(
                formatter,
                "{} {command:?}: {source}",
                message::HOST_EXEC_SPAWN_FAILED
            ),
        }
    }
}

impl Error for HostToolError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Authorization(source) => Some(source),
            Self::Spawn { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, ffi::OsString, fs, os::unix::fs::PermissionsExt};

    use tempfile::tempdir;

    use super::{host_exec, host_list_commands};
    use crate::{
        host_bridge::{HOST_EXEC_DSL_VERSION, HostExecPolicy, HostExecRequest},
        profile::{
            HostExecAllowProfile, HostExecArguments, HostExecEnvironmentMode,
            HostExecEnvironmentProfile, HostExecProfile,
        },
    };

    #[tokio::test]
    async fn executes_only_the_authorized_program_with_literal_arguments() {
        let directory = tempdir().expect("temporary directory should exist");
        let executable = directory.path().join("argument-printer");
        fs::write(
            &executable,
            "#!/bin/sh\nprintf '%s\\n' \"$@\"\nprintf stderr >&2\nexit 7\n",
        )
        .expect("test executable should be written");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
            .expect("test executable should be executable");
        let policy = HostExecPolicy::from_profile(&profile("printer", executable), BTreeMap::new())
            .expect("test policy should build")
            .expect("test policy should be enabled");

        let output = host_exec(
            &policy,
            &HostExecRequest {
                version: HOST_EXEC_DSL_VERSION,
                command: "printer".to_owned(),
                args: vec!["$(uname)".to_owned(), "; exit 0".to_owned()],
            },
            directory.path(),
        )
        .await
        .expect("authorized command should finish");

        assert_eq!(output.stdout, "$(uname)\n; exit 0\n");
        assert_eq!(output.stderr, "stderr");
        assert_eq!(output.exit_code, Some(7));
    }

    #[test]
    fn discovery_returns_metadata_and_environment_names_without_values() {
        let policy = HostExecPolicy::from_profile(
            &profile("printer", "/usr/bin/printf".into()),
            BTreeMap::from([(
                OsString::from("CLOISTER_SECRET_NAME"),
                OsString::from("secret-value"),
            )]),
        )
        .expect("test policy should build")
        .expect("test policy should be enabled");

        let output = host_list_commands(&policy);
        let rendered = serde_json::to_string(&output).expect("discovery should serialize");

        assert_eq!(output.version, HOST_EXEC_DSL_VERSION);
        assert_eq!(output.commands[0].name, "printer");
        assert_eq!(output.commands[0].description, "Print arguments");
        assert_eq!(output.commands[0].arguments, "any");
        assert_eq!(output.environment.mode, "inherit-all");
        assert_eq!(output.environment.variable_names, ["CLOISTER_SECRET_NAME"]);
        assert!(!output.audit_logging);
        assert!(!rendered.contains("secret-value"));
    }

    fn profile(name: &str, executable: std::path::PathBuf) -> HostExecProfile {
        HostExecProfile {
            enabled: true,
            environment: HostExecEnvironmentProfile {
                mode: HostExecEnvironmentMode::InheritAll,
            },
            allow: vec![HostExecAllowProfile {
                name: name.to_owned(),
                executable,
                description: "Print arguments".to_owned(),
                arguments: HostExecArguments::Any,
            }],
        }
    }
}
