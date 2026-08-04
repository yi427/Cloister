//! Shared lifecycle for natural coding-agent entry points.

use std::{
    env,
    error::Error,
    ffi::OsString,
    fmt, fs, io,
    net::SocketAddr,
    num::NonZeroU16,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{ExitCode, ExitStatus},
};

use clap::{Args, ValueHint};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::{
    agent::AgentAdapter,
    error::message,
    host_bridge::{
        BridgeToken, BridgeTokenError, HostBridgeServerError, serve as serve_host_bridge,
    },
    preflight::{PreflightError, resolve_launch},
    profile::{AgentState, LoadProfileError, Profile},
    runtime::{
        HOST_BRIDGE_GUEST_NAME, HostBridgeLaunch, RuntimeExecutionError, RuntimePlanError, execute,
        plan_agent_container,
    },
};

use super::config::default_profile_path;

#[derive(Debug, Args)]
pub(super) struct AgentArgs {
    /// Path to a Profile V4 TOML file.
    ///
    /// Defaults to ~/.config/cloister/profile.toml.
    #[arg(long, value_name = "PROFILE", value_hint = ValueHint::FilePath)]
    profile: Option<PathBuf>,

    /// Host project directory mounted at /workspace.
    ///
    /// Defaults to the current directory.
    #[arg(long, value_name = "DIRECTORY", value_hint = ValueHint::DirPath)]
    workspace: Option<PathBuf>,

    /// Print the runtime plan without starting the agent.
    #[arg(long)]
    dry_run: bool,

    /// Disable the default authenticated macOS host.exec MCP bridge.
    #[arg(long)]
    no_host_bridge: bool,

    /// Loopback port used by the default host bridge.
    #[arg(long, default_value = "17834", value_name = "PORT")]
    host_bridge_port: NonZeroU16,

    /// Arguments passed directly to the agent.
    #[arg(last = true, value_name = "ARGUMENT")]
    arguments: Vec<OsString>,
}

pub(super) async fn execute_agent(
    agent: &dyn AgentAdapter,
    arguments: AgentArgs,
) -> Result<ExitCode, AgentCommandError> {
    let profile = load_selected_profile(arguments.profile.as_deref())?;
    let workspace = match arguments.workspace {
        Some(workspace) => workspace,
        None => env::current_dir().map_err(AgentCommandError::CurrentDirectory)?,
    };
    let resolved = resolve_launch(profile, workspace)?;

    let shared_state = match resolved.profile().agent.state {
        AgentState::Isolated => None,
        AgentState::Shared if arguments.dry_run => Some(agent_state_directory_path(agent)?),
        AgentState::Shared => Some(prepare_agent_state_directory(agent)?),
    };
    if arguments.dry_run {
        let endpoint = host_bridge_endpoint(arguments.host_bridge_port);
        let host_bridge =
            (!arguments.no_host_bridge).then(|| HostBridgeLaunch::dry_run(endpoint.as_str()));
        let plan = plan_agent_container(
            &resolved,
            agent,
            shared_state.as_deref(),
            host_bridge,
            &arguments.arguments,
        )?;
        print!("{plan}");
        return Ok(ExitCode::SUCCESS);
    }

    let host_bridge = if arguments.no_host_bridge {
        None
    } else {
        Some(RunningHostBridge::start(arguments.host_bridge_port).await?)
    };
    let bridge_launch = host_bridge.as_ref().map(RunningHostBridge::launch);
    let plan = match plan_agent_container(
        &resolved,
        agent,
        shared_state.as_deref(),
        bridge_launch,
        &arguments.arguments,
    ) {
        Ok(plan) => plan,
        Err(error) => {
            if let Some(bridge) = host_bridge {
                bridge.shutdown().await?;
            }
            return Err(error.into());
        }
    };

    if let Some(bridge) = &host_bridge {
        bridge.announce(agent.display_name());
    }
    let execution = execute(plan.command()).await;
    let shutdown = match host_bridge {
        Some(bridge) => bridge.shutdown().await,
        None => Ok(()),
    };

    match (execution, shutdown) {
        (Err(error), _) => Err(error.into()),
        (Ok(_), Err(error)) => Err(error),
        (Ok(status), Ok(())) => Ok(child_exit_code(status)),
    }
}

