//! Error types for session persistence.

use std::time::Duration;

use arky_error::ClassifiedError;
use arky_protocol::SessionId;
use serde_json::{Value, json};
use thiserror::Error;

/// Errors returned by session stores.
#[derive(Debug, Clone, Error)]
pub enum SessionError {
    /// A referenced session does not exist.
    #[error("session `{session_id}` was not found")]
    NotFound {
        /// Missing session identifier.
        session_id: SessionId,
    },
    /// An underlying storage operation failed.
    #[error("{message}")]
    StorageFailure {
        /// Human-readable storage failure message.
        message: String,
        /// Session identifier when the failure was scoped to one session.
        session_id: Option<SessionId>,
        /// Storage operation that failed.
        operation: String,
        /// Whether retrying the operation is safe.
        retryable: bool,
        /// Suggested retry delay for callers, when applicable.
        retry_after: Option<Duration>,
    },
    /// Replay data is unavailable for the requested session.
    #[error("replay is unavailable for session `{session_id}`: {reason}")]
    ReplayUnavailable {
        /// Session identifier lacking replay data.
        session_id: SessionId,
        /// Human-readable explanation of why replay is unavailable.
        reason: String,
    },
    /// The session has expired and can no longer be resumed safely.
    #[error("session `{session_id}` has expired")]
    Expired {
        /// Expired session identifier.
        session_id: SessionId,
        /// Expiration timestamp in milliseconds since the Unix epoch.
        expired_at_ms: Option<u64>,
    },
}

impl SessionError {
    /// Creates a generic storage failure.
    #[must_use]
    pub fn storage_failure(
        message: impl Into<String>,
        session_id: Option<SessionId>,
        operation: impl Into<String>,
    ) -> Self {
        Self::StorageFailure {
            message: message.into(),
            session_id,
            operation: operation.into(),
            retryable: false,
            retry_after: None,
        }
    }

    /// Creates a retryable storage failure.
    #[must_use]
    pub fn retryable_storage_failure(
        message: impl Into<String>,
        session_id: Option<SessionId>,
        operation: impl Into<String>,
        retry_after: Option<Duration>,
    ) -> Self {
        Self::StorageFailure {
            message: message.into(),
            session_id,
            operation: operation.into(),
            retryable: true,
            retry_after,
        }
    }

    /// Creates a replay-unavailable error for a session.
    #[must_use]
    pub fn replay_unavailable(session_id: SessionId, reason: impl Into<String>) -> Self {
        Self::ReplayUnavailable {
            session_id,
            reason: reason.into(),
        }
    }

    /// Creates an expired-session error.
    #[must_use]
    pub const fn expired(session_id: SessionId, expired_at_ms: Option<u64>) -> Self {
        Self::Expired {
            session_id,
            expired_at_ms,
        }
    }

    fn correction_payload(&self) -> Value {
        match self {
            Self::NotFound { session_id } => json!({
                "session_id": session_id,
            }),
            Self::StorageFailure {
                session_id,
                operation,
                retryable,
                retry_after,
                message,
            } => json!({
                "session_id": session_id,
                "operation": operation,
                "retryable": retryable,
                "retry_after_ms": retry_after.map(|value| value.as_millis()),
                "message": message,
            }),
            Self::ReplayUnavailable { session_id, reason } => json!({
                "session_id": session_id,
                "reason": reason,
            }),
            Self::Expired {
                session_id,
                expired_at_ms,
            } => json!({
                "session_id": session_id,
                "expired_at_ms": expired_at_ms,
            }),
        }
    }
}

impl ClassifiedError for SessionError {
    fn error_code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "SESSION_NOT_FOUND",
            Self::StorageFailure { .. } => "SESSION_STORAGE_FAILURE",
            Self::ReplayUnavailable { .. } => "SESSION_REPLAY_UNAVAILABLE",
            Self::Expired { .. } => "SESSION_EXPIRED",
        }
    }

    fn is_retryable(&self) -> bool {
        match self {
            Self::StorageFailure { retryable, .. } => *retryable,
            _ => false,
        }
    }

    fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::StorageFailure { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    fn http_status(&self) -> u16 {
        match self {
            Self::NotFound { .. } => 404,
            Self::StorageFailure { .. } => 500,
            Self::ReplayUnavailable { .. } => 409,
            Self::Expired { .. } => 410,
        }
    }

    fn correction_context(&self) -> Option<Value> {
        Some(self.correction_payload())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use arky_error::ClassifiedError;
    use pretty_assertions::assert_eq;

    use crate::SessionError;

    #[test]
    fn session_error_classification_should_match_expected_metadata() {
        let session_id = arky_protocol::SessionId::new();
        let cases = vec![
            (
                SessionError::NotFound {
                    session_id: session_id.clone(),
                },
                "SESSION_NOT_FOUND",
                false,
                404,
            ),
            (
                SessionError::storage_failure(
                    "disk write failed",
                    Some(session_id.clone()),
                    "append_messages",
                ),
                "SESSION_STORAGE_FAILURE",
                false,
                500,
            ),
            (
                SessionError::retryable_storage_failure(
                    "database busy",
                    Some(session_id.clone()),
                    "append_events",
                    Some(Duration::from_millis(100)),
                ),
                "SESSION_STORAGE_FAILURE",
                true,
                500,
            ),
            (
                SessionError::replay_unavailable(
                    session_id.clone(),
                    "in-memory replay persistence disabled",
                ),
                "SESSION_REPLAY_UNAVAILABLE",
                false,
                409,
            ),
            (
                SessionError::expired(session_id, Some(5_000)),
                "SESSION_EXPIRED",
                false,
                410,
            ),
        ];

        for (error, expected_code, expected_retryable, expected_status) in cases {
            assert_eq!(error.error_code(), expected_code);
            assert_eq!(error.is_retryable(), expected_retryable);
            assert_eq!(error.http_status(), expected_status);
        }
    }
}
