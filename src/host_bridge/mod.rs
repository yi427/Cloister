//! Authenticated MCP bridge for explicit host shell access.

mod client;
mod command;
mod policy;
mod server;
mod token;
mod tools;

pub use client::{HostBridgeClientError, call_host_exec, call_host_list_commands};
pub use command::build_host_process;
pub use policy::{
    AllowedHostCommand, AuthorizedHostCommand, HOST_EXEC_DSL_VERSION, HostEnvironment,
    HostExecAuthorizationError, HostExecPolicy, HostExecPolicyBuildError, HostExecRequest,
};
pub use server::{HostBridgeServerError, serve};
pub use token::{BridgeToken, BridgeTokenError};
pub use tools::{HostCommandInfo, HostEnvironmentInfo, HostExecOutput, HostListCommandsOutput};
