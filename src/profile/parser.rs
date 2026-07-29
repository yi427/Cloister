//! Side-effect-free profile parsing.

use std::{error::Error, fmt, ops::Range};

use crate::error::message;

use super::Profile;

/// Parses a TOML document into the Profile V1 data model.
///
/// This function checks TOML syntax and the shape of the strongly typed model.
/// Environment-dependent and security-policy checks belong to validation.
pub fn parse_profile(input: &str) -> Result<Profile, ParseProfileError> {
    toml::from_str(input).map_err(ParseProfileError::from)
}

/// TOML syntax or model-shape error produced while parsing a profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseProfileError {
    source: toml::de::Error,
}

impl ParseProfileError {
    /// Human-readable reason without rendering the complete source document.
    pub fn message(&self) -> &str {
        self.source.message()
    }

    /// Byte range in the original TOML document associated with the error.
    pub fn span(&self) -> Option<Range<usize>> {
        self.source.span()
    }

    pub(super) fn diagnostic(&self) -> &toml::de::Error {
        &self.source
    }
}

impl From<toml::de::Error> for ParseProfileError {
    fn from(source: toml::de::Error) -> Self {
        Self { source }
    }
}

impl fmt::Display for ParseProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: {}",
            message::PROFILE_PARSE_FAILED,
            self.source
        )
    }
}

impl Error for ParseProfileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}
