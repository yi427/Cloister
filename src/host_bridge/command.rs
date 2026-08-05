//! Direct host-process construction after policy authorization.

use tokio::process::Command;

use super::AuthorizedHostCommand;

/// Builds a direct process invocation with exactly the trusted environment.
///
/// No shell is involved. The child inherits no ambient variables beyond the
/// complete snapshot carried by the authorized command.
pub fn build_host_process(command: &AuthorizedHostCommand) -> Command {
    let mut process = Command::new(command.executable());
    process
        .args(command.arguments())
        .env_clear()
        .envs(command.environment());
    process
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, ffi::OsString};

    use super::build_host_process;
    use crate::host_bridge::{
        AllowedHostCommand, HOST_EXEC_DSL_VERSION, HostEnvironment, HostExecPolicy, HostExecRequest,
    };

    #[tokio::test]
    async fn injects_exactly_the_complete_environment_snapshot() {
        let environment = BTreeMap::from([
            (OsString::from("CLOISTER_ENV_ONE"), OsString::from("one")),
            (OsString::from("CLOISTER_ENV_TWO"), OsString::from("two")),
        ]);
        let policy = HostExecPolicy::new(
            [AllowedHostCommand::new("env", "/usr/bin/env")
                .expect("test allow entry should be valid")],
            environment.clone(),
        )
        .expect("test policy should be valid");
        let authorized = policy
            .authorize(&HostExecRequest {
                version: HOST_EXEC_DSL_VERSION,
                command: "env".to_owned(),
                args: Vec::new(),
            })
            .expect("configured command should be authorized");

        let output = build_host_process(&authorized)
            .output()
            .await
            .expect("environment probe should run");
        assert!(output.status.success());

        let actual = String::from_utf8(output.stdout)
            .expect("environment probe should emit UTF-8")
            .lines()
            .map(|line| {
                let (name, value) = line
                    .split_once('=')
                    .expect("environment output should contain a name and value");
                (OsString::from(name), OsString::from(value))
            })
            .collect::<HostEnvironment>();

        assert_eq!(actual, environment);
    }
}
