//! Fail-closed validation rules for parsed profiles.

use garde::{Error, Report, Validate};

use crate::error::message;

use super::{AgentProfiles, NetworkMode, PROFILE_SCHEMA_VERSION, Profile};

/// Aggregate of all static validation failures found in one profile.
pub type ProfileValidationErrors = Report;

/// Applies static product and security rules to a parsed profile.
///
/// Host-dependent checks, such as whether a path exists or whether enough
/// memory is available, belong to runtime preflight rather than this function.
pub fn validate_profile(profile: &Profile) -> Result<(), ProfileValidationErrors> {
    profile.validate()
}

pub(super) fn validate_schema_version(value: &u32, _: &()) -> garde::Result {
    if *value == PROFILE_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(Error::new(format!(
            "{}: found {value}; expected {PROFILE_SCHEMA_VERSION}",
            message::UNSUPPORTED_SCHEMA_VERSION
        )))
    }
}

pub(super) fn validate_required_text(value: &str, _: &()) -> garde::Result {
    if value.trim().is_empty() {
        Err(Error::new(message::VALUE_MUST_NOT_BE_BLANK))
    } else {
        Ok(())
    }
}

pub(super) fn validate_network_mode(mode: &NetworkMode, _: &()) -> garde::Result {
    match mode {
        NetworkMode::Default => Ok(()),
        NetworkMode::Restricted => Err(Error::new(message::NETWORK_RESTRICTED_NOT_IMPLEMENTED)),
    }
}

pub(super) fn validate_agents_enabled(agents: &AgentProfiles, _: &()) -> garde::Result {
    if agents.codex.enabled || agents.claude.enabled {
        Ok(())
    } else {
        Err(Error::new(message::AT_LEAST_ONE_AGENT_REQUIRED))
    }
}
