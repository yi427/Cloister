//! Host-dependent checks and path resolution performed before runtime planning.

mod workspace;

pub use workspace::{PreflightError, ResolvedProfile, resolve_profile_workspace};
