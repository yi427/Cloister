//! Centralized user-facing error messages.

pub(crate) mod message {
    pub const CLI_ERROR_PREFIX: &str = "error";

    pub const MEMORY_INVALID_FORMAT: &str = "memory must be a positive integer followed by M or G";
    pub const MEMORY_MUST_BE_NON_ZERO: &str = "memory must be greater than zero";
    pub const MEMORY_OVERFLOW: &str = "memory is too large";

    pub const PROFILE_PARSE_FAILED: &str = "failed to parse profile";
    pub const PROFILE_PATH_RESOLUTION_FAILED: &str = "failed to resolve profile path";
    pub const PROFILE_READ_FAILED: &str = "failed to read profile";
    pub const PROFILE_VALIDATION_FAILED: &str = "profile validation failed";
    pub const WORKSPACE_PATH_RESOLUTION_FAILED: &str = "failed to resolve workspace path";
    pub const WORKSPACE_PATH_NOT_DIRECTORY: &str = "workspace path is not a directory";

    pub const UNSUPPORTED_SCHEMA_VERSION: &str = "schema_version is not supported";
    pub const VALUE_MUST_NOT_BE_BLANK: &str = "must not be blank";
    pub const PATH_MUST_NOT_BE_EMPTY: &str = "must not be empty";
    pub const PATH_MUST_BE_ABSOLUTE: &str = "must be absolute";
    pub const PATH_MUST_NOT_BE_ROOT: &str = "must not be the filesystem root";
    pub const PATH_MUST_NOT_CONTAIN_PARENT: &str = "must not contain '..'";
    pub const WORKSPACE_COPY_NOT_IMPLEMENTED: &str = "copy workspace mode is not implemented";
    pub const NETWORK_RESTRICTED_NOT_IMPLEMENTED: &str =
        "restricted network mode is not implemented";
    pub const AT_LEAST_ONE_AGENT_REQUIRED: &str = "at least one coding agent must be enabled";
    pub const CONTAINER_EXECUTION_NOT_IMPLEMENTED: &str =
        "container execution is not implemented; pass --dry-run to inspect the plan";
    pub const MOUNT_PATH_CONTAINS_SEPARATOR: &str = "workspace mount paths must not contain ','";

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
    pub const BRIDGE_REQUEST_TIMED_OUT: &str = "host bridge request timed out";
    pub const HOST_EXEC_SPAWN_FAILED: &str = "failed to start the host shell";
    pub const HOST_EXEC_FAILED: &str = "host command failed";
}
