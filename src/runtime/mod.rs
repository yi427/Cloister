//! Runtime-neutral plans and Apple container command construction.

mod apple_container;
mod executor;
mod plan;

pub use apple_container::{
    APPLE_CONTAINER_PROGRAM, HOST_BRIDGE_GUEST_NAME, HOST_BRIDGE_LOCALHOST_ADDRESS,
    HostBridgeLaunch, RuntimePlanError, dns_create_command, dns_list_command,
    image_inspect_command, image_pull_command, plan_codex_container, system_start_command,
    system_status_command,
};
pub use executor::{RuntimeExecutionError, execute, execute_output};
pub use plan::{CodexStateMount, CommandSpec, NetworkExposure, RuntimePlan, WorkspaceMount};
