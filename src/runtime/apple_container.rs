//! Translation from resolved profiles to Apple container 1.2 command arguments.

use std::{
    error::Error,
    ffi::{OsStr, OsString},
    fmt,
};

use crate::{
    error::message,
    preflight::ResolvedProfile,
    profile::{AgentState, Architecture, NetworkMode, Profile},
};

use super::plan::{
    AgentKind, AgentStateMount, CommandSpec, NetworkExposure, RuntimePlan, WorkspaceMount,
    WorkspaceMountAccess,
};

const CODEX_STATE_GUEST_PATH: &str = "/cloister/agents/codex";
const WORKSPACE_GUEST_PATH: &str = "/workspace";

/// Produces an inspectable `container run` plan without starting a process.
pub fn plan_apple_container(
    resolved: &ResolvedProfile,
    container_command: &[OsString],
) -> Result<RuntimePlan, RuntimePlanError> {
    let agents = &resolved.profile().agents;
    if (agents.codex.enabled && agents.codex.state == AgentState::Shared)
        || (agents.claude.enabled && agents.claude.state == AgentState::Shared)
    {
        return Err(RuntimePlanError::SharedStateRequiresAgentCommand);
    }

    plan(resolved, Vec::new(), Vec::new(), container_command)
}

/// Produces an Apple container plan that launches Codex.
pub fn plan_codex_container(
    resolved: &ResolvedProfile,
    shared_state: Option<&std::path::Path>,
    codex_arguments: &[OsString],
) -> Result<RuntimePlan, RuntimePlanError> {
    let profile = resolved.profile();
    if !profile.agents.codex.enabled {
        return Err(RuntimePlanError::CodexDisabled);
    }

    let mut command = Vec::with_capacity(codex_arguments.len() + 1);
    command.push(OsString::from("codex"));
    command.extend_from_slice(codex_arguments);

    let (state_mounts, environments) = match profile.agents.codex.state {
        AgentState::Isolated => (Vec::new(), Vec::new()),
        AgentState::Shared => {
            let host = shared_state.ok_or(RuntimePlanError::SharedCodexStateMissing)?;
            reject_mount_separator(host)?;
            let mount = AgentStateMount {
                agent: AgentKind::Codex,
                host: host.to_owned(),
                guest: CODEX_STATE_GUEST_PATH.into(),
            };
            (
                vec![mount],
                vec![("CODEX_HOME", OsString::from(CODEX_STATE_GUEST_PATH))],
            )
        }
    };

    plan(resolved, state_mounts, environments, &command)
}

