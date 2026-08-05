//! Profile definitions, parsing, and validation.

mod loader;
mod model;
mod parser;
mod validation;

pub use loader::{LoadProfileError, load_profile};
pub use model::{
    AgentProfile, AgentState, Architecture, CpuCount, GuestProfile, HostExecAllowProfile,
    HostExecArguments, HostExecEnvironmentMode, HostExecEnvironmentProfile, HostExecProfile,
    HostProfile, ImageProfile, MemorySize, NetworkMode, NetworkProfile, PROFILE_SCHEMA_VERSION,
    ParseMemorySizeError, Profile,
};
pub use parser::{ParseProfileError, parse_profile};
pub use validation::{ProfileValidationErrors, validate_profile};
