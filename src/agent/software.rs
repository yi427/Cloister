//! Host-coordinated installation of guest software into immutable version directories.
//! Only a dedicated preparation container receives a writable staging mount.

use std::{
    collections::BTreeMap,
    env, fs,
    fs::{File, OpenOptions},
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::{
    agent::AgentAdapter,
    preflight::ResolvedGuestProxy,
    profile::{AgentVersion, Profile},
    runtime::{agent_install_command, execute_managed, name_container},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Installation {
    directory: String,
    version: String,
    image: String,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Inventory {
    schema_version: u32,
    latest: BTreeMap<String, Latest>,
    installations: Vec<Installation>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Latest {
    directory: String,
    checked_at: u64,
}

#[derive(Debug)]
pub struct Software {
    pub directory: PathBuf,
    pub version: String,
}

#[derive(Default)]
pub struct SoftwareOptions {
    pub version: Option<AgentVersion>,
    pub update: bool,
    pub offline: bool,
}

pub fn selected_version<'a>(
    profile: &'a Profile,
    agent: &dyn AgentAdapter,
    options: &'a SoftwareOptions,
) -> &'a AgentVersion {
    options
        .version
        .as_ref()
        .unwrap_or_else(|| match agent.state_directory_name() {
            "codex" => &profile.agent.codex,
            "claude" => &profile.agent.claude,
            _ => unreachable!("supported agent adapter"),
        })
}

pub fn software_directory(agent: &dyn AgentAdapter) -> io::Result<PathBuf> {
    let base = env::var_os("XDG_DATA_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|p| !p.is_empty())
                .map(|p| PathBuf::from(p).join(".local/share"))
        })
        .ok_or_else(|| invalid("HOME or XDG_DATA_HOME is required for agent software"))?;
    let base = if base.is_absolute() {
        base
    } else {
        env::current_dir()?.join(base)
    };
    Ok(base
        .join("cloister/software/linux-arm64")
        .join(agent.state_directory_name()))
}

/// A writable workspace/state alias must not expose the shared program cache.
pub fn check_writable_mount(root: &Path, writable: &Path) -> io::Result<()> {
    let software = canonical_future(
        root.ancestors()
            .nth(2)
            .ok_or_else(|| invalid("invalid software root"))?,
    )?;
    let writable = canonical_future(writable)?;
    if software.starts_with(&writable) || writable.starts_with(&software) {
        return Err(invalid(
            "writable workspace or Agent state overlaps Cloister's software cache; select a separate workspace/data directory",
        ));
    }
    Ok(())
}

fn canonical_future(path: &Path) -> io::Result<PathBuf> {
    match path.canonicalize() {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or(error)?;
            let mut resolved = canonical_future(parent)?;
            match path.components().next_back() {
                Some(std::path::Component::ParentDir) => {
                    resolved.pop();
                }
                Some(std::path::Component::Normal(name)) => resolved.push(name),
                _ => return Err(invalid("invalid software path")),
            }
            Ok(resolved)
        }
        Err(error) => Err(error),
    }
}

/// Read-only inventory inspection; missing software is a normal first-run state.
pub fn inspect(
    root: &Path,
    profile: &Profile,
    selector: &AgentVersion,
) -> io::Result<Option<Software>> {
    match validate_managed_directories(root, false) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    }
    let inventory = read_inventory(root)?;
    selected(&inventory, profile, selector)
        .map(|entry| materialize(root, entry))
        .transpose()
}

pub async fn prepare(
    profile: &Profile,
    agent: &dyn AgentAdapter,
    proxy: Option<&ResolvedGuestProxy>,
    options: &SoftwareOptions,
    cancellation: &CancellationToken,
) -> io::Result<Software> {
    let root = software_directory(agent)?;
    prepare_at(&root, profile, agent, proxy, options, cancellation).await
}

