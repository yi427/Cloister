//! Small MCP client used to exercise the host bridge from the CLI.

use std::{error::Error, fmt};

use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde::de::DeserializeOwned;

use crate::error::message;

use super::{BridgeToken, HostExecOutput, HostExecRequest, HostListCommandsOutput};

/// Calls structured `host.exec` through an authenticated MCP endpoint.
pub async fn call_host_exec(
    endpoint: &str,
    token: &BridgeToken,
    request: &HostExecRequest,
) -> Result<HostExecOutput, HostBridgeClientError> {
    let arguments = serde_json::to_value(request)
        .expect("HostExecRequest should serialize")
        .as_object()
        .cloned()
        .expect("host.exec arguments are an object");
    call_tool(endpoint, token, "host.exec", arguments).await
}

/// Calls read-only `host.list_commands` through an authenticated MCP endpoint.
pub async fn call_host_list_commands(
    endpoint: &str,
    token: &BridgeToken,
) -> Result<HostListCommandsOutput, HostBridgeClientError> {
    call_tool(
        endpoint,
        token,
        "host.list_commands",
        serde_json::Map::new(),
    )
    .await
}

async fn call_tool<T: DeserializeOwned>(
    endpoint: &str,
    token: &BridgeToken,
    tool: &str,
    arguments: serde_json::Map<String, serde_json::Value>,
) -> Result<T, HostBridgeClientError> {
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
    let result = client
        .call_tool(CallToolRequestParams::new(tool.to_owned()).with_arguments(arguments))
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
    Transport { detail: String },
    Tool { detail: String },
    InvalidResponse,
}

impl fmt::Display for HostBridgeClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport { detail } | Self::Tool { detail } => {
                write!(formatter, "{}: {detail}", message::BRIDGE_CLIENT_FAILED)
            }
            Self::InvalidResponse => formatter.write_str(message::BRIDGE_RESPONSE_INVALID),
        }
    }
}

impl Error for HostBridgeClientError {}
