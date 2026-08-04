//! Interactive setup for the default Cloister environment.

use std::{
    error::Error,
    fmt, fs,
    io::{self, BufRead, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Args, ValueHint};
use serde::Deserialize;
use tempfile::NamedTempFile;

use crate::{
    error::message,
    profile::{
        AgentState, Architecture, CodexProfile, CpuCount, GuestProfile, ImageProfile, MemorySize,
        NetworkMode, NetworkProfile, PROFILE_SCHEMA_VERSION, Profile, validate_profile,
    },
    runtime::{
        CommandSpec, HOST_BRIDGE_GUEST_NAME, dns_create_command, dns_list_command, execute,
        execute_output, image_inspect_command, image_pull_command, system_start_command,
        system_status_command,
    },
};

use super::{
    check::{command_description, command_output, execute_checks, parse_json},
    config::default_profile_path,
};

const DEFAULT_CPUS: u16 = 4;
const DEFAULT_IMAGE_REFERENCE: &str = concat!("ghcr.io/yi427/cloister:", env!("CARGO_PKG_VERSION"));
const DEFAULT_LOCALE: &str = "en_US.UTF-8";
const DEFAULT_MEMORY: &str = "8G";
const DEFAULT_NAME: &str = "default";
const DEFAULT_TIMEZONE: &str = "America/New_York";
const DEFAULT_USER: &str = "cloister";

#[derive(Debug, Args)]
pub(super) struct InitArgs {
    /// Path where the new Profile V3 TOML file will be created.
    ///
    /// Defaults to ~/.config/cloister/profile.toml.
    #[arg(long, value_name = "PROFILE", value_hint = ValueHint::FilePath)]
    profile: Option<PathBuf>,
}

impl InitArgs {
    pub(super) async fn execute(self) -> Result<ExitCode, InitCommandError> {
        let path = self
            .profile
            .or_else(default_profile_path)
            .ok_or(InitCommandError::HomeDirectoryMissing)?;
        reject_existing_target(&path)?;

        let stdin = io::stdin();
        let mut input = stdin.lock();
        let stdout = io::stdout();
        let mut output = stdout.lock();
        let profile = prompt_profile(&mut input, &mut output)?;
        print_summary(&path, &profile, &mut output)?;

        let runtime = inspect_runtime().await;
        let should_create = match &runtime {
            RuntimeState::Missing => {
                print_missing_runtime(&mut output)?;
                prompt_yes_no(
                    &mut input,
                    &mut output,
                    "Create the Profile without runtime setup?",
                    false,
                )?
            }
            RuntimeState::Unavailable(detail) => {
                writeln!(output, "\nRuntime status could not be read: {detail}")?;
                prompt_yes_no(
                    &mut input,
                    &mut output,
                    "Create the Profile and leave runtime setup incomplete?",
                    false,
                )?
            }
            RuntimeState::Running { .. } | RuntimeState::NeedsStart { .. } => {
                prompt_yes_no(&mut input, &mut output, "Create this Profile?", true)?
            }
        };

        if !should_create {
            writeln!(output, "No changes made.")?;
            return Ok(ExitCode::SUCCESS);
        }

        write_profile_atomically(&path, &profile)?;
        writeln!(output, "Created Profile at {}.", path.display())?;
        output.flush()?;

        provision_runtime(&runtime, &profile, &mut input, &mut output).await?;

        writeln!(output, "\nVerifying the initialized environment...")?;
        output.flush()?;
        Ok(execute_checks(Some(path)).await)
    }
}

fn prompt_profile(
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<Profile, InitCommandError> {
    writeln!(output, "Cloister interactive setup")?;
    writeln!(output, "Press Enter to accept a value shown in brackets.\n")?;

    let name = prompt_non_blank(input, output, "Profile name", DEFAULT_NAME)?;
    let image = prompt_non_blank(
        input,
        output,
        "Exact image reference",
        DEFAULT_IMAGE_REFERENCE,
    )?;
    let cpus = prompt_cpus(input, output)?;
    let memory = prompt_memory(input, output)?;
    let persistent_state = prompt_yes_no(
        input,
        output,
        "Persist Codex login and state across projects?",
        true,
    )?;
    let profile = Profile {
        schema_version: PROFILE_SCHEMA_VERSION,
        name,
        image: ImageProfile {
            reference: image,
            architecture: Architecture::Arm64,
        },
        guest: GuestProfile {
            cpus,
            memory,
            user: DEFAULT_USER.to_owned(),
            locale: DEFAULT_LOCALE.to_owned(),
            timezone: DEFAULT_TIMEZONE.to_owned(),
        },
        network: NetworkProfile {
            mode: NetworkMode::Default,
        },
        codex: CodexProfile {
            state: if persistent_state {
                AgentState::Shared
            } else {
                AgentState::Isolated
            },
        },
    };
    validate_profile(&profile).map_err(|source| InitCommandError::GeneratedProfile { source })?;
    Ok(profile)
}

fn prompt_non_blank(
    input: &mut impl BufRead,
    output: &mut impl Write,
    label: &str,
    default: &str,
) -> Result<String, InitCommandError> {
    loop {
        let value = prompt_line(input, output, &format!("{label} [{default}]: "))?;
        let value = if value.is_empty() {
            default.to_owned()
        } else {
            value
        };
        if value.trim().is_empty() {
            writeln!(output, "Value must not be blank.")?;
        } else {
            return Ok(value);
        }
    }
}

fn prompt_cpus(
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<CpuCount, InitCommandError> {
    loop {
        let value = prompt_line(input, output, &format!("Guest CPUs [{DEFAULT_CPUS}]: "))?;
        let value = if value.is_empty() {
            DEFAULT_CPUS.to_string()
        } else {
            value
        };
        if let Ok(value) = value.parse::<u16>()
            && let Some(cpus) = CpuCount::new(value)
        {
            return Ok(cpus);
        }
        writeln!(output, "CPU count must be an integer from 1 to 65535.")?;
    }
}

fn prompt_memory(
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<MemorySize, InitCommandError> {
    loop {
        let value = prompt_line(input, output, &format!("Guest memory [{DEFAULT_MEMORY}]: "))?;
        let value = if value.is_empty() {
            DEFAULT_MEMORY.to_owned()
        } else {
            value
        };
        match value.parse::<MemorySize>() {
            Ok(memory) => return Ok(memory),
            Err(error) => writeln!(output, "Invalid memory: {error}.")?,
        }
    }
}

fn prompt_yes_no(
    input: &mut impl BufRead,
    output: &mut impl Write,
    question: &str,
    default: bool,
) -> Result<bool, InitCommandError> {
    let choices = if default { "Y/n" } else { "y/N" };
    loop {
        let value = prompt_line(input, output, &format!("{question} [{choices}]: "))?;
        match value.to_ascii_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => writeln!(output, "Please answer yes or no.")?,
        }
    }
}

fn prompt_line(
    input: &mut impl BufRead,
    output: &mut impl Write,
    prompt: &str,
) -> Result<String, InitCommandError> {
    write!(output, "{prompt}")?;
    output.flush()?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Err(InitCommandError::InputClosed);
    }
    Ok(line.trim().to_owned())
}

fn print_summary(
    path: &Path,
    profile: &Profile,
    output: &mut impl Write,
) -> Result<(), InitCommandError> {
    writeln!(output, "\nProfile summary:")?;
    writeln!(output, "  Path: {}", path.display())?;
    writeln!(output, "  Name: {}", profile.name)?;
    writeln!(output, "  Image: {}", profile.image.reference)?;
    writeln!(
        output,
        "  Guest: arm64, {} CPU(s), {} memory",
        profile.guest.cpus.get(),
        profile.guest.memory
    )?;
    writeln!(output, "  Network: default (outbound internet enabled)")?;
    match profile.codex.state {
        AgentState::Shared => writeln!(
            output,
            "  Codex state: shared (may contain credentials and history)"
        )?,
        AgentState::Isolated => writeln!(output, "  Codex state: isolated (ephemeral)")?,
    }
    Ok(())
}

fn print_missing_runtime(output: &mut impl Write) -> Result<(), InitCommandError> {
    writeln!(output, "\nApple container was not found on PATH.")?;
    writeln!(
        output,
        "Install its signed package from https://github.com/apple/container/releases"
    )?;
    writeln!(output, "Then run: container system start")?;
    Ok(())
}

async fn provision_runtime(
    initial_runtime: &RuntimeState,
    profile: &Profile,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<(), InitCommandError> {
    let runtime_ready = match initial_runtime {
        RuntimeState::Missing | RuntimeState::Unavailable(_) => false,
        RuntimeState::Running { version } => {
            writeln!(output, "Runtime is running: {version}")?;
            true
        }
        RuntimeState::NeedsStart { detail } => {
            writeln!(output, "Apple container runtime is not ready: {detail}")?;
            if prompt_yes_no(input, output, "Start the runtime now?", true)? {
                let command = system_start_command();
                run_setup_command(&command, output).await?;
                match inspect_runtime().await {
                    RuntimeState::Running { version } => {
                        writeln!(output, "Runtime is running: {version}")?;
                        true
                    }
                    RuntimeState::NeedsStart { detail } => {
                        writeln!(
                            output,
                            "Runtime is still not ready ({detail}); skipping image and DNS setup."
                        )?;
                        false
                    }
                    RuntimeState::Missing => {
                        writeln!(
                            output,
                            "Runtime command disappeared; skipping remaining setup."
                        )?;
                        false
                    }
                    RuntimeState::Unavailable(detail) => {
                        writeln!(output, "Runtime status failed: {detail}")?;
                        false
                    }
                }
            } else {
                writeln!(output, "Runtime start skipped.")?;
                false
            }
        }
    };

    if !runtime_ready {
        return Ok(());
    }

    provision_image(profile, input, output).await?;
    provision_dns(input, output).await?;
    Ok(())
}

async fn provision_image(
    profile: &Profile,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<(), InitCommandError> {
    let inspect = image_inspect_command(&profile.image.reference);
    match command_output(&inspect).await {
        Ok(_) => writeln!(
            output,
            "Image is already available: {}",
            profile.image.reference
        )?,
        Err(detail) => {
            writeln!(output, "Image is not available: {detail}")?;
            if prompt_yes_no(input, output, "Pull the exact ARM64 image now?", true)? {
                run_setup_command(&image_pull_command(&profile.image.reference), output).await?;
            } else {
                writeln!(output, "Image pull skipped.")?;
            }
        }
    }
    Ok(())
}

async fn provision_dns(
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<(), InitCommandError> {
    let command = dns_list_command();
    let output_bytes = match command_output(&command).await {
        Ok(output) => output,
        Err(detail) => {
            writeln!(output, "DNS status failed: {detail}")?;
            return Ok(());
        }
    };
    let domains: Vec<String> = match parse_json(&output_bytes, &command) {
        Ok(domains) => domains,
        Err(detail) => {
            writeln!(output, "DNS status failed: {detail}")?;
            return Ok(());
        }
    };
    if domains
        .iter()
        .any(|domain| domain == HOST_BRIDGE_GUEST_NAME)
    {
        writeln!(
            output,
            "DNS is already configured: {HOST_BRIDGE_GUEST_NAME}"
        )?;
        return Ok(());
    }

    writeln!(
        output,
        "DNS mapping '{HOST_BRIDGE_GUEST_NAME}' is not configured."
    )?;
    writeln!(
        output,
        "This privileged Apple container change disables Private Relay."
    )?;
    if prompt_yes_no(input, output, "Configure the DNS mapping with sudo?", false)? {
        run_setup_command(&dns_create_command(), output).await?;
    } else {
        writeln!(output, "DNS setup skipped.")?;
    }
    Ok(())
}

async fn run_setup_command(
    command: &CommandSpec,
    output: &mut impl Write,
) -> Result<(), InitCommandError> {
    let description = command_description(command);
    writeln!(output, "Running: {description}")?;
    output.flush()?;
    match execute(command).await {
        Ok(status) if status.success() => {
            writeln!(output, "Completed: {description}")?;
        }
        Ok(status) => {
            writeln!(output, "Command failed with {status}: {description}")?;
        }
        Err(error) => {
            writeln!(output, "Command could not start: {error}")?;
        }
    }
    Ok(())
}

async fn inspect_runtime() -> RuntimeState {
    let command = system_status_command();
    let output = match execute_output(&command).await {
        Ok(output) => output,
        Err(error) if error.is_not_found() => return RuntimeState::Missing,
        Err(error) => return RuntimeState::Unavailable(error.to_string()),
    };
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return RuntimeState::NeedsStart {
            detail: if detail.is_empty() {
                format!(
                    "'{}' exited with {}",
                    command_description(&command),
                    output.status
                )
            } else {
                format!("'{}' failed: {detail}", command_description(&command))
            },
        };
    }
    let status: RuntimeStatus = match parse_json(&output.stdout, &command) {
        Ok(status) => status,
        Err(detail) => return RuntimeState::Unavailable(detail),
    };
    if status.status == "running" {
        RuntimeState::Running {
            version: status.api_server_version,
        }
    } else {
        RuntimeState::NeedsStart {
            detail: format!("service status is {}", status.status),
        }
    }
}

fn reject_existing_target(path: &Path) -> Result<(), InitCommandError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(InitCommandError::ProfileExists {
            path: path.to_owned(),
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(InitCommandError::InspectTarget {
            path: path.to_owned(),
            source,
        }),
    }
}

fn write_profile_atomically(path: &Path, profile: &Profile) -> Result<(), InitCommandError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| InitCommandError::CreateParent {
        path: parent.to_owned(),
        source,
    })?;

    let serialized =
        toml::to_string_pretty(profile).map_err(|source| InitCommandError::Serialize { source })?;
    let mut temporary =
        NamedTempFile::new_in(parent).map_err(|source| InitCommandError::CreateTemporary {
            path: parent.to_owned(),
            source,
        })?;
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|source| InitCommandError::Write {
            path: path.to_owned(),
            source,
        })?;
    temporary
        .write_all(serialized.as_bytes())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| InitCommandError::Write {
            path: path.to_owned(),
            source,
        })?;
    temporary
        .persist_noclobber(path)
        .map_err(|error| InitCommandError::Persist {
            path: path.to_owned(),
            source: error.error,
        })?;
    Ok(())
}