async fn prepare_at(
    root: &Path,
    profile: &Profile,
    agent: &dyn AgentAdapter,
    proxy: Option<&ResolvedGuestProxy>,
    options: &SoftwareOptions,
    cancellation: &CancellationToken,
) -> io::Result<Software> {
    let selector = selected_version(profile, agent, options);
    if options.offline {
        return inspect(root, profile, selector)?.ok_or_else(|| invalid("agent software is not installed for this image/version; run once without --offline"));
    }
    prepare_directory(root)?;
    let _lock = acquire_lock(root, cancellation).await?;
    let mut inventory = read_inventory(root)?;
    let previous = selected(&inventory, profile, selector).cloned();
    let cached = previous
        .as_ref()
        .map(|entry| materialize(root, entry))
        .transpose()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_secs();
    let fresh = inventory
        .latest
        .get(&profile.image.reference)
        .is_some_and(|latest| {
            now >= latest.checked_at
                && now - latest.checked_at < u64::from(profile.agent.update_interval_hours) * 3600
        });
    if !options.update
        && (!selector.is_latest() || fresh)
        && let Some(cached) = cached
    {
        return Ok(cached);
    }

    println!(
        "{} software: checking {} from registry.npmjs.org",
        agent.display_name(),
        selector.as_str()
    );
    let staging = tempfile::Builder::new()
        .prefix("install-")
        .tempdir_in(root)?;
    let mut command = agent_install_command(
        profile,
        agent,
        selector,
        previous.as_ref().map(|i| i.version.as_str()),
        staging.path(),
        proxy,
    )
    .map_err(io::Error::other)?;
    let name = format!(
        "cloister-install-{}",
        staging.path().file_name().unwrap().to_string_lossy()
    );
    name_container(&mut command, &name);
    // Package-manager diagnostics can include proxy credentials: do not forward them.
    let attempt = execute_managed(&command, &name, cancellation, true).await;
    if cancellation.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "agent installation interrupted",
        ));
    }
    let successful = attempt.as_ref().is_ok_and(|status| status.success());
    if !successful {
        if !options.update
            && let Some(cached) = cached
        {
            eprintln!(
                "cloister: warning: {} update check failed; using installed {}",
                agent.display_name(),
                cached.version
            );
            return Ok(cached);
        }
        return Err(invalid(
            "agent installation failed; check runtime, network/proxy and requested version; existing installations were preserved",
        ));
    }
    let version = read_regular(&staging.path().join("version"), 128)?;
    let version = std::str::from_utf8(&version)
        .map_err(io::Error::other)?
        .trim();
    let parsed: AgentVersion = version.parse().map_err(invalid)?;
    if parsed.is_latest() || (!selector.is_latest() && selector != &parsed) {
        return Err(invalid("installer returned an unexpected version"));
    }
    let entry = if let Some(previous) = previous.filter(|entry| entry.version == version) {
        previous
    } else {
        let entry = Installation {
            directory: staging
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            version: version.to_owned(),
            image: profile.image.reference.clone(),
        };
        materialize(root, &entry)?;
        // Keep old installations: active sessions may still be using them.
        let _ = staging.keep();
        inventory.installations.push(entry.clone());
        entry
    };
    if selector.is_latest() {
        inventory.latest.insert(
            profile.image.reference.clone(),
            Latest {
                directory: entry.directory.clone(),
                checked_at: now,
            },
        );
    }
    inventory.schema_version = 1;
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    serde_json::to_writer_pretty(&mut file, &inventory)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    file.persist(root.join("inventory.json"))
        .map_err(|e| e.error)?;
    materialize(root, &entry)
}

fn selected<'a>(
    inventory: &'a Inventory,
    profile: &Profile,
    selector: &AgentVersion,
) -> Option<&'a Installation> {
    inventory.installations.iter().rev().find(|i| {
        i.image == profile.image.reference
            && if selector.is_latest() {
                inventory
                    .latest
                    .get(&profile.image.reference)
                    .is_some_and(|latest| latest.directory == i.directory)
            } else {
                i.version == selector.as_str()
            }
    })
}

fn materialize(root: &Path, entry: &Installation) -> io::Result<Software> {
    if !entry.directory.starts_with("install-")
        || !entry
            .directory
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
    {
        return Err(invalid("unsafe agent installation directory in inventory"));
    }
    let directory = root.join(&entry.directory);
    validate_directory(&directory)?;
    let modules = directory.join("node_modules");
    if fs::symlink_metadata(&modules)?.file_type().is_symlink() || !modules.is_dir() {
        return Err(invalid("agent installation is incomplete"));
    }
    let agent = root
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| invalid("invalid agent software root"))?;
    let executable = modules.join(".bin").join(agent).canonicalize()?;
    if !executable.starts_with(directory.canonicalize()?)
        || !executable.is_file()
        || fs::metadata(&executable)?.mode() & 0o111 == 0
    {
        return Err(invalid(
            "agent executable is missing or escapes the installation",
        ));
    }
    let parsed: AgentVersion = entry.version.parse().map_err(invalid)?;
    if parsed.is_latest() {
        return Err(invalid("inventory must record an exact version"));
    }
    Ok(Software {
        directory: directory.canonicalize()?,
        version: entry.version.clone(),
    })
}

