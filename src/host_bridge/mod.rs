//! Authenticated MCP bridge for explicit host shell access.

mod client;
mod server;
mod token;
mod tools;

pub use client::{HostBridgeClientError, call_host_exec};
pub use server::{HostBridgeServerError, serve};
pub use token::{BridgeToken, BridgeTokenError};
pub use tools::{HostExecInput, HostExecOutput};
