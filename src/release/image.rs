//! Classification of Profile image references against the running CLI release.

use std::{error::Error, fmt};

use semver::Version;

pub(crate) const OFFICIAL_IMAGE_REPOSITORY: &str = "ghcr.io/yi427/cloister";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ImageCompatibility {
    PairedRelease { version: Version },
    ImmutableTesting { revision: String },
    Custom,
}

impl ImageCompatibility {
    pub(crate) fn detail(&self, reference: &str) -> String {
        match self {
            Self::PairedRelease { version } => {
                format!("CLI {version} matches official image {version}")
            }
            Self::ImmutableTesting { revision } => format!(
                "immutable testing image '{reference}' at revision {revision}; release compatibility is not guaranteed"
            ),
            Self::Custom => {
                format!("custom image '{reference}'; release compatibility cannot be verified")
            }
        }
    }

    pub(crate) const fn is_warning(&self) -> bool {
        !matches!(self, Self::PairedRelease { .. })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ImageCompatibilityError {
    OfficialReleaseMismatch { expected: Version, found: Version },
    MutableOfficialTag { tag: String },
    UnsupportedOfficialReference { reference: String },
}

impl fmt::Display for ImageCompatibilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OfficialReleaseMismatch { expected, found } if found < expected => write!(
                formatter,
                "official image version mismatch: CLI {expected} requires '{}', found official image {found}; run 'cloister profile upgrade --dry-run'",
                official_release_reference(expected)
            ),
            Self::OfficialReleaseMismatch { expected, found } => write!(
                formatter,
                "official image version mismatch: CLI {expected} requires '{}', found newer official image {found}; install the matching CLI before using this Profile",
                official_release_reference(expected)
            ),
            Self::MutableOfficialTag { tag } => write!(
                formatter,
                "official image tag '{tag}' is mutable and cannot be used by a Profile; use an exact X.Y.Z release or immutable sha-<full-commit> testing image"
            ),
            Self::UnsupportedOfficialReference { reference } => write!(
                formatter,
                "unsupported official image reference '{reference}'; use an exact X.Y.Z release or immutable sha-<full-commit> testing image"
            ),
        }
    }
}

impl Error for ImageCompatibilityError {}

pub(crate) fn current_cli_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION"))
        .expect("the Cargo package version must be valid semantic versioning")
}

pub(crate) fn official_release_reference(version: &Version) -> String {
    format!("{OFFICIAL_IMAGE_REPOSITORY}:{version}")
}

pub(crate) fn classify_image(
    reference: &str,
) -> Result<ImageCompatibility, ImageCompatibilityError> {
    classify_image_for(reference, &current_cli_version())
}

pub(crate) fn classify_image_for(
    reference: &str,
    expected: &Version,
) -> Result<ImageCompatibility, ImageCompatibilityError> {
    let Some(suffix) = reference.strip_prefix(OFFICIAL_IMAGE_REPOSITORY) else {
        return Ok(ImageCompatibility::Custom);
    };
    if !suffix.is_empty() && !suffix.starts_with(':') && !suffix.starts_with('@') {
        return Ok(ImageCompatibility::Custom);
    }
    let Some(tag) = suffix.strip_prefix(':').filter(|tag| !tag.is_empty()) else {
        return Err(ImageCompatibilityError::UnsupportedOfficialReference {
            reference: reference.to_owned(),
        });
    };

    if matches!(tag, "main" | "latest") || is_minor_channel(tag) {
        return Err(ImageCompatibilityError::MutableOfficialTag {
            tag: tag.to_owned(),
        });
    }
    if let Some(revision) = immutable_revision(tag) {
        return Ok(ImageCompatibility::ImmutableTesting {
            revision: revision.to_owned(),
        });
    }
    if let Ok(found) = Version::parse(tag)
        && found.build.is_empty()
    {
        return if &found == expected {
            Ok(ImageCompatibility::PairedRelease { version: found })
        } else {
            Err(ImageCompatibilityError::OfficialReleaseMismatch {
                expected: expected.clone(),
                found,
            })
        };
    }

    Err(ImageCompatibilityError::UnsupportedOfficialReference {
        reference: reference.to_owned(),
    })
}

fn is_minor_channel(tag: &str) -> bool {
    let mut components = tag.split('.');
    matches!(
        (components.next(), components.next(), components.next()),
        (Some(major), Some(minor), None)
            if !major.is_empty()
                && !minor.is_empty()
                && major.bytes().all(|byte| byte.is_ascii_digit())
                && minor.bytes().all(|byte| byte.is_ascii_digit())
    )
}

fn immutable_revision(tag: &str) -> Option<&str> {
    tag.strip_prefix("sha-").filter(|revision| {
        revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(value: &str) -> Version {
        Version::parse(value).expect("test version should parse")
    }

    #[test]
    fn pairs_only_the_exact_official_release() {
        let expected = version("0.2.0");

        assert_eq!(
            classify_image_for("ghcr.io/yi427/cloister:0.2.0", &expected),
            Ok(ImageCompatibility::PairedRelease {
                version: expected.clone()
            })
        );
        assert_eq!(
            classify_image_for("ghcr.io/yi427/cloister:0.1.0", &expected),
            Err(ImageCompatibilityError::OfficialReleaseMismatch {
                expected: expected.clone(),
                found: version("0.1.0")
            })
        );
        assert_eq!(
            classify_image_for("ghcr.io/yi427/cloister:0.3.0", &expected),
            Err(ImageCompatibilityError::OfficialReleaseMismatch {
                expected,
                found: version("0.3.0")
            })
        );
    }

    #[test]
    fn accepts_only_a_full_commit_tag_as_an_official_testing_image() {
        let expected = version("0.2.0");
        let revision = "0123456789abcdef0123456789abcdef01234567";

        assert_eq!(
            classify_image_for(&format!("ghcr.io/yi427/cloister:sha-{revision}"), &expected),
            Ok(ImageCompatibility::ImmutableTesting {
                revision: revision.to_owned()
            })
        );
        assert!(matches!(
            classify_image_for("ghcr.io/yi427/cloister:sha-0123456", &expected),
            Err(ImageCompatibilityError::UnsupportedOfficialReference { .. })
        ));
    }

    #[test]
    fn rejects_moving_or_unknown_official_references() {
        let expected = version("0.2.0");

        for tag in ["main", "latest", "0.2"] {
            assert_eq!(
                classify_image_for(&format!("ghcr.io/yi427/cloister:{tag}"), &expected),
                Err(ImageCompatibilityError::MutableOfficialTag {
                    tag: tag.to_owned()
                })
            );
        }
        for reference in [
            "ghcr.io/yi427/cloister",
            "ghcr.io/yi427/cloister@sha256:0123",
            "ghcr.io/yi427/cloister:nightly",
        ] {
            assert!(matches!(
                classify_image_for(reference, &expected),
                Err(ImageCompatibilityError::UnsupportedOfficialReference { .. })
            ));
        }
    }

    #[test]
    fn preserves_explicit_local_and_custom_image_support() {
        let expected = version("0.2.0");

        for reference in [
            "cloister:dev",
            "registry.example.test/team/cloister:stable",
            "ghcr.io/yi427/cloister-fork:0.2.0",
        ] {
            assert_eq!(
                classify_image_for(reference, &expected),
                Ok(ImageCompatibility::Custom)
            );
        }
    }
}
