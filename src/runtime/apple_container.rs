//! Translation from a resolved Codex launch to Apple container arguments.

use std::{
    error::Error,
    ffi::{OsStr, OsString},
    fmt,
    path::Path,
};

use crate::{
    error::message,
    preflight::ResolvedLaunch,
    profile::{AgentState, Architecture, NetworkMode, Profile},
};

use super::plan::{CodexStateMount, CommandSpec, NetworkExposure, RuntimePlan, WorkspaceMount};

const CODEX_STATE_GUEST_PATH: &str = "/cloister/agents/codex";
const WORKSPACE_GUEST_PATH: &str = "/workspace";

/// Produces an inspectable Codex launch plan without starting a process.
pub fn plan_codex_container(
    resolved: &ResolvedLaunch,
    shared_state: Option<&Path>,
    codex_arguments: &[OsString],
) -> Result<RuntimePlan, RuntimePlanError> {
    reject_mount_separator(resolved.workspace())?;

    let workspace = WorkspaceMount {
        host: resolved.workspace().to_owned(),
        guest: WORKSPACE_GUEST_PATH.into(),
    };
    let codex_state = match resolved.profile().codex.state {
        AgentState::Isolated => None,
        AgentState::Shared => {
            let host = shared_state.ok_or(RuntimePlanError::SharedCodexStateMissing)?;
            reject_mount_separator(host)?;
            Some(CodexStateMount {
                host: host.to_owned(),
                guest: CODEX_STATE_GUEST_PATH.into(),
            })
        }
    };
    let command = build_run_command(
        resolved.profile(),
        &workspace,
        codex_state.as_ref(),
        codex_arguments,
    );

    Ok(RuntimePlan {
        profile_name: resolved.profile().name.clone(),
        network: network_exposure(resolved.profile().network.mode),
        proxy: resolved.profile().network.proxy.clone(),
        workspace,
        codex_state,
        command,
    })
}

fn build_run_command(
    profile: &Profile,
    workspace: &WorkspaceMount,
    codex_state: Option<&CodexStateMount>,
    codex_arguments: &[OsString],
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
    if let Some(proxy) = &profile.network.proxy {
        for name in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"] {
            command.environment(name, proxy.as_str());
        }
    }
    if let Some(state) = codex_state {
        command.environment("CODEX_HOME", state.guest());
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
    if let Some(state) = codex_state {
        command.option("--mount", bind_mount_argument(state.host(), state.guest()));
    }
    command.option("--label", format!("org.cloister.profile={}", profile.name));

    command.container_command("codex", codex_arguments);
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

    fn container_command(&mut self, program: &'static str, arguments: &[OsString]) {
        self.container_command.push(OsString::from(program));
        self.container_command.extend_from_slice(arguments);
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimePlanError {
    MountPathContainsSeparator { path: OsString },
    SharedCodexStateMissing,
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
            Self::SharedCodexStateMissing => {
                formatter.write_str(message::SHARED_CODEX_STATE_MISSING)
            }
        }
    }
}

impl Error for RuntimePlanError {}
