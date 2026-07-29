//! Authenticated Streamable HTTP MCP server.

use std::{error::Error, fmt, io};

use axum::{
    Router,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use rmcp::{
    Json, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::{
        StreamableHttpServerConfig, StreamableHttpService,
        streamable_http_server::session::local::LocalSessionManager,
    },
};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::error::message;

use super::{BridgeToken, tools};

#[derive(Clone, Debug)]
struct HostBridgeService {
    tool_router: ToolRouter<Self>,
}

impl HostBridgeService {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router(router = tool_router)]
impl HostBridgeService {
    #[tool(
        name = "host.exec",
        description = "Execute an arbitrary shell command as the macOS user running the Cloister host bridge"
    )]
    async fn host_exec(
        &self,
        Parameters(input): Parameters<super::HostExecInput>,
    ) -> Result<Json<super::HostExecOutput>, String> {
        let started = std::time::Instant::now();
        let result = tools::host_exec(&input.command).await;

        match &result {
            Ok(output) => eprintln!(
                "audit capability=host.exec outcome=completed exit_code={:?} duration_ms={}",
                output.exit_code, output.duration_ms
            ),
            Err(error) => eprintln!(
                "audit capability=host.exec outcome=failed duration_ms={} error={error}",
                started.elapsed().as_millis()
            ),
        }

        result.map(Json).map_err(|error| error.to_string())
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for HostBridgeService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Cloister exposes host.exec, which runs arbitrary shell commands with the permissions of the macOS user running this bridge.",
        )
    }
}

/// Serves the authenticated MCP endpoint until cancellation.
pub async fn serve(
    listener: TcpListener,
    token: BridgeToken,
    cancellation: CancellationToken,
) -> Result<(), HostBridgeServerError> {
    let local_address = listener
        .local_addr()
        .map_err(HostBridgeServerError::InspectListener)?;
    if !local_address.ip().is_loopback() {
        return Err(HostBridgeServerError::NonLoopback {
            address: local_address,
        });
    }

    let service: StreamableHttpService<HostBridgeService, LocalSessionManager> =
        StreamableHttpService::new(
            || Ok(HostBridgeService::new()),
            Default::default(),
            StreamableHttpServerConfig::default()
                .with_json_response(true)
                .with_sse_keep_alive(None)
                .with_cancellation_token(cancellation.child_token()),
        );
    let router = Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(token, authorize));

    axum::serve(listener, router)
        .with_graceful_shutdown(cancellation.cancelled_owned())
        .await
        .map_err(HostBridgeServerError::Serve)
}

async fn authorize(State(expected): State<BridgeToken>, request: Request, next: Next) -> Response {
    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|candidate| expected.matches_bearer(candidate));

    if authorized {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
        )
            .into_response()
    }
}

/// Failure while serving the host capability endpoint.
#[derive(Debug)]
pub enum HostBridgeServerError {
    InspectListener(io::Error),
    NonLoopback { address: std::net::SocketAddr },
    Serve(io::Error),
}

impl fmt::Display for HostBridgeServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InspectListener(source) => {
                write!(formatter, "{}: {source}", message::BRIDGE_LISTEN_FAILED)
            }
            Self::NonLoopback { address } => write!(
                formatter,
                "{}: {address}",
                message::BRIDGE_NON_LOOPBACK_LISTEN
            ),
            Self::Serve(source) => write!(formatter, "{}: {source}", message::BRIDGE_SERVE_FAILED),
        }
    }
}

impl Error for HostBridgeServerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InspectListener(source) | Self::Serve(source) => Some(source),
            Self::NonLoopback { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HostBridgeService;

    #[test]
    fn exposes_only_the_host_exec_tool() {
        let service = HostBridgeService::new();
        let names = service
            .tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect::<Vec<_>>();

        assert_eq!(names, ["host.exec"]);
    }
}
