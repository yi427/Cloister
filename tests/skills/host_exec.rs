//! Contract tests for the canonical Host Exec Skill source.

const SKILL: &str = include_str!("../../skills/host-exec/SKILL.md");
const OPENAI_METADATA: &str = include_str!("../../skills/host-exec/agents/openai.yaml");
const CONTAINERFILE: &str = include_str!("../../images/rust-node/Containerfile");
const ENTRYPOINT: &str = include_str!("../../images/rust-node/entrypoint.sh");
const DOCKERIGNORE: &str = include_str!("../../.dockerignore");
const MAKEFILE: &str = include_str!("../../Makefile");
const PUBLISH_WORKFLOW: &str = include_str!("../../.github/workflows/publish-image.yml");

#[test]
fn describes_only_the_connected_host_tool_surface() {
    assert!(SKILL.starts_with("---\nname: host-exec\ndescription:"));
    assert!(SKILL.contains("`cloister_host` MCP tools"));
    assert!(SKILL.contains("Call `host.list_commands`"));
    assert!(SKILL.contains("fixed Host working directory"));
    assert!(SKILL.contains("Call `host.exec`"));
    assert!(SKILL.contains("call `host.exec_status`"));
    assert!(SKILL.contains("call `host.exec_cancel`"));
    assert!(SKILL.contains("`wait_ms: 10000`"));
    assert!(SKILL.contains("status wait in flight"));
    assert!(SKILL.contains("bounded wait"));
}

#[test]
fn requires_discovery_literal_arguments_and_fail_closed_behavior() {
    for requirement in [
        "Select only a returned command name",
        "Pass every argument as a separate literal string",
        "Do not substitute another command",
        "report the error without using",
        "State that a prompt appeared only when one was actually observed",
        "runs with the permissions of the macOS user",
        "passes the argument vector",
        "directly without a shell",
    ] {
        assert!(
            SKILL.contains(requirement),
            "missing semantic: {requirement}"
        );
    }
}

#[test]
fn keeps_shared_workspace_file_operations_inside_the_guest() {
    for requirement in [
        "Do not use for reading, writing, listing, or patching files",
        "Use Guest file tools",
        "live read-write mount",
        "Do not use `host.exec` merely to move workspace content",
        "do not Base64-encode a workspace file",
        "`python -c`",
        "prefer paths relative to its",
        "outside `/workspace`",
        "Do not treat an allowed interpreter as a Host file API",
    ] {
        assert!(
            SKILL.contains(requirement),
            "missing workspace boundary: {requirement}"
        );
    }
}

#[test]
fn declares_the_canonical_skill_and_mcp_dependency() {
    assert!(OPENAI_METADATA.contains("display_name: \"Cloister Host Exec\""));
    assert!(OPENAI_METADATA.contains("$host-exec"));
    assert!(OPENAI_METADATA.contains("type: \"mcp\""));
    assert!(OPENAI_METADATA.contains("value: \"cloister_host\""));
}

#[test]
fn image_exposes_one_canonical_source_without_persistent_state_writes() {
    assert_eq!(CONTAINERFILE.matches("COPY skills/host-exec").count(), 1);
    assert!(CONTAINERFILE.contains("/usr/local/share/cloister/skills/host-exec"));
    assert!(CONTAINERFILE.contains("claude-skill-root/.claude/skills/host-exec"));

    assert!(ENTRYPOINT.contains("${CLOISTER_HOST_BRIDGE_TOKEN:-}"));
    assert!(ENTRYPOINT.contains("${HOME}/.agents/skills"));
    assert!(ENTRYPOINT.contains("refusing to overwrite existing Codex Skill"));
    assert!(!ENTRYPOINT.contains("${CODEX_HOME}/skills"));
    assert!(!ENTRYPOINT.contains("${CLAUDE_CONFIG_DIR}/skills"));
}

#[test]
fn image_build_context_and_publishing_include_the_canonical_skill() {
    assert!(DOCKERIGNORE.contains("!skills/host-exec/**"));
    assert!(MAKEFILE.contains("IMAGE_CONTEXT := ."));
    assert!(PUBLISH_WORKFLOW.contains("- skills/host-exec/**"));
    assert!(PUBLISH_WORKFLOW.contains("context: ."));
}
