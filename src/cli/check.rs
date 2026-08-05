//! Read-only readiness checks for the natural agent workflow.

use std::{env, path::PathBuf, process::ExitCode};

use crate::{
    error::message,
    preflight::{inspect_host_executable, resolve_guest_proxy, resolve_host_command},
    profile::{Architecture, Profile, load_profile},
    runtime::{
        CommandSpec, HOST_BRIDGE_GUEST_NAME, RuntimeExecutionError, dns_list_command,
        execute_output, image_inspect_command, system_status_command,
    },
};
use clap::{Args, ValueHint};
use serde::Deserialize;

use super::config::default_profile_path;

#[derive(Debug, Args)]
pub(super) struct CheckArgs {
    /// Path to a Profile V6 TOML file.
    ///
    /// Defaults to ~/.config/cloister/profile.toml.
    #[arg(long, value_name = "PROFILE", value_hint = ValueHint::FilePath)]
    profile: Option<PathBuf>,
}

impl CheckArgs {
    pub(super) async fn execute(self) -> ExitCode {
        execute_checks(self.profile).await
    }
}

pub(super) async fn execute_checks(profile_path: Option<PathBuf>) -> ExitCode {
    let mut report = CheckReport::default();

    let profile = check_profile(profile_path, &mut report);
    match &profile {
        Some(profile) => {
            record_result(&mut report, "Guest proxy", check_guest_proxy(profile));
            check_host_policy(profile, &mut report);
        }
        None => {
            report.skip("Guest proxy", "Profile is unavailable");
            report.skip("Host policy", "Profile is unavailable");
        }
    }
    let runtime_ready = record_result(&mut report, "Runtime", check_runtime().await);

    match (&profile, runtime_ready) {
        (Some(profile), true) => {
            record_result(&mut report, "Image", check_image(profile).await);
        }
        (None, _) => report.skip("Image", "Profile is unavailable"),
        (Some(_), false) => report.skip("Image", "runtime is unavailable"),
    }

    if runtime_ready {
        record_result(&mut report, "DNS", check_dns().await);
    } else {
        report.skip("DNS", "runtime is unavailable");
    }

    report.finish()
}

fn check_guest_proxy(profile: &Profile) -> Result<String, String> {
    match resolve_guest_proxy(profile.network.proxy, env::vars_os())
        .map_err(|error| error.to_string())?
    {
        None => Ok("disabled by Profile".to_owned()),
        Some(proxy) => {
            let mapping = if proxy.loopback_rewritten() {
                "loopback mapped to host.container.internal"
            } else {
                "host address preserved"
            };
            Ok(format!(
                "inherit from {} ({mapping}; value redacted)",
                proxy.source_variable()
            ))
        }
    }
}

fn check_host_policy(profile: &Profile, report: &mut CheckReport) {
    let policy = &profile.host.exec;
    let state = if policy.enabled {
        "enabled"
    } else {
        "disabled"
    };
    report.pass(
        "Host policy",
        format!(
            "{state}, environment inherit-all, {} allowed command(s)",
            policy.allow.len()
        ),
    );

    let path = env::var_os("PATH");
    for command in &policy.allow {
        match inspect_host_executable(&command.executable) {
            Ok(executable) => report.pass(
                "Host command",
                format!(
                    "'{}': declared '{}', resolved '{}'",
                    command.name,
                    executable.declared().display(),
                    executable.resolved().display()
                ),
            ),
            Err(error) => {
                let mut detail = format!("'{}': {error}", command.name);
                if let Some(file_name) = command.executable.file_name()
                    && let Ok(replacement) = resolve_host_command(file_name, path.as_deref())
                {
                    detail.push_str(&format!(
                        "\nCurrent PATH finds '{}' at '{}' (resolved as '{}'); update this Profile entry explicitly.",
                        file_name.to_string_lossy(),
                        replacement.declared().display(),
                        replacement.resolved().display()
                    ));
                }
                report.fail("Host command", detail);
            }
        }
    }
}

fn check_profile(path: Option<PathBuf>, report: &mut CheckReport) -> Option<Profile> {
    let path = match path.or_else(default_profile_path) {
        Some(path) => path,
        None => {
            report.fail("Profile", message::HOME_DIRECTORY_MISSING);
            return None;
        }
    };

    match load_profile(&path) {
        Ok(profile) => {
            report.pass(
                "Profile",
                format!("'{}' ({})", profile.name, path.display()),
            );
            Some(profile)
        }
        Err(error) => {
            report.fail("Profile", error.to_string());
            None
        }
    }
}

async fn check_runtime() -> Result<String, String> {
    let command = system_status_command();
    let output = command_output(&command).await?;
    let status: RuntimeStatus = parse_json(&output, &command)?;
    if status.status != "running" {
        return Err(format!("Apple container service is {}", status.status));
    }

    Ok(status.api_server_version)
}

