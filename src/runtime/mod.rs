//! Runtime-neutral plans and Apple container command construction.

mod apple_container;
mod executor;
mod packet_filter;
mod plan;

pub use apple_container::{
    APPLE_CONTAINER_PROGRAM, HOST_BRIDGE_GUEST_NAME, HOST_BRIDGE_LOCALHOST_ADDRESS,
    HostBridgeLaunch, RuntimePlanError, agent_install_command, delete_container_command,
    dns_create_command, dns_list_command, host_connectivity_command, image_inspect_command,
    image_pull_command, name_container, plan_agent_container, system_start_command,
    system_status_command,
};
pub use executor::{RuntimeExecutionError, execute, execute_managed, execute_output};
pub use packet_filter::{HOST_PF_ANCHOR, pf_configuration_command};
pub use plan::{
    AgentStateMount, CommandSpec, GuestProxyPlan, HostCommandPlan, NetworkExposure, RuntimePlan,
    WorkspaceMount,
};
