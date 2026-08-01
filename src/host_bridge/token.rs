//! Creation and validation of per-environment bearer tokens.

use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use subtle::ConstantTimeEq;

use crate::error::message;

const TOKEN_BYTES: usize = 32;

/// A validated bearer token whose secret is redacted from debug output.
#[derive(Clone)]
pub struct BridgeToken {
    encoded: Arc<str>,
}

impl BridgeToken {
    /// Generates an in-memory token for one bridge lifecycle.
    pub fn generate() -> Result<Self, BridgeTokenError> {
        let mut bytes = [0_u8; TOKEN_BYTES];
        getrandom::fill(&mut bytes).map_err(|source| BridgeTokenError::Random {
            detail: source.to_string(),
        })?;

        Ok(Self {
            encoded: URL_SAFE_NO_PAD.encode(bytes).into(),
        })
    }

    /// Loads an existing token file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, BridgeTokenError> {
        let path = path.as_ref();
        validate_file_metadata(path)?;
        let encoded = fs::read_to_string(path).map_err(|source| BridgeTokenError::Read {
            path: path.to_owned(),
            source,
        })?;

        Self::parse(encoded.trim()).map_err(|_| BridgeTokenError::Invalid {
            path: path.to_owned(),
        })
    }

    /// Loads an existing token or creates a new owner-only token file.
    pub fn load_or_create(path: impl AsRef<Path>) -> Result<Self, BridgeTokenError> {
        let path = path.as_ref();

        match fs::symlink_metadata(path) {
            Ok(_) => return Self::load(path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(BridgeTokenError::Read {
                    path: path.to_owned(),
                    source,
                });
            }
        }

        let token = Self::generate()?;

        match create_token_file(path, token.encoded.as_bytes()) {
            Ok(()) => Ok(token),
            Err(BridgeTokenError::Create { source, .. })
                if source.kind() == io::ErrorKind::AlreadyExists =>
            {
                Self::load(path)
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn secret(&self) -> &str {
        &self.encoded
    }

    pub(crate) fn matches_bearer(&self, candidate: &str) -> bool {
        candidate.len() == self.encoded.len()
            && bool::from(candidate.as_bytes().ct_eq(self.encoded.as_bytes()))
    }

    fn parse(encoded: &str) -> Result<Self, ()> {
        let decoded = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| ())?;
        if decoded.len() != TOKEN_BYTES || URL_SAFE_NO_PAD.encode(&decoded) != encoded {
            return Err(());
        }

        Ok(Self {
            encoded: encoded.to_owned().into(),
        })
    }
}

impl fmt::Debug for BridgeToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BridgeToken")
            .field("encoded", &"[REDACTED]")
            .finish()
    }
}

fn validate_file_metadata(path: &Path) -> Result<(), BridgeTokenError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| BridgeTokenError::Read {
        path: path.to_owned(),
        source,
    })?;

    if !metadata.file_type().is_file() || !owner_only(&metadata) {
        return Err(BridgeTokenError::Insecure {
            path: path.to_owned(),
        });
    }

    Ok(())
}

#[cfg(unix)]
fn owner_only(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o077 == 0
}

#[cfg(not(unix))]
fn owner_only(_: &fs::Metadata) -> bool {
    true
}

#[cfg(unix)]
fn create_token_file(path: &Path, token: &[u8]) -> Result<(), BridgeTokenError> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|source| BridgeTokenError::Create {
            path: path.to_owned(),
            source,
        })?;

    file.write_all(token)
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(|source| BridgeTokenError::Write {
            path: path.to_owned(),
            source,
        })
}

#[cfg(not(unix))]
fn create_token_file(path: &Path, token: &[u8]) -> Result<(), BridgeTokenError> {
    use std::io::Write;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| BridgeTokenError::Create {
            path: path.to_owned(),
            source,
        })?;
    file.write_all(token)
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(|source| BridgeTokenError::Write {
            path: path.to_owned(),
            source,
        })
}

/// Failure while reading or creating an ephemeral bridge token.
#[derive(Debug)]
pub enum BridgeTokenError {
    Read { path: PathBuf, source: io::Error },
    Create { path: PathBuf, source: io::Error },
    Write { path: PathBuf, source: io::Error },
    Invalid { path: PathBuf },
    Insecure { path: PathBuf },
    Random { detail: String },
}

impl fmt::Display for BridgeTokenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => write!(
                formatter,
                "{} '{}': {source}",
                message::BRIDGE_TOKEN_READ_FAILED,
                path.display()
            ),
            Self::Create { path, source } => write!(
                formatter,
                "{} '{}': {source}",
                message::BRIDGE_TOKEN_CREATE_FAILED,
                path.display()
            ),
            Self::Write { path, source } => write!(
                formatter,
                "{} '{}': {source}",
                message::BRIDGE_TOKEN_WRITE_FAILED,
                path.display()
            ),
            Self::Invalid { path } => {
                write!(
                    formatter,
                    "{}: '{}'",
                    message::BRIDGE_TOKEN_INVALID,
                    path.display()
                )
            }
            Self::Insecure { path } => write!(
                formatter,
                "{}: '{}'",
                message::BRIDGE_TOKEN_INSECURE,
                path.display()
            ),
            Self::Random { detail } => {
                write!(
                    formatter,
                    "{}: {detail}",
                    message::BRIDGE_TOKEN_CREATE_FAILED
                )
            }
        }
    }
}

impl Error for BridgeTokenError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source, .. }
            | Self::Create { source, .. }
            | Self::Write { source, .. } => Some(source),
            Self::Invalid { .. } | Self::Insecure { .. } | Self::Random { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{BridgeToken, BridgeTokenError};

    #[test]
    fn generates_an_in_memory_token_without_exposing_it_through_debug() {
        let token = BridgeToken::generate().expect("token should be generated");

        assert!(!token.secret().is_empty());
        assert!(!format!("{token:?}").contains(token.secret()));
    }

    #[test]
    fn creates_and_loads_an_owner_only_token() {
        let directory = tempdir().expect("temporary directory should exist");
        let path = directory.path().join("bridge.token");

        let created = BridgeToken::load_or_create(&path).expect("token should be created");
        let loaded = BridgeToken::load(&path).expect("token should load");

        assert_eq!(created.secret(), loaded.secret());
        assert!(!created.secret().is_empty());
        assert!(!format!("{created:?}").contains(created.secret()));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            assert_eq!(
                fs::metadata(&path)
                    .expect("token metadata should exist")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_token_readable_by_other_users() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().expect("temporary directory should exist");
        let path = directory.path().join("bridge.token");
        let token = BridgeToken::load_or_create(&path).expect("token should be created");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
            .expect("permissions should change");

        let error = BridgeToken::load(&path).expect_err("insecure token should fail");

        assert!(matches!(error, BridgeTokenError::Insecure { .. }));
        assert!(!error.to_string().contains(token.secret()));
    }

    #[test]
    fn rejects_malformed_token_content() {
        let directory = tempdir().expect("temporary directory should exist");
        let path = directory.path().join("bridge.token");
        fs::write(&path, "not-a-token\n").expect("fixture token should be written");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .expect("permissions should change");
        }

        let error = BridgeToken::load(&path).expect_err("malformed token should fail");

        assert!(matches!(error, BridgeTokenError::Invalid { .. }));
    }
}
