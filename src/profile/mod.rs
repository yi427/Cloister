//! Profile definitions, parsing, and validation.

mod loader;
mod model;
mod parser;
mod validation;

pub use loader::{LoadProfileError, load_profile};
pub use model::{
    AgentProfile, AgentState, AgentVersion, Architecture, CpuCount, GuestProfile,
    HostExecAllowProfile, HostExecArguments, HostExecEnvironmentMode, HostExecEnvironmentProfile,
    HostExecMode, HostExecProfile, HostProfile, ImageProfile, MemorySize, NetworkMode,
    NetworkProfile, NetworkProxyMode, PROFILE_SCHEMA_VERSION, ParseMemorySizeError, Profile,
};
pub use parser::{ParseProfileError, parse_profile};
pub use validation::{ProfileValidationErrors, validate_profile};
