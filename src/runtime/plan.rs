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
    pub(super) network: NetworkExposure,
    pub(super) workspace: WorkspaceMount,
    pub(super) codex_state: Option<CodexStateMount>,
    pub(super) host_bridge_endpoint: Option<String>,
    pub(super) command: CommandSpec,
}

impl RuntimePlan {
    pub fn profile_name(&self) -> &str {
        &self.profile_name
    }

    pub const fn network(&self) -> NetworkExposure {
        self.network
    }

    pub fn workspace(&self) -> &WorkspaceMount {
        &self.workspace
    }

    pub fn codex_state(&self) -> Option<&CodexStateMount> {
        self.codex_state.as_ref()
    }

    pub fn host_bridge_endpoint(&self) -> Option<&str> {
        self.host_bridge_endpoint.as_deref()
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
        if let Some(state) = &self.codex_state {
            writeln!(
                formatter,
                "Codex state: {} -> {} (shared across projects)",
                state.host.display(),
                state.guest.display()
            )?;
        } else {
            writeln!(formatter, "Codex state: ephemeral")?;
        }
        if let Some(endpoint) = &self.host_bridge_endpoint {
            writeln!(formatter, "Host bridge: {endpoint}")?;
            writeln!(
                formatter,
                "Host capability: host.exec (arbitrary macOS user commands)"
            )?;
            writeln!(formatter, "Codex MCP approval: prompt")?;
            writeln!(
                formatter,
                "Host bridge token: ephemeral, forwarded, and redacted"
            )?;
        } else {
            writeln!(formatter, "Host bridge: disabled")?;
        }

        writeln!(formatter, "Agent: Codex")?;
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

/// Cloister-managed persistent state exposed to Codex.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexStateMount {
    pub(super) host: PathBuf,
    pub(super) guest: PathBuf,
}

impl CodexStateMount {
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