fn read_inventory(root: &Path) -> io::Result<Inventory> {
    match read_regular(&root.join("inventory.json"), 4 * 1024 * 1024) {
        Ok(bytes) => {
            let inventory: Inventory = serde_json::from_slice(&bytes)?;
            if inventory.schema_version != 1 {
                return Err(invalid("unsupported software inventory schema"));
            }
            Ok(inventory)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Inventory::default()),
        Err(e) => Err(e),
    }
}

fn read_regular(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > limit || metadata.nlink() != 1 {
        return Err(invalid("unsafe or oversized software metadata"));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("oversized software metadata"));
    }
    Ok(bytes)
}

fn prepare_directory(path: &Path) -> io::Result<()> {
    validate_managed_directories(path, true)
}

fn validate_managed_directories(path: &Path, create: bool) -> io::Result<()> {
    // Validate every Cloister-managed component; allow the user-selected XDG base.
    let base = path
        .ancestors()
        .nth(4)
        .ok_or_else(|| invalid("invalid software path"))?;
    if create {
        fs::create_dir_all(base)?;
    }
    let mut current = base.to_owned();
    for component in path
        .strip_prefix(base)
        .map_err(io::Error::other)?
        .components()
    {
        current.push(component);
        if create {
            match fs::DirBuilder::new().mode(0o700).create(&current) {
                Ok(()) => (),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(e),
            }
        }
        validate_directory(&current)?;
    }
    Ok(())
}

fn validate_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    // SAFETY: geteuid takes no arguments and has no memory side effects.
    let uid = unsafe { libc::geteuid() };
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != uid
        || metadata.mode() & 0o022 != 0
    {
        return Err(invalid(format!(
            "unsafe software directory '{}'",
            path.display()
        )));
    }
    Ok(())
}

async fn acquire_lock(root: &Path, cancellation: &CancellationToken) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(root.join("install.lock"))?;
    let metadata = file.metadata()?;
    // SAFETY: geteuid takes no arguments.
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(invalid("unsafe agent installation lock"));
    }
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => (),
            Err(std::fs::TryLockError::Error(e)) => return Err(e),
        }
        tokio::select! {
            () = cancellation.cancelled() => return Err(io::Error::new(io::ErrorKind::Interrupted, "waiting for agent installation interrupted")),
            () = tokio::time::sleep(Duration::from_millis(100)) => (),
        }
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn rejects_symlinks_in_managed_ancestors_before_creating_children() {
        let temp = tempfile::tempdir().unwrap();
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, temp.path().join("cloister")).unwrap();
        let root = temp.path().join("cloister/software/linux-arm64/codex");
        assert!(prepare_directory(&root).is_err());
        assert!(validate_managed_directories(&root, false).is_err());
        assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn locks_are_exclusive_cancellable_and_released_on_drop() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("cloister/software/linux-arm64/codex");
        prepare_directory(&root).unwrap();
        let cancellation = CancellationToken::new();
        let first = acquire_lock(&root, &cancellation).await.unwrap();
        let second = CancellationToken::new();
        second.cancel();
        assert_eq!(
            acquire_lock(&root, &second).await.unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        drop(first);
        let _next = acquire_lock(&root, &cancellation).await.unwrap();
    }

    #[tokio::test]
    async fn rejects_symlink_lock_without_touching_target() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("cloister/software/linux-arm64/codex");
        prepare_directory(&root).unwrap();
        let target = temp.path().join("untouched");
        fs::write(&target, "original").unwrap();
        symlink(&target, root.join("install.lock")).unwrap();
        assert!(
            acquire_lock(&root, &CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(fs::read_to_string(target).unwrap(), "original");
    }

    #[test]
    fn rejects_inventory_path_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let entry = Installation {
            directory: "install-../../outside".into(),
            version: "1.2.3".into(),
            image: "cloister:dev".into(),
        };
        assert!(materialize(temp.path(), &entry).is_err());
    }

    #[test]
    fn rejects_writable_cache_aliases_even_before_cache_creation() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("cloister/software/linux-arm64/codex");
        assert!(check_writable_mount(&root, temp.path()).is_err());
        let project = temp.path().join("project");
        fs::create_dir(&project).unwrap();
        assert!(check_writable_mount(&root, &project).is_ok());
        let alias = temp.path().join("alias");
        symlink(temp.path(), &alias).unwrap();
        assert!(check_writable_mount(&root, &alias).is_err());
    }
}
