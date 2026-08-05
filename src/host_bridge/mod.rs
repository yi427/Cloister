//! Authenticated MCP bridge for explicit host shell access.

mod audit;
mod client;
mod command;
mod execution;
mod policy;
mod server;
mod token;
mod tools;

pub use audit::{
    AUDIT_SCHEMA_VERSION, AUDIT_SEGMENT_BYTES, AUDIT_TOTAL_BYTES, AuditLogError,
    AuditLogInspection, AuditLogPathError, default_audit_log_path, inspect_audit_log_path,
};
pub use client::{
    HostBridgeClientError, call_host_exec, call_host_exec_cancel, call_host_exec_status,
    call_host_list_commands,
};
pub use command::build_host_process;
pub use execution::{
    HostExecCancelOutput, HostExecCancelRequest, HostExecOutput, HostExecStatusOutput,
    HostExecStatusRequest, HostExecutionState, HostOutputChunk, HostOutputStream,
};
pub use policy::{
    AllowedHostCommand, AuthorizedHostCommand, HOST_EXEC_DSL_VERSION, HostEnvironment,
    HostExecAuthorizationError, HostExecPolicy, HostExecPolicyBuildError, HostExecRequest,
};
pub use server::{HostBridgeContext, HostBridgeServerError, serve};
pub use token::{BridgeToken, BridgeTokenError};
pub use tools::{HostCommandInfo, HostEnvironmentInfo, HostListCommandsOutput};
