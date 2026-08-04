//! Codex-specific state and command construction.

use std::{ffi::OsString, path::Path};

use super::{AgentAdapter, AgentCommand, AgentHostBridge};

/// Adapter for the Codex CLI bundled in the Cloister image.
#[derive(Clone, Copy, Debug, Default)]
pub struct CodexAgent;

impl AgentAdapter for CodexAgent {
    fn display_name(&self) -> &'static str {
        "Codex"
    }

    fn state_directory_name(&self) -> &'static str {
        "codex"
    }

    fn shared_state_guest_path(&self) -> &'static Path {
        Path::new("/cloister/agents/codex")
    }

    fn state_environment(&self) -> &'static str {
        "CODEX_HOME"
    }

    fn build_command(
        &self,
        host_bridge: Option<AgentHostBridge<'_>>,
        arguments: &[OsString],
    ) -> AgentCommand {
        let mut command_arguments = Vec::new();
        if let Some(bridge) = host_bridge {
            push_config(
                &mut command_arguments,
                format!("mcp_servers.cloister_host.url=\"{}\"", bridge.endpoint()),
            );
            push_config(
                &mut command_arguments,
                format!(
                    "mcp_servers.cloister_host.bearer_token_env_var=\"{}\"",
                    bridge.bearer_token_environment()
                ),
            );
            push_config(
                &mut command_arguments,
                "mcp_servers.cloister_host.required=true",
            );
            push_config(
                &mut command_arguments,
                "mcp_servers.cloister_host.enabled_tools=[\"host.exec\"]",
            );
            push_config(
                &mut command_arguments,
                "mcp_servers.cloister_host.default_tools_approval_mode=\"prompt\"",
            );
        }
        command_arguments.extend_from_slice(arguments);

        AgentCommand::new("codex", command_arguments)
    }
}

fn push_config(arguments: &mut Vec<OsString>, value: impl Into<OsString>) {
    arguments.push(OsString::from("--config"));
    arguments.push(value.into());
}

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};

    use super::CodexAgent;
    use crate::agent::{AgentAdapter, AgentHostBridge};

    #[test]
    fn builds_the_plain_codex_command_without_shell_parsing() {
        let forwarded = [
            OsString::from("--config"),
            OsString::from("model_reasoning_effort=high"),
        ];

        let command = CodexAgent.build_command(None, &forwarded);

        assert_eq!(command.executable(), OsStr::new("codex"));
        assert_eq!(command.arguments(), forwarded);
    }

    #[test]
    fn injects_the_transient_host_bridge_before_forwarded_arguments() {
        let bridge = AgentHostBridge::new(
            "http://host.container.internal:17834/mcp",
            "CLOISTER_HOST_BRIDGE_TOKEN",
        );
        let forwarded = [OsString::from("--version")];

        let command = CodexAgent.build_command(Some(bridge), &forwarded);

        assert_eq!(command.executable(), OsStr::new("codex"));
        assert!(command.arguments().windows(2).any(|pair| {
            pair[0] == "--config"
                && pair[1]
                    == "mcp_servers.cloister_host.bearer_token_env_var=\"CLOISTER_HOST_BRIDGE_TOKEN\""
        }));
        assert!(command.arguments().ends_with(&forwarded));
    }
}
