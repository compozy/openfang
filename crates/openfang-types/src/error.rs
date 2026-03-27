//! Shared error types for the OpenFang system.

use arky_error::ClassifiedError;
use thiserror::Error;

/// Top-level error type for the OpenFang system.
#[derive(Error, Debug)]
pub enum OpenFangError {
    /// The requested agent was not found.
    #[error("Agent not found: {0}")]
    AgentNotFound(String),

    /// An agent with this name or ID already exists.
    #[error("Agent already exists: {0}")]
    AgentAlreadyExists(String),

    /// A capability check failed.
    #[error("Capability denied: {0}")]
    CapabilityDenied(String),

    /// A resource quota was exceeded.
    #[error("Resource quota exceeded: {0}")]
    QuotaExceeded(String),

    /// The agent is in an invalid state for the requested operation.
    #[error("Agent is in invalid state '{current}' for operation '{operation}'")]
    InvalidState {
        /// The current state of the agent.
        current: String,
        /// The operation that was attempted.
        operation: String,
    },

    /// The requested session was not found.
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    /// A memory substrate error occurred.
    #[error("Memory error: {0}")]
    Memory(String),

    /// A tool execution failed.
    #[error("Tool execution failed: {tool_id} — {reason}")]
    ToolExecution {
        /// The tool that failed.
        tool_id: String,
        /// Why it failed.
        reason: String,
    },

    /// An LLM driver error occurred.
    #[error("LLM driver error: {0}")]
    LlmDriver(String),

    /// A configuration error occurred.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Failed to parse an agent manifest.
    #[error("Manifest parsing error: {0}")]
    ManifestParse(String),

    /// A WASM sandbox error occurred.
    #[error("WASM sandbox error: {0}")]
    Sandbox(String),

    /// A network error occurred.
    #[error("Network error: {0}")]
    Network(String),

    /// A serialization/deserialization error occurred.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// The agent loop exceeded the maximum iteration count.
    #[error("Max iterations exceeded ({0}). Configure a higher limit in agent.toml under [autonomous] max_iterations")]
    MaxIterationsExceeded(u32),

    /// The kernel is shutting down.
    #[error("Shutdown in progress")]
    ShuttingDown,

    /// An I/O error occurred.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// An internal error occurred.
    #[error("Internal error: {0}")]
    Internal(String),

    /// Authentication/authorization denied.
    #[error("Auth denied: {0}")]
    AuthDenied(String),

    /// Metering/cost tracking error.
    #[error("Metering error: {0}")]
    MeteringError(String),

    /// Invalid user input.
    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

impl ClassifiedError for OpenFangError {
    fn error_code(&self) -> &'static str {
        match self {
            Self::AgentNotFound(_) => "AGENT_NOT_FOUND",
            Self::AgentAlreadyExists(_) => "AGENT_ALREADY_EXISTS",
            Self::CapabilityDenied(_) => "CAPABILITY_DENIED",
            Self::QuotaExceeded(_) => "QUOTA_EXCEEDED",
            Self::InvalidState { .. } => "INVALID_STATE",
            Self::SessionNotFound(_) => "SESSION_NOT_FOUND",
            Self::Memory(_) => "MEMORY_ERROR",
            Self::ToolExecution { .. } => "TOOL_EXECUTION_FAILED",
            Self::LlmDriver(_) => "LLM_DRIVER_ERROR",
            Self::Config(_) => "CONFIG_ERROR",
            Self::ManifestParse(_) => "MANIFEST_PARSE_ERROR",
            Self::Sandbox(_) => "SANDBOX_ERROR",
            Self::Network(_) => "NETWORK_ERROR",
            Self::Serialization(_) => "SERIALIZATION_ERROR",
            Self::MaxIterationsExceeded(_) => "MAX_ITERATIONS_EXCEEDED",
            Self::ShuttingDown => "SHUTTING_DOWN",
            Self::Io(_) => "IO_ERROR",
            Self::Internal(_) => "INTERNAL_ERROR",
            Self::AuthDenied(_) => "AUTH_DENIED",
            Self::MeteringError(_) => "METERING_ERROR",
            Self::InvalidInput(_) => "INVALID_INPUT",
        }
    }

    fn is_retryable(&self) -> bool {
        matches!(self, Self::Network(_) | Self::LlmDriver(_))
    }

    fn http_status(&self) -> u16 {
        match self {
            Self::AgentNotFound(_) | Self::SessionNotFound(_) => 404,
            Self::AgentAlreadyExists(_) | Self::InvalidState { .. } => 409,
            Self::CapabilityDenied(_) | Self::AuthDenied(_) => 403,
            Self::QuotaExceeded(_) | Self::MaxIterationsExceeded(_) => 429,
            Self::LlmDriver(_) | Self::Network(_) => 502,
            Self::Config(_)
            | Self::ManifestParse(_)
            | Self::Serialization(_)
            | Self::InvalidInput(_) => 400,
            Self::ShuttingDown => 503,
            Self::Memory(_)
            | Self::ToolExecution { .. }
            | Self::Sandbox(_)
            | Self::Io(_)
            | Self::Internal(_)
            | Self::MeteringError(_) => 500,
        }
    }

