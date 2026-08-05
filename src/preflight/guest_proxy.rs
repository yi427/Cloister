//! Resolution of an explicit host-proxy inheritance policy for Apple guests.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    ffi::{OsStr, OsString},
    fmt,
};

use url::{Host, Url};

use crate::{error::message, profile::NetworkProxyMode};

/// Apple-container DNS name that forwards guest traffic to the macOS host.
pub const APPLE_CONTAINER_HOST_NAME: &str = "host.container.internal";

const PROXY_SOURCE_PRECEDENCE: [&str; 6] = [
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
    "HTTP_PROXY",
    "http_proxy",
];
const NO_PROXY_NAMES: [&str; 2] = ["NO_PROXY", "no_proxy"];
const REQUIRED_NO_PROXY_ENTRIES: [&str; 4] =
    [APPLE_CONTAINER_HOST_NAME, "localhost", "127.0.0.1", "::1"];

/// Proxy values resolved once from the trusted host environment.
#[derive(Clone, Eq, PartialEq)]
pub struct ResolvedGuestProxy {
    source_variable: &'static str,
    proxy_url: OsString,
    no_proxy: OsString,
    loopback_rewritten: bool,
}

impl ResolvedGuestProxy {
    pub const fn source_variable(&self) -> &'static str {
        self.source_variable
    }

    pub const fn loopback_rewritten(&self) -> bool {
        self.loopback_rewritten
    }

    pub(crate) fn proxy_url(&self) -> &OsStr {
        &self.proxy_url
    }

    pub(crate) fn no_proxy(&self) -> &OsStr {
        &self.no_proxy
    }
}

impl fmt::Debug for ResolvedGuestProxy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedGuestProxy")
            .field("source_variable", &self.source_variable)
            .field("proxy_url", &"[REDACTED]")
            .field("no_proxy", &"[REDACTED]")
            .field("loopback_rewritten", &self.loopback_rewritten)
            .finish()
    }
}

/// Resolves the Profile-selected proxy behavior from one host environment snapshot.
pub fn resolve_guest_proxy(
    mode: NetworkProxyMode,
    environment: impl IntoIterator<Item = (OsString, OsString)>,
) -> Result<Option<ResolvedGuestProxy>, GuestProxyResolutionError> {
    if mode == NetworkProxyMode::Disabled {
        return Ok(None);
    }

    detect_inherited_guest_proxy(environment)?.map_or_else(
        || Err(GuestProxyResolutionError::Missing),
        |proxy| Ok(Some(proxy)),
    )
}

/// Detects and validates a supported host proxy without requiring one to exist.
pub fn detect_inherited_guest_proxy(
    environment: impl IntoIterator<Item = (OsString, OsString)>,
) -> Result<Option<ResolvedGuestProxy>, GuestProxyResolutionError> {
    let environment = environment.into_iter().collect::<BTreeMap<_, _>>();
    let Some((source_variable, value)) = PROXY_SOURCE_PRECEDENCE.iter().find_map(|name| {
        environment
            .get(OsStr::new(name))
            .filter(|value| !value.is_empty())
            .map(|value| (*name, value))
    }) else {
        return Ok(None);
    };

    let value = value
        .to_str()
        .ok_or(GuestProxyResolutionError::NonUnicode {
            variable: source_variable,
        })?;
    let mut proxy_url = Url::parse(value).map_err(|_| GuestProxyResolutionError::InvalidUrl {
        variable: source_variable,
    })?;
    if !matches!(proxy_url.scheme(), "http" | "https") {
        return Err(GuestProxyResolutionError::UnsupportedScheme {
            variable: source_variable,
            scheme: proxy_url.scheme().to_owned(),
        });
    }

    let loopback_rewritten = match proxy_url.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(host)) => host.is_loopback(),
        Some(Host::Ipv6(host)) => host.is_loopback(),
        None => {
            return Err(GuestProxyResolutionError::MissingHost {
                variable: source_variable,
            });
        }
    };
    if loopback_rewritten {
        proxy_url
            .set_host(Some(APPLE_CONTAINER_HOST_NAME))
            .map_err(|_| GuestProxyResolutionError::Rewrite {
                variable: source_variable,
            })?;
    }

    let no_proxy = merged_no_proxy(&environment)?;
    Ok(Some(ResolvedGuestProxy {
        source_variable,
        proxy_url: OsString::from(proxy_url.as_str()),
        no_proxy: OsString::from(no_proxy),
        loopback_rewritten,
    }))
}

fn merged_no_proxy(
    environment: &BTreeMap<OsString, OsString>,
) -> Result<String, GuestProxyResolutionError> {
    let mut entries = Vec::new();
    let mut normalized = BTreeSet::new();

    for name in NO_PROXY_NAMES {
        let Some(value) = environment
            .get(OsStr::new(name))
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let value = value
            .to_str()
            .ok_or(GuestProxyResolutionError::NonUnicode { variable: name })?;
        for entry in value
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            if normalized.insert(entry.to_ascii_lowercase()) {
                entries.push(entry.to_owned());
            }
        }
    }

    for entry in REQUIRED_NO_PROXY_ENTRIES {
        if normalized.insert(entry.to_ascii_lowercase()) {
            entries.push(entry.to_owned());
        }
    }

    Ok(entries.join(","))
}

