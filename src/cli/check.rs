//! Read-only readiness checks for the natural agent workflow.

use std::{ffi::OsStr, path::PathBuf, process::ExitCode};

use clap::{Args, ValueHint};
use serde::Deserialize;
use tokio::process::Command;

use crate::{
    error::message,
    profile::{Architecture, Profile, load_profile},
    runtime::APPLE_CONTAINER_PROGRAM,
};

use super::config::default_profile_path;

const HOST_BRIDGE_GUEST_NAME: &str = "host.container.internal";

#[derive(Debug, Args)]
pub(super) struct CheckArgs {
    /// Path to a Profile V3 TOML file.
    ///
    /// Defaults to ~/.config/cloister/profile.toml.
    #[arg(long, value_name = "PROFILE", value_hint = ValueHint::FilePath)]
    profile: Option<PathBuf>,
}

impl CheckArgs {
    pub(super) async fn execute(self) -> ExitCode {
        let mut report = CheckReport::default();

        let profile = check_profile(self.profile, &mut report);
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
    let arguments = ["system", "status", "--format", "json"];
    let output = container_output(&arguments).await?;
    let status: RuntimeStatus = parse_json(&output, &arguments)?;
    if status.status != "running" {
        return Err(format!("Apple container service is {}", status.status));
    }

    Ok(status.api_server_version)
}

async fn check_image(profile: &Profile) -> Result<String, String> {
    let reference = profile.image.reference.as_str();
    let arguments = ["image", "inspect", reference];
    let output = container_output(&arguments).await?;
    let images: Vec<ImageInspection> = parse_json(&output, &arguments)?;
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
    let arguments = ["system", "dns", "list", "--format", "json"];
    let output = container_output(&arguments).await?;
    let domains: Vec<String> = parse_json(&output, &arguments)?;
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

async fn container_output(arguments: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new(APPLE_CONTAINER_PROGRAM)
        .args(arguments.iter().map(OsStr::new))
        .output()
        .await
        .map_err(|error| format!("failed to start '{APPLE_CONTAINER_PROGRAM}': {error}"))?;

    if output.status.success() {
        return Ok(output.stdout);
    }

    let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let command = format!("{APPLE_CONTAINER_PROGRAM} {}", arguments.join(" "));
    if detail.is_empty() {
        Err(format!("'{command}' exited with {}", output.status))
    } else {
        Err(format!("'{command}' failed: {detail}"))
    }
}

fn parse_json<T>(output: &[u8], arguments: &[&str]) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_slice(output).map_err(|error| {
        format!(
            "'{} {}' returned invalid JSON: {error}",
            APPLE_CONTAINER_PROGRAM,
            arguments.join(" ")
        )
    })
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
