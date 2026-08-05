//! Side-effect-free profile parsing.

use std::{error::Error, fmt, ops::Range};

use serde::Deserialize;

use crate::error::message;

use super::{PROFILE_SCHEMA_VERSION, Profile};

#[derive(Deserialize)]
struct ProfileHeader {
    schema_version: toml::Spanned<u32>,
}

/// Parses a TOML document into the Profile V5 data model.
///
/// This function checks TOML syntax and the shape of the strongly typed model.
/// Environment-dependent and security-policy checks belong to validation.
pub fn parse_profile(input: &str) -> Result<Profile, ParseProfileError> {
    let header: ProfileHeader = toml::from_str(input).map_err(ParseProfileError::from)?;
    let found = *header.schema_version.get_ref();
    if found != PROFILE_SCHEMA_VERSION {
        return Err(ParseProfileError::UnsupportedSchemaVersion {
            found,
            expected: PROFILE_SCHEMA_VERSION,
            span: header.schema_version.span(),
        });
    }

    toml::from_str(input).map_err(ParseProfileError::from)
}

/// TOML syntax or model-shape error produced while parsing a profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParseProfileError {
    Toml(toml::de::Error),
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
            Self::UnsupportedSchemaVersion { .. } => message::UNSUPPORTED_SCHEMA_VERSION,
        }
    }

    /// Byte range in the original TOML document associated with the error.
    pub fn span(&self) -> Option<Range<usize>> {
        match self {
            Self::Toml(source) => source.span(),
            Self::UnsupportedSchemaVersion { span, .. } => Some(span.clone()),
        }
    }

    pub(super) fn fmt_diagnostic(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml(source) => write!(formatter, "{source}"),
            Self::UnsupportedSchemaVersion {
                found, expected, ..
            } => write!(
                formatter,
                "{}: found {found}; expected {expected}",
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
            Self::UnsupportedSchemaVersion { .. } => None,
        }
    }
}
