//! Provider error classification and helpers.

use std::time::Duration;

use arky_error::ClassifiedError;
use arky_protocol::ProviderId;
use serde_json::{Value, json};
use thiserror::Error;

/// Errors shared by the provider contract and subprocess infrastructure.
#[derive(Debug, Clone, Error)]
pub enum ProviderError {
    /// A provider identifier was not found in the registry.
    #[error("provider `{provider_id}` is not registered")]
    NotFound {
        /// Missing provider identifier.
        provider_id: ProviderId,
    },
    /// A required CLI binary is missing from the system.
    #[error("required binary `{binary}` was not found")]
    BinaryNotFound {
        /// Missing binary name or path.
        binary: String,
    },
    /// A managed process exited unexpectedly or could not be spawned.
    #[error("process `{command}` crashed")]
    ProcessCrashed {
        /// Human-readable command label.
        command: String,
        /// Exit code when the process exited normally.
        exit_code: Option<i32>,
        /// Stderr excerpt or process failure context.
        stderr: Option<String>,
    },
    /// A transport stream was interrupted or cancelled.
    #[error("{message}")]
    StreamInterrupted {
        /// Human-readable interruption detail.
        message: String,
    },
    /// The provider emitted invalid or out-of-contract data.
    #[error("{message}")]
    ProtocolViolation {
        /// Human-readable protocol failure detail.
        message: String,
        /// Optional structured protocol context.
        details: Option<Value>,
    },
    /// Authentication failed against the underlying provider.
    #[error("{message}")]
    AuthFailed {
        /// Human-readable authentication detail.
        message: String,
    },
    /// The provider rejected the request due to rate limiting.
    #[error("{message}")]
    RateLimited {
        /// Human-readable rate limit detail.
        message: String,
        /// Suggested retry delay.
        retry_after: Option<Duration>,
    },
}

impl ProviderError {
    /// Creates a not-found error.
    #[must_use]
    pub const fn not_found(provider_id: ProviderId) -> Self {
        Self::NotFound { provider_id }
    }

    /// Creates a binary-not-found error.
    #[must_use]
    pub fn binary_not_found(binary: impl Into<String>) -> Self {
        Self::BinaryNotFound {
            binary: binary.into(),
        }
    }

    /// Creates a process-crashed error.
    #[must_use]
    pub fn process_crashed(
        command: impl Into<String>,
        exit_code: Option<i32>,
        stderr: Option<String>,
    ) -> Self {
        Self::ProcessCrashed {
            command: command.into(),
            exit_code,
            stderr,
        }
    }

    /// Creates a stream-interrupted error.
    #[must_use]
    pub fn stream_interrupted(message: impl Into<String>) -> Self {
        Self::StreamInterrupted {
            message: message.into(),
        }
    }

    /// Creates a protocol-violation error.
    #[must_use]
    pub fn protocol_violation(message: impl Into<String>, details: Option<Value>) -> Self {
        Self::ProtocolViolation {
            message: message.into(),
            details,
        }
    }

    /// Creates an authentication failure.
    #[must_use]
    pub fn auth_failed(message: impl Into<String>) -> Self {
        Self::AuthFailed {
            message: message.into(),
        }
    }

    /// Creates a rate-limited error.
    #[must_use]
    pub fn rate_limited(message: impl Into<String>, retry_after: Option<Duration>) -> Self {
        Self::RateLimited {
            message: message.into(),
            retry_after,
        }
    }
}

impl ClassifiedError for ProviderError {
    fn error_code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "PROVIDER_NOT_FOUND",
            Self::BinaryNotFound { .. } => "PROVIDER_BINARY_NOT_FOUND",
            Self::ProcessCrashed { .. } => "PROVIDER_PROCESS_CRASHED",
            Self::StreamInterrupted { .. } => "PROVIDER_STREAM_INTERRUPTED",
            Self::ProtocolViolation { .. } => "PROVIDER_PROTOCOL_VIOLATION",
            Self::AuthFailed { .. } => "PROVIDER_AUTH_FAILED",
            Self::RateLimited { .. } => "PROVIDER_RATE_LIMITED",
        }
    }

    fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::ProcessCrashed { .. } | Self::StreamInterrupted { .. } | Self::RateLimited { .. }
        )
    }

    fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    fn http_status(&self) -> u16 {
        match self {
            Self::NotFound { .. } => 404,
            Self::BinaryNotFound { .. } => 424,
            Self::ProcessCrashed { .. } | Self::ProtocolViolation { .. } => 502,
            Self::StreamInterrupted { .. } => 503,
            Self::AuthFailed { .. } => 401,
            Self::RateLimited { .. } => 429,
        }
    }

    fn correction_context(&self) -> Option<Value> {
        match self {
            Self::NotFound { provider_id } => Some(json!({
                "provider_id": provider_id.as_str(),
            })),
            Self::BinaryNotFound { binary } => Some(json!({
                "binary": binary,
            })),
            Self::ProcessCrashed {
                command,
                exit_code,
                stderr,
            } => Some(json!({
                "command": command,
                "exit_code": exit_code,
                "stderr": stderr,
            })),
            Self::ProtocolViolation { details, .. } => details.clone(),
            Self::RateLimited { retry_after, .. } => Some(json!({
                "retry_after_ms": retry_after.map(|duration| duration.as_millis()),
            })),
            Self::StreamInterrupted { .. } | Self::AuthFailed { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use arky_error::ClassifiedError;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::ProviderError;
    use arky_protocol::ProviderId;

    #[test]
    fn provider_error_classification_should_match_contract() {
        let cases = vec![
            (
                ProviderError::not_found(ProviderId::new("codex")),
                "PROVIDER_NOT_FOUND",
                false,
                None,
                404,
            ),
            (
                ProviderError::binary_not_found("codex"),
                "PROVIDER_BINARY_NOT_FOUND",
                false,
                None,
                424,
            ),
            (
                ProviderError::process_crashed("codex", Some(9), Some("killed".to_owned())),
                "PROVIDER_PROCESS_CRASHED",
                true,
                None,
                502,
            ),
            (
                ProviderError::stream_interrupted("cancelled"),
                "PROVIDER_STREAM_INTERRUPTED",
                true,
                None,
                503,
            ),
            (
                ProviderError::protocol_violation(
                    "invalid rpc envelope",
                    Some(json!({ "field": "id" })),
                ),
                "PROVIDER_PROTOCOL_VIOLATION",
                false,
                None,
                502,
            ),
            (
                ProviderError::auth_failed("invalid api key"),
                "PROVIDER_AUTH_FAILED",
                false,
                None,
                401,
            ),
            (
                ProviderError::rate_limited("too many requests", Some(Duration::from_secs(3))),
                "PROVIDER_RATE_LIMITED",
                true,
                Some(Duration::from_secs(3)),
                429,
            ),
        ];

        for (error, code, retryable, retry_after, http_status) in cases {
            assert_eq!(error.error_code(), code);
            assert_eq!(error.is_retryable(), retryable);
            assert_eq!(error.retry_after(), retry_after);
            assert_eq!(error.http_status(), http_status);
        }
    }
}
