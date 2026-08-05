//! Authorization model for structured host execution.

use std::{collections::BTreeMap, error::Error, ffi::OsString, fmt, path::PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::profile::{HostExecArguments, HostExecProfile};

/// Structured host-execution request version understood by this policy layer.
pub const HOST_EXEC_DSL_VERSION: u32 = 1;

/// Model-supplied portion of a planned `host.exec` request.
///
/// The executable is deliberately absent. It is selected only from the host
/// policy after `command` has been authorized.
#[derive(Clone, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostExecRequest {
    pub version: u32,
    pub command: String,
    pub args: Vec<String>,
}

impl fmt::Debug for HostExecRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostExecRequest")
            .field("version", &self.version)
            .field("command", &self.command)
            .field("argument_count", &self.args.len())
            .finish()
    }
}

/// Complete host environment snapshot selected by trusted bridge code.
pub type HostEnvironment = BTreeMap<OsString, OsString>;

/// One executable that may be selected by its stable command name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AllowedHostCommand {
    name: String,
    executable: PathBuf,
    description: String,
    arguments: HostExecArguments,
}

impl AllowedHostCommand {
    /// Builds an allowlist entry with an explicit absolute executable path.
    pub fn new(
        name: impl Into<String>,
        executable: impl Into<PathBuf>,
    ) -> Result<Self, HostExecPolicyBuildError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(HostExecPolicyBuildError::BlankCommandName);
        }

        let executable = executable.into();
        if !executable.is_absolute() {
            return Err(HostExecPolicyBuildError::RelativeExecutable {
                command: name,
                executable,
            });
        }

        Ok(Self {
            name,
            executable,
            description: String::new(),
            arguments: HostExecArguments::Any,
        })
    }

    /// Builds an allowlist entry with the Profile metadata exposed by discovery.
    pub fn with_metadata(
        name: impl Into<String>,
        executable: impl Into<PathBuf>,
        description: impl Into<String>,
        arguments: HostExecArguments,
    ) -> Result<Self, HostExecPolicyBuildError> {
        let mut command = Self::new(name, executable)?;
        command.description = description.into();
        command.arguments = arguments;
        Ok(command)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn executable(&self) -> &std::path::Path {
        &self.executable
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub const fn arguments(&self) -> HostExecArguments {
        self.arguments
    }
}

/// Immutable in-memory command allowlist.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct HostExecPolicy {
    commands: BTreeMap<String, AllowedHostCommand>,
    environment: HostEnvironment,
}

impl HostExecPolicy {
    /// Builds a policy with a complete trusted host environment snapshot.
    pub fn new(
        commands: impl IntoIterator<Item = AllowedHostCommand>,
        environment: HostEnvironment,
    ) -> Result<Self, HostExecPolicyBuildError> {
        let mut by_name = BTreeMap::new();
        for command in commands {
            let name = command.name.clone();
            if by_name.insert(name.clone(), command).is_some() {
                return Err(HostExecPolicyBuildError::DuplicateCommandName { command: name });
            }
        }

        Ok(Self {
            commands: by_name,
            environment,
        })
    }

