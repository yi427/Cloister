//! Centralized user-facing error messages.

pub(crate) mod message {
    pub const CLI_ERROR_PREFIX: &str = "error";

    pub const MEMORY_INVALID_FORMAT: &str = "memory must be a positive integer followed by M or G";
    pub const MEMORY_MUST_BE_NON_ZERO: &str = "memory must be greater than zero";
    pub const MEMORY_OVERFLOW: &str = "memory is too large";
    pub const PROFILE_PARSE_FAILED: &str = "failed to parse profile";
    pub const PROFILE_READ_FAILED: &str = "failed to read profile";
    pub const PROFILE_VALIDATION_FAILED: &str = "profile validation failed";
    pub const WORKSPACE_PATH_RESOLUTION_FAILED: &str = "failed to resolve workspace path";
    pub const WORKSPACE_PATH_NOT_DIRECTORY: &str = "workspace path is not a directory";
    pub const WORKSPACE_PATH_IS_ROOT: &str = "workspace must not be the filesystem root";

    pub const UNSUPPORTED_SCHEMA_VERSION: &str = "schema_version is not supported";
    pub const VALUE_MUST_NOT_BE_BLANK: &str = "must not be blank";
    pub const HOST_EXECUTABLE_MUST_BE_ABSOLUTE: &str =
        "host command executable must be an absolute path";
    pub const HOST_COMMAND_NAME_DUPLICATED: &str = "host command name must be unique";
    pub const MOUNT_PATH_CONTAINS_SEPARATOR: &str = "bind mount paths must not contain ','";
    pub const RUNTIME_START_FAILED: &str = "failed to start runtime";
    pub const SHARED_AGENT_STATE_MISSING: &str = "state requires a Cloister-managed host directory";
    pub const CURRENT_DIRECTORY_FAILED: &str = "failed to resolve the current directory";
    pub const HOME_DIRECTORY_MISSING: &str =
        "HOME is not set; cannot locate Cloister configuration or state";
    pub const AGENT_STATE_CREATE_FAILED: &str = "failed to create agent state directory";
    pub const AGENT_STATE_METADATA_FAILED: &str = "failed to inspect agent state directory";
    pub const AGENT_STATE_INVALID: &str =
        "agent state path must be a real directory, not a file or symbolic link";
    pub const AGENT_STATE_PERMISSIONS_FAILED: &str =
        "failed to restrict agent state directory permissions";

    pub const BRIDGE_TOKEN_READ_FAILED: &str = "failed to read host bridge token";
    pub const BRIDGE_TOKEN_CREATE_FAILED: &str = "failed to create host bridge token";
    pub const BRIDGE_TOKEN_WRITE_FAILED: &str = "failed to write host bridge token";
    pub const BRIDGE_TOKEN_INVALID: &str = "host bridge token is invalid";
    pub const BRIDGE_TOKEN_INSECURE: &str =
        "host bridge token must be a regular file accessible only by its owner";
    pub const BRIDGE_NON_LOOPBACK_LISTEN: &str =
        "host bridge may only listen on a loopback address";
    pub const BRIDGE_LISTEN_FAILED: &str = "failed to bind host bridge";
    pub const BRIDGE_SERVE_FAILED: &str = "host bridge server failed";
    pub const BRIDGE_SIGNAL_FAILED: &str = "failed to wait for shutdown signal";
    pub const BRIDGE_CLIENT_FAILED: &str = "host bridge request failed";
    pub const BRIDGE_RESPONSE_INVALID: &str = "host bridge returned an invalid response";
    pub const HOST_EXEC_SPAWN_FAILED: &str = "failed to start authorized host command";
    pub const HOST_EXEC_FAILED: &str = "host command failed";
}
