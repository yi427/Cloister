//! Data model for versioned Cloister profiles.

use std::{
    error::Error,
    fmt,
    num::{NonZeroU16, NonZeroU64},
    path::PathBuf,
    str::FromStr,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::error::message;

/// Schema version implemented by the current profile model.
pub const PROFILE_SCHEMA_VERSION: u32 = 1;

/// Complete configuration for one Cloister development environment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, garde::Validate)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    #[garde(custom(super::validation::validate_schema_version))]
    pub schema_version: u32,
    #[garde(custom(super::validation::validate_required_text))]
    pub name: String,
    #[garde(dive)]
    pub image: ImageProfile,
    #[garde(dive)]
    pub guest: GuestProfile,
    #[garde(dive)]
    pub workspace: WorkspaceProfile,
    #[garde(dive)]
    pub network: NetworkProfile,
    #[garde(custom(super::validation::validate_agents_enabled))]
    pub agents: AgentProfiles,
}

/// OCI image selection for the guest environment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, garde::Validate)]
#[serde(deny_unknown_fields)]
pub struct ImageProfile {
    #[garde(custom(super::validation::validate_required_text))]
    pub reference: String,
    #[garde(skip)]
    pub architecture: Architecture,
}

/// Guest architecture supported by Cloister.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Architecture {
    Arm64,
}

/// Resource and identity settings applied inside the guest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, garde::Validate)]
#[serde(deny_unknown_fields)]
pub struct GuestProfile {
    #[garde(custom(super::validation::validate_required_text))]
    pub hostname: String,
    #[garde(skip)]
    pub cpus: CpuCount,
    #[garde(skip)]
    pub memory: MemorySize,
    #[garde(custom(super::validation::validate_required_text))]
    pub user: String,
    #[garde(custom(super::validation::validate_required_text))]
    pub locale: String,
    #[garde(custom(super::validation::validate_required_text))]
    pub timezone: String,
}

/// Non-zero virtual CPU count allocated to the guest.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CpuCount(NonZeroU16);

impl CpuCount {
    pub const fn new(value: u16) -> Option<Self> {
        match NonZeroU16::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

/// Positive guest memory size, stored with MiB granularity.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MemorySize(NonZeroU64);

impl MemorySize {
    const MEBIBYTES_PER_GIBIBYTE: u64 = 1024;

    pub const fn from_mebibytes(mebibytes: u64) -> Option<Self> {
        match NonZeroU64::new(mebibytes) {
            Some(mebibytes) => Some(Self(mebibytes)),
            None => None,
        }
    }

    pub const fn as_mebibytes(self) -> u64 {
        self.0.get()
    }
}

impl FromStr for MemorySize {
    type Err = ParseMemorySizeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (amount, multiplier) = if let Some(amount) = value.strip_suffix('M') {
            (amount, 1)
        } else if let Some(amount) = value.strip_suffix('G') {
            (amount, Self::MEBIBYTES_PER_GIBIBYTE)
        } else {
            return Err(ParseMemorySizeError::InvalidFormat);
        };

        if amount.is_empty() || !amount.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ParseMemorySizeError::InvalidFormat);
        }

        let amount = amount
            .parse::<u64>()
            .map_err(|_| ParseMemorySizeError::Overflow)?;
        let mebibytes = amount
            .checked_mul(multiplier)
            .ok_or(ParseMemorySizeError::Overflow)?;

        Self::from_mebibytes(mebibytes).ok_or(ParseMemorySizeError::Zero)
    }
}

impl fmt::Display for MemorySize {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mebibytes = self.as_mebibytes();

        if mebibytes.is_multiple_of(Self::MEBIBYTES_PER_GIBIBYTE) {
            write!(formatter, "{}G", mebibytes / Self::MEBIBYTES_PER_GIBIBYTE)
        } else {
            write!(formatter, "{mebibytes}M")
        }
    }
}

impl Serialize for MemorySize {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for MemorySize {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

/// Reason a memory size could not be represented by Profile V1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseMemorySizeError {
    InvalidFormat,
    Zero,
    Overflow,
}

impl fmt::Display for ParseMemorySizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => formatter.write_str(message::MEMORY_INVALID_FORMAT),
            Self::Zero => formatter.write_str(message::MEMORY_MUST_BE_NON_ZERO),
            Self::Overflow => formatter.write_str(message::MEMORY_OVERFLOW),
        }
    }
}

impl Error for ParseMemorySizeError {}

/// Host project exposure inside the guest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, garde::Validate)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceProfile {
    #[garde(custom(super::validation::validate_workspace_mode))]
    pub mode: WorkspaceMode,
    #[garde(custom(super::validation::validate_host_workspace_path))]
    pub host: PathBuf,
    #[garde(custom(super::validation::validate_guest_workspace_path))]
    pub guest: PathBuf,
    #[garde(skip)]
    pub access: WorkspaceAccess,
}

/// How project files move between the host and guest.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceMode {
    Bind,
    Copy,
}

/// Access granted to the guest for workspace files.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceAccess {
    ReadOnly,
    ReadWrite,
}

/// Network policy requested for the environment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, garde::Validate)]
#[serde(deny_unknown_fields)]
pub struct NetworkProfile {
    #[garde(custom(super::validation::validate_network_mode))]
    pub mode: NetworkMode,
}

/// Network modes represented by Profile V1.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkMode {
    Default,
    Restricted,
}

/// AI coding agents available inside the environment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProfiles {
    pub codex: AgentProfile,
    pub claude: AgentProfile,
}

/// Runtime policy shared by each supported coding agent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProfile {
    pub enabled: bool,
    pub state: AgentState,
}

/// Persistence policy for an agent's credentials and local state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentState {
    Isolated,
}

#[cfg(test)]
mod tests {
    use serde::{
        Deserialize,
        de::value::{Error as ValueError, StrDeserializer, U16Deserializer},
    };

    use super::{CpuCount, MemorySize, ParseMemorySizeError};

    #[test]
    fn cpu_count_must_be_non_zero() {
        assert_eq!(CpuCount::new(4).map(CpuCount::get), Some(4));
        assert_eq!(CpuCount::new(0), None);

        let deserializer = U16Deserializer::<ValueError>::new(0);
        assert!(CpuCount::deserialize(deserializer).is_err());
    }

    #[test]
    fn memory_size_accepts_mebibytes_and_gibibytes() {
        let mebibytes = "512M".parse::<MemorySize>().expect("512M should parse");
        let gibibytes = "8G".parse::<MemorySize>().expect("8G should parse");

        assert_eq!(mebibytes.as_mebibytes(), 512);
        assert_eq!(mebibytes.to_string(), "512M");
        assert_eq!(gibibytes.as_mebibytes(), 8192);
        assert_eq!(gibibytes.to_string(), "8G");
    }

    #[test]
    fn memory_size_rejects_invalid_values() {
        for value in ["hello", "0G", "1.5G", "512", "-1G", "8g"] {
            assert!(
                value.parse::<MemorySize>().is_err(),
                "{value} should be rejected"
            );
        }

        let deserializer = StrDeserializer::<ValueError>::new("hello");
        assert!(MemorySize::deserialize(deserializer).is_err());

        assert_eq!("0M".parse::<MemorySize>(), Err(ParseMemorySizeError::Zero));
    }

    #[test]
    fn memory_size_rejects_overflow() {
        assert_eq!(
            "18446744073709551615G".parse::<MemorySize>(),
            Err(ParseMemorySizeError::Overflow)
        );
    }
}
