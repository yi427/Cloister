//! Translation from a resolved agent launch to Apple container arguments.

use std::{
    error::Error,
    ffi::{OsStr, OsString},
    fmt,
    path::Path,
};

use crate::{
    agent::{AgentAdapter, AgentCommand, AgentHostBridge},
    error::message,
    preflight::{HostExecutableCheckError, ResolvedLaunch, inspect_host_executable},
    profile::{AgentState, Architecture, NetworkMode, Profile},
};

use super::plan::{
    AgentStateMount, CommandSpec, HostCommandPlan, NetworkExposure, RuntimePlan,
    SecretEnvironmentVariable, WorkspaceMount,
};

const HOST_BRIDGE_TOKEN_ENVIRONMENT: &str = "CLOISTER_HOST_BRIDGE_TOKEN";
const WORKSPACE_GUEST_PATH: &str = "/workspace";

/// Executable used for the Apple container runtime.
pub const APPLE_CONTAINER_PROGRAM: &str = "container";

/// Guest DNS name forwarded to macOS loopback for the authenticated host bridge.
pub const HOST_BRIDGE_GUEST_NAME: &str = "host.container.internal";

/// Documentation-range address used by Apple container's localhost forwarding.
pub const HOST_BRIDGE_LOCALHOST_ADDRESS: &str = "203.0.113.113";

/// Produces the read-only system status query used by readiness checks.
pub fn system_status_command() -> CommandSpec {
    apple_container_command(["system", "status", "--format", "json"])
}

/// Produces the command that starts Apple container's system services.
pub fn system_start_command() -> CommandSpec {
    apple_container_command(["system", "start"])
}

/// Produces a read-only query for one exact image reference.
pub fn image_inspect_command(reference: &str) -> CommandSpec {
    apple_container_command(["image", "inspect", reference])
}

/// Produces an ARM64-only pull for one exact image reference.
pub fn image_pull_command(reference: &str) -> CommandSpec {
    apple_container_command(["image", "pull", "--arch", "arm64", reference])
}

/// Produces the read-only DNS domain query used by readiness checks.
pub fn dns_list_command() -> CommandSpec {
    apple_container_command(["system", "dns", "list", "--format", "json"])
}

/// Produces the privileged, explicit localhost forwarding command.
pub fn dns_create_command() -> CommandSpec {
    CommandSpec {
        program: OsString::from("sudo"),
        arguments: [
            APPLE_CONTAINER_PROGRAM,
            "system",
            "dns",
            "create",
            HOST_BRIDGE_GUEST_NAME,
            "--localhost",
            HOST_BRIDGE_LOCALHOST_ADDRESS,
        ]
        .into_iter()
        .map(OsString::from)
        .collect(),
        secret_environment: Vec::new(),
    }
}

fn apple_container_command<'a>(arguments: impl IntoIterator<Item = &'a str>) -> CommandSpec {
    CommandSpec {
        program: OsString::from(APPLE_CONTAINER_PROGRAM),
        arguments: arguments.into_iter().map(OsString::from).collect(),
        secret_environment: Vec::new(),
    }
}

/// Transient MCP endpoint and bearer token injected for one agent invocation.
#[derive(Clone, Copy)]
pub struct HostBridgeLaunch<'a> {
    endpoint: &'a str,
    bearer_token: Option<&'a str>,
}

impl<'a> HostBridgeLaunch<'a> {
    pub const fn new(endpoint: &'a str, bearer_token: &'a str) -> Self {
        Self {
            endpoint,
            bearer_token: Some(bearer_token),
        }
    }

    pub const fn dry_run(endpoint: &'a str) -> Self {
        Self {
            endpoint,
            bearer_token: None,
        }
    }
}

impl fmt::Debug for HostBridgeLaunch<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostBridgeLaunch")
            .field("endpoint", &self.endpoint)
            .field("bearer_token", &"[REDACTED]")
            .finish()
    }
}

