//! Public Profile V3 model integration tests.

use cloister::profile::{
    AgentState, Architecture, CodexProfile, CpuCount, GuestProfile, ImageProfile, MemorySize,
    NetworkMode, NetworkProfile, PROFILE_SCHEMA_VERSION, Profile,
};

#[test]
fn constructs_profile_v3_through_the_public_api() {
    let profile = Profile {
        schema_version: PROFILE_SCHEMA_VERSION,
        name: "rust-default".to_owned(),
        image: ImageProfile {
            reference: "cloister/rust-node:dev".to_owned(),
            architecture: Architecture::Arm64,
        },
        guest: GuestProfile {
            cpus: CpuCount::new(4).expect("CPU count should be non-zero"),
            memory: "8G".parse::<MemorySize>().expect("memory should parse"),
            user: "cloister".to_owned(),
            locale: "zh_CN.UTF-8".to_owned(),
            timezone: "Asia/Shanghai".to_owned(),
        },
        network: NetworkProfile {
            mode: NetworkMode::Default,
        },
        codex: CodexProfile {
            state: AgentState::Isolated,
        },
    };

    assert_eq!(profile.schema_version, 3);
    assert_eq!(profile.image.architecture, Architecture::Arm64);
    assert_eq!(profile.guest.cpus.get(), 4);
    assert_eq!(profile.guest.memory.as_mebibytes(), 8192);
    assert_eq!(profile.network.mode, NetworkMode::Default);
    assert_eq!(profile.codex.state, AgentState::Isolated);
}