fn plan(
    resolved: &ResolvedProfile,
    agent_state_mounts: Vec<AgentStateMount>,
    environments: Vec<(&'static str, OsString)>,
    container_command: &[OsString],
) -> Result<RuntimePlan, RuntimePlanError> {
    let profile = resolved.profile();

    if profile.network.mode != NetworkMode::Default {
        return Err(RuntimePlanError::UnsupportedNetworkMode);
    }
    reject_mount_separator(resolved.workspace())?;
    let workspace = WorkspaceMount {
        host: resolved.workspace().to_owned(),
        guest: WORKSPACE_GUEST_PATH.into(),
        access: WorkspaceMountAccess::ReadWrite,
    };
    let enabled_agents = [
        profile.agents.codex.enabled.then_some(AgentKind::Codex),
        profile.agents.claude.enabled.then_some(AgentKind::Claude),
    ]
    .into_iter()
    .flatten()
    .collect();
    let command = build_run_command(
        profile,
        &workspace,
        &agent_state_mounts,
        &environments,
        container_command,
    );

    Ok(RuntimePlan {
        profile_name: profile.name.clone(),
        guest_hostname: profile.guest.hostname.clone(),
        network: NetworkExposure::DefaultWithInternetEgress,
        workspace,
        agent_state_mounts,
        enabled_agents,
        command,
    })
}

fn build_run_command(
    profile: &Profile,
    workspace: &WorkspaceMount,
    agent_state_mounts: &[AgentStateMount],
    environments: &[(&'static str, OsString)],
    container_command: &[OsString],
) -> CommandSpec {
    let mut command = ContainerRunCommandBuilder::new(&profile.image.reference);

    command.flag("--rm");
    command.option("--name", &profile.guest.hostname);
    command.option("--arch", architecture_argument(profile.image.architecture));
    command.option("--cpus", profile.guest.cpus.get().to_string());
    command.option("--memory", profile.guest.memory.to_string());
    command.option("--user", &profile.guest.user);
    command.option("--workdir", workspace.guest());

    command.environment("LANG", &profile.guest.locale);
    command.environment("LC_ALL", &profile.guest.locale);
    command.environment("TZ", &profile.guest.timezone);
    for (name, value) in environments {
        command.environment(name, value);
    }

    command.flag("--interactive");
    command.flag("--tty");
    command.flag("--read-only");
    command.option("--tmpfs", "/tmp");
    command.option("--network", "default");
    command.option("--mount", mount_argument(workspace));
    for mount in agent_state_mounts {
        command.option(
            "--mount",
            bind_mount_argument(mount.host(), mount.guest(), false),
        );
    }
    command.option("--label", format!("org.cloister.profile={}", profile.name));

    command.container_command(container_command);
    command.finish()
}

fn architecture_argument(architecture: Architecture) -> &'static str {
    match architecture {
        Architecture::Arm64 => "arm64",
    }
}

fn mount_argument(workspace: &WorkspaceMount) -> OsString {
    bind_mount_argument(
        workspace.host(),
        workspace.guest(),
        workspace.access() == WorkspaceMountAccess::ReadOnly,
    )
}

fn bind_mount_argument(
    host: &std::path::Path,
    guest: &std::path::Path,
    readonly: bool,
) -> OsString {
    let mut argument = OsString::from("type=bind,source=");
    argument.push(host);
    argument.push(",target=");
    argument.push(guest);
    if readonly {
        argument.push(",readonly");
    }
    argument
}

struct ContainerRunCommandBuilder {
    options: Vec<OsString>,
    image: OsString,
    container_command: Vec<OsString>,
}

impl ContainerRunCommandBuilder {
    fn new(image: impl AsRef<OsStr>) -> Self {
        Self {
            options: vec![OsString::from("run")],
            image: image.as_ref().to_owned(),
            container_command: Vec::new(),
        }
    }

    fn flag(&mut self, flag: &'static str) {
        self.options.push(OsString::from(flag));
    }

    fn option(&mut self, option: &'static str, value: impl AsRef<OsStr>) {
        self.flag(option);
        self.options.push(value.as_ref().to_owned());
    }

    fn environment(&mut self, name: &'static str, value: impl AsRef<OsStr>) {
        let mut assignment = OsString::from(name);
        assignment.push("=");
        assignment.push(value);
        self.option("--env", assignment);
    }

    fn container_command(&mut self, command: &[OsString]) {
        self.container_command.extend_from_slice(command);
    }

    fn finish(self) -> CommandSpec {
        let mut arguments =
            Vec::with_capacity(self.options.len() + self.container_command.len() + 1);
        arguments.extend(self.options);
        arguments.push(self.image);
        arguments.extend(self.container_command);

        CommandSpec {
            program: OsString::from("container"),
            arguments,
        }
    }
}

fn reject_mount_separator(path: &std::path::Path) -> Result<(), RuntimePlanError> {
    if path.as_os_str().as_encoded_bytes().contains(&b',') {
        Err(RuntimePlanError::MountPathContainsSeparator {
            path: path.as_os_str().to_owned(),
        })
    } else {
        Ok(())
    }
}

/// Profile setting that cannot be represented safely by the current adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimePlanError {
    UnsupportedNetworkMode,
    MountPathContainsSeparator { path: OsString },
    CodexDisabled,
    SharedCodexStateMissing,
    SharedStateRequiresAgentCommand,
}

impl fmt::Display for RuntimePlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedNetworkMode => {
                formatter.write_str(message::NETWORK_RESTRICTED_NOT_IMPLEMENTED)
            }
            Self::MountPathContainsSeparator { path } => {
                write!(
                    formatter,
                    "{}: {:?}",
                    message::MOUNT_PATH_CONTAINS_SEPARATOR,
                    OsStr::new(path)
                )
            }
            Self::CodexDisabled => formatter.write_str(message::CODEX_DISABLED),
            Self::SharedCodexStateMissing => {
                formatter.write_str(message::SHARED_CODEX_STATE_MISSING)
            }
            Self::SharedStateRequiresAgentCommand => {
                formatter.write_str(message::SHARED_STATE_REQUIRES_AGENT_COMMAND)
            }
        }
    }
}

impl Error for RuntimePlanError {}
