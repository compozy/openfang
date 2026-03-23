//! Error types for hook execution, parsing, and isolation.

use std::time::Duration;

use arky_error::ClassifiedError;
use serde_json::{Value, json};
use thiserror::Error;

use crate::HookEvent;

/// Errors produced by the hook system.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HookError {
    /// Hook execution failed before producing a valid result.
    #[error("{message}")]
    ExecutionFailed {
        /// Human-readable failure message.
        message: String,
        /// Lifecycle event being handled, when known.
        event: Option<HookEvent>,
        /// Hook name for diagnostics, when known.
        hook_name: Option<String>,
    },
    /// Hook execution exceeded its timeout budget.
    #[error("{message}")]
    Timeout {
        /// Human-readable timeout message.
        message: String,
        /// Lifecycle event being handled, when known.
        event: Option<HookEvent>,
        /// Hook name for diagnostics, when known.
        hook_name: Option<String>,
        /// Timeout duration, when known.
        duration: Option<Duration>,
    },
    /// Hook output could not be parsed into the expected shape.
    #[error("{message}")]
    InvalidOutput {
        /// Human-readable parsing failure.
        message: String,
        /// Lifecycle event being handled, when known.
        event: Option<HookEvent>,
        /// Hook name for diagnostics, when known.
        hook_name: Option<String>,
        /// Raw output captured from the hook, when safe to keep.
        output: Option<String>,
    },
    /// A panic was caught and isolated from the rest of the agent.
    #[error("{message}")]
    PanicIsolated {
        /// Human-readable panic summary.
        message: String,
        /// Lifecycle event being handled, when known.
        event: Option<HookEvent>,
        /// Hook name for diagnostics, when known.
        hook_name: Option<String>,
    },
}

impl HookError {
    /// Creates an execution-failed error.
    #[must_use]
    pub fn execution_failed(
        message: impl Into<String>,
        event: Option<HookEvent>,
        hook_name: Option<String>,
    ) -> Self {
        Self::ExecutionFailed {
            message: message.into(),
            event,
            hook_name,
        }
    }

    /// Creates a timeout error.
    #[must_use]
    pub fn timeout(
        message: impl Into<String>,
        event: Option<HookEvent>,
        hook_name: Option<String>,
        duration: Option<Duration>,
    ) -> Self {
        Self::Timeout {
            message: message.into(),
            event,
            hook_name,
            duration,
        }
    }

    /// Creates an invalid-output error.
    #[must_use]
    pub fn invalid_output(
        message: impl Into<String>,
        event: Option<HookEvent>,
        hook_name: Option<String>,
        output: Option<String>,
    ) -> Self {
        Self::InvalidOutput {
            message: message.into(),
            event,
            hook_name,
            output,
        }
    }

    /// Creates a panic-isolated error.
    #[must_use]
    pub fn panic_isolated(
        message: impl Into<String>,
        event: Option<HookEvent>,
        hook_name: Option<String>,
    ) -> Self {
        Self::PanicIsolated {
            message: message.into(),
            event,
            hook_name,
        }
    }

    fn correction_payload(&self) -> Value {
        match self {
            Self::ExecutionFailed {
                message,
                event,
                hook_name,
            }
            | Self::PanicIsolated {
                message,
                event,
                hook_name,
            } => json!({
                "message": message,
                "event": event.map(HookEvent::as_str),
                "hook_name": hook_name,
            }),
            Self::Timeout {
                message,
                event,
                hook_name,
                duration,
            } => json!({
                "message": message,
                "event": event.map(HookEvent::as_str),
                "hook_name": hook_name,
                "duration_ms": duration.map(|value| value.as_millis()),
            }),
            Self::InvalidOutput {
                message,
                event,
                hook_name,
                output,
            } => json!({
                "message": message,
                "event": event.map(HookEvent::as_str),
                "hook_name": hook_name,
                "output": output,
            }),
        }
    }
}

impl ClassifiedError for HookError {
    fn error_code(&self) -> &'static str {
        match self {
            Self::ExecutionFailed { .. } => "HOOK_EXECUTION_FAILED",
            Self::Timeout { .. } => "HOOK_TIMEOUT",
            Self::InvalidOutput { .. } => "HOOK_INVALID_OUTPUT",
            Self::PanicIsolated { .. } => "HOOK_PANIC_ISOLATED",
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
            Self::ExecutionFailed { .. } | Self::PanicIsolated { .. } => 500,
            Self::Timeout { .. } => 504,
            Self::InvalidOutput { .. } => 422,
        }
    }

    fn correction_context(&self) -> Option<Value> {
        Some(self.correction_payload())
    }
}
