//! Fail-closed validation rules for parsed profiles.

use std::{collections::BTreeSet, path::Path};

use garde::{Error, Path as ValidationPath, Report, Validate};

use crate::error::message;

use super::{PROFILE_SCHEMA_VERSION, Profile};

/// Aggregate of all static validation failures found in one profile.
pub type ProfileValidationErrors = Report;

/// Applies static product and security rules to a parsed profile.
///
/// Host-dependent checks, such as whether a path exists or whether enough
/// memory is available, belong to runtime preflight rather than this function.
pub fn validate_profile(profile: &Profile) -> Result<(), ProfileValidationErrors> {
    let mut report = match profile.validate() {
        Ok(()) => Report::new(),
        Err(report) => report,
    };

    let mut names = BTreeSet::new();
    for command in &profile.host.exec.allow {
        if !names.insert(command.name.as_str()) {
            report.append(
                ValidationPath::new("host").join("exec").join("allow"),
                Error::new(message::HOST_COMMAND_NAME_DUPLICATED),
            );
        }
    }

    if report.is_empty() {
        Ok(())
    } else {
        Err(report)
    }
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

pub(super) fn validate_absolute_executable(value: &Path, _: &()) -> garde::Result {
    if value.is_absolute() {
        Ok(())
    } else {
        Err(Error::new(message::HOST_EXECUTABLE_MUST_BE_ABSOLUTE))
    }
}
