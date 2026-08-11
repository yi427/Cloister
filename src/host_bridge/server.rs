//! Authenticated Streamable HTTP MCP server.

use std::{error::Error, fmt, io, path::PathBuf, sync::Arc};

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
    model::{MetaObject, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::{
        StreamableHttpServerConfig, StreamableHttpService,
        streamable_http_server::session::local::LocalSessionManager,
    },
};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::error::message;

use super::{
    AuditLogError, BridgeToken, HostExecPolicy, audit::AuditController,
    execution::ExecutionManager, tools,
};

const ALLOWED_MCP_HOSTS: [&str; 4] = ["localhost", "127.0.0.1", "::1", "host.container.internal"];
const REQUIRES_USER_INTERACTION: &str = "anthropic/requiresUserInteraction";

#[derive(Clone, Debug)]
struct HostBridgeService {
    policy: Arc<HostExecPolicy>,
    working_directory: Arc<PathBuf>,
    executions: Arc<ExecutionManager>,
    tool_router: ToolRouter<Self>,
}

impl HostBridgeService {
    fn new(
        policy: Arc<HostExecPolicy>,
        working_directory: Arc<PathBuf>,
        executions: Arc<ExecutionManager>,
    ) -> Self {
        let mut tool_router = Self::tool_router();
        let host_exec = tool_router
            .map
            .get_mut("host.exec")
            .expect("the host.exec route should be generated");
        let mut metadata = MetaObject::new();
        metadata.0.insert(
            REQUIRES_USER_INTERACTION.to_owned(),
            serde_json::Value::Bool(true),
        );
        host_exec.attr.meta = Some(metadata);

        Self {
            policy,
            working_directory,
            executions,
            tool_router,
        }
    }
}

#[tool_router(router = tool_router)]
impl HostBridgeService {
    #[tool(
        name = "host.list_commands",
        description = "List the fixed Host working directory and commands allowed by the immutable Cloister Profile policy",
        annotations(read_only_hint = true)
    )]
    async fn host_list_commands(&self) -> Json<super::HostListCommandsOutput> {
        Json(tools::host_list_commands(
            &self.policy,
            self.working_directory.as_path(),
            true,
        ))
    }

    #[tool(
        name = "host.exec",
        description = "Execute one Profile-allowed host command with a literal argument vector and no shell parsing"
    )]
    async fn host_exec(
        &self,
        Parameters(input): Parameters<super::HostExecRequest>,
    ) -> Result<Json<super::HostExecOutput>, String> {
        let result = tools::host_exec(
            &self.executions,
            &self.policy,
            &input,
            &self.working_directory,
        )
        .await;

        result.map(Json).map_err(|error| error.to_string())
    }

    #[tool(
        name = "host.exec_status",
        description = "Read Host Exec state and incremental retained output, optionally waiting for new output or a terminal state",
        annotations(read_only_hint = true)
    )]
    async fn host_exec_status(
        &self,
        Parameters(input): Parameters<super::HostExecStatusRequest>,
    ) -> Result<Json<super::HostExecStatusOutput>, String> {
        tools::host_exec_status(&self.executions, &input)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "host.exec_cancel",
        description = "Request cancellation of a running Host Exec execution and its process group"
    )]
    async fn host_exec_cancel(
        &self,
        Parameters(input): Parameters<super::HostExecCancelRequest>,
    ) -> Result<Json<super::HostExecCancelOutput>, String> {
        tools::host_exec_cancel(&self.executions, &input)
            .map(Json)
            .map_err(|error| error.to_string())
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for HostBridgeService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Call host.list_commands before host.exec. It reports the fixed Host working directory and immutable Profile-allowed commands; prefer workspace-relative paths. host.exec starts only an allowed executable and returns an execution ID. While it is running, call host.exec_status with its cursor and a bounded wait for new output or completion. Call host.exec_cancel when a running execution is no longer needed. Arguments are passed literally without a shell, and host processes still use the permissions of the macOS user running this bridge.",
        )
    }
}

/// Serves the authenticated MCP endpoint until cancellation.
pub async fn serve(
    listener: TcpListener,
    token: BridgeToken,
    policy: HostExecPolicy,
    context: HostBridgeContext,
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
    let configured_working_directory = context.working_directory;
    let working_directory =
        std::fs::canonicalize(&configured_working_directory).map_err(|source| {
            HostBridgeServerError::WorkingDirectory {
                path: configured_working_directory,
                source,
            }
        })?;
    if !working_directory.is_dir() {
        return Err(HostBridgeServerError::WorkingDirectoryNotDirectory {
            path: working_directory,
        });
    }
    if working_directory.parent().is_none() {
        return Err(HostBridgeServerError::WorkingDirectoryIsRoot {
            path: working_directory,
        });
    }

    let audit = AuditController::start(
        context.audit_log_path,
        context.profile_name,
        context.agent_name,
        working_directory.clone(),
    )
    .map_err(HostBridgeServerError::Audit)?;
    let policy = Arc::new(policy);
    let working_directory = Arc::new(working_directory);
    let executions = ExecutionManager::new(audit.log());
    let service_executions = Arc::clone(&executions);
    let service: StreamableHttpService<HostBridgeService, LocalSessionManager> =
        StreamableHttpService::new(
            move || {
                Ok(HostBridgeService::new(
                    Arc::clone(&policy),
                    Arc::clone(&working_directory),
                    Arc::clone(&service_executions),
                ))
            },
            Default::default(),
            StreamableHttpServerConfig::default()
                .with_allowed_hosts(ALLOWED_MCP_HOSTS)
                .with_json_response(true)
                .with_sse_keep_alive(None)
                .with_cancellation_token(cancellation.child_token()),
        );
    let router = Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(token, authorize));

    let result = axum::serve(listener, router)
        .with_graceful_shutdown(cancellation.cancelled_owned())
        .await;
    executions.shutdown().await;
    let audit_result = audit.shutdown().await;
    result.map_err(HostBridgeServerError::Serve)?;
    audit_result.map_err(HostBridgeServerError::Audit)
}

