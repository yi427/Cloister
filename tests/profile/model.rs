//! Public Profile V5 model integration tests.

use std::{collections::BTreeMap, ffi::OsString, path::Path};

use cloister::{
    host_bridge::{HOST_EXEC_DSL_VERSION, HostExecPolicy, HostExecRequest},
    profile::{
        AgentProfile, AgentState, Architecture, CpuCount, GuestProfile, HostExecAllowProfile,
        HostExecArguments, HostExecEnvironmentMode, HostExecEnvironmentProfile, HostExecProfile,
        HostProfile, ImageProfile, MemorySize, NetworkMode, NetworkProfile, PROFILE_SCHEMA_VERSION,
        Profile,
    },
};

#[test]
fn constructs_profile_v5_through_the_public_api() {
    let profile = Profile {
        schema_version: PROFILE_SCHEMA_VERSION,
        name: "rust-default".to_owned(),
        image: ImageProfile {
            reference: "cloister:dev".to_owned(),
            architecture: Architecture::Arm64,
        },
        guest: GuestProfile {
            cpus: CpuCount::new(4).expect("CPU count should be non-zero"),
            memory: "8G".parse::<MemorySize>().expect("memory should parse"),
            user: "cloister".to_owned(),
            locale: "en_US.UTF-8".to_owned(),
            timezone: "America/New_York".to_owned(),
        },
        network: NetworkProfile {
            mode: NetworkMode::Default,
        },
        agent: AgentProfile {
            state: AgentState::Isolated,
        },
        host: HostProfile {
            exec: HostExecProfile {
                enabled: true,
                environment: HostExecEnvironmentProfile {
                    mode: HostExecEnvironmentMode::InheritAll,
                },
                allow: vec![HostExecAllowProfile {
                    name: "xcodebuild".to_owned(),
                    executable: "/usr/bin/xcodebuild".into(),
                    description: "Build and test Xcode projects".to_owned(),
                    arguments: HostExecArguments::Any,
                }],
            },
        },
    };

    assert_eq!(profile.schema_version, 5);
    assert_eq!(profile.image.architecture, Architecture::Arm64);
    assert_eq!(profile.guest.cpus.get(), 4);
    assert_eq!(profile.guest.memory.as_mebibytes(), 8192);
    assert_eq!(profile.network.mode, NetworkMode::Default);
    assert_eq!(profile.agent.state, AgentState::Isolated);
    assert_eq!(
        profile.host.exec.environment.mode,
        HostExecEnvironmentMode::InheritAll
    );
    assert_eq!(profile.host.exec.allow[0].name, "xcodebuild");
}

#[test]
fn converts_the_enabled_profile_allowlist_into_host_policy() {
    let profile =
        cloister::profile::parse_profile(include_str!("../fixtures/profiles/valid/default.toml"))
            .expect("default profile should parse");
    let environment = BTreeMap::from([(
        OsString::from("CLOISTER_PROFILE_ENV"),
        OsString::from("preserved"),
    )]);
    let policy = HostExecPolicy::from_profile(&profile.host.exec, environment.clone())
        .expect("validated Profile should produce a policy")
        .expect("enabled Profile should produce a policy");
    let authorized = policy
        .authorize(&HostExecRequest {
            version: HOST_EXEC_DSL_VERSION,
            command: "xcodebuild".to_owned(),
            args: vec!["-version".to_owned()],
        })
        .expect("Profile allow entry should authorize the command");

    assert_eq!(authorized.executable(), Path::new("/usr/bin/xcodebuild"));
    assert_eq!(authorized.arguments(), ["-version"]);
    assert_eq!(authorized.environment(), &environment);
}

#[test]
fn disabled_profile_does_not_produce_host_policy() {
    let mut profile =
        cloister::profile::parse_profile(include_str!("../fixtures/profiles/valid/default.toml"))
            .expect("default profile should parse");
    profile.host.exec.enabled = false;

    let policy = HostExecPolicy::from_profile(&profile.host.exec, BTreeMap::new())
        .expect("disabled Profile should still be structurally valid");

    assert!(policy.is_none());
}
