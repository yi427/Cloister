//! Host-dependent checks and path resolution performed before runtime planning.

mod guest_proxy;
mod host_command;
mod workspace;

pub use guest_proxy::{
    APPLE_CONTAINER_HOST_NAME, GuestProxyResolutionError, ResolvedGuestProxy,
    detect_inherited_guest_proxy, resolve_guest_proxy,
};
pub use host_command::{
    HostCommandLookupError, HostExecutableCheckError, ResolvedHostExecutable,
    inspect_host_executable, resolve_host_command,
};
pub use workspace::{PreflightError, ResolvedLaunch, resolve_launch};
