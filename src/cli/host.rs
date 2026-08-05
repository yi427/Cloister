//! Host capability bridge commands.

use std::{env, error::Error, fmt, io, net::SocketAddr, path::PathBuf};

use clap::{Args, Subcommand, ValueHint};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::{
    error::message,
    host_bridge::{
        BridgeToken, BridgeTokenError, HOST_EXEC_DSL_VERSION, HostBridgeClientError,
        HostBridgeServerError, HostExecPolicy, HostExecPolicyBuildError, HostExecRequest,
        call_host_exec, serve,
    },
    preflight::{
        HostExecutableCheckError, PreflightError, inspect_host_executable, resolve_launch,
    },
    profile::{LoadProfileError, load_profile},
};

use super::config::default_profile_path;

#[derive(Debug, Args)]
pub(super) struct HostArgs {
    #[command(subcommand)]
    command: HostCommand,
}

#[derive(Debug, Subcommand)]
enum HostCommand {
    /// Serve the authenticated, Profile-governed Host MCP tools.
    Serve {
        /// Loopback address used by the host bridge.
        #[arg(long, default_value = "127.0.0.1:17834")]
        listen: SocketAddr,

        /// Owner-only file used to load or create the bridge bearer token.
        #[arg(long, value_name = "TOKEN_FILE", value_hint = ValueHint::FilePath)]
        token_file: PathBuf,

        /// Path to a Profile V5 TOML file.
        ///
        /// Defaults to ~/.config/cloister/profile.toml.
        #[arg(long, value_name = "PROFILE", value_hint = ValueHint::FilePath)]
        profile: Option<PathBuf>,
    },

    /// Execute one allowed command through the Host MCP bridge.
    Exec {
        /// Full Streamable HTTP MCP endpoint.
        #[arg(long, value_name = "URL")]
        endpoint: String,

        /// Owner-only bridge bearer token file.
        #[arg(long, value_name = "TOKEN_FILE", value_hint = ValueHint::FilePath)]
        token_file: PathBuf,

        /// Stable command name from the bridge's Profile allowlist.
        #[arg(value_name = "COMMAND")]
        command: String,

        /// Literal arguments passed directly to the allowed executable.
        #[arg(last = true, value_name = "ARGUMENT")]
        arguments: Vec<String>,
    },
}

impl HostArgs {
    pub(super) async fn execute(self) -> Result<(), HostCommandError> {
        match self.command {
            HostCommand::Serve {
                listen,
                token_file,
                profile,
            } => serve_command(listen, token_file, profile).await,
            HostCommand::Exec {
                endpoint,
                token_file,
                command,
                arguments,
            } => exec_command(&endpoint, token_file, command, arguments).await,
        }
    }
}

async fn serve_command(
    listen: SocketAddr,
    token_file: PathBuf,
    profile_path: Option<PathBuf>,
) -> Result<(), HostCommandError> {
    if !listen.ip().is_loopback() {
        return Err(HostCommandError::NonLoopback { address: listen });
    }

    let profile_path = profile_path
        .or_else(default_profile_path)
        .ok_or(HostCommandError::HomeDirectoryMissing)?;
    let profile = load_profile(&profile_path)?;
    let workspace = env::current_dir().map_err(HostCommandError::CurrentDirectory)?;
    let resolved = resolve_launch(profile, workspace)?;
    for command in &resolved.profile().host.exec.allow {
        inspect_host_executable(&command.executable).map_err(|source| {
            HostCommandError::HostExecutable {
                command: command.name.clone(),
                source,
            }
        })?;
    }
    let policy =
        HostExecPolicy::from_profile(&resolved.profile().host.exec, env::vars_os().collect())?
            .ok_or(HostCommandError::HostExecDisabled)?;
    let token = BridgeToken::load_or_create(&token_file)?;
    let listener = TcpListener::bind(listen)
        .await
        .map_err(|source| HostCommandError::Listen {
            address: listen,
            source,
        })?;
    let local_address = listener
        .local_addr()
        .map_err(|source| HostCommandError::Listen {
            address: listen,
            source,
        })?;
    let cancellation = CancellationToken::new();

    println!("Host bridge listening on http://{local_address}/mcp");
    println!(
        "Profile: {} ({})",
        resolved.profile().name,
        profile_path.display()
    );
    println!("Working directory: {}", resolved.workspace().display());
    println!("Token file: {}", token_file.display());
    println!("Tools: host.list_commands, host.exec");
    println!("Allowed commands: {}", policy.command_count());

    let server = serve(
        listener,
        token,
        policy,
        resolved.workspace().to_owned(),
        cancellation.clone(),
    );
    tokio::pin!(server);
    let server_result = tokio::select! {
        result = &mut server => result,
        signal = tokio::signal::ctrl_c() => {
            signal.map_err(|source| HostCommandError::Signal {
                detail: source.to_string(),
            })?;
            cancellation.cancel();
            server.await
        }
    };

    server_result.map_err(HostCommandError::Server)
}