fn host_bridge_endpoint(port: NonZeroU16) -> String {
    format!("http://{HOST_BRIDGE_GUEST_NAME}:{port}/mcp")
}

struct RunningHostBridge {
    token: BridgeToken,
    endpoint: String,
    cancellation: CancellationToken,
    task: JoinHandle<Result<(), HostBridgeServerError>>,
}

impl RunningHostBridge {
    async fn start(port: NonZeroU16) -> Result<Self, AgentCommandError> {
        let token = BridgeToken::generate()?;
        let address = SocketAddr::from(([127, 0, 0, 1], port.get()));
        let listener = TcpListener::bind(address)
            .await
            .map_err(|source| AgentCommandError::BridgeListen { address, source })?;
        let cancellation = CancellationToken::new();
        let server_cancellation = cancellation.clone();
        let server_token = token.clone();
        let task = tokio::spawn(async move {
            serve_host_bridge(listener, server_token, server_cancellation).await
        });

        Ok(Self {
            token,
            endpoint: host_bridge_endpoint(port),
            cancellation,
            task,
        })
    }

    fn launch(&self) -> HostBridgeLaunch<'_> {
        HostBridgeLaunch::new(&self.endpoint, self.token.secret())
    }

    fn announce(&self, agent_name: &str) {
        println!("Host bridge: {}", self.endpoint);
        println!("Host capability: host.exec (arbitrary macOS user commands)");
        println!("{agent_name} MCP approval: prompt");
    }

    async fn shutdown(self) -> Result<(), AgentCommandError> {
        self.cancellation.cancel();
        self.task
            .await
            .map_err(AgentCommandError::BridgeTask)?
            .map_err(AgentCommandError::BridgeServer)
    }
}

fn load_selected_profile(path: Option<&Path>) -> Result<Profile, AgentCommandError> {
    let path = match path {
        Some(path) => path.to_owned(),
        None => default_profile_path().ok_or(AgentCommandError::HomeDirectoryMissing)?,
    };
    crate::profile::load_profile(path).map_err(AgentCommandError::Load)
}

fn child_exit_code(status: ExitStatus) -> ExitCode {
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map(ExitCode::from)
        .unwrap_or(ExitCode::FAILURE)
}

fn agent_state_directory_path(agent: &dyn AgentAdapter) -> Result<PathBuf, AgentCommandError> {
    let base = env::var_os("XDG_DATA_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|path| !path.is_empty())
                .map(|home| PathBuf::from(home).join(".local/share"))
        })
        .ok_or(AgentCommandError::HomeDirectoryMissing)?;
    Ok(base
        .join("cloister/agents")
        .join(agent.state_directory_name()))
}

fn prepare_agent_state_directory(agent: &dyn AgentAdapter) -> Result<PathBuf, AgentCommandError> {
    let path = agent_state_directory_path(agent)?;
    fs::create_dir_all(&path).map_err(|source| AgentCommandError::AgentState {
        path: path.clone(),
        kind: AgentStateDirectoryErrorKind::Create(source),
    })?;

    let metadata = fs::symlink_metadata(&path).map_err(|source| AgentCommandError::AgentState {
        path: path.clone(),
        kind: AgentStateDirectoryErrorKind::Metadata(source),
    })?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(AgentCommandError::AgentState {
            path,
            kind: AgentStateDirectoryErrorKind::Invalid,
        });
    }

    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(|source| {
        AgentCommandError::AgentState {
            path: path.clone(),
            kind: AgentStateDirectoryErrorKind::Permissions(source),
        }
    })?;

    path.canonicalize()
        .map_err(|source| AgentCommandError::AgentState {
            path,
            kind: AgentStateDirectoryErrorKind::Metadata(source),
        })
}

