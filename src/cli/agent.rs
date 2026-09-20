//! Shared lifecycle for natural coding-agent entry points.

use std::{
    env,
    error::Error,
    ffi::OsString,
    fmt, fs, io,
    net::SocketAddr,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{ExitCode, ExitStatus},
};

use clap::{Args, ValueHint};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::{
    agent::{
        AgentAdapter,
        software::{self, SoftwareOptions},
    },
    error::message,
    host_bridge::{
        AuditLogPathError, BridgeToken, BridgeTokenError, HostBridgeContext, HostBridgeServerError,
        HostEnvironment, HostExecPolicy, HostExecPolicyBuildError, default_audit_log_path,
        serve as serve_host_bridge,
    },
    preflight::{
        GuestProxyResolutionError, HostExecutableCheckError, PreflightError,
        inspect_host_executable, resolve_guest_proxy, resolve_launch,
    },
    profile::{AgentState, AgentVersion, LoadProfileError, Profile},
    release::image::{ImageCompatibilityError, classify_image},
    runtime::{
        HOST_BRIDGE_GUEST_NAME, HostBridgeLaunch, RuntimeExecutionError, RuntimePlanError,
        execute_managed, name_container, plan_agent_container,
    },
};

use super::config::default_profile_path;

#[derive(Debug, Args)]
pub(super) struct AgentArgs {
    /// Path to a Profile V7 TOML file.
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

    /// Disable the Profile-enabled authenticated Host MCP bridge.
    #[arg(long)]
    no_host_bridge: bool,

    /// Loopback bridge port; 0 lets the operating system allocate a free port.
    #[arg(long, default_value = "0", value_name = "PORT")]
    host_bridge_port: u16,

    /// Use an exact agent version or latest for this invocation.
    #[arg(long, value_name = "VERSION")]
    agent_version: Option<AgentVersion>,

    /// Check/install agent software now, regardless of the update interval.
    #[arg(long, conflicts_with = "offline")]
    update_agent: bool,

    /// Use installed agent software without checking or downloading updates.
    /// Does not disable the agent's network access.
    #[arg(long)]
    offline: bool,

    /// Arguments passed directly to the agent.
    #[arg(last = true, value_name = "ARGUMENT")]
    arguments: Vec<OsString>,
}

