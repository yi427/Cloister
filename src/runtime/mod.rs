//! Runtime-neutral plans and Apple container command construction.

mod apple_container;
mod executor;
mod plan;

pub use apple_container::{RuntimePlanError, plan_apple_container};
pub use executor::{RuntimeExecutionError, execute};
pub use plan::{
    AgentKind, CommandSpec, NetworkExposure, RuntimePlan, WorkspaceMount, WorkspaceMountAccess,
};
