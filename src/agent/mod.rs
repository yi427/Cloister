//! Agent-specific launch adapters consumed by the shared CLI and runtime.

mod claude;
mod codex;

use std::{
    ffi::{OsStr, OsString},
    path::Path,
};

pub use claude::ClaudeAgent;
pub use codex::CodexAgent;

/// Public Host MCP connection details that may be rendered in agent arguments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentHostBridge<'a> {
    endpoint: &'a str,
    bearer_token_environment: &'static str,
}

impl<'a> AgentHostBridge<'a> {
    pub const fn new(endpoint: &'a str, bearer_token_environment: &'static str) -> Self {
        Self {
            endpoint,
            bearer_token_environment,
        }
    }

    pub const fn endpoint(self) -> &'a str {
        self.endpoint
    }

    pub const fn bearer_token_environment(self) -> &'static str {
        self.bearer_token_environment
    }
}

/// Agent executable and arguments placed after the container image reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentCommand {
    executable: OsString,
    arguments: Vec<OsString>,
}

impl AgentCommand {
    /// Creates a direct guest process invocation without shell parsing.
    pub fn new(executable: impl AsRef<OsStr>, arguments: Vec<OsString>) -> Self {
        Self {
            executable: executable.as_ref().to_owned(),
            arguments,
        }
    }

    pub fn executable(&self) -> &OsStr {
        &self.executable
    }

    pub fn arguments(&self) -> &[OsString] {
        &self.arguments
    }

    pub(crate) fn into_parts(self) -> (OsString, Vec<OsString>) {
        (self.executable, self.arguments)
    }
}

/// Explicit differences between supported coding-agent CLIs.
pub trait AgentAdapter {
    /// Human-readable name used in runtime plans and diagnostics.
    fn display_name(&self) -> &'static str;

    /// Directory name below Cloister's host-side `agents` state root.
    fn state_directory_name(&self) -> &'static str;

    /// Guest path used when persistent agent state is mounted.
    fn shared_state_guest_path(&self) -> &'static Path;

    /// Environment variable through which the agent discovers its state.
    fn state_environment(&self) -> &'static str;

    /// Builds only the guest agent command, without Apple container arguments.
    fn build_command(
        &self,
        host_bridge: Option<AgentHostBridge<'_>>,
        arguments: &[OsString],
    ) -> AgentCommand;
}
