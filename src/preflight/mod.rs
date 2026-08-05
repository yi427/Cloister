//! Host-dependent checks and path resolution performed before runtime planning.

mod host_command;
mod workspace;

pub use host_command::{
    HostCommandLookupError, HostExecutableCheckError, ResolvedHostExecutable,
    inspect_host_executable, resolve_host_command,
};
pub use workspace::{PreflightError, ResolvedLaunch, resolve_launch};