pub(super) async fn execute_agent(
    agent: &dyn AgentAdapter,
    arguments: AgentArgs,
) -> Result<ExitCode, AgentCommandError> {
    let profile = load_selected_profile(arguments.profile.as_deref())?;
    let image_reference = profile.image.reference.as_str();
    let image_compatibility =
        classify_image(image_reference).map_err(AgentCommandError::ImageCompatibility)?;
    if image_compatibility.is_warning() {
        eprintln!(
            "cloister: warning: {}",
            image_compatibility.detail(image_reference)
        );
    }
    let workspace = match arguments.workspace {
        Some(workspace) => workspace,
        None => env::current_dir().map_err(AgentCommandError::CurrentDirectory)?,
    };
    let resolved = resolve_launch(profile, workspace)?;
    let host_environment = env::vars_os().collect::<HostEnvironment>();
    let guest_proxy =
        resolve_guest_proxy(resolved.profile().network.proxy, host_environment.clone())?;
    let host_bridge_enabled = !arguments.no_host_bridge && resolved.profile().host.exec.enabled;
    let audit_log_path = host_bridge_enabled
        .then(default_audit_log_path)
        .transpose()
        .map_err(AgentCommandError::AuditPath)?;
    if host_bridge_enabled {
        validate_host_executables(resolved.profile())?;
    }

    let software_root = software::software_directory(agent).map_err(AgentCommandError::Software)?;
    software::check_writable_mount(&software_root, resolved.workspace())
        .map_err(AgentCommandError::Software)?;
    let shared_state = match resolved.profile().agent.state {
        AgentState::Isolated => None,
        AgentState::Shared if arguments.dry_run => Some(agent_state_directory_path(agent)?),
        AgentState::Shared => Some(prepare_agent_state_directory(agent)?),
    };
    if let Some(state) = &shared_state {
        software::check_writable_mount(&software_root, state)
            .map_err(AgentCommandError::Software)?;
    }
    if host_bridge_enabled {
        validate_host_skill_conflict(agent, shared_state.as_deref())?;
    }
    let software_options = SoftwareOptions {
        version: arguments.agent_version,
        update: arguments.update_agent,
        offline: arguments.offline,
    };
    let selector = software::selected_version(resolved.profile(), agent, &software_options);
    if arguments.dry_run {
        let cached = software::inspect(&software_root, resolved.profile(), selector)
            .map_err(AgentCommandError::Software)?;
        if software_options.offline && cached.is_none() {
            return Err(AgentCommandError::Software(io::Error::other(
                "agent software is not installed for this image/version; --offline cannot launch",
            )));
        }
        let software_path = cached
            .as_ref()
            .map(|s| s.directory.clone())
            .unwrap_or_else(|| software_root.join("<selected-at-launch>"));
        let endpoint = host_bridge_endpoint(arguments.host_bridge_port);
        let host_bridge = audit_log_path
            .as_deref()
            .map(|path| HostBridgeLaunch::dry_run(endpoint.as_str(), path));
        let plan = plan_agent_container(
            &resolved,
            agent,
            shared_state.as_deref(),
            host_bridge,
            guest_proxy.as_ref(),
            &arguments.arguments,
            &software_path,
        )?;
        println!(
            "Agent software policy: {} (automatic checks every {}h; update-now={}; offline={})",
            selector.as_str(),
            resolved.profile().agent.update_interval_hours,
            software_options.update,
            software_options.offline
        );
        println!(
            "Agent software installed: {}",
            cached
                .as_ref()
                .map_or("not installed; download on launch", |s| s.version.as_str())
        );
        if host_bridge_enabled && arguments.host_bridge_port == 0 {
            println!("Host bridge port: automatic; :0 below is a planning placeholder");
        }
        print!("{plan}");
        return Ok(ExitCode::SUCCESS);
    }

    let cancellation = CancellationToken::new();
    let _signals = SignalGuard::start(cancellation.clone()).map_err(AgentCommandError::Software)?;
    let software = software::prepare(
        resolved.profile(),
        agent,
        guest_proxy.as_ref(),
        &software_options,
        &cancellation,
    )
    .await
    .map_err(AgentCommandError::Software)?;
    println!(
        "{} version: {} (session software is read-only)",
        agent.display_name(),
        software.version
    );

    let session = format!(
        "cloister-session-{}",
        BridgeToken::generate()?
            .secret()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(24)
            .collect::<String>()
    );

    let host_bridge = if !host_bridge_enabled {
        None
    } else {
        let policy = HostExecPolicy::from_profile(&resolved.profile().host.exec, host_environment)?
            .expect("enabled Profile policy should produce a Host Exec policy");
        Some(
            RunningHostBridge::start(
                arguments.host_bridge_port,
                policy,
                resolved.profile().name.clone(),
                agent.display_name().to_owned(),
                resolved.workspace().to_owned(),
                audit_log_path.expect("enabled Host bridge should have an audit path"),
                &cancellation,
            )
            .await?,
        )
    };
    let bridge_launch = host_bridge.as_ref().map(RunningHostBridge::launch);
    let mut plan = match plan_agent_container(
        &resolved,
        agent,
        shared_state.as_deref(),
        bridge_launch,
        guest_proxy.as_ref(),
        &arguments.arguments,
        &software.directory,
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
    name_container(plan.command_mut(), &session);
    let execution = execute_managed(plan.command(), &session, &cancellation, false).await;
    let shutdown = match host_bridge {
        Some(bridge) => bridge.shutdown().await,
        None => Ok(()),
    };

    match (execution, shutdown) {
        (Err(error), _) => Err(AgentCommandError::Software(error)),
        (Ok(_), Err(error)) => Err(error),
        (Ok(status), Ok(())) => Ok(child_exit_code(status)),
    }
}

fn host_bridge_endpoint(port: u16) -> String {
    format!("http://{HOST_BRIDGE_GUEST_NAME}:{port}/mcp")
}

fn validate_host_executables(profile: &Profile) -> Result<(), AgentCommandError> {
    for command in &profile.host.exec.allow {
        inspect_host_executable(&command.executable).map_err(|source| {
            AgentCommandError::HostExecutable {
                command: command.name.clone(),
                source,
            }
        })?;
    }
    Ok(())
}

fn validate_host_skill_conflict(
    agent: &dyn AgentAdapter,
    shared_state: Option<&Path>,
) -> Result<(), AgentCommandError> {
    let (Some(shared_state), Some(relative_path)) =
        (shared_state, agent.host_skill_conflict_path())
    else {
        return Ok(());
    };
    let path = shared_state.join(relative_path);
    match fs::symlink_metadata(&path) {
        Ok(_) => Err(AgentCommandError::HostSkillConflict { path }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(AgentCommandError::HostSkillConflictInspection { path, source }),
    }
}

struct RunningHostBridge {
    token: BridgeToken,
    endpoint: String,
    allowed_command_count: usize,
    audit_log_path: PathBuf,
    cancellation: CancellationToken,
    task: JoinHandle<Result<(), HostBridgeServerError>>,
}

impl RunningHostBridge {
    async fn start(
        port: u16,
        policy: HostExecPolicy,
        profile_name: String,
        agent_name: String,
        working_directory: PathBuf,
        audit_log_path: PathBuf,
        session_cancellation: &CancellationToken,
    ) -> Result<Self, AgentCommandError> {
        let token = BridgeToken::generate()?;
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = TcpListener::bind(address)
            .await
            .map_err(|source| AgentCommandError::BridgeListen { address, source })?;
        let port = listener
            .local_addr()
            .map_err(|source| AgentCommandError::BridgeListen { address, source })?
            .port();
        let cancellation = session_cancellation.child_token();
        let server_cancellation = cancellation.clone();
        let server_token = token.clone();
        let allowed_command_count = policy.command_count();
        let server_audit_log_path = audit_log_path.clone();
        let task = tokio::spawn(async move {
            serve_host_bridge(
                listener,
                server_token,
                policy,
                HostBridgeContext::new(
                    profile_name,
                    agent_name,
                    working_directory,
                    server_audit_log_path,
                ),
                server_cancellation,
            )
            .await
        });

        Ok(Self {
            token,
            endpoint: host_bridge_endpoint(port),
            allowed_command_count,
            audit_log_path,
            cancellation,
            task,
        })
    }

    fn launch(&self) -> HostBridgeLaunch<'_> {
        HostBridgeLaunch::new(&self.endpoint, self.token.secret(), &self.audit_log_path)
    }

    fn announce(&self, agent_name: &str) {
        println!("Host bridge: {}", self.endpoint);
        println!(
            "Host audit log: {} (JSONL, owner-only, 20 MiB total)",
            self.audit_log_path.display()
        );
        println!(
            "Host capabilities: host.list_commands, host.exec, host.exec_status, host.exec_cancel ({} Profile-allowed command(s); macOS user permissions)",
            self.allowed_command_count
        );
        println!("{agent_name} MCP approval: prompt for host.exec only");
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
    Software(io::Error),
    AuditPath(AuditLogPathError),
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
    BridgePolicy(HostExecPolicyBuildError),
    CurrentDirectory(io::Error),
    Execution(RuntimeExecutionError),
    GuestProxy(GuestProxyResolutionError),
    HomeDirectoryMissing,
    ImageCompatibility(ImageCompatibilityError),
    HostExecutable {
        command: String,
        source: HostExecutableCheckError,
    },
    HostSkillConflict {
        path: PathBuf,
    },
    HostSkillConflictInspection {
        path: PathBuf,
        source: io::Error,
    },
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
            Self::Software(error) => write!(formatter, "agent runtime: {error}"),
            Self::AuditPath(error) => error.fmt(formatter),
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
            Self::BridgePolicy(error) => {
                write!(formatter, "failed to build Host Exec policy: {error}")
            }
            Self::CurrentDirectory(source) => {
                write!(formatter, "{}: {source}", message::CURRENT_DIRECTORY_FAILED)
            }
            Self::Execution(error) => error.fmt(formatter),
            Self::GuestProxy(error) => error.fmt(formatter),
            Self::HomeDirectoryMissing => formatter.write_str(message::HOME_DIRECTORY_MISSING),
            Self::ImageCompatibility(error) => error.fmt(formatter),
            Self::HostExecutable { command, source } => write!(
                formatter,
                "host command '{command}' is unavailable: {source}"
            ),
            Self::HostSkillConflict { path } => write!(
                formatter,
                "refusing to shadow or overwrite the existing Host Skill at '{}'; remove or rename it before enabling the Cloister Host bridge",
                path.display()
            ),
            Self::HostSkillConflictInspection { path, source } => write!(
                formatter,
                "failed to inspect the Host Skill conflict path '{}': {source}",
                path.display()
            ),
            Self::Load(error) => error.fmt(formatter),
            Self::Preflight(error) => error.fmt(formatter),
            Self::Plan(error) => error.fmt(formatter),
        }
    }
}

