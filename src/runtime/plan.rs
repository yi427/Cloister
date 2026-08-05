//! Inspectable representation of a planned environment.

use std::{
    ffi::{OsStr, OsString},
    fmt,
    path::{Path, PathBuf},
};

/// Complete execution plan for one development environment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimePlan {
    pub(super) profile_name: String,
    pub(super) agent_name: String,
    pub(super) network: NetworkExposure,
    pub(super) workspace: WorkspaceMount,
    pub(super) agent_state: Option<AgentStateMount>,
    pub(super) host_bridge_endpoint: Option<String>,
    pub(super) host_commands: Vec<HostCommandPlan>,
    pub(super) command: CommandSpec,
}

impl RuntimePlan {
    pub fn profile_name(&self) -> &str {
        &self.profile_name
    }

    pub fn agent_name(&self) -> &str {
        &self.agent_name
    }

    pub const fn network(&self) -> NetworkExposure {
        self.network
    }

    pub fn workspace(&self) -> &WorkspaceMount {
        &self.workspace
    }

    pub fn agent_state(&self) -> Option<&AgentStateMount> {
        self.agent_state.as_ref()
    }

    pub fn host_bridge_endpoint(&self) -> Option<&str> {
        self.host_bridge_endpoint.as_deref()
    }

    pub fn host_commands(&self) -> &[HostCommandPlan] {
        &self.host_commands
    }

    pub fn command(&self) -> &CommandSpec {
        &self.command
    }
}

impl fmt::Display for RuntimePlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Profile: {}", self.profile_name)?;
        writeln!(formatter, "Runtime: Apple container")?;
        writeln!(formatter, "Root filesystem: read-only")?;
        writeln!(formatter, "Network: {}", self.network)?;
        writeln!(
            formatter,
            "Workspace: {} -> {} (read-write)",
            self.workspace.host.display(),
            self.workspace.guest.display()
        )?;
        writeln!(formatter, "SSH agent forwarding: disabled")?;
        writeln!(formatter, "Host credential mounts: none")?;
        if let Some(state) = &self.agent_state {
            writeln!(
                formatter,
                "{} state: {} -> {} (shared across projects)",
                self.agent_name,
                state.host.display(),
                state.guest.display()
            )?;
        } else {
            writeln!(formatter, "{} state: ephemeral", self.agent_name)?;
        }
        if let Some(endpoint) = &self.host_bridge_endpoint {
            writeln!(formatter, "Host bridge: {endpoint}")?;
            writeln!(
                formatter,
                "Host capabilities: host.list_commands, host.exec (Profile-governed; macOS user permissions)"
            )?;
            writeln!(
                formatter,
                "Host policy: inherit-all environment, {} allowed command(s)",
                self.host_commands.len()
            )?;
            for command in &self.host_commands {
                writeln!(
                    formatter,
                    "  {}: declared '{}', resolved '{}', arguments any",
                    command.name,
                    command.declared.display(),
                    command.resolved.display()
                )?;
            }
            writeln!(formatter, "{} MCP approval: prompt", self.agent_name)?;
            writeln!(
                formatter,
                "Host bridge token: ephemeral, forwarded, and redacted"
            )?;
        } else {
            writeln!(formatter, "Host bridge: disabled")?;
        }

        writeln!(formatter, "Agent: {}", self.agent_name)?;
        writeln!(formatter, "Lifecycle: run and remove after exit")?;
        writeln!(formatter, "Command:")?;
        writeln!(formatter, "  program: {:?}", self.command.program)?;
        writeln!(formatter, "  arguments:")?;
        for (index, argument) in self.command.arguments.iter().enumerate() {
            writeln!(formatter, "    [{index}] {argument:?}")?;
        }
        for variable in &self.command.secret_environment {
            writeln!(
                formatter,
                "  host environment: {:?}=[REDACTED]",
                variable.name
            )?;
        }

        Ok(())
    }
}

/// Non-secret Host Exec command details rendered in a runtime plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostCommandPlan {
    pub(super) name: String,
    pub(super) declared: PathBuf,
    pub(super) resolved: PathBuf,
}

impl HostCommandPlan {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn declared(&self) -> &Path {
        &self.declared
    }

    pub fn resolved(&self) -> &Path {
        &self.resolved
    }
}

/// Cloister-managed persistent state exposed to one agent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentStateMount {
    pub(super) host: PathBuf,
    pub(super) guest: PathBuf,
}

impl AgentStateMount {
    pub fn host(&self) -> &Path {
        &self.host
    }

    pub fn guest(&self) -> &Path {
        &self.guest
    }
}

/// Direct process invocation without shell parsing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandSpec {
    pub(super) program: OsString,
    pub(super) arguments: Vec<OsString>,
    pub(super) secret_environment: Vec<SecretEnvironmentVariable>,
}

impl CommandSpec {
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    pub fn arguments(&self) -> &[OsString] {
        &self.arguments
    }

    pub fn secret_environment_names(&self) -> impl Iterator<Item = &OsStr> {
        self.secret_environment
            .iter()
            .map(|variable| variable.name.as_os_str())
    }

    pub(super) fn secret_environment(&self) -> &[SecretEnvironmentVariable] {
        &self.secret_environment
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct SecretEnvironmentVariable {
    pub(super) name: OsString,
    value: OsString,
}

impl SecretEnvironmentVariable {
    pub(super) fn new(name: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        Self {
            name: name.as_ref().to_owned(),
            value: value.as_ref().to_owned(),
        }
    }

    pub(super) fn value(&self) -> &OsStr {
        &self.value
    }
}

impl fmt::Debug for SecretEnvironmentVariable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretEnvironmentVariable")
            .field("name", &self.name)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

/// Host workspace mount included in the runtime command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceMount {
    pub(super) host: PathBuf,
    pub(super) guest: PathBuf,
}

impl WorkspaceMount {
    pub fn host(&self) -> &Path {
        &self.host
    }

    pub fn guest(&self) -> &Path {
        &self.guest
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkExposure {
    DefaultWithInternetEgress,
}

impl fmt::Display for NetworkExposure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DefaultWithInternetEgress => {
                formatter.write_str("default (outbound internet enabled)")
            }
        }
    }
}
