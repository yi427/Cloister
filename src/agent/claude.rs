//! Claude Code-specific state and command construction.

use std::{ffi::OsString, path::Path};

use serde_json::json;

use super::{AgentAdapter, AgentCommand, AgentHostBridge};

/// Adapter for the Claude Code CLI bundled in the Cloister image.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClaudeAgent;

impl AgentAdapter for ClaudeAgent {
    fn display_name(&self) -> &'static str {
        "Claude"
    }

    fn state_directory_name(&self) -> &'static str {
        "claude"
    }

    fn shared_state_guest_path(&self) -> &'static Path {
        Path::new("/cloister/agents/claude")
    }

    fn state_environment(&self) -> &'static str {
        "CLAUDE_CONFIG_DIR"
    }

    fn build_command(
        &self,
        host_bridge: Option<AgentHostBridge<'_>>,
        arguments: &[OsString],
    ) -> AgentCommand {
        let mut command_arguments = Vec::new();
        if let Some(bridge) = host_bridge {
            let authorization = format!("Bearer ${{{}}}", bridge.bearer_token_environment());
            let config = json!({
                "mcpServers": {
                    "cloister_host": {
                        "type": "http",
                        "url": bridge.endpoint(),
                        "headers": {
                            "Authorization": authorization,
                        },
                        "alwaysLoad": true,
                    },
                },
            });
            command_arguments.push(OsString::from("--mcp-config"));
            command_arguments.push(OsString::from(config.to_string()));
        }
        command_arguments.extend_from_slice(arguments);

        AgentCommand::new("claude", command_arguments)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};

    use serde_json::Value;

    use super::ClaudeAgent;
    use crate::agent::{AgentAdapter, AgentHostBridge};

    #[test]
    fn builds_the_plain_claude_command_without_shell_parsing() {
        let forwarded = [OsString::from("--model"), OsString::from("sonnet")];

        let command = ClaudeAgent.build_command(None, &forwarded);

        assert_eq!(command.executable(), OsStr::new("claude"));
        assert_eq!(command.arguments(), forwarded);
    }

    #[test]
    fn injects_an_environment_backed_host_bridge_config_before_arguments() {
        let endpoint = "http://host.container.internal:17834/mcp";
        let bridge = AgentHostBridge::new(endpoint, "CLOISTER_HOST_BRIDGE_TOKEN");
        let forwarded = [OsString::from("--version")];

        let command = ClaudeAgent.build_command(Some(bridge), &forwarded);

        assert_eq!(command.executable(), OsStr::new("claude"));
        assert_eq!(command.arguments()[0], "--mcp-config");
        let config: Value = serde_json::from_str(
            command.arguments()[1]
                .to_str()
                .expect("generated MCP JSON should be UTF-8"),
        )
        .expect("generated MCP JSON should parse");
        assert_eq!(
            config,
            serde_json::json!({
                "mcpServers": {
                    "cloister_host": {
                        "type": "http",
                        "url": endpoint,
                        "headers": {
                            "Authorization": "Bearer ${CLOISTER_HOST_BRIDGE_TOKEN}",
                        },
                        "alwaysLoad": true,
                    },
                },
            })
        );
        assert!(command.arguments().ends_with(&forwarded));
        assert!(
            !command
                .arguments()
                .iter()
                .any(|argument| argument == "--strict-mcp-config")
        );
    }
}
