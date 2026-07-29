//! Profile definitions, parsing, and validation.

mod loader;
mod model;
mod parser;
mod validation;

pub use loader::{LoadProfileError, load_profile};
pub use model::{
    AgentState, Architecture, CodexProfile, CpuCount, GuestProfile, ImageProfile, MemorySize,
    NetworkMode, NetworkProfile, PROFILE_SCHEMA_VERSION, ParseMemorySizeError, ParseProxyUrlError,
    Profile, ProxyUrl,
};
pub use parser::{ParseProfileError, parse_profile};
pub use validation::{ProfileValidationErrors, validate_profile};
