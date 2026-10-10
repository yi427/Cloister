//! Check configured and loaded loopback forwarding without modifying host networking.

use crate::{
    profile::Profile,
    runtime::{
        HOST_BRIDGE_GUEST_NAME, HOST_BRIDGE_LOCALHOST_ADDRESS, HOST_PF_ANCHOR,
        HOST_PF_NESTED_ANCHOR, execute_output, pf_configuration_command,
    },
};

use super::{CheckReport, connectivity};

pub(super) async fn check(report: &mut CheckReport, profile: &Profile, image_ready: bool) {
    let anchor = match configuration().await {
        Ok(anchor) => {
            report.pass("PF configuration", "host loopback redirect is present in the expanded /etc/pf.conf (disk configuration only)");
            anchor
        }
        Err(error) => {
            report.fail("PF configuration", format!("{error}\n{}", repair_hint()));
            report.skip(
                "Host connectivity",
                "fix the on-disk forwarding configuration first",
            );
            return;
        }
    };
    if !image_ready {
        report.skip(
            "Host connectivity",
            "a compatible local image is required for the temporary probe",
        );
        return;
    }
    match connectivity::probe(profile).await {
        Ok(()) => report.pass("Host connectivity", "temporary container reached the host loopback listener directly (no proxy; MCP protocol not tested)"),
        Err(error) => report.fail("Host connectivity", format!(
            "{error}\nCheck loaded PF rules and TUN/VPN routing to {HOST_BRIDGE_LOCALHOST_ADDRESS}:\nsudo pfctl -s info; sudo pfctl -s nat; sudo pfctl -a {anchor} -s nat"
        )),
    }
}

fn repair_hint() -> String {
    format!(
        "Recreate this mapping with Apple container (reloads host PF rules):\nsudo container system dns delete {HOST_BRIDGE_GUEST_NAME} && sudo container system dns create {HOST_BRIDGE_GUEST_NAME} --localhost {HOST_BRIDGE_LOCALHOST_ADDRESS}"
    )
}

async fn configuration() -> Result<&'static str, String> {
    let output = execute_output(&pf_configuration_command())
        .await
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("pfctl could not parse /etc/pf.conf and its included rule files; check their syntax and readability".into());
    }
    validate_configuration(
        &String::from_utf8_lossy(&output.stdout),
        &String::from_utf8_lossy(&output.stderr),
    )
}

fn validate_configuration(expanded: &str, diagnostics: &str) -> Result<&'static str, String> {
    let anchor = recognized_anchor(expanded, diagnostics).ok_or_else(|| format!(
        "DNS is configured, but /etc/pf.conf does not invoke rdr-anchor \"{HOST_PF_ANCHOR}\" or load \"{HOST_PF_NESTED_ANCHOR}\" through a redirect wildcard; the container redirect is disconnected"
    ))?;
    if !has_redirect(expanded) {
        return Err(format!(
            "/etc/pf.conf does not load the expected redirect {HOST_BRIDGE_LOCALHOST_ADDRESS} -> 127.0.0.1; the rule file may be missing or not loaded"
        ));
    }
    Ok(anchor)
}

fn recognized_anchor(rules: &str, diagnostics: &str) -> Option<&'static str> {
    for anchor in [HOST_PF_ANCHOR, HOST_PF_NESTED_ANCHOR] {
        if has_redirect_anchor(rules, anchor) {
            return Some(anchor);
        }
    }
    // For nested anchors, macOS pfctl prints relative wildcard names in stdout.
    // Its verbose stderr retains the full loaded anchor name and source file.
    // A wildcard alone must not make an unrelated/missing rule file pass.
    if (has_redirect_anchor(rules, "com.apple/*") || has_redirect_anchor(rules, "/*"))
        && diagnostics.lines().any(|line| {
            fields(line)
                == [
                    "Loading",
                    "anchor",
                    HOST_PF_NESTED_ANCHOR,
                    "from",
                    "/etc/pf.anchors/com.apple.container",
                ]
        })
    {
        return Some(HOST_PF_NESTED_ANCHOR);
    }
    None
}