/// Produces an inspectable agent launch plan without starting a process.
pub fn plan_agent_container(
    resolved: &ResolvedLaunch,
    agent: &dyn AgentAdapter,
    shared_state: Option<&Path>,
    host_bridge: Option<HostBridgeLaunch<'_>>,
    agent_arguments: &[OsString],
) -> Result<RuntimePlan, RuntimePlanError> {
    reject_mount_separator(resolved.workspace())?;

    let workspace = WorkspaceMount {
        host: resolved.workspace().to_owned(),
        guest: WORKSPACE_GUEST_PATH.into(),
    };
    let agent_state = match resolved.profile().agent.state {
        AgentState::Isolated => None,
        AgentState::Shared => {
            let host = shared_state.ok_or(RuntimePlanError::SharedAgentStateMissing {
                agent: agent.display_name(),
            })?;
            reject_mount_separator(host)?;
            Some(AgentStateMount {
                host: host.to_owned(),
                guest: agent.shared_state_guest_path().to_owned(),
            })
        }
    };
    let agent_bridge = host_bridge
        .map(|bridge| AgentHostBridge::new(bridge.endpoint, HOST_BRIDGE_TOKEN_ENVIRONMENT));
    let host_commands = match host_bridge {
        Some(_) if !resolved.profile().host.exec.enabled => {
            return Err(RuntimePlanError::HostExecDisabled);
        }
        Some(_) => resolved
            .profile()
            .host
            .exec
            .allow
            .iter()
            .map(|command| {
                let executable =
                    inspect_host_executable(&command.executable).map_err(|source| {
                        RuntimePlanError::HostExecutable {
                            command: command.name.clone(),
                            source,
                        }
                    })?;
                Ok(HostCommandPlan {
                    name: command.name.clone(),
                    declared: executable.declared().to_owned(),
                    resolved: executable.resolved().to_owned(),
                })
            })
            .collect::<Result<Vec<_>, RuntimePlanError>>()?,
        None => Vec::new(),
    };
    let agent_command = agent.build_command(agent_bridge, agent_arguments);
    let command = build_run_command(
        resolved.profile(),
        &workspace,
        agent,
        agent_state.as_ref(),
        host_bridge,
        agent_command,
    );

    Ok(RuntimePlan {
        profile_name: resolved.profile().name.clone(),
        agent_name: agent.display_name().to_owned(),
        network: network_exposure(resolved.profile().network.mode),
        workspace,
        agent_state,
        host_bridge_endpoint: host_bridge.map(|bridge| bridge.endpoint.to_owned()),
        host_commands,
        command,
    })
}

fn build_run_command(
    profile: &Profile,
    workspace: &WorkspaceMount,
    agent: &dyn AgentAdapter,
    agent_state: Option<&AgentStateMount>,
    host_bridge: Option<HostBridgeLaunch<'_>>,
    agent_command: AgentCommand,
) -> CommandSpec {
    let mut command = ContainerRunCommandBuilder::new(&profile.image.reference);

    command.flag("--rm");
    command.option("--arch", architecture_argument(profile.image.architecture));
    command.option("--cpus", profile.guest.cpus.get().to_string());
    command.option("--memory", profile.guest.memory.to_string());
    command.option("--user", &profile.guest.user);
    command.option("--workdir", workspace.guest());

    command.environment("LANG", &profile.guest.locale);
    command.environment("LC_ALL", &profile.guest.locale);
    command.environment("TZ", &profile.guest.timezone);
    if let Some(state) = agent_state {
        command.environment(agent.state_environment(), state.guest());
    }
    if let Some(bridge) = host_bridge {
        command.forward_secret_environment(HOST_BRIDGE_TOKEN_ENVIRONMENT, bridge.bearer_token);
    }

    command.flag("--interactive");
    command.flag("--tty");
    command.flag("--read-only");
    command.option("--tmpfs", "/tmp");
    command.option("--network", network_argument(profile.network.mode));
    command.option(
        "--mount",
        bind_mount_argument(workspace.host(), workspace.guest()),
    );
    if let Some(state) = agent_state {
        command.option("--mount", bind_mount_argument(state.host(), state.guest()));
    }
    command.option("--label", format!("org.cloister.profile={}", profile.name));

    command.agent_command(agent_command);
    command.finish()
}