    fn correction_context(&self) -> Option<serde_json::Value> {
        None
    }
}

/// Alias for Result with OpenFangError.
pub type OpenFangResult<T> = Result<T, OpenFangError>;

#[cfg(test)]
mod tests {
    use arky_error::ClassifiedError;
    use pretty_assertions::assert_eq;

    use super::OpenFangError;

    fn classification_cases() -> Vec<(OpenFangError, &'static str, bool, u16)> {
        vec![
            (
                OpenFangError::AgentNotFound("agent-1".to_string()),
                "AGENT_NOT_FOUND",
                false,
                404,
            ),
            (
                OpenFangError::AgentAlreadyExists("agent-1".to_string()),
                "AGENT_ALREADY_EXISTS",
                false,
                409,
            ),
            (
                OpenFangError::CapabilityDenied("shell_exec".to_string()),
                "CAPABILITY_DENIED",
                false,
                403,
            ),
            (
                OpenFangError::QuotaExceeded("daily budget".to_string()),
                "QUOTA_EXCEEDED",
                false,
                429,
            ),
            (
                OpenFangError::InvalidState {
                    current: "idle".to_string(),
                    operation: "resume".to_string(),
                },
                "INVALID_STATE",
                false,
                409,
            ),
            (
                OpenFangError::SessionNotFound("session-1".to_string()),
                "SESSION_NOT_FOUND",
                false,
                404,
            ),
            (
                OpenFangError::Memory("sqlite unavailable".to_string()),
                "MEMORY_ERROR",
                false,
                500,
            ),
            (
                OpenFangError::ToolExecution {
                    tool_id: "web.search".to_string(),
                    reason: "timeout".to_string(),
                },
                "TOOL_EXECUTION_FAILED",
                false,
                500,
            ),
            (
                OpenFangError::LlmDriver("provider unavailable".to_string()),
                "LLM_DRIVER_ERROR",
                true,
                502,
            ),
            (
                OpenFangError::Config("missing api key".to_string()),
                "CONFIG_ERROR",
                false,
                400,
            ),
            (
                OpenFangError::ManifestParse("invalid toml".to_string()),
                "MANIFEST_PARSE_ERROR",
                false,
                400,
            ),
            (
                OpenFangError::Sandbox("process crashed".to_string()),
                "SANDBOX_ERROR",
                false,
                500,
            ),
            (
                OpenFangError::Network("dns failed".to_string()),
                "NETWORK_ERROR",
                true,
                502,
            ),
            (
                OpenFangError::Serialization("bad payload".to_string()),
                "SERIALIZATION_ERROR",
                false,
                400,
            ),
            (
                OpenFangError::MaxIterationsExceeded(16),
                "MAX_ITERATIONS_EXCEEDED",
                false,
                429,
            ),
            (OpenFangError::ShuttingDown, "SHUTTING_DOWN", false, 503),
            (
                OpenFangError::Io(std::io::Error::other("disk full")),
                "IO_ERROR",
                false,
                500,
            ),
            (
                OpenFangError::Internal("unexpected state".to_string()),
                "INTERNAL_ERROR",
                false,
                500,
            ),
            (
                OpenFangError::AuthDenied("token rejected".to_string()),
                "AUTH_DENIED",
                false,
                403,
            ),
            (
                OpenFangError::MeteringError("budget store unavailable".to_string()),
                "METERING_ERROR",
                false,
                500,
            ),
            (
                OpenFangError::InvalidInput("empty prompt".to_string()),
                "INVALID_INPUT",
                false,
                400,
            ),
        ]
    }

    #[test]
    fn openfang_error_error_codes_should_match_contract() {
        let cases = classification_cases();
        assert_eq!(cases.len(), 21);

        for (error, expected_code, _, _) in cases {
            assert_eq!(error.error_code(), expected_code);
        }
    }

    #[test]
    fn openfang_error_http_statuses_should_match_contract() {
        let cases = classification_cases();
        assert_eq!(cases.len(), 21);

        for (error, _, _, expected_status) in cases {
            assert_eq!(error.http_status(), expected_status);
        }
    }

    #[test]
    fn openfang_error_retryability_should_match_contract() {
        let cases = classification_cases();
        assert_eq!(cases.len(), 21);

        for (error, _, expected_retryable, _) in cases {
            assert_eq!(error.is_retryable(), expected_retryable);
        }
    }
}
