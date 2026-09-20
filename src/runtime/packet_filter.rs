//! Read-only macOS packet-filter queries used by readiness checks.

use super::CommandSpec;

pub const HOST_PF_ANCHOR: &str = "com.apple.container";

fn pfctl(arguments: &[&str]) -> CommandSpec {
    CommandSpec {
        program: "pfctl".into(),
        arguments: arguments.iter().map(|arg| (*arg).into()).collect(),
        secret_environment: Vec::new(),
    }
}

/// Parse and expand the on-disk configuration without loading or changing rules.
pub fn pf_configuration_command() -> CommandSpec {
    pfctl(&["-n", "-v", "-f", "/etc/pf.conf"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_configuration_without_loading_rules_or_requesting_privilege() {
        let command = pf_configuration_command();
        assert_eq!(command.program(), "pfctl");
        assert_eq!(command.arguments(), ["-n", "-v", "-f", "/etc/pf.conf"]);
        assert!(command.secret_environment().is_empty());
    }
}