fn architecture_argument(architecture: Architecture) -> &'static str {
    match architecture {
        Architecture::Arm64 => "arm64",
    }
}

fn network_argument(mode: NetworkMode) -> &'static str {
    match mode {
        NetworkMode::Default => "default",
    }
}

fn network_exposure(mode: NetworkMode) -> NetworkExposure {
    match mode {
        NetworkMode::Default => NetworkExposure::DefaultWithInternetEgress,
    }
}

fn bind_mount_argument(host: &Path, guest: &Path) -> OsString {
    let mut argument = OsString::from("type=bind,source=");
    argument.push(host);
    argument.push(",target=");
    argument.push(guest);
    argument
}

struct ContainerRunCommandBuilder {
    options: Vec<OsString>,
    secret_environment: Vec<SecretEnvironmentVariable>,
    image: OsString,
    container_command: Vec<OsString>,
}

impl ContainerRunCommandBuilder {
    fn new(image: impl AsRef<OsStr>) -> Self {
        Self {
            options: vec![OsString::from("run")],
            secret_environment: Vec::new(),
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

    fn environment(&mut self, name: &str, value: impl AsRef<OsStr>) {
        let mut assignment = OsString::from(name);
        assignment.push("=");
        assignment.push(value);
        self.option("--env", assignment);
    }

    fn forward_secret_environment(&mut self, name: &'static str, value: Option<&str>) {
        self.option("--env", name);
        if let Some(value) = value {
            self.secret_environment
                .push(SecretEnvironmentVariable::new(name, value));
        }
    }

    fn agent_command(&mut self, command: AgentCommand) {
        let (executable, arguments) = command.into_parts();
        self.container_command.push(executable);
        self.container_command.extend(arguments);
    }

    fn finish(self) -> CommandSpec {
        let mut arguments =
            Vec::with_capacity(self.options.len() + self.container_command.len() + 2);
        arguments.extend(self.options);
        // Apple container otherwise treats agent flags such as `--version` as
        // global runtime options even after the image reference.
        arguments.push(OsString::from("--"));
        arguments.push(self.image);
        arguments.extend(self.container_command);

        CommandSpec {
            program: OsString::from(APPLE_CONTAINER_PROGRAM),
            arguments,
            secret_environment: self.secret_environment,
        }
    }
}

fn reject_mount_separator(path: &Path) -> Result<(), RuntimePlanError> {
    if path.as_os_str().as_encoded_bytes().contains(&b',') {
        Err(RuntimePlanError::MountPathContainsSeparator {
            path: path.as_os_str().to_owned(),
        })
    } else {
        Ok(())
    }
}

/// Profile or launch input that cannot be represented safely.
#[derive(Debug)]
pub enum RuntimePlanError {
    MountPathContainsSeparator {
        path: OsString,
    },
    SharedAgentStateMissing {
        agent: &'static str,
    },
    HostExecDisabled,
    HostExecutable {
        command: String,
        source: HostExecutableCheckError,
    },
}

impl fmt::Display for RuntimePlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MountPathContainsSeparator { path } => write!(
                formatter,
                "{}: {:?}",
                message::MOUNT_PATH_CONTAINS_SEPARATOR,
                OsStr::new(path)
            ),
            Self::SharedAgentStateMissing { agent } => write!(
                formatter,
                "shared {agent} {}",
                message::SHARED_AGENT_STATE_MISSING
            ),
            Self::HostExecDisabled => {
                formatter.write_str("Host Exec is disabled by the selected Profile")
            }
            Self::HostExecutable { command, source } => write!(
                formatter,
                "host command '{command}' is unavailable: {source}"
            ),
        }
    }
}

impl Error for RuntimePlanError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::HostExecutable { source, .. } => Some(source),
            Self::MountPathContainsSeparator { .. }
            | Self::SharedAgentStateMissing { .. }
            | Self::HostExecDisabled => None,
        }
    }
}
