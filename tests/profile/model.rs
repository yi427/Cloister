//! Public Profile V1 model integration tests.

use std::path::PathBuf;

use cloister::profile::{
    AgentProfile, AgentProfiles, AgentState, Architecture, CpuCount, GuestProfile, ImageProfile,
    MemorySize, NetworkMode, NetworkProfile, PROFILE_SCHEMA_VERSION, Profile, WorkspaceAccess,
    WorkspaceMode, WorkspaceProfile,
};

#[test]
fn constructs_profile_v1_through_the_public_api() {
    let profile = Profile {
        schema_version: PROFILE_SCHEMA_VERSION,
        name: "rust-default".to_owned(),
        image: ImageProfile {
            reference: "cloister/rust-node:dev".to_owned(),
            architecture: Architecture::Arm64,
        },
        guest: GuestProfile {
            hostname: "cloister".to_owned(),
            cpus: CpuCount::new(4).expect("CPU count should be non-zero"),
            memory: "8G".parse::<MemorySize>().expect("memory should parse"),
            user: "cloister".to_owned(),
            locale: "zh_CN.UTF-8".to_owned(),
            timezone: "Asia/Shanghai".to_owned(),
        },
        workspace: WorkspaceProfile {
            mode: WorkspaceMode::Bind,
            host: PathBuf::from("."),
            guest: PathBuf::from("/workspace"),
            access: WorkspaceAccess::ReadWrite,
        },
        network: NetworkProfile {
            mode: NetworkMode::Default,
        },
        agents: AgentProfiles {
            codex: AgentProfile {
                enabled: true,
                state: AgentState::Isolated,
            },
            claude: AgentProfile {
                enabled: true,
                state: AgentState::Isolated,
            },
        },
    };

    assert_eq!(profile.schema_version, 1);
    assert_eq!(profile.image.architecture, Architecture::Arm64);
    assert_eq!(profile.guest.cpus.get(), 4);
    assert_eq!(profile.guest.memory.as_mebibytes(), 8192);
    assert_eq!(profile.workspace.mode, WorkspaceMode::Bind);
    assert_eq!(profile.workspace.access, WorkspaceAccess::ReadWrite);
    assert_eq!(profile.network.mode, NetworkMode::Default);
    assert_eq!(profile.agents.codex.state, AgentState::Isolated);
}