async fn check_image(profile: &Profile) -> Result<String, String> {
    let reference = profile.image.reference.as_str();
    let command = image_inspect_command(reference);
    let output = command_output(&command).await?;
    let images: Vec<ImageInspection> = parse_json(&output, &command)?;
    let architecture = architecture_name(profile.image.architecture);
    let has_compatible_variant = images.iter().any(|image| {
        image.variants.iter().any(|variant| {
            variant.platform.os == "linux" && variant.platform.architecture == architecture
        })
    });
    if !has_compatible_variant {
        return Err(format!(
            "image '{reference}' has no linux/{architecture} variant"
        ));
    }

    Ok(format!("'{reference}' (linux/{architecture})"))
}

async fn check_dns() -> Result<String, String> {
    let command = dns_list_command();
    let output = command_output(&command).await?;
    let domains: Vec<String> = parse_json(&output, &command)?;
    if !domains
        .iter()
        .any(|domain| domain == HOST_BRIDGE_GUEST_NAME)
    {
        return Err(format!(
            "'{HOST_BRIDGE_GUEST_NAME}' is not configured; see README setup instructions"
        ));
    }

    Ok(format!("'{HOST_BRIDGE_GUEST_NAME}' is configured"))
}

pub(super) async fn command_output(command: &CommandSpec) -> Result<Vec<u8>, String> {
    let output = execute_output(command).await.map_err(|error| match error {
        RuntimeExecutionError::Start { program, source } => {
            format!("failed to start '{}': {source}", program.to_string_lossy())
        }
    })?;

    if output.status.success() {
        return Ok(output.stdout);
    }

    let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let command = command_description(command);
    if detail.is_empty() {
        Err(format!("'{command}' exited with {}", output.status))
    } else {
        Err(format!("'{command}' failed: {detail}"))
    }
}

pub(super) fn parse_json<T>(output: &[u8], command: &CommandSpec) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_slice(output).map_err(|error| {
        format!(
            "'{}' returned invalid JSON: {error}",
            command_description(command),
        )
    })
}

pub(super) fn command_description(command: &CommandSpec) -> String {
    std::iter::once(command.program())
        .chain(
            command
                .arguments()
                .iter()
                .map(|argument| argument.as_os_str()),
        )
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

const fn architecture_name(architecture: Architecture) -> &'static str {
    match architecture {
        Architecture::Arm64 => "arm64",
    }
}

fn record_result(
    report: &mut CheckReport,
    name: &'static str,
    result: Result<String, String>,
) -> bool {
    match result {
        Ok(detail) => {
            report.pass(name, detail);
            true
        }
        Err(detail) => {
            report.fail(name, detail);
            false
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeStatus {
    api_server_version: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct ImageInspection {
    variants: Vec<ImageVariant>,
}

#[derive(Debug, Deserialize)]
struct ImageVariant {
    platform: ImagePlatform,
}

#[derive(Debug, Deserialize)]
struct ImagePlatform {
    architecture: String,
    os: String,
}

#[derive(Debug)]
enum CheckOutcome {
    Pass,
    Fail,
    Skip,
}

#[derive(Debug)]
struct CheckResult {
    name: &'static str,
    detail: String,
    outcome: CheckOutcome,
}

#[derive(Debug, Default)]
struct CheckReport {
    results: Vec<CheckResult>,
}

impl CheckReport {
    fn pass(&mut self, name: &'static str, detail: impl Into<String>) {
        self.push(name, detail, CheckOutcome::Pass);
    }

    fn fail(&mut self, name: &'static str, detail: impl Into<String>) {
        self.push(name, detail, CheckOutcome::Fail);
    }

    fn skip(&mut self, name: &'static str, detail: impl Into<String>) {
        self.push(name, detail, CheckOutcome::Skip);
    }

    fn push(&mut self, name: &'static str, detail: impl Into<String>, outcome: CheckOutcome) {
        self.results.push(CheckResult {
            name,
            detail: detail.into(),
            outcome,
        });
    }

    fn finish(self) -> ExitCode {
        let failed = self
            .results
            .iter()
            .filter(|result| matches!(result.outcome, CheckOutcome::Fail))
            .count();
        let skipped = self
            .results
            .iter()
            .filter(|result| matches!(result.outcome, CheckOutcome::Skip))
            .count();

        for result in self.results {
            let status = match result.outcome {
                CheckOutcome::Pass => "PASS",
                CheckOutcome::Fail => "FAIL",
                CheckOutcome::Skip => "SKIP",
            };
            print_result(status, result.name, &result.detail);
        }
        println!();

        if failed == 0 {
            println!("All checks passed.");
            ExitCode::SUCCESS
        } else {
            println!("{failed} check(s) failed; {skipped} skipped.");
            ExitCode::FAILURE
        }
    }
}

fn print_result(status: &str, name: &str, detail: &str) {
    let mut lines = detail.lines();
    println!(
        "[{status}] {name}: {}",
        lines.next().unwrap_or("no details available")
    );
    for line in lines {
        println!("       {line}");
    }
}