#[derive(Debug)]
enum RuntimeState {
    Missing,
    Unavailable(String),
    Running { version: String },
    NeedsStart { detail: String },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeStatus {
    api_server_version: String,
    status: String,
}

#[derive(Debug)]
pub(super) enum InitCommandError {
    HomeDirectoryMissing,
    ProfileExists { path: PathBuf },
    InspectTarget { path: PathBuf, source: io::Error },
    Input(io::Error),
    InputClosed,
    GeneratedProfile { source: garde::Report },
    CreateParent { path: PathBuf, source: io::Error },
    Serialize { source: toml::ser::Error },
    CreateTemporary { path: PathBuf, source: io::Error },
    Write { path: PathBuf, source: io::Error },
    Persist { path: PathBuf, source: io::Error },
}

impl fmt::Display for InitCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HomeDirectoryMissing => formatter.write_str(message::HOME_DIRECTORY_MISSING),
            Self::ProfileExists { path } => write!(
                formatter,
                "Profile already exists at {}; refusing to overwrite it",
                path.display()
            ),
            Self::InspectTarget { path, source } => {
                write!(formatter, "failed to inspect {}: {source}", path.display())
            }
            Self::Input(source) => write!(formatter, "interactive input failed: {source}"),
            Self::InputClosed => {
                formatter.write_str("interactive input closed before setup completed")
            }
            Self::GeneratedProfile { source } => {
                write!(formatter, "generated Profile is invalid: {source}")
            }
            Self::CreateParent { path, source } => write!(
                formatter,
                "failed to create Profile directory {}: {source}",
                path.display()
            ),
            Self::Serialize { source } => {
                write!(formatter, "failed to serialize Profile: {source}")
            }
            Self::CreateTemporary { path, source } => write!(
                formatter,
                "failed to create a temporary Profile in {}: {source}",
                path.display()
            ),
            Self::Write { path, source } => {
                write!(
                    formatter,
                    "failed to write Profile {}: {source}",
                    path.display()
                )
            }
            Self::Persist { path, source } => write!(
                formatter,
                "failed to install Profile at {} without overwriting: {source}",
                path.display()
            ),
        }
    }
}

impl Error for InitCommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InspectTarget { source, .. }
            | Self::CreateParent { source, .. }
            | Self::CreateTemporary { source, .. }
            | Self::Write { source, .. }
            | Self::Persist { source, .. }
            | Self::Input(source) => Some(source),
            Self::GeneratedProfile { source } => Some(source),
            Self::Serialize { source } => Some(source),
            Self::HomeDirectoryMissing | Self::ProfileExists { .. } | Self::InputClosed => None,
        }
    }
}

impl From<io::Error> for InitCommandError {
    fn from(source: io::Error) -> Self {
        Self::Input(source)
    }
}
