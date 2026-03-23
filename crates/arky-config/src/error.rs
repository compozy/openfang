//! Error types for configuration loading and validation.

use std::path::PathBuf;

use arky_error::ClassifiedError;
use serde_json::{json, Value};
use thiserror::Error;

/// A machine-readable validation issue tied to a specific config field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    field: String,
    message: String,
    suggestion: Option<String>,
}

impl ValidationIssue {
    /// Creates a validation issue.
    #[must_use]
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            suggestion: None,
        }
    }

    /// Attaches a correction suggestion to the issue.
    #[must_use]
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    /// Returns the field path that failed validation.
    #[must_use]
    pub const fn field(&self) -> &str {
        self.field.as_str()
    }

    /// Returns the human-readable validation message.
    #[must_use]
    pub const fn message(&self) -> &str {
        self.message.as_str()
    }

    /// Returns a suggested correction when one exists.
    #[must_use]
    pub fn suggestion(&self) -> Option<&str> {
        self.suggestion.as_deref()
    }
}

/// Errors produced while loading or validating Arky configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// A config document could not be parsed or decoded.
    #[error("{message}")]
    ParseFailed {
        /// Human-readable parse failure message.
        message: String,
        /// Optional source path for file-backed parsing.
        path: Option<PathBuf>,
        /// Optional document source or format label.
        format: Option<&'static str>,
    },
    /// A decoded config document failed semantic validation.
    #[error("{message}")]
    ValidationFailed {
        /// Human-readable validation summary.
        message: String,
        /// Individual validation issues.
        issues: Vec<ValidationIssue>,
    },
    /// A requested config file could not be found.
    #[error("configuration file not found: {path}")]
    NotFound {
        /// Missing config file path.
        path: PathBuf,
    },
    /// A required provider binary could not be found on disk or on `PATH`.
    #[error("missing binary `{binary}` for provider `{provider}`")]
    MissingBinary {
        /// Provider entry name.
        provider: String,
        /// Binary name or path that was searched.
        binary: String,
    },
}

impl ConfigError {
    pub(crate) fn parse(
        message: impl Into<String>,
        path: Option<PathBuf>,
        format: Option<&'static str>,
    ) -> Self {
        Self::ParseFailed {
            message: message.into(),
            path,
            format,
        }
    }

    pub(crate) fn validation(issues: Vec<ValidationIssue>) -> Self {
        let message = if issues.len() == 1 {
            "configuration validation failed with 1 issue".to_owned()
        } else {
            format!(
                "configuration validation failed with {} issues",
                issues.len()
            )
        };

        Self::ValidationFailed { message, issues }
    }
}

impl ClassifiedError for ConfigError {
    fn error_code(&self) -> &'static str {
        match self {
            Self::ParseFailed { .. } => "CONFIG_PARSE_FAILED",
            Self::ValidationFailed { .. } => "CONFIG_VALIDATION_FAILED",
            Self::NotFound { .. } => "CONFIG_NOT_FOUND",
            Self::MissingBinary { .. } => "CONFIG_MISSING_BINARY",
        }
    }

    fn http_status(&self) -> u16 {
        match self {
            Self::ParseFailed { .. } => 400,
            Self::ValidationFailed { .. } => 422,
            Self::NotFound { .. } => 404,
            Self::MissingBinary { .. } => 424,
        }
    }

    fn correction_context(&self) -> Option<Value> {
        Some(match self {
            Self::ParseFailed {
                path,
                format,
                message,
            } => json!({
                "path": path.as_ref().map(|value| value.to_string_lossy().into_owned()),
                "format": format,
                "message": message,
            }),
            Self::ValidationFailed { issues, .. } => json!({
                "issues": issues.iter().map(|issue| {
                    json!({
                        "field": issue.field(),
                        "message": issue.message(),
                        "suggestion": issue.suggestion(),
                    })
                }).collect::<Vec<_>>(),
            }),
            Self::NotFound { path } => json!({
                "path": path.to_string_lossy(),
            }),
            Self::MissingBinary { provider, binary } => json!({
                "provider": provider,
                "binary": binary,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;

    use super::{ConfigError, ValidationIssue};
    use arky_error::ClassifiedError;

    #[test]
    fn config_error_classification_should_match_expected_metadata() {
        let cases = vec![
            (
                ConfigError::parse(
                    "invalid toml",
                    Some(PathBuf::from("arky.toml")),
                    Some("toml"),
                ),
                "CONFIG_PARSE_FAILED",
                false,
                400,
            ),
            (
                ConfigError::validation(vec![ValidationIssue::new(
                    "providers.default.kind",
                    "is required",
                )]),
                "CONFIG_VALIDATION_FAILED",
                false,
                422,
            ),
            (
                ConfigError::NotFound {
                    path: PathBuf::from("missing.toml"),
                },
                "CONFIG_NOT_FOUND",
                false,
                404,
            ),
            (
                ConfigError::MissingBinary {
                    provider: "default".to_owned(),
                    binary: "claude".to_owned(),
                },
                "CONFIG_MISSING_BINARY",
                false,
                424,
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