/// Immutable host-side metadata and audit destination for one bridge lifecycle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostBridgeContext {
    profile_name: String,
    agent_name: String,
    working_directory: PathBuf,
    audit_log_path: PathBuf,
}

impl HostBridgeContext {
    pub fn new(
        profile_name: impl Into<String>,
        agent_name: impl Into<String>,
        working_directory: impl Into<PathBuf>,
        audit_log_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            profile_name: profile_name.into(),
            agent_name: agent_name.into(),
            working_directory: working_directory.into(),
            audit_log_path: audit_log_path.into(),
        }
    }

    pub fn audit_log_path(&self) -> &std::path::Path {
        &self.audit_log_path
    }
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
    Audit(AuditLogError),
    InspectListener(io::Error),
    NonLoopback { address: std::net::SocketAddr },
    WorkingDirectory { path: PathBuf, source: io::Error },
    WorkingDirectoryNotDirectory { path: PathBuf },
    WorkingDirectoryIsRoot { path: PathBuf },
    Serve(io::Error),
}

impl fmt::Display for HostBridgeServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Audit(source) => source.fmt(formatter),
            Self::InspectListener(source) => {
                write!(formatter, "{}: {source}", message::BRIDGE_LISTEN_FAILED)
            }
            Self::NonLoopback { address } => write!(
                formatter,
                "{}: {address}",
                message::BRIDGE_NON_LOOPBACK_LISTEN
            ),
            Self::WorkingDirectory { path, source } => write!(
                formatter,
                "failed to resolve Host Exec working directory '{}': {source}",
                path.display()
            ),
            Self::WorkingDirectoryNotDirectory { path } => write!(
                formatter,
                "Host Exec working directory is not a directory: '{}'",
                path.display()
            ),
            Self::WorkingDirectoryIsRoot { path } => write!(
                formatter,
                "Host Exec working directory must not be the filesystem root: '{}'",
                path.display()
            ),
            Self::Serve(source) => write!(formatter, "{}: {source}", message::BRIDGE_SERVE_FAILED),
        }
    }
}

impl Error for HostBridgeServerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Audit(source) => Some(source),
            Self::InspectListener(source)
            | Self::WorkingDirectory { source, .. }
            | Self::Serve(source) => Some(source),
            Self::NonLoopback { .. }
            | Self::WorkingDirectoryNotDirectory { .. }
            | Self::WorkingDirectoryIsRoot { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::{ExecutionManager, HostBridgeService, REQUIRES_USER_INTERACTION};
    use crate::host_bridge::{HostEnvironment, HostExecPolicy, audit::AuditController};

    #[tokio::test]
    async fn exposes_discovery_and_structured_execution_tools() {
        let directory = tempdir().expect("temporary directory should exist");
        let audit = AuditController::start(
            directory
                .path()
                .join("state/cloister/audit/host-exec.jsonl"),
            "test".to_owned(),
            "test-agent".to_owned(),
            directory.path().to_owned(),
        )
        .expect("test audit should start");
        let policy = HostExecPolicy::new([], HostEnvironment::new())
            .expect("empty test policy should build");
        let executions = ExecutionManager::new(audit.log());
        let service = HostBridgeService::new(
            Arc::new(policy),
            Arc::new(std::env::current_dir().expect("current directory should resolve")),
            Arc::clone(&executions),
        );
        let mut tools = service.tool_router.list_all();
        tools.sort_by(|left, right| left.name.cmp(&right.name));
        let names = tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            [
                "host.exec",
                "host.exec_cancel",
                "host.exec_status",
                "host.list_commands"
            ]
        );
        let host_exec = tools
            .iter()
            .find(|tool| tool.name == "host.exec")
            .expect("host.exec should be present");
        assert_eq!(
            host_exec
                .meta
                .as_ref()
                .and_then(|metadata| metadata.0.get(REQUIRES_USER_INTERACTION)),
            Some(&serde_json::Value::Bool(true))
        );
        let host_exec_status = tools
            .iter()
            .find(|tool| tool.name == "host.exec_status")
            .expect("host.exec_status should be present");
        let status_properties = host_exec_status
            .input_schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .expect("host.exec_status should expose object properties");
        assert!(status_properties.contains_key("wait_ms"));
        assert!(
            host_exec_status
                .input_schema
                .get("required")
                .and_then(serde_json::Value::as_array)
                .is_none_or(|required| required.iter().all(|name| name != "wait_ms"))
        );
        for tool in tools
            .iter()
            .filter(|tool| tool.name.as_ref() != "host.exec")
        {
            assert!(
                tool.meta
                    .as_ref()
                    .and_then(|metadata| metadata.0.get(REQUIRES_USER_INTERACTION))
                    .is_none(),
                "{} should not request a second approval",
                tool.name
            );
        }
        drop(service);
        executions.shutdown().await;
        audit.shutdown().await.expect("test audit should stop");
    }
}
