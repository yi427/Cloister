//! Side-effect-free profile parsing.

use std::{error::Error, fmt, ops::Range};

use serde::Deserialize;

use crate::error::message;

use super::{PROFILE_SCHEMA_VERSION, Profile};

#[derive(Deserialize)]
struct ProfileHeader {
    schema_version: toml::Spanned<u32>,
}

/// Parses Profile V7 (allowlist only) or V8 (explicit Host Exec mode).
///
/// This function checks TOML syntax and the shape of the strongly typed model.
/// Environment-dependent and security-policy checks belong to validation.
pub fn parse_profile(input: &str) -> Result<Profile, ParseProfileError> {
    let header: ProfileHeader = toml::from_str(input).map_err(ParseProfileError::from)?;
    let found = *header.schema_version.get_ref();
    if found != 7 && found != PROFILE_SCHEMA_VERSION {
        return Err(ParseProfileError::UnsupportedSchemaVersion {
            found,
            expected: PROFILE_SCHEMA_VERSION,
            span: header.schema_version.span(),
        });
    }

    let profile = toml::from_str(input).map_err(ParseProfileError::from)?;
    if found == PROFILE_SCHEMA_VERSION {
        let document: toml::Value = toml::from_str(input).map_err(ParseProfileError::from)?;
        if document
            .get("host")
            .and_then(|host| host.get("exec"))
            .and_then(|exec| exec.get("mode"))
            .is_none()
        {
            return Err(ParseProfileError::MissingHostExecMode {
                span: header.schema_version.span(),
            });
        }
    }
    Ok(profile)
}

/// TOML syntax or model-shape error produced while parsing a profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParseProfileError {
    Toml(toml::de::Error),
    MissingHostExecMode {
        span: Range<usize>,
    },
    UnsupportedSchemaVersion {
        found: u32,
        expected: u32,
        span: Range<usize>,
    },
}

impl ParseProfileError {
    /// Human-readable reason without rendering the complete source document.
    pub fn message(&self) -> &str {
        match self {
            Self::Toml(source) => source.message(),
            Self::MissingHostExecMode { .. } => "Profile V8 requires explicit host.exec.mode",
            Self::UnsupportedSchemaVersion { .. } => message::UNSUPPORTED_SCHEMA_VERSION,
        }
    }

    /// Byte range in the original TOML document associated with the error.
    pub fn span(&self) -> Option<Range<usize>> {
        match self {
            Self::Toml(source) => source.span(),
            Self::MissingHostExecMode { span } => Some(span.clone()),
            Self::UnsupportedSchemaVersion { span, .. } => Some(span.clone()),
        }
    }

    pub(super) fn fmt_diagnostic(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml(source) => write!(formatter, "{source}"),
            Self::MissingHostExecMode { .. } => formatter.write_str(self.message()),
            Self::UnsupportedSchemaVersion {
                found, expected, ..
            } => write!(
                formatter,
                "{}: found {found}; expected 7 or {expected}",
                message::UNSUPPORTED_SCHEMA_VERSION
            ),
        }
    }
}

impl From<toml::de::Error> for ParseProfileError {
    fn from(source: toml::de::Error) -> Self {
        Self::Toml(source)
    }
}

impl fmt::Display for ParseProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: ", message::PROFILE_PARSE_FAILED)?;
        self.fmt_diagnostic(formatter)
    }
}

impl Error for ParseProfileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Toml(source) => Some(source),
            Self::UnsupportedSchemaVersion { .. } | Self::MissingHostExecMode { .. } => None,
        }
    }
}
