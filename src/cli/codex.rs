//! Natural Codex entry point backed by the shared agent lifecycle.

use std::process::ExitCode;

use clap::Args;

use crate::agent::CodexAgent;

use super::agent::{AgentArgs, AgentCommandError, execute_agent};

#[derive(Debug, Args)]
pub(super) struct CodexArgs {
    #[command(flatten)]
    arguments: AgentArgs,
}

impl CodexArgs {
    pub(super) async fn execute(self) -> Result<ExitCode, AgentCommandError> {
        execute_agent(&CodexAgent, self.arguments).await
    }
}