async fn exec_command(
    endpoint: &str,
    token_file: PathBuf,
    command: String,
    arguments: Vec<String>,
) -> Result<(), HostCommandError> {
    let token = BridgeToken::load(token_file)?;
    let output = call_host_exec(
        endpoint,
        &token,
        &HostExecRequest {
            version: HOST_EXEC_DSL_VERSION,
            command,
            args: arguments,
        },
    )
    .await?;

    print!("{}", output.stdout);
    eprint!("{}", output.stderr);

    if output.exit_code == Some(0) {
        Ok(())
    } else {
        Err(HostCommandError::HostExec {
            exit_code: output.exit_code,
        })
    }
}

#[derive(Debug)]
pub(super) enum HostCommandError {
    Token(BridgeTokenError),
    HomeDirectoryMissing,
    Load(LoadProfileError),
    Policy(HostExecPolicyBuildError),
    HostExecDisabled,
    CurrentDirectory(io::Error),
    Preflight(PreflightError),
    HostExecutable {
        command: String,
        source: HostExecutableCheckError,
    },
    NonLoopback {
        address: SocketAddr,
    },
    Listen {
        address: SocketAddr,
        source: io::Error,
    },
    Server(HostBridgeServerError),
    Client(HostBridgeClientError),
    Signal {
        detail: String,
    },
    HostExec {
        exit_code: Option<i32>,
    },
}

impl fmt::Display for HostCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Token(error) => error.fmt(formatter),
            Self::HomeDirectoryMissing => formatter.write_str(message::HOME_DIRECTORY_MISSING),
            Self::Load(error) => error.fmt(formatter),
            Self::Policy(error) => write!(formatter, "failed to build Host Exec policy: {error}"),
            Self::HostExecDisabled => {
                formatter.write_str("Host Exec is disabled by the selected Profile")
            }
            Self::CurrentDirectory(source) => {
                write!(formatter, "{}: {source}", message::CURRENT_DIRECTORY_FAILED)
            }
            Self::Preflight(error) => error.fmt(formatter),
            Self::HostExecutable { command, source } => write!(
                formatter,
                "host command '{command}' is unavailable: {source}"
            ),
            Self::NonLoopback { address } => {
                write!(
                    formatter,
                    "{}: {address}",
                    message::BRIDGE_NON_LOOPBACK_LISTEN
                )
            }
            Self::Listen { address, source } => write!(
                formatter,
                "{} on {address}: {source}",
                message::BRIDGE_LISTEN_FAILED
            ),
            Self::Server(error) => error.fmt(formatter),
            Self::Client(error) => error.fmt(formatter),
            Self::Signal { detail } => {
                write!(formatter, "{}: {detail}", message::BRIDGE_SIGNAL_FAILED)
            }
            Self::HostExec { exit_code } => write!(
                formatter,
                "{} with exit code {exit_code:?}",
                message::HOST_EXEC_FAILED
            ),
        }
    }
}

impl Error for HostCommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Token(error) => Some(error),
            Self::Load(error) => Some(error),
            Self::Policy(error) => Some(error),
            Self::HostExecutable { source, .. } => Some(source),
            Self::CurrentDirectory(source) => Some(source),
            Self::Preflight(error) => Some(error),
            Self::Listen { source, .. } => Some(source),
            Self::Server(error) => Some(error),
            Self::Client(error) => Some(error),
            Self::HomeDirectoryMissing
            | Self::HostExecDisabled
            | Self::NonLoopback { .. }
            | Self::Signal { .. }
            | Self::HostExec { .. } => None,
        }
    }
}

impl From<BridgeTokenError> for HostCommandError {
    fn from(error: BridgeTokenError) -> Self {
        Self::Token(error)
    }
}

impl From<HostBridgeClientError> for HostCommandError {
    fn from(error: HostBridgeClientError) -> Self {
        Self::Client(error)
    }
}

impl From<LoadProfileError> for HostCommandError {
    fn from(error: LoadProfileError) -> Self {
        Self::Load(error)
    }
}

impl From<HostExecPolicyBuildError> for HostCommandError {
    fn from(error: HostExecPolicyBuildError) -> Self {
        Self::Policy(error)
    }
}

impl From<PreflightError> for HostCommandError {
    fn from(error: PreflightError) -> Self {
        Self::Preflight(error)
    }
}
