//! Error types for tool registration, name translation, and execution.

use std::time::Duration;

use arky_error::ClassifiedError;
use serde_json::{Value, json};
use thiserror::Error;

/// Errors produced by the tool system.
#[derive(Debug, Error)]
pub enum ToolError {
    /// Tool input or naming data failed validation.
    #[error("{message}")]
    InvalidArgs {
        /// Human-readable validation failure.
        message: String,
        /// Structured details for recovery or logging.
        details: Option<Value>,
    },
    /// Tool execution failed after validation succeeded.
    #[error("{message}")]
    ExecutionFailed {
        /// Human-readable execution failure.
        message: String,
        /// Canonical tool name when known.
        canonical_name: Option<String>,
    },
    /// Tool execution exceeded a timeout budget.
    #[error("{message}")]
    Timeout {
        /// Human-readable timeout description.
        message: String,
        /// Canonical tool name when known.
        canonical_name: Option<String>,
        /// Requested timeout duration, when known.
        duration: Option<Duration>,
    },
    /// Tool execution was cancelled cooperatively.
    #[error("{message}")]
    Cancelled {
        /// Human-readable cancellation description.
        message: String,
        /// Canonical tool name when known.
        canonical_name: Option<String>,
    },
    /// A canonical tool name collided with an existing registration.
    #[error("tool name collision: {canonical_name}")]
    NameCollision {
        /// Canonical tool name that was already registered.
        canonical_name: String,
    },
}

impl ToolError {
    /// Creates an invalid-arguments error.
    #[must_use]
    pub fn invalid_args(message: impl Into<String>, details: Option<Value>) -> Self {
        Self::InvalidArgs {
            message: message.into(),
            details,
        }
    }

    /// Creates an execution-failed error.
    #[must_use]
    pub fn execution_failed(message: impl Into<String>, canonical_name: Option<String>) -> Self {
        Self::ExecutionFailed {
            message: message.into(),
            canonical_name,
        }
    }

    /// Creates a timeout error.
    #[must_use]
    pub fn timeout(
        message: impl Into<String>,
        canonical_name: Option<String>,
        duration: Option<Duration>,
    ) -> Self {
        Self::Timeout {
            message: message.into(),
            canonical_name,
            duration,
        }
    }

    /// Creates a cancelled error.
    #[must_use]
    pub fn cancelled(message: impl Into<String>, canonical_name: Option<String>) -> Self {
        Self::Cancelled {
            message: message.into(),
            canonical_name,
        }
    }

    /// Creates a name-collision error.
    #[must_use]
    pub fn name_collision(canonical_name: impl Into<String>) -> Self {
        Self::NameCollision {
            canonical_name: canonical_name.into(),
        }
    }
}

impl ClassifiedError for ToolError {
    fn error_code(&self) -> &'static str {
        match self {
            Self::InvalidArgs { .. } => "TOOL_INVALID_ARGS",
            Self::ExecutionFailed { .. } => "TOOL_EXECUTION_FAILED",
            Self::Timeout { .. } => "TOOL_TIMEOUT",
            Self::Cancelled { .. } => "TOOL_CANCELLED",
            Self::NameCollision { .. } => "TOOL_NAME_COLLISION",
        }
    }

    fn is_retryable(&self) -> bool {
        matches!(self, Self::Timeout { .. })
    }

    fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Timeout { duration, .. } => *duration,
            _ => None,
        }
    }

    fn http_status(&self) -> u16 {
        match self {
            Self::InvalidArgs { .. } => 400,
            Self::ExecutionFailed { .. } => 500,
            Self::Timeout { .. } => 504,
            Self::Cancelled { .. } => 499,
            Self::NameCollision { .. } => 409,
        }
    }

    fn correction_context(&self) -> Option<Value> {
        Some(match self {
            Self::InvalidArgs { details, .. } => json!({
                "details": details,
            }),
            Self::ExecutionFailed {
                canonical_name,
                message,
            }
            | Self::Cancelled {
                canonical_name,
                message,
            } => json!({
                "canonical_name": canonical_name,
                "message": message,
            }),
            Self::Timeout {
                canonical_name,
                duration,
                message,
            } => json!({
                "canonical_name": canonical_name,
                "duration_ms": duration.map(|value| value.as_millis()),
                "message": message,
            }),
            Self::NameCollision { canonical_name } => json!({
                "canonical_name": canonical_name,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use arky_error::ClassifiedError;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use crate::ToolError;

    #[test]
    fn tool_error_classification_should_match_expected_metadata() {
        let cases = vec![
            (
                ToolError::invalid_args("invalid schema", Some(json!({ "field": "input.path" }))),
                "TOOL_INVALID_ARGS",
                false,
                400,
            ),
            (
                ToolError::execution_failed(
                    "process crashed",
                    Some("mcp/local/read_file".to_owned()),
                ),
                "TOOL_EXECUTION_FAILED",
                false,
                500,
            ),
            (
                ToolError::timeout(
                    "tool timed out",
                    Some("mcp/local/read_file".to_owned()),
                    Some(Duration::from_secs(3)),
                ),
                "TOOL_TIMEOUT",
                true,
                504,
            ),
            (
                ToolError::cancelled("tool cancelled", Some("mcp/local/read_file".to_owned())),
                "TOOL_CANCELLED",
                false,
                499,
            ),
            (
                ToolError::name_collision("mcp/local/read_file"),
                "TOOL_NAME_COLLISION",
                false,
                409,
            ),
        ];

        for (error, expected_code, expected_retryable, expected_status) in cases {
            let actual = (
                error.error_code(),
                error.is_retryable(),
                error.http_status(),
                error.correction_context().is_some(),
            );

            let expected = (expected_code, expected_retryable, expected_status, true);

            assert_eq!(actual, expected);
        }
    }
}
