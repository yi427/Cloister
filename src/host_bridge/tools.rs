//! Profile-governed tools exposed through the Host MCP bridge.

use std::{path::Path, sync::Arc};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::profile::HostExecArguments;

use super::{
    HOST_EXEC_DSL_VERSION, HostExecCancelOutput, HostExecCancelRequest, HostExecOutput,
    HostExecPolicy, HostExecRequest, HostExecStatusOutput, HostExecStatusRequest,
    execution::{ExecutionError, ExecutionManager},
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

pub(super) fn host_list_commands(
    policy: &HostExecPolicy,
    audit_logging: bool,
) -> HostListCommandsOutput {
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
        audit_logging,
    }
}

pub(super) async fn host_exec(
    executions: &Arc<ExecutionManager>,
    policy: &HostExecPolicy,
    request: &HostExecRequest,
    working_directory: &Path,
) -> Result<HostExecOutput, ExecutionError> {
    executions.start(policy, request, working_directory).await
}

pub(super) fn host_exec_status(
    executions: &ExecutionManager,
    request: &HostExecStatusRequest,
) -> Result<HostExecStatusOutput, ExecutionError> {
    executions.status(request)
}

pub(super) fn host_exec_cancel(
    executions: &ExecutionManager,
    request: &HostExecCancelRequest,
) -> Result<HostExecCancelOutput, ExecutionError> {
    executions.cancel(request)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, ffi::OsString, fs, os::unix::fs::PermissionsExt};

    use tempfile::tempdir;

    use super::{host_exec, host_list_commands};
    use crate::{
        host_bridge::{
            HOST_EXEC_DSL_VERSION, HostExecPolicy, HostExecRequest, HostExecutionState,
            HostOutputStream, audit::AuditController, execution::ExecutionManager,
        },
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

        let audit = AuditController::start(
            directory
                .path()
                .join("state/cloister/audit/host-exec.jsonl"),
            "test".to_owned(),
            "test-agent".to_owned(),
            directory.path().to_owned(),
        )
        .expect("test audit should start");
        let executions = ExecutionManager::new(audit.log());
        let mut output = host_exec(
            &executions,
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

        for _ in 0..20 {
            if output.state.is_terminal() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            output = executions
                .status(&crate::host_bridge::HostExecStatusRequest {
                    execution_id: output.execution_id.clone(),
                    cursor: None,
                })
                .expect("execution status should exist");
        }

        assert_eq!(output.state, HostExecutionState::Completed);
        let stdout = output
            .chunks
            .iter()
            .filter(|chunk| chunk.stream == HostOutputStream::Stdout)
            .map(|chunk| chunk.text.as_str())
            .collect::<String>();
        let stderr = output
            .chunks
            .iter()
            .filter(|chunk| chunk.stream == HostOutputStream::Stderr)
            .map(|chunk| chunk.text.as_str())
            .collect::<String>();
        assert_eq!(stdout, "$(uname)\n; exit 0\n");
        assert_eq!(stderr, "stderr");
        assert_eq!(output.exit_code, Some(7));
        executions.shutdown().await;
        audit.shutdown().await.expect("test audit should stop");
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

        let output = host_list_commands(&policy, true);
        let rendered = serde_json::to_string(&output).expect("discovery should serialize");

        assert_eq!(output.version, HOST_EXEC_DSL_VERSION);
        assert_eq!(output.commands[0].name, "printer");
        assert_eq!(output.commands[0].description, "Print arguments");
        assert_eq!(output.commands[0].arguments, "any");
        assert_eq!(output.environment.mode, "inherit-all");
        assert_eq!(output.environment.variable_names, ["CLOISTER_SECRET_NAME"]);
        assert!(output.audit_logging);
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