fn has_redirect_anchor(rules: &str, anchor: &str) -> bool {
    rules.lines().any(|line| {
        let fields = fields(line);
        fields.len() == 3
            && fields[0] == "rdr-anchor"
            && fields[1].trim_matches('"') == anchor
            && fields[2] == "all"
    })
}

fn has_redirect(rules: &str) -> bool {
    rules.lines().any(|line| {
        let fields = fields(line);
        fields.len() == 8
            && fields[..5] == ["rdr", "inet", "from", "any", "to"]
            && fields[5].trim_end_matches("/32") == HOST_BRIDGE_LOCALHOST_ADDRESS
            && fields[6] == "->"
            && fields[7].trim_end_matches("/32") == "127.0.0.1"
    })
}

fn fields(line: &str) -> Vec<&str> {
    line.split('#')
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_expanded_rules_not_comments_or_unrelated_anchors() {
        let ready = include_str!("../../../tests/fixtures/packet-filter/configured.txt");
        assert_eq!(validate_configuration(ready, "").unwrap(), HOST_PF_ANCHOR);
        let disconnected = include_str!("../../../tests/fixtures/packet-filter/disconnected.txt");
        assert!(
            validate_configuration(disconnected, "")
                .unwrap_err()
                .contains("does not invoke rdr-anchor")
        );
        assert!(validate_configuration(&ready.replace("rdr-anchor", "# rdr-anchor"), "").is_err());
        assert!(
            validate_configuration(&ready.replace("com.apple.container", "other"), "").is_err()
        );
        assert!(validate_configuration(&ready.replace("rdr inet", "# rdr inet"), "").is_err());
        assert!(validate_configuration(&ready.replace("127.0.0.1", "127.0.0.2"), "").is_err());
    }

    #[test]
    fn accepts_the_container_1_5_nested_anchor_from_verbose_diagnostics() {
        let expanded = include_str!("../../../tests/fixtures/packet-filter/configured-nested.txt");
        let diagnostics =
            include_str!("../../../tests/fixtures/packet-filter/configured-nested-stderr.txt");
        assert_eq!(
            validate_configuration(expanded, diagnostics).unwrap(),
            HOST_PF_NESTED_ANCHOR
        );
        assert_eq!(
            validate_configuration(&expanded.replace("/*", "com.apple/*"), diagnostics).unwrap(),
            HOST_PF_NESTED_ANCHOR
        );
    }

    #[test]
    fn rejects_wildcards_without_the_expected_nested_anchor_and_rule_file() {
        let expanded = include_str!("../../../tests/fixtures/packet-filter/configured-nested.txt");
        let diagnostics =
            include_str!("../../../tests/fixtures/packet-filter/configured-nested-stderr.txt");
        for invalid in [
            String::new(),
            diagnostics.replace("com.apple/container", "other/container"),
            diagnostics.replace("pf.anchors/com.apple.container", "pf.anchors/other"),
            format!("# {diagnostics}"),
        ] {
            assert!(validate_configuration(expanded, &invalid).is_err());
        }
        assert!(
            validate_configuration(&expanded.replace("rdr-anchor", "nat-anchor"), diagnostics)
                .is_err()
        );
        assert!(
            validate_configuration(
                &expanded.replace("203.0.113.113", "203.0.113.114"),
                diagnostics
            )
            .is_err()
        );
    }

    #[test]
    fn requires_an_unrestricted_host_redirect_and_accepts_single_host_masks() {
        assert!(has_redirect(
            "rdr inet from any to 203.0.113.113/32 -> 127.0.0.1/32"
        ));
        assert!(!has_redirect(
            "rdr inet from any to 203.0.113.113 port 17834 -> 127.0.0.1"
        ));
        assert!(!has_redirect(
            "rdr inet from any to 203.0.113.113/24 -> 127.0.0.1"
        ));
    }
}
