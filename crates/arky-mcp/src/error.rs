//! Error types for MCP connectivity, translation, and runtime behavior.

use std::time::Duration;

use arky_error::ClassifiedError;
use serde_json::{Value, json};
use thiserror::Error;

/// Errors produced by the MCP integration layer.
#[derive(Debug, Error)]
pub enum McpError {
    /// The client could not establish or maintain a transport connection.
    #[error("{message}")]
    ConnectionFailed {
        /// Human-readable failure description.
        message: String,
        /// Optional logical server name.
        server_name: Option<String>,
        /// Structured details for troubleshooting.
        details: Option<Value>,
    },
    /// The remote peer or rmcp transport violated protocol expectations.
    #[error("{message}")]
    ProtocolError {
        /// Human-readable failure description.
        message: String,
        /// Structured details for troubleshooting.
        details: Option<Value>,
    },
    /// HTTP authentication or OAuth authorization failed.
    #[error("{message}")]
    AuthFailed {
        /// Human-readable failure description.
        message: String,
        /// Structured details for troubleshooting.
        details: Option<Value>,
    },
    /// A connected MCP server crashed or exited unexpectedly.
    #[error("{message}")]
    ServerCrashed {
        /// Human-readable failure description.
        message: String,
        /// Optional logical server name.
        server_name: Option<String>,
        /// Structured details for troubleshooting.
        details: Option<Value>,
    },
    /// Schema translation between Arky and MCP failed.
    #[error("{message}")]
    SchemaMismatch {
        /// Human-readable failure description.
        message: String,
        /// Structured details for troubleshooting.
        details: Option<Value>,
    },
}

impl McpError {
    /// Creates a connection-failed error.
    #[must_use]
    pub fn connection_failed(
        message: impl Into<String>,
        server_name: Option<String>,
        details: Option<Value>,
    ) -> Self {
        Self::ConnectionFailed {
            message: message.into(),
            server_name,
            details,
        }
    }

    /// Creates a protocol error.
    #[must_use]
    pub fn protocol_error(message: impl Into<String>, details: Option<Value>) -> Self {
        Self::ProtocolError {
            message: message.into(),
            details,
        }
    }

    /// Creates an auth-failed error.
    #[must_use]
    pub fn auth_failed(message: impl Into<String>, details: Option<Value>) -> Self {
        Self::AuthFailed {
            message: message.into(),
            details,
        }
    }

    /// Creates a server-crashed error.
    #[must_use]
    pub fn server_crashed(
        message: impl Into<String>,
        server_name: Option<String>,
        details: Option<Value>,
    ) -> Self {
        Self::ServerCrashed {
            message: message.into(),
            server_name,
            details,
        }
    }

    /// Creates a schema-mismatch error.
    #[must_use]
    pub fn schema_mismatch(message: impl Into<String>, details: Option<Value>) -> Self {
        Self::SchemaMismatch {
            message: message.into(),
            details,
        }
    }
}

impl ClassifiedError for McpError {
    fn error_code(&self) -> &'static str {
        match self {
            Self::ConnectionFailed { .. } => "MCP_CONNECTION_FAILED",
            Self::ProtocolError { .. } => "MCP_PROTOCOL_ERROR",
            Self::AuthFailed { .. } => "MCP_AUTH_FAILED",
            Self::ServerCrashed { .. } => "MCP_SERVER_CRASHED",
            Self::SchemaMismatch { .. } => "MCP_SCHEMA_MISMATCH",
        }
    }

    fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::ConnectionFailed { .. } | Self::ServerCrashed { .. }
        )
    }

    fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::ConnectionFailed { .. } | Self::ServerCrashed { .. } => {
                Some(Duration::from_secs(1))
            }
            _ => None,
        }
    }

    fn http_status(&self) -> u16 {
        match self {
            Self::ConnectionFailed { .. } => 503,
            Self::ProtocolError { .. } | Self::ServerCrashed { .. } => 502,
            Self::AuthFailed { .. } => 401,
            Self::SchemaMismatch { .. } => 400,
        }
    }

    fn correction_context(&self) -> Option<Value> {
        Some(match self {
            Self::ConnectionFailed {
                message,
                server_name,
                details,
            }
            | Self::ServerCrashed {
                message,
                server_name,
                details,
            } => json!({
                "message": message,
                "server_name": server_name,
                "details": details,
            }),
            Self::ProtocolError { message, details }
            | Self::AuthFailed { message, details }
            | Self::SchemaMismatch { message, details } => json!({
                "message": message,
                "details": details,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use arky_error::ClassifiedError;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use crate::McpError;

    #[test]
    fn mcp_error_classification_should_match_expected_metadata() {
        let cases = vec![
            (
                McpError::connection_failed("dial failed", Some("filesystem".to_owned()), None),
                "MCP_CONNECTION_FAILED",
                true,
                503,
            ),
            (
                McpError::protocol_error(
                    "invalid json-rpc payload",
                    Some(json!({ "phase": "initialize" })),
                ),
                "MCP_PROTOCOL_ERROR",
                false,
                502,
            ),
            (
                McpError::auth_failed("unauthorized", None),
                "MCP_AUTH_FAILED",
                false,
                401,
            ),
            (
                McpError::server_crashed(
                    "child exited with signal",
                    Some("filesystem".to_owned()),
                    None,
                ),
                "MCP_SERVER_CRASHED",
                true,
                502,
            ),
            (
                McpError::schema_mismatch(
                    "tool schema must be a JSON object",
                    Some(json!({ "tool": "read_file" })),
                ),
                "MCP_SCHEMA_MISMATCH",
                false,
                400,
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
