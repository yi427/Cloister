//! Runtime-neutral plans and Apple container command construction.

mod apple_container;
mod executor;
mod plan;

pub use apple_container::{RuntimePlanError, plan_apple_container, plan_codex_container};
pub use executor::{RuntimeExecutionError, execute};
pub use plan::{
    AgentKind, AgentStateMount, CommandSpec, NetworkExposure, RuntimePlan, WorkspaceMount,
    WorkspaceMountAccess,
};
