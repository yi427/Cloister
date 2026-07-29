//! Small MCP client used to exercise the host bridge from the CLI.

use std::{error::Error, fmt, time::Duration};

use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};

use crate::error::message;

use super::{BridgeToken, HostExecOutput};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Calls `host.exec` through an authenticated MCP endpoint.
pub async fn call_host_exec(
    endpoint: &str,
    token: &BridgeToken,
    command: &str,
) -> Result<HostExecOutput, HostBridgeClientError> {
    tokio::time::timeout(REQUEST_TIMEOUT, call(endpoint, token, command))
        .await
        .map_err(|_| HostBridgeClientError::Timeout)?
}

async fn call(
    endpoint: &str,
    token: &BridgeToken,
    command: &str,
) -> Result<HostExecOutput, HostBridgeClientError> {
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint.to_owned())
            .auth_header(token.secret().to_owned()),
    );
    let client =
        ().serve(transport)
            .await
            .map_err(|error| HostBridgeClientError::Transport {
                detail: error.to_string(),
            })?;
    let arguments = serde_json::json!({ "command": command })
        .as_object()
        .cloned()
        .expect("host.exec arguments are an object");
    let result = client
        .call_tool(CallToolRequestParams::new("host.exec").with_arguments(arguments))
        .await
        .map_err(|error| HostBridgeClientError::Transport {
            detail: error.to_string(),
        })?;
    let _ = client.cancel().await;

    if result.is_error == Some(true) {
        let detail = result
            .content
            .iter()
            .filter_map(|content| content.as_text())
            .map(|content| content.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        return Err(HostBridgeClientError::Tool { detail });
    }

    let structured = result
        .structured_content
        .ok_or(HostBridgeClientError::InvalidResponse)?;
    serde_json::from_value(structured).map_err(|_| HostBridgeClientError::InvalidResponse)
}

/// Failure while connecting to or invoking the host bridge.
#[derive(Debug)]
pub enum HostBridgeClientError {
    Timeout,
    Transport { detail: String },
    Tool { detail: String },
    InvalidResponse,
}

impl fmt::Display for HostBridgeClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => formatter.write_str(message::BRIDGE_REQUEST_TIMED_OUT),
            Self::Transport { detail } | Self::Tool { detail } => {
                write!(formatter, "{}: {detail}", message::BRIDGE_CLIENT_FAILED)
            }
            Self::InvalidResponse => formatter.write_str(message::BRIDGE_RESPONSE_INVALID),
        }
    }
}

impl Error for HostBridgeClientError {}
