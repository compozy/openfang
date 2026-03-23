//! Shared error contracts for Arky.
//!
//! This crate stays at the bottom of the internal dependency graph so every
//! other crate can implement common error-classification behavior without
//! introducing cycles through `arky-core`.

mod classifier;
mod union;

use std::time::Duration;

use serde_json::Value;

pub use crate::{
    classifier::{ClassifiedResult, ErrorCategory, ErrorClassifier, ErrorInput, ErrorPattern},
    union::{RuntimeError, RuntimeErrorData, RuntimeErrorKind},
};

/// Classification metadata shared by all Arky library errors.
pub trait ClassifiedError: std::error::Error + Send + Sync {
    /// Stable machine-readable error code.
    fn error_code(&self) -> &'static str;

    /// Whether the operation can be retried safely.
    fn is_retryable(&self) -> bool {
        false
    }

    /// Recommended delay before retrying, when applicable.
    fn retry_after(&self) -> Option<Duration> {
        None
    }

    /// HTTP-style status code for server and transport mappings.
    fn http_status(&self) -> u16 {
        500
    }

    /// Optional structured data that can help higher-level recovery logic.
    fn correction_context(&self) -> Option<Value> {
        None
    }
}

/// Structured fields suitable for emitting into logs or traces.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ErrorLogEntry {
    /// Stable machine-readable error code.
    pub error_code: &'static str,
    /// Human-readable error message.
    pub message: String,
    /// Whether the failed operation can be retried safely.
    pub is_retryable: bool,
    /// Recommended delay before retrying, when applicable.
    pub retry_after: Option<Duration>,
    /// HTTP-style status for transport or API surfaces.
    pub http_status: u16,
    /// Structured data for higher-level recovery logic.
    pub correction_context: Option<Value>,
}

impl ErrorLogEntry {
    /// Creates a structured log entry with explicit values.
    #[must_use]
    pub fn new(
        error_code: &'static str,
        message: impl Into<String>,
        is_retryable: bool,
        retry_after: Option<Duration>,
        http_status: u16,
        correction_context: Option<Value>,
    ) -> Self {
        Self {
            error_code,
            message: message.into(),
            is_retryable,
            retry_after,
            http_status,
            correction_context,
        }
    }

    /// Extracts structured log fields from a classified error.
    #[must_use]
    pub fn from_error<E>(error: &E) -> Self
    where
        E: ClassifiedError + ?Sized,
    {
        Self::new(
            error.error_code(),
            error.to_string(),
            error.is_retryable(),
            error.retry_after(),
            error.http_status(),
            error.correction_context(),
        )
    }
}

/// A normalized HTTP-facing projection of a classified error.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct HttpErrorMapping {
    /// HTTP-style status code.
    pub status: u16,
    /// Stable machine-readable error code.
    pub error_code: &'static str,
    /// Human-readable error message.
    pub message: String,
    /// Suggested retry delay for `Retry-After`-style handling.
    pub retry_after: Option<Duration>,
    /// Structured data for clients that support self-correction.
    pub correction_context: Option<Value>,
}

impl HttpErrorMapping {
    /// Creates an explicit HTTP mapping.
    #[must_use]
    pub fn new(
        status: u16,
        error_code: &'static str,
        message: impl Into<String>,
        retry_after: Option<Duration>,
        correction_context: Option<Value>,
    ) -> Self {
        Self {
            status,
            error_code,
            message: message.into(),
            retry_after,
            correction_context,
        }
    }

    /// Extracts an HTTP-facing mapping from a classified error.
    #[must_use]
    pub fn from_error<E>(error: &E) -> Self
    where
        E: ClassifiedError + ?Sized,
    {
        Self::new(
            error.http_status(),
            error.error_code(),
            error.to_string(),
            error.retry_after(),
            error.correction_context(),
        )
    }
}

/// Converts any classified error into structured log fields.
#[must_use]
pub fn classify_error<E>(error: &E) -> ErrorLogEntry
where
    E: ClassifiedError + ?Sized,
{
    ErrorLogEntry::from_error(error)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use thiserror::Error;

    use super::{ClassifiedError, ErrorLogEntry, HttpErrorMapping, classify_error};

    #[derive(Debug, Error)]
    #[error("default failure for {resource}")]
    struct DefaultError {
        resource: &'static str,
    }

    impl ClassifiedError for DefaultError {
        fn error_code(&self) -> &'static str {
            "ERROR_DEFAULT_FAILURE"
        }
    }

    #[derive(Debug, Error)]
    enum CustomError {
        #[error("provider rate limited")]
        RateLimited,
    }

    impl ClassifiedError for CustomError {
        fn error_code(&self) -> &'static str {
            "PROVIDER_RATE_LIMITED"
        }

        fn is_retryable(&self) -> bool {
            true
        }

        fn retry_after(&self) -> Option<Duration> {
            Some(Duration::from_secs(30))
        }

        fn http_status(&self) -> u16 {
            429
        }

        fn correction_context(&self) -> Option<Value> {
            Some(json!({
                "retry_strategy": "exponential_backoff",
                "safe_to_retry": true,
            }))
        }
    }

    #[test]
    fn classified_error_defaults_should_match_the_techspec() {
        let error = DefaultError {
            resource: "workspace",
        };

        let actual = (
            error.is_retryable(),
            error.retry_after(),
            error.http_status(),
            error.correction_context(),
        );

        let expected = (false, None, 500, None);

        assert_eq!(actual, expected);
    }

    #[test]
    fn classified_error_overrides_should_be_respected() {
        let error = CustomError::RateLimited;

        let actual = (
            error.error_code(),
            error.is_retryable(),
            error.retry_after(),
            error.http_status(),
            error.correction_context(),
        );

        let expected = (
            "PROVIDER_RATE_LIMITED",
            true,
            Some(Duration::from_secs(30)),
            429,
            Some(json!({
                "retry_strategy": "exponential_backoff",
                "safe_to_retry": true,
            })),
        );

        assert_eq!(actual, expected);
    }

    #[test]
    fn error_log_entry_should_capture_structured_fields() {
        let error = CustomError::RateLimited;

        let actual = classify_error(&error);

        let expected = ErrorLogEntry::new(
            "PROVIDER_RATE_LIMITED",
            "provider rate limited",
            true,
            Some(Duration::from_secs(30)),
            429,
            Some(json!({
                "retry_strategy": "exponential_backoff",
                "safe_to_retry": true,
            })),
        );

        assert_eq!(actual, expected);
    }

    #[test]
    fn http_error_mapping_should_capture_http_projection() {
        let error = CustomError::RateLimited;

        let actual = HttpErrorMapping::from_error(&error);

        let expected = HttpErrorMapping::new(
            429,
            "PROVIDER_RATE_LIMITED",
            "provider rate limited",
            Some(Duration::from_secs(30)),
            Some(json!({
                "retry_strategy": "exponential_backoff",
                "safe_to_retry": true,
            })),
        );

        assert_eq!(actual, expected);
    }

    #[test]
    fn thiserror_display_should_surface_the_human_message() {
        let error = DefaultError {
            resource: "workspace",
        };

        assert_eq!(error.to_string(), "default failure for workspace");
    }
}