    /// Builds the enabled policy represented by a validated Profile V5.
    pub fn from_profile(
        profile: &HostExecProfile,
        environment: HostEnvironment,
    ) -> Result<Option<Self>, HostExecPolicyBuildError> {
        if !profile.enabled {
            return Ok(None);
        }

        let commands = profile
            .allow
            .iter()
            .map(|command| {
                AllowedHostCommand::with_metadata(
                    command.name.clone(),
                    command.executable.clone(),
                    command.description.clone(),
                    command.arguments,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        Self::new(commands, environment).map(Some)
    }

    /// Resolves a structured request into an executable selected by policy.
    pub fn authorize(
        &self,
        request: &HostExecRequest,
    ) -> Result<AuthorizedHostCommand, HostExecAuthorizationError> {
        if request.version != HOST_EXEC_DSL_VERSION {
            return Err(HostExecAuthorizationError::UnsupportedVersion {
                expected: HOST_EXEC_DSL_VERSION,
                found: request.version,
            });
        }

        let allowed = self.commands.get(&request.command).ok_or_else(|| {
            HostExecAuthorizationError::CommandNotAllowed {
                command: request.command.clone(),
            }
        })?;

        Ok(AuthorizedHostCommand {
            command_name: allowed.name.clone(),
            executable: allowed.executable.clone(),
            arguments: request.args.clone(),
            environment: self.environment.clone(),
        })
    }

    /// Commands exposed by `host.list_commands`, ordered by stable name.
    pub fn commands(&self) -> impl Iterator<Item = &AllowedHostCommand> {
        self.commands.values()
    }

    /// Number of commands authorized by this immutable policy.
    pub fn command_count(&self) -> usize {
        self.commands.len()
    }

    /// Trusted environment variable names, never their values.
    pub fn environment_names(&self) -> impl Iterator<Item = &std::ffi::OsStr> {
        self.environment.keys().map(OsString::as_os_str)
    }
}

impl fmt::Debug for HostExecPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostExecPolicy")
            .field("commands", &self.commands)
            .field(
                "environment_names",
                &self.environment.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// Command vector produced only after a policy authorization succeeds.
#[derive(Clone, Eq, PartialEq)]
pub struct AuthorizedHostCommand {
    command_name: String,
    executable: PathBuf,
    arguments: Vec<String>,
    environment: HostEnvironment,
}

impl AuthorizedHostCommand {
    pub fn command_name(&self) -> &str {
        &self.command_name
    }

    pub fn executable(&self) -> &std::path::Path {
        &self.executable
    }

    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    pub fn environment(&self) -> &HostEnvironment {
        &self.environment
    }
}

impl fmt::Debug for AuthorizedHostCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthorizedHostCommand")
            .field("command_name", &self.command_name)
            .field("executable", &self.executable)
            .field("argument_count", &self.arguments.len())
            .field(
                "environment_names",
                &self.environment.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// Invalid construction of the trusted allowlist.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostExecPolicyBuildError {
    BlankCommandName,
    RelativeExecutable {
        command: String,
        executable: PathBuf,
    },
    DuplicateCommandName {
        command: String,
    },
}

impl fmt::Display for HostExecPolicyBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlankCommandName => formatter.write_str("host command name must not be blank"),
            Self::RelativeExecutable {
                command,
                executable,
            } => write!(
                formatter,
                "host command {command:?} executable must be absolute: {}",
                executable.display()
            ),
            Self::DuplicateCommandName { command } => {
                write!(formatter, "host command name is duplicated: {command:?}")
            }
        }
    }
}

impl Error for HostExecPolicyBuildError {}

/// Reason a model-supplied request was not authorized.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostExecAuthorizationError {
    UnsupportedVersion { expected: u32, found: u32 },
    CommandNotAllowed { command: String },
}

impl fmt::Display for HostExecAuthorizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion { expected, found } => write!(
                formatter,
                "host execution request version {found} is unsupported; expected {expected}"
            ),
            Self::CommandNotAllowed { command } => {
                write!(formatter, "host command is not allowed: {command:?}")
            }
        }
    }
}

