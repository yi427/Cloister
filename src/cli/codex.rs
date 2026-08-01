//! Natural Codex entry point backed by the current project directory.

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
    error::message,
    host_bridge::{
        BridgeToken, BridgeTokenError, HostBridgeServerError, serve as serve_host_bridge,
    },
    preflight::{PreflightError, resolve_launch},
    profile::{AgentState, LoadProfileError, Profile},
    runtime::{
        HostBridgeLaunch, RuntimeExecutionError, RuntimePlanError, execute, plan_codex_container,
    },
};

const HOST_BRIDGE_GUEST_NAME: &str = "host.container.internal";

#[derive(Debug, Args)]
pub(super) struct CodexArgs {
    /// Path to a Profile V3 TOML file.
    ///
    /// Defaults to ~/.config/cloister/profile.toml.
    #[arg(long, value_name = "PROFILE", value_hint = ValueHint::FilePath)]
    profile: Option<PathBuf>,

    /// Host project directory mounted at /workspace.
    ///
    /// Defaults to the current directory.
    #[arg(long, value_name = "DIRECTORY", value_hint = ValueHint::DirPath)]
    workspace: Option<PathBuf>,

    /// Print the runtime plan without starting Codex.
    #[arg(long)]
    dry_run: bool,

    /// Disable the default authenticated macOS host.exec MCP bridge.
    #[arg(long)]
    no_host_bridge: bool,

    /// Loopback port used by the default host bridge.
    #[arg(long, default_value = "17834", value_name = "PORT")]
    host_bridge_port: NonZeroU16,

    /// Arguments passed directly to Codex.
    #[arg(last = true, value_name = "ARGUMENT")]
    arguments: Vec<OsString>,
}

impl CodexArgs {
    pub(super) async fn execute(self) -> Result<ExitCode, CodexCommandError> {
        let profile = load_selected_profile(self.profile.as_deref())?;
        let workspace = match self.workspace {
            Some(workspace) => workspace,
            None => env::current_dir().map_err(CodexCommandError::CurrentDirectory)?,
        };
        let resolved = resolve_launch(profile, workspace)?;

        let shared_state = match resolved.profile().codex.state {
            AgentState::Isolated => None,
            AgentState::Shared if self.dry_run => Some(codex_state_directory_path()?),
            AgentState::Shared => Some(prepare_codex_state_directory()?),
        };
        if self.dry_run {
            let endpoint = host_bridge_endpoint(self.host_bridge_port);
            let host_bridge =
                (!self.no_host_bridge).then(|| HostBridgeLaunch::dry_run(endpoint.as_str()));
            let plan = plan_codex_container(
                &resolved,
                shared_state.as_deref(),
                host_bridge,
                &self.arguments,
            )?;
            print!("{plan}");
            return Ok(ExitCode::SUCCESS);
        }

        let host_bridge = if self.no_host_bridge {
            None
        } else {
            Some(RunningHostBridge::start(self.host_bridge_port).await?)
        };
        let bridge_launch = host_bridge.as_ref().map(RunningHostBridge::launch);
        let plan = match plan_codex_container(
            &resolved,
            shared_state.as_deref(),
            bridge_launch,
            &self.arguments,
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
            bridge.announce();
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
    async fn start(port: NonZeroU16) -> Result<Self, CodexCommandError> {
        let token = BridgeToken::generate()?;
        let address = SocketAddr::from(([127, 0, 0, 1], port.get()));
        let listener = TcpListener::bind(address)
            .await
            .map_err(|source| CodexCommandError::BridgeListen { address, source })?;
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

    fn announce(&self) {
        println!("Host bridge: {}", self.endpoint);
        println!("Host capability: host.exec (arbitrary macOS user commands)");
        println!("Codex MCP approval: prompt");
    }

    async fn shutdown(self) -> Result<(), CodexCommandError> {
        self.cancellation.cancel();
        self.task
            .await
            .map_err(CodexCommandError::BridgeTask)?
            .map_err(CodexCommandError::BridgeServer)
    }
}

fn load_selected_profile(path: Option<&Path>) -> Result<Profile, CodexCommandError> {
    let path = match path {
        Some(path) => path.to_owned(),
        None => default_profile_path()?,
    };
    crate::profile::load_profile(path).map_err(CodexCommandError::Load)
}

fn child_exit_code(status: ExitStatus) -> ExitCode {
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map(ExitCode::from)
        .unwrap_or(ExitCode::FAILURE)
}

fn default_profile_path() -> Result<PathBuf, CodexCommandError> {
    env::var_os("XDG_CONFIG_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|path| !path.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })
        .map(|directory| directory.join("cloister/profile.toml"))
        .ok_or(CodexCommandError::HomeDirectoryMissing)
}

fn codex_state_directory_path() -> Result<PathBuf, CodexCommandError> {
    let base = env::var_os("XDG_DATA_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|path| !path.is_empty())
                .map(|home| PathBuf::from(home).join(".local/share"))
        })
        .ok_or(CodexCommandError::HomeDirectoryMissing)?;
    Ok(base.join("cloister/agents/codex"))
}

fn prepare_codex_state_directory() -> Result<PathBuf, CodexCommandError> {
    let path = codex_state_directory_path()?;
    fs::create_dir_all(&path).map_err(|source| CodexCommandError::AgentState {
        path: path.clone(),
        kind: AgentStateDirectoryErrorKind::Create(source),
    })?;

    let metadata = fs::symlink_metadata(&path).map_err(|source| CodexCommandError::AgentState {
        path: path.clone(),
        kind: AgentStateDirectoryErrorKind::Metadata(source),
    })?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(CodexCommandError::AgentState {
            path,
            kind: AgentStateDirectoryErrorKind::Invalid,
        });
    }

    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(|source| {
        CodexCommandError::AgentState {
            path: path.clone(),
            kind: AgentStateDirectoryErrorKind::Permissions(source),
        }
    })?;

    path.canonicalize()
        .map_err(|source| CodexCommandError::AgentState {
            path,
            kind: AgentStateDirectoryErrorKind::Metadata(source),
        })
}

#[derive(Debug)]
pub(super) enum CodexCommandError {
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

impl fmt::Display for CodexCommandError {
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

impl Error for CodexCommandError {
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

impl From<PreflightError> for CodexCommandError {
    fn from(error: PreflightError) -> Self {
        Self::Preflight(error)
    }
}

impl From<RuntimePlanError> for CodexCommandError {
    fn from(error: RuntimePlanError) -> Self {
        Self::Plan(error)
    }
}

impl From<RuntimeExecutionError> for CodexCommandError {
    fn from(error: RuntimeExecutionError) -> Self {
        Self::Execution(error)
    }
}

impl From<BridgeTokenError> for CodexCommandError {
    fn from(error: BridgeTokenError) -> Self {
        Self::BridgeToken(error)
    }
}
