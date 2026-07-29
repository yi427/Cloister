//! Centralized user-facing error messages.

pub(crate) mod message {
    pub const CLI_ERROR_PREFIX: &str = "error";

    pub const MEMORY_INVALID_FORMAT: &str = "memory must be a positive integer followed by M or G";
    pub const MEMORY_MUST_BE_NON_ZERO: &str = "memory must be greater than zero";
    pub const MEMORY_OVERFLOW: &str = "memory is too large";

    pub const PROFILE_PARSE_FAILED: &str = "failed to parse profile";
    pub const PROFILE_READ_FAILED: &str = "failed to read profile";
    pub const PROFILE_VALIDATION_FAILED: &str = "profile validation failed";

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
}
