//! Translation from resolved profiles to Apple container 1.2 command arguments.

use std::{
    error::Error,
    ffi::{OsStr, OsString},
    fmt,
};

use crate::{
    error::message,
    preflight::ResolvedProfile,
    profile::{Architecture, NetworkMode, Profile, WorkspaceAccess, WorkspaceMode},
};

use super::plan::{
    AgentKind, CommandSpec, NetworkExposure, RuntimePlan, WorkspaceMount, WorkspaceMountAccess,
};

/// Produces an inspectable `container run` plan without starting a process.
pub fn plan_apple_container(
    resolved: &ResolvedProfile,
    container_command: &[OsString],
) -> Result<RuntimePlan, RuntimePlanError> {
    let profile = resolved.profile();

    if profile.workspace.mode != WorkspaceMode::Bind {
        return Err(RuntimePlanError::UnsupportedWorkspaceMode);
    }
    if profile.network.mode != NetworkMode::Default {
        return Err(RuntimePlanError::UnsupportedNetworkMode);
    }
    reject_mount_separator(&profile.workspace.host)?;
    reject_mount_separator(&profile.workspace.guest)?;

    let workspace_access = match profile.workspace.access {
        WorkspaceAccess::ReadOnly => WorkspaceMountAccess::ReadOnly,
        WorkspaceAccess::ReadWrite => WorkspaceMountAccess::ReadWrite,
    };
    let workspace = WorkspaceMount {
        host: profile.workspace.host.clone(),
        guest: profile.workspace.guest.clone(),
        access: workspace_access,
    };
    let enabled_agents = [
        profile.agents.codex.enabled.then_some(AgentKind::Codex),
        profile.agents.claude.enabled.then_some(AgentKind::Claude),
    ]
    .into_iter()
    .flatten()
    .collect();
    let command = build_run_command(profile, &workspace, container_command);

    Ok(RuntimePlan {
        profile_name: profile.name.clone(),
        guest_hostname: profile.guest.hostname.clone(),
        network: NetworkExposure::DefaultWithInternetEgress,
        workspace,
        enabled_agents,
        command,
    })
}

fn build_run_command(
    profile: &Profile,
    workspace: &WorkspaceMount,
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

    command.flag("--interactive");
    command.flag("--tty");
    command.flag("--read-only");
    command.option("--tmpfs", "/tmp");
    command.option("--network", "default");
    command.option("--mount", mount_argument(workspace));
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
    let mut argument = OsString::from("type=bind,source=");
    argument.push(workspace.host());
    argument.push(",target=");
    argument.push(workspace.guest());
    if workspace.access() == WorkspaceMountAccess::ReadOnly {
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

    fn environment(&mut self, name: &'static str, value: &str) {
        self.option("--env", format!("{name}={value}"));
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
    UnsupportedWorkspaceMode,
    UnsupportedNetworkMode,
    MountPathContainsSeparator { path: OsString },
}

impl fmt::Display for RuntimePlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedWorkspaceMode => {
                formatter.write_str(message::WORKSPACE_COPY_NOT_IMPLEMENTED)
            }
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
        }
    }
}

impl Error for RuntimePlanError {}