impl Error for AgentCommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Software(error) => Some(error),
            Self::AuditPath(error) => Some(error),
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
            Self::BridgePolicy(error) => Some(error),
            Self::CurrentDirectory(source) => Some(source),
            Self::Execution(error) => Some(error),
            Self::GuestProxy(error) => Some(error),
            Self::HomeDirectoryMissing => None,
            Self::ImageCompatibility(error) => Some(error),
            Self::HostExecutable { source, .. } => Some(source),
            Self::HostSkillConflict { .. } => None,
            Self::HostSkillConflictInspection { source, .. } => Some(source),
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

impl From<GuestProxyResolutionError> for AgentCommandError {
    fn from(error: GuestProxyResolutionError) -> Self {
        Self::GuestProxy(error)
    }
}

impl From<BridgeTokenError> for AgentCommandError {
    fn from(error: BridgeTokenError) -> Self {
        Self::BridgeToken(error)
    }
}

impl From<HostExecPolicyBuildError> for AgentCommandError {
    fn from(error: HostExecPolicyBuildError) -> Self {
        Self::BridgePolicy(error)
    }
}

struct SignalGuard(JoinHandle<()>);
impl SignalGuard {
    fn start(cancellation: CancellationToken) -> io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut terminate = signal(SignalKind::terminate())?;
        let mut hangup = signal(SignalKind::hangup())?;
        Ok(Self(tokio::spawn(async move {
            tokio::select! { _ = interrupt.recv() => (), _ = terminate.recv() => (), _ = hangup.recv() => () }
            cancellation.cancel();
        })))
    }
}
impl Drop for SignalGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}
