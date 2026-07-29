//! Host capability bridge commands.

use std::{error::Error, fmt, io, net::SocketAddr, path::PathBuf};

use clap::{Args, Subcommand, ValueHint};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::{
    error::message,
    host_bridge::{
        BridgeToken, BridgeTokenError, HostBridgeClientError, HostBridgeServerError,
        call_host_exec, serve,
    },
};

#[derive(Debug, Args)]
pub(super) struct HostArgs {
    #[command(subcommand)]
    command: HostCommand,
}

#[derive(Debug, Subcommand)]
enum HostCommand {
    /// Serve the authenticated host shell MCP tool.
    Serve {
        /// Loopback address used by the host bridge.
        #[arg(long, default_value = "127.0.0.1:17834")]
        listen: SocketAddr,

        /// Owner-only file used to load or create the bridge bearer token.
        #[arg(long, value_name = "TOKEN_FILE", value_hint = ValueHint::FilePath)]
        token_file: PathBuf,
    },

    /// Execute a shell command through the host bridge.
    Exec {
        /// Full Streamable HTTP MCP endpoint.
        #[arg(long, value_name = "URL")]
        endpoint: String,

        /// Owner-only bridge bearer token file.
        #[arg(long, value_name = "TOKEN_FILE", value_hint = ValueHint::FilePath)]
        token_file: PathBuf,

        /// Shell command to execute on the host.
        #[arg(value_name = "COMMAND")]
        command: String,
    },
}

impl HostArgs {
    pub(super) async fn execute(self) -> Result<(), HostCommandError> {
        match self.command {
            HostCommand::Serve { listen, token_file } => serve_command(listen, token_file).await,
            HostCommand::Exec {
                endpoint,
                token_file,
                command,
            } => exec_command(&endpoint, token_file, &command).await,
        }
    }
}

async fn serve_command(listen: SocketAddr, token_file: PathBuf) -> Result<(), HostCommandError> {
    if !listen.ip().is_loopback() {
        return Err(HostCommandError::NonLoopback { address: listen });
    }

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
    println!("Token file: {}", token_file.display());
    println!("Tool: host.exec (arbitrary host command execution)");

    let server = serve(listener, token, cancellation.clone());
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
    command: &str,
) -> Result<(), HostCommandError> {
    let token = BridgeToken::load(token_file)?;
    let output = call_host_exec(endpoint, &token, command).await?;

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
            Self::Listen { source, .. } => Some(source),
            Self::Server(error) => Some(error),
            Self::Client(error) => Some(error),
            Self::NonLoopback { .. } | Self::Signal { .. } | Self::HostExec { .. } => None,
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
