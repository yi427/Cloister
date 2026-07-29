//! Host-dependent checks and path resolution performed before runtime planning.

mod workspace;

pub use workspace::{PreflightError, ResolvedLaunch, resolve_launch};