/// Host environment that cannot satisfy an explicit proxy-inheritance policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GuestProxyResolutionError {
    Missing,
    NonUnicode {
        variable: &'static str,
    },
    InvalidUrl {
        variable: &'static str,
    },
    UnsupportedScheme {
        variable: &'static str,
        scheme: String,
    },
    MissingHost {
        variable: &'static str,
    },
    Rewrite {
        variable: &'static str,
    },
}

impl fmt::Display for GuestProxyResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => formatter.write_str(message::GUEST_PROXY_MISSING),
            Self::NonUnicode { variable } => {
                write!(
                    formatter,
                    "{}: {variable}",
                    message::GUEST_PROXY_NON_UNICODE
                )
            }
            Self::InvalidUrl { variable } => {
                write!(
                    formatter,
                    "{}: {variable}",
                    message::GUEST_PROXY_INVALID_URL
                )
            }
            Self::UnsupportedScheme { variable, scheme } => write!(
                formatter,
                "{}: {variable} uses {scheme:?}",
                message::GUEST_PROXY_UNSUPPORTED_SCHEME
            ),
            Self::MissingHost { variable } => {
                write!(
                    formatter,
                    "{}: {variable}",
                    message::GUEST_PROXY_MISSING_HOST
                )
            }
            Self::Rewrite { variable } => {
                write!(
                    formatter,
                    "{}: {variable}",
                    message::GUEST_PROXY_REWRITE_FAILED
                )
            }
        }
    }
}

impl Error for GuestProxyResolutionError {}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, ffi::OsString};

    use super::{
        APPLE_CONTAINER_HOST_NAME, GuestProxyResolutionError, detect_inherited_guest_proxy,
        resolve_guest_proxy,
    };
    use crate::profile::NetworkProxyMode;

    fn environment(entries: &[(&str, &str)]) -> BTreeMap<OsString, OsString> {
        entries
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect()
    }

    #[test]
    fn disabled_policy_does_not_require_or_inspect_host_proxy_variables() {
        let resolved = resolve_guest_proxy(
            NetworkProxyMode::Disabled,
            environment(&[("HTTPS_PROXY", "not a URL")]),
        )
        .expect("disabled proxy should ignore the host environment");

        assert!(resolved.is_none());
    }

    #[test]
    fn inherited_policy_requires_a_supported_proxy_variable() {
        let error = resolve_guest_proxy(NetworkProxyMode::Inherit, environment(&[]))
            .expect_err("missing proxy should fail an explicit inherit policy");

        assert_eq!(error, GuestProxyResolutionError::Missing);
    }

    #[test]
    fn rewrites_loopback_and_extends_no_proxy_without_exposing_values() {
        let secret = "proxy-password";
        let resolved = detect_inherited_guest_proxy(environment(&[
            (
                "HTTPS_PROXY",
                &format!("http://user:{secret}@127.0.0.1:3080"),
            ),
            ("NO_PROXY", "example.com,localhost"),
        ]))
        .expect("proxy should resolve")
        .expect("proxy should be present");

        assert_eq!(resolved.source_variable(), "HTTPS_PROXY");
        assert!(resolved.loopback_rewritten());
        assert!(
            resolved
                .proxy_url()
                .to_string_lossy()
                .contains(APPLE_CONTAINER_HOST_NAME)
        );
        let no_proxy = resolved.no_proxy().to_string_lossy();
        assert!(no_proxy.contains("example.com"));
        assert!(no_proxy.contains(APPLE_CONTAINER_HOST_NAME));
        assert_eq!(no_proxy.matches("localhost").count(), 1);
        assert!(!format!("{resolved:?}").contains(secret));
    }

    #[test]
    fn preserves_a_non_loopback_proxy_and_uses_https_precedence() {
        let resolved = detect_inherited_guest_proxy(environment(&[
            ("ALL_PROXY", "http://all.example:1080"),
            ("HTTPS_PROXY", "https://secure.example:8443"),
        ]))
        .expect("proxy should resolve")
        .expect("proxy should be present");

        assert_eq!(resolved.source_variable(), "HTTPS_PROXY");
        assert!(!resolved.loopback_rewritten());
        assert_eq!(
            resolved.proxy_url(),
            OsString::from("https://secure.example:8443/")
        );
    }

    #[test]
    fn rejects_an_unsupported_proxy_scheme_without_rendering_the_value() {
        let error = detect_inherited_guest_proxy(environment(&[(
            "HTTPS_PROXY",
            "socks5://private-proxy.example:1080",
        )]))
        .expect_err("unsupported proxy should fail");

        assert!(matches!(
            error,
            GuestProxyResolutionError::UnsupportedScheme { .. }
        ));
        assert!(!error.to_string().contains("private-proxy.example"));
    }
}