impl Error for HostExecAuthorizationError {}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, ffi::OsString, path::Path};

    use super::{
        AllowedHostCommand, HOST_EXEC_DSL_VERSION, HostEnvironment, HostExecAuthorizationError,
        HostExecPolicy, HostExecPolicyBuildError, HostExecRequest,
    };

    const SECRET_VALUE: &str = "do-not-render-this-value";

    fn host_environment() -> HostEnvironment {
        BTreeMap::from([
            (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
            (
                OsString::from("CLOISTER_TEST_SECRET"),
                OsString::from(SECRET_VALUE),
            ),
        ])
    }

    fn policy() -> HostExecPolicy {
        HostExecPolicy::new(
            [AllowedHostCommand::new("xcodebuild", "/usr/bin/xcodebuild")
                .expect("test allow entry should be valid")],
            host_environment(),
        )
        .expect("test policy should be valid")
    }

    #[test]
    fn resolves_the_executable_only_from_the_allowlist() {
        let request = HostExecRequest {
            version: HOST_EXEC_DSL_VERSION,
            command: "xcodebuild".to_owned(),
            args: vec!["-version".to_owned()],
        };

        let authorized = policy()
            .authorize(&request)
            .expect("configured command should be authorized");

        assert_eq!(authorized.command_name(), "xcodebuild");
        assert_eq!(authorized.executable(), Path::new("/usr/bin/xcodebuild"));
        assert_eq!(authorized.arguments(), ["-version"]);
    }

    #[test]
    fn injects_the_complete_trusted_host_environment() {
        let request = HostExecRequest {
            version: HOST_EXEC_DSL_VERSION,
            command: "xcodebuild".to_owned(),
            args: Vec::new(),
        };

        let authorized = policy()
            .authorize(&request)
            .expect("configured command should be authorized");

        assert_eq!(authorized.environment(), &host_environment());
    }

    #[test]
    fn rejects_a_command_that_is_not_in_the_allowlist() {
        let request = HostExecRequest {
            version: HOST_EXEC_DSL_VERSION,
            command: "python3".to_owned(),
            args: vec!["-c".to_owned(), "print('host code')".to_owned()],
        };

        assert_eq!(
            policy().authorize(&request),
            Err(HostExecAuthorizationError::CommandNotAllowed {
                command: "python3".to_owned()
            })
        );
    }

    #[test]
    fn preserves_shell_metacharacters_as_literal_arguments() {
        let arguments = vec![
            "; rm -rf /".to_owned(),
            "$(id)".to_owned(),
            "*.secret".to_owned(),
        ];
        let request = HostExecRequest {
            version: HOST_EXEC_DSL_VERSION,
            command: "xcodebuild".to_owned(),
            args: arguments.clone(),
        };

        let authorized = policy()
            .authorize(&request)
            .expect("configured command should be authorized");

        assert_eq!(authorized.arguments(), arguments);
    }

    #[test]
    fn rejects_an_unsupported_request_version() {
        let request = HostExecRequest {
            version: HOST_EXEC_DSL_VERSION + 1,
            command: "xcodebuild".to_owned(),
            args: Vec::new(),
        };

        assert_eq!(
            policy().authorize(&request),
            Err(HostExecAuthorizationError::UnsupportedVersion {
                expected: HOST_EXEC_DSL_VERSION,
                found: HOST_EXEC_DSL_VERSION + 1
            })
        );
    }

    #[test]
    fn request_schema_rejects_a_model_supplied_executable() {
        let error = serde_json::from_value::<HostExecRequest>(serde_json::json!({
            "version": HOST_EXEC_DSL_VERSION,
            "command": "xcodebuild",
            "args": ["-version"],
            "executable": "/bin/zsh"
        }))
        .expect_err("request executable must not enter the policy model");

        assert!(error.to_string().contains("unknown field `executable`"));
    }

    #[test]
    fn request_schema_rejects_a_model_supplied_environment() {
        let error = serde_json::from_value::<HostExecRequest>(serde_json::json!({
            "version": HOST_EXEC_DSL_VERSION,
            "command": "xcodebuild",
            "args": ["-version"],
            "env": { "PATH": "/untrusted" }
        }))
        .expect_err("request environment must not enter the policy model");

        assert!(error.to_string().contains("unknown field `env`"));
    }

    #[test]
    fn debug_output_includes_environment_names_without_values() {
        let request = HostExecRequest {
            version: HOST_EXEC_DSL_VERSION,
            command: "xcodebuild".to_owned(),
            args: vec![SECRET_VALUE.to_owned()],
        };
        let policy = policy();
        let authorized = policy
            .authorize(&request)
            .expect("configured command should be authorized");

        let request_debug = format!("{request:?}");
        let policy_debug = format!("{policy:?}");
        let authorized_debug = format!("{authorized:?}");

        assert!(policy_debug.contains("CLOISTER_TEST_SECRET"));
        assert!(authorized_debug.contains("CLOISTER_TEST_SECRET"));
        for debug_output in [request_debug, policy_debug, authorized_debug] {
            assert!(!debug_output.contains(SECRET_VALUE));
        }
    }

    #[test]
    fn rejects_a_relative_executable_in_the_trusted_policy() {
        assert_eq!(
            AllowedHostCommand::new("xcodebuild", "bin/xcodebuild"),
            Err(HostExecPolicyBuildError::RelativeExecutable {
                command: "xcodebuild".to_owned(),
                executable: "bin/xcodebuild".into()
            })
        );
    }

    #[test]
    fn rejects_duplicate_allowlist_names() {
        let first = AllowedHostCommand::new("xcodebuild", "/usr/bin/xcodebuild")
            .expect("first test allow entry should be valid");
        let duplicate = AllowedHostCommand::new("xcodebuild", "/opt/bin/xcodebuild")
            .expect("duplicate test allow entry should be structurally valid");

        assert_eq!(
            HostExecPolicy::new([first, duplicate], HostEnvironment::new()),
            Err(HostExecPolicyBuildError::DuplicateCommandName {
                command: "xcodebuild".to_owned()
            })
        );
    }
}
