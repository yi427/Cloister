//! Natural Claude Code entry point backed by the shared agent lifecycle.

use std::process::ExitCode;

use clap::Args;

use crate::agent::ClaudeAgent;

use super::agent::{AgentArgs, AgentCommandError, execute_agent};

#[derive(Debug, Args)]
pub(super) struct ClaudeArgs {
    #[command(flatten)]
    arguments: AgentArgs,
}

impl ClaudeArgs {
    pub(super) async fn execute(self) -> Result<ExitCode, AgentCommandError> {
        execute_agent(&ClaudeAgent, self.arguments).await
    }
}
