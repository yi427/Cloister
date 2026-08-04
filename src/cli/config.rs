//! Shared CLI configuration path resolution.

use std::{env, path::PathBuf};

/// Resolves the default Cloister Profile path from XDG or HOME.
pub(super) fn default_profile_path() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|path| !path.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })
        .map(|directory| directory.join("cloister/profile.toml"))
}
