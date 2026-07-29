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
    pub(super) guest_hostname: String,
    pub(super) network: NetworkExposure,
    pub(super) workspace: WorkspaceMount,
    pub(super) enabled_agents: Vec<AgentKind>,
    pub(super) command: CommandSpec,
}

impl RuntimePlan {
    pub fn profile_name(&self) -> &str {
        &self.profile_name
    }

    pub fn guest_hostname(&self) -> &str {
        &self.guest_hostname
    }

    pub const fn network(&self) -> NetworkExposure {
        self.network
    }

    pub fn workspace(&self) -> &WorkspaceMount {
        &self.workspace
    }

    pub fn enabled_agents(&self) -> &[AgentKind] {
        &self.enabled_agents
    }

    pub fn command(&self) -> &CommandSpec {
        &self.command
    }
}

impl fmt::Display for RuntimePlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Profile: {}", self.profile_name)?;
        writeln!(formatter, "Runtime: Apple container")?;
        writeln!(
            formatter,
            "Guest hostname: {} (mapped through --name)",
            self.guest_hostname
        )?;
        writeln!(formatter, "Root filesystem: read-only")?;
        writeln!(formatter, "Network: {}", self.network)?;
        writeln!(
            formatter,
            "Workspace: {} -> {} ({})",
            self.workspace.host.display(),
            self.workspace.guest.display(),
            self.workspace.access
        )?;
        writeln!(formatter, "SSH agent forwarding: disabled")?;
        writeln!(formatter, "Host credential mounts: none")?;
        writeln!(
            formatter,
            "Agent state: isolated policy (storage provisioning deferred)"
        )?;

        write!(formatter, "Enabled agents: ")?;
        for (index, agent) in self.enabled_agents.iter().enumerate() {
            if index > 0 {
                write!(formatter, ", ")?;
            }
            write!(formatter, "{agent}")?;
        }
        writeln!(formatter)?;
        writeln!(formatter, "Lifecycle: run and remove after exit")?;
        writeln!(formatter, "Command:")?;
        writeln!(formatter, "  program: {:?}", self.command.program)?;
        writeln!(formatter, "  arguments:")?;
        for (index, argument) in self.command.arguments.iter().enumerate() {
            writeln!(formatter, "    [{index}] {argument:?}")?;
        }

        Ok(())
    }
}

/// Direct process invocation without shell parsing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandSpec {
    pub(super) program: OsString,
    pub(super) arguments: Vec<OsString>,
}

impl CommandSpec {
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    pub fn arguments(&self) -> &[OsString] {
        &self.arguments
    }
}

/// Host workspace mount included in the runtime command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceMount {
    pub(super) host: PathBuf,
    pub(super) guest: PathBuf,
    pub(super) access: WorkspaceMountAccess,
}

impl WorkspaceMount {
    pub fn host(&self) -> &Path {
        &self.host
    }

    pub fn guest(&self) -> &Path {
        &self.guest
    }

    pub const fn access(&self) -> WorkspaceMountAccess {
        self.access
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceMountAccess {
    ReadOnly,
    ReadWrite,
}

impl fmt::Display for WorkspaceMountAccess {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadOnly => formatter.write_str("read-only"),
            Self::ReadWrite => formatter.write_str("read-write"),
        }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentKind {
    Codex,
    Claude,
}

impl fmt::Display for AgentKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codex => formatter.write_str("codex"),
            Self::Claude => formatter.write_str("claude"),
        }
    }
}
