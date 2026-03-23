//! Unified runtime-error projection across Arky crates.

use std::{error::Error, fmt, time::Duration};

use serde_json::Value;

use crate::ClassifiedError;

/// Category of classified runtime error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeErrorKind {
    /// Provider layer failures.
    Provider,
    /// Tool execution or registration failures.
    Tool,
    /// Session persistence failures.
    Session,
    /// Hook lifecycle failures.
    Hook,
    /// Configuration loading or validation failures.
    Config,
    /// Server/API surface failures.
    Server,
    /// MCP transport or schema failures.
    Mcp,
    /// Fallback bucket for unrecognized classified errors.
    Unknown,
}

/// Serializable runtime error data extracted from a classified source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeErrorData {
    /// Stable machine-readable error code.
    pub error_code: &'static str,
    /// Human-readable message.
    pub message: String,
    /// Whether the operation can be retried safely.
    pub is_retryable: bool,
    /// Suggested retry delay, when present.
    pub retry_after: Option<Duration>,
    /// HTTP-style status for transport mappings.
    pub http_status: u16,
    /// Structured correction context.
    pub correction_context: Option<Value>,
}

impl RuntimeErrorData {
    /// Extracts runtime error data from any classified error.
    #[must_use]
    pub fn from_classified<E>(error: &E) -> Self
    where
        E: ClassifiedError + ?Sized,
    {
        Self {
            error_code: error.error_code(),
            message: error.to_string(),
            is_retryable: error.is_retryable(),
            retry_after: error.retry_after(),
            http_status: error.http_status(),
            correction_context: error.correction_context(),
        }
    }
}

/// Unified runtime-error projection across Arky crates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    /// Provider layer failure.
    Provider(RuntimeErrorData),
    /// Tool layer failure.
    Tool(RuntimeErrorData),
    /// Session layer failure.
    Session(RuntimeErrorData),
    /// Hook layer failure.
    Hook(RuntimeErrorData),
    /// Configuration layer failure.
    Config(RuntimeErrorData),
    /// Server/API layer failure.
    Server(RuntimeErrorData),
    /// MCP layer failure.
    Mcp(RuntimeErrorData),
    /// Unknown classified error category.
    Unknown(RuntimeErrorData),
}

impl RuntimeError {
    /// Wraps any classified error into the unified runtime-error enum.
    #[must_use]
    pub fn from_classified<E>(error: &E) -> Self
    where
        E: ClassifiedError + ?Sized,
    {
        let data = RuntimeErrorData::from_classified(error);
        match classify_kind(data.error_code) {
            RuntimeErrorKind::Provider => Self::Provider(data),
            RuntimeErrorKind::Tool => Self::Tool(data),
            RuntimeErrorKind::Session => Self::Session(data),
            RuntimeErrorKind::Hook => Self::Hook(data),
            RuntimeErrorKind::Config => Self::Config(data),
            RuntimeErrorKind::Server => Self::Server(data),
            RuntimeErrorKind::Mcp => Self::Mcp(data),
            RuntimeErrorKind::Unknown => Self::Unknown(data),
        }
    }

    /// Returns the normalized kind for this runtime error.
    #[must_use]
    pub const fn kind(&self) -> RuntimeErrorKind {
        match self {
            Self::Provider(_) => RuntimeErrorKind::Provider,
            Self::Tool(_) => RuntimeErrorKind::Tool,
            Self::Session(_) => RuntimeErrorKind::Session,
            Self::Hook(_) => RuntimeErrorKind::Hook,
            Self::Config(_) => RuntimeErrorKind::Config,
            Self::Server(_) => RuntimeErrorKind::Server,
            Self::Mcp(_) => RuntimeErrorKind::Mcp,
            Self::Unknown(_) => RuntimeErrorKind::Unknown,
        }
    }

    /// Returns the wrapped runtime error data.
    #[must_use]
    pub const fn data(&self) -> &RuntimeErrorData {
        match self {
            Self::Provider(data)
            | Self::Tool(data)
            | Self::Session(data)
            | Self::Hook(data)
            | Self::Config(data)
            | Self::Server(data)
            | Self::Mcp(data)
            | Self::Unknown(data) => data,
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.data().message.as_str())
    }
}

impl Error for RuntimeError {}

impl<E> From<&E> for RuntimeError
where
    E: ClassifiedError + ?Sized,
{
    fn from(error: &E) -> Self {
        Self::from_classified(error)
    }
}

fn classify_kind(error_code: &str) -> RuntimeErrorKind {
    if error_code.starts_with("PROVIDER_") {
        return RuntimeErrorKind::Provider;
    }
    if error_code.starts_with("TOOL_") {
        return RuntimeErrorKind::Tool;
    }
    if error_code.starts_with("SESSION_") {
        return RuntimeErrorKind::Session;
    }
    if error_code.starts_with("HOOK_") {
        return RuntimeErrorKind::Hook;
    }
    if error_code.starts_with("CONFIG_") {
        return RuntimeErrorKind::Config;
    }
    if error_code.starts_with("SERVER_") {
        return RuntimeErrorKind::Server;
    }
    if error_code.starts_with("MCP_") {
        return RuntimeErrorKind::Mcp;
    }

    RuntimeErrorKind::Unknown
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pretty_assertions::assert_eq;
    use serde_json::json;
    use thiserror::Error;

    use super::{RuntimeError, RuntimeErrorKind};
    use crate::ClassifiedError;
    use serde_json::Value;

    #[derive(Debug, Error)]
    #[error("provider rate limited")]
    struct FakeProviderError;

    impl ClassifiedError for FakeProviderError {
        fn error_code(&self) -> &'static str {
            "PROVIDER_RATE_LIMITED"
        }

        fn is_retryable(&self) -> bool {
            true
        }

        fn retry_after(&self) -> Option<Duration> {
            Some(Duration::from_secs(2))
        }

        fn http_status(&self) -> u16 {
            429
        }

        fn correction_context(&self) -> Option<Value> {
            Some(json!({ "provider": "codex" }))
        }
    }

    #[test]
    fn runtime_error_should_wrap_provider_shaped_errors() {
        let runtime_error = RuntimeError::from_classified(&FakeProviderError);

        assert_eq!(runtime_error.kind(), RuntimeErrorKind::Provider);
        assert_eq!(runtime_error.data().error_code, "PROVIDER_RATE_LIMITED");
        assert_eq!(runtime_error.data().is_retryable, true);
    }
}
