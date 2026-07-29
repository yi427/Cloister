//! Runtime-neutral plans and Apple container command construction.

mod apple_container;
mod executor;
mod plan;

pub use apple_container::{RuntimePlanError, plan_codex_container};
pub use executor::{RuntimeExecutionError, execute};
pub use plan::{CodexStateMount, CommandSpec, NetworkExposure, RuntimePlan, WorkspaceMount};