#[derive(Debug)]
pub(super) enum AgentCommandError {
    AgentState {
        path: PathBuf,
        kind: AgentStateDirectoryErrorKind,
    },
    BridgeListen {
        address: SocketAddr,
        source: io::Error,
    },
    BridgeServer(HostBridgeServerError),
    BridgeTask(tokio::task::JoinError),
    BridgeToken(BridgeTokenError),
    CurrentDirectory(io::Error),
    Execution(RuntimeExecutionError),
    HomeDirectoryMissing,
    Load(LoadProfileError),
    Preflight(PreflightError),
    Plan(RuntimePlanError),
}

#[derive(Debug)]
pub(super) enum AgentStateDirectoryErrorKind {
    Create(io::Error),
    Metadata(io::Error),
    Invalid,
    Permissions(io::Error),
}

impl fmt::Display for AgentCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AgentState { path, kind } => {
                let prefix = match kind {
                    AgentStateDirectoryErrorKind::Create(_) => message::AGENT_STATE_CREATE_FAILED,
                    AgentStateDirectoryErrorKind::Metadata(_) => {
                        message::AGENT_STATE_METADATA_FAILED
                    }
                    AgentStateDirectoryErrorKind::Invalid => message::AGENT_STATE_INVALID,
                    AgentStateDirectoryErrorKind::Permissions(_) => {
                        message::AGENT_STATE_PERMISSIONS_FAILED
                    }
                };
                write!(formatter, "{prefix} '{}'", path.display())?;
                match kind {
                    AgentStateDirectoryErrorKind::Create(source)
                    | AgentStateDirectoryErrorKind::Metadata(source)
                    | AgentStateDirectoryErrorKind::Permissions(source) => {
                        write!(formatter, ": {source}")
                    }
                    AgentStateDirectoryErrorKind::Invalid => Ok(()),
                }
            }
            Self::BridgeListen { address, source } => write!(
                formatter,
                "{} on {address}: {source}",
                message::BRIDGE_LISTEN_FAILED
            ),
            Self::BridgeServer(error) => error.fmt(formatter),
            Self::BridgeTask(error) => {
                write!(formatter, "{}: {error}", message::BRIDGE_SERVE_FAILED)
            }
            Self::BridgeToken(error) => error.fmt(formatter),
            Self::CurrentDirectory(source) => {
                write!(formatter, "{}: {source}", message::CURRENT_DIRECTORY_FAILED)
            }
            Self::Execution(error) => error.fmt(formatter),
            Self::HomeDirectoryMissing => formatter.write_str(message::HOME_DIRECTORY_MISSING),
            Self::Load(error) => error.fmt(formatter),
            Self::Preflight(error) => error.fmt(formatter),
            Self::Plan(error) => error.fmt(formatter),
        }
    }
}

impl Error for AgentCommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::AgentState { kind, .. } => match kind {
                AgentStateDirectoryErrorKind::Create(source)
                | AgentStateDirectoryErrorKind::Metadata(source)
                | AgentStateDirectoryErrorKind::Permissions(source) => Some(source),
                AgentStateDirectoryErrorKind::Invalid => None,
            },
            Self::BridgeListen { source, .. } => Some(source),
            Self::BridgeServer(error) => Some(error),
            Self::BridgeTask(error) => Some(error),
            Self::BridgeToken(error) => Some(error),
            Self::CurrentDirectory(source) => Some(source),
            Self::Execution(error) => Some(error),
            Self::HomeDirectoryMissing => None,
            Self::Load(error) => Some(error),
            Self::Preflight(error) => Some(error),
            Self::Plan(error) => Some(error),
        }
    }
}

impl From<PreflightError> for AgentCommandError {
    fn from(error: PreflightError) -> Self {
        Self::Preflight(error)
    }
}

impl From<RuntimePlanError> for AgentCommandError {
    fn from(error: RuntimePlanError) -> Self {
        Self::Plan(error)
    }
}

impl From<RuntimeExecutionError> for AgentCommandError {
    fn from(error: RuntimeExecutionError) -> Self {
        Self::Execution(error)
    }
}

impl From<BridgeTokenError> for AgentCommandError {
    fn from(error: BridgeTokenError) -> Self {
        Self::BridgeToken(error)
    }
}
