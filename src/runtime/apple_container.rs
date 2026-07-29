//! Translation from resolved profiles to Apple container 1.2 command arguments.

use std::{
    error::Error,
    ffi::{OsStr, OsString},
    fmt,
};

use crate::{
    error::message,
    preflight::ResolvedProfile,
    profile::{Architecture, NetworkMode, WorkspaceAccess, WorkspaceMode},
};

use super::plan::{
    AgentKind, CommandSpec, NetworkExposure, RuntimePlan, WorkspaceMount, WorkspaceMountAccess,
};

/// Produces an inspectable `container create` plan without starting a process.
pub fn plan_apple_container(resolved: &ResolvedProfile) -> Result<RuntimePlan, RuntimePlanError> {
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
    let command = build_create_command(resolved, workspace_access);

    Ok(RuntimePlan {
        profile_name: profile.name.clone(),
        guest_hostname: profile.guest.hostname.clone(),
        network: NetworkExposure::DefaultWithInternetEgress,
        workspace,
        enabled_agents,
        command,
    })
}

fn build_create_command(
    resolved: &ResolvedProfile,
    workspace_access: WorkspaceMountAccess,
) -> CommandSpec {
    let profile = resolved.profile();
    let architecture = match profile.image.architecture {
        Architecture::Arm64 => "arm64",
    };
    let arguments = vec![
        OsString::from("create"),
        OsString::from("--name"),
        OsString::from(&profile.guest.hostname),
        OsString::from("--arch"),
        OsString::from(architecture),
        OsString::from("--cpus"),
        OsString::from(profile.guest.cpus.get().to_string()),
        OsString::from("--memory"),
        OsString::from(profile.guest.memory.to_string()),
        OsString::from("--user"),
        OsString::from(&profile.guest.user),
        OsString::from("--workdir"),
        profile.workspace.guest.as_os_str().to_owned(),
        OsString::from("--env"),
        OsString::from(format!("LANG={}", profile.guest.locale)),
        OsString::from("--env"),
        OsString::from(format!("LC_ALL={}", profile.guest.locale)),
        OsString::from("--env"),
        OsString::from(format!("TZ={}", profile.guest.timezone)),
        OsString::from("--interactive"),
        OsString::from("--tty"),
        OsString::from("--read-only"),
        OsString::from("--tmpfs"),
        OsString::from("/tmp"),
        OsString::from("--network"),
        OsString::from("default"),
        OsString::from("--mount"),
        mount_argument(resolved, workspace_access),
        OsString::from("--label"),
        OsString::from(format!("org.cloister.profile={}", profile.name)),
        OsString::from(&profile.image.reference),
    ];

    CommandSpec {
        program: OsString::from("container"),
        arguments,
    }
}

fn mount_argument(resolved: &ResolvedProfile, workspace_access: WorkspaceMountAccess) -> OsString {
    let workspace = &resolved.profile().workspace;
    let mut argument = OsString::from("type=bind,source=");
    argument.push(&workspace.host);
    argument.push(",target=");
    argument.push(&workspace.guest);
    if workspace_access == WorkspaceMountAccess::ReadOnly {
        argument.push(",readonly");
    }
    argument
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
