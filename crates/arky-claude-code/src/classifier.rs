//! Claude-specific stderr and terminal error classification.

use std::{sync::Arc, time::Duration};

use arky_error::{ClassifiedResult, ErrorCategory, ErrorClassifier, ErrorInput, ErrorPattern};
use arky_provider::ProviderError;

/// Shared Claude error classifier.
#[derive(Debug, Clone)]
pub struct ClaudeErrorClassifier {
    inner: Arc<ErrorClassifier>,
}

impl ClaudeErrorClassifier {
    /// Builds the default Claude classifier with the P0 pattern registry.
    #[must_use]
    pub fn new() -> Self {
        let mut classifier = ErrorClassifier::new();
        classifier.register_patterns("claude-code", default_patterns());
        Self {
            inner: Arc::new(classifier),
        }
    }

    /// Classifies a Claude failure from stderr, message, and exit metadata.
    #[must_use]
    pub fn classify(
        &self,
        stderr: Option<&str>,
        message: Option<&str>,
        exit_code: Option<i32>,
        error_code: Option<&str>,
    ) -> ClassifiedResult {
        self.inner.classify(&ErrorInput {
            stderr,
            message,
            exit_code,
            error_code,
            ..ErrorInput::default()
        })
    }

    /// Maps a classified Claude failure into a provider error.
    #[must_use]
    pub fn to_provider_error(
        &self,
        classified: &ClassifiedResult,
        detail: impl Into<String>,
        command: Option<&str>,
        exit_code: Option<i32>,
        stderr: Option<&str>,
    ) -> ProviderError {
        let detail = detail.into();

        match classified.category {
            ErrorCategory::Authentication => ProviderError::auth_failed(detail),
            ErrorCategory::RateLimit => {
                ProviderError::rate_limited(detail, classified.retry_after_hint)
            }
            ErrorCategory::SpawnFailure => {
                if classified.matched_pattern.as_deref() == Some("binary_missing") {
                    return ProviderError::binary_not_found(command.unwrap_or("claude"));
                }
                ProviderError::process_crashed(
                    command.unwrap_or("claude"),
                    exit_code,
                    stderr.map(ToOwned::to_owned),
                )
            }
            ErrorCategory::NetworkError
            | ErrorCategory::Timeout
            | ErrorCategory::StreamCorruption
            | ErrorCategory::ServerOverloaded => ProviderError::stream_interrupted(detail),
            ErrorCategory::InternalError
                if classified.matched_pattern.as_deref() == Some("process_crash") =>
            {
                ProviderError::process_crashed(
                    command.unwrap_or("claude"),
                    exit_code,
                    stderr.map(ToOwned::to_owned),
                )
            }
            _ => ProviderError::protocol_violation(
                detail,
                Some(serde_json::json!({
                    "category": format!("{:?}", classified.category),
                    "pattern": classified.matched_pattern,
                    "exit_code": exit_code,
                    "stderr": stderr,
                })),
            ),
        }
    }

    /// Formats one agent-facing recovery message.
    #[must_use]
    pub fn format_for_agent(
        &self,
        classified: &ClassifiedResult,
        original_error: &str,
        attempt: u32,
    ) -> String {
        self.inner
            .format_for_agent(classified, original_error, attempt)
    }
}

impl Default for ClaudeErrorClassifier {
    fn default() -> Self {
        Self::new()
    }
}

fn default_patterns() -> Vec<ErrorPattern> {
    let mut patterns = Vec::with_capacity(18);
    patterns.extend(authentication_patterns());
    patterns.extend(transport_patterns());
    patterns.extend(execution_patterns());
    patterns.extend(validation_patterns());
    patterns
}

fn authentication_patterns() -> [ErrorPattern; 4] {
    [
        pattern(
            "authentication",
            r"(?i)(401|not logged in|authentication required|authentication failed|please log ?in|claude login|invalid api key|api key|token\s+(is\s+)?invalid|unauthenticated)",
            ErrorCategory::Authentication,
            false,
            None,
        ),
        pattern(
            "rate_limit",
            r"(?i)(429|rate limit|too many requests|quota exceeded|status\s*429|retry\s+after)",
            ErrorCategory::RateLimit,
            true,
            Some(Duration::from_secs(30)),
        ),
        pattern(
            "authorization",
            r"(?i)(403|forbidden|not authorized|authorization|insufficient permissions?)",
            ErrorCategory::Authorization,
            false,
            None,
        ),
        pattern(
            "permission_denied",
            r"(?i)(permission denied|operation not permitted)",
            ErrorCategory::Authorization,
            false,
            None,
        ),
    ]
}

fn transport_patterns() -> [ErrorPattern; 5] {
    [
        pattern(
            "overloaded",
            r"(?i)(overloaded|server overloaded|temporarily unavailable|capacity)",
            ErrorCategory::ServerOverloaded,
            true,
            Some(Duration::from_secs(15)),
        ),
        pattern(
            "network",
            r"(?i)(network|socket hang up|fetch failed|connection reset|connection refused|dns|temporary failure in name resolution|econnreset|econnrefused|enotfound|eai_again|ehostunreach|enetunreach|epipe|eio)",
            ErrorCategory::NetworkError,
            true,
            Some(Duration::from_secs(5)),
        ),
        pattern(
            "timeout",
            r"(?i)(timeout|timed out|deadline exceeded|request\s+timeout|etimedout)",
            ErrorCategory::Timeout,
            true,
            Some(Duration::from_secs(5)),
        ),
        pattern(
            "json_parse",
            r"(?i)(json.*parse|unexpected token|unexpected end of json|syntaxerror.*json|failed to parse json)",
            ErrorCategory::ParseError,
            false,
            None,
        ),
        pattern(
            "stream_corrupt",
            r"(?i)(claude_code_no_result|ended before a result message|stream ended before|malformed stream|stream corrupted|invalid stream)",
            ErrorCategory::StreamCorruption,
            true,
            None,
        ),
    ]
}

fn execution_patterns() -> [ErrorPattern; 5] {
    [
        pattern(
            "tool_execution",
            r#"(?i)(tool execution failed|tool failed|tool error|tool invocation failed|mcp tool failed|tool\s+"?.+"?\s+failed)"#,
            ErrorCategory::ToolExecution,
            false,
            None,
        ),
        pattern(
            "spawn_failure",
            r"(?i)(spawn failed|failed to start|failed to launch|spawn\s|enoent|eacces)",
            ErrorCategory::SpawnFailure,
            true,
            None,
        ),
        pattern(
            "server_error",
            r"(?i)(internal server error|status 500|server error|unexpected provider error)",
            ErrorCategory::InternalError,
            true,
            Some(Duration::from_secs(5)),
        ),
        pattern(
            "process_crash",
            r"(?i)(process exited|signal|segmentation fault|core dumped|unexpected exit|crashed)",
            ErrorCategory::InternalError,
            true,
            None,
        ),
        pattern(
            "binary_missing",
            r"(?i)(command not found|binary not found|no such file or directory|executable not found)",
            ErrorCategory::SpawnFailure,
            false,
            None,
        ),
    ]
}

fn validation_patterns() -> [ErrorPattern; 4] {
    [
        pattern(
            "context_length",
            r"(?i)(context length|maximum context|prompt is too long|too many tokens)",
            ErrorCategory::ContextLength,
            false,
            None,
        ),
        pattern(
            "content_filter",
            r"(?i)(content filter|safety policy|blocked by policy|refused due to policy)",
            ErrorCategory::ContentFilter,
            false,
            None,
        ),
        pattern(
            "model_not_found",
            r"(?i)(model not found|unknown model|unsupported model|invalid model)",
            ErrorCategory::ModelNotFound,
            false,
            None,
        ),
        pattern(
            "config_invalid",
            r"(?i)(settings validation|invalid request|invalid argument|validation|bad request|malformed request|config invalid)",
            ErrorCategory::ConfigInvalid,
            false,
            None,
        ),
    ]
}

fn pattern(
    name: &'static str,
    regex: &str,
    category: ErrorCategory,
    is_retryable: bool,
    retry_after_hint: Option<Duration>,
) -> ErrorPattern {
    ErrorPattern::new(name, regex, category, is_retryable, retry_after_hint)
        .expect("Claude error regexes should compile")
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::ClaudeErrorClassifier;

    #[test]
    fn classifier_should_match_all_p0_patterns() {
        let classifier = ClaudeErrorClassifier::new();
        let cases = vec![
            ("authentication", "Failed to authenticate: invalid API key"),
            ("rate_limit", "429 retry after 30 seconds"),
            ("overloaded", "Server overloaded, try again later"),
            ("network", "ECONNRESET: socket hang up"),
            ("timeout", "request timeout after 450 ms"),
            ("json_parse", "Unexpected end of JSON while decoding result"),
            (
                "stream_corrupt",
                "CLAUDE_CODE_NO_RESULT: stream ended before a result message",
            ),
            ("tool_execution", "tool \"search\" failed"),
            ("spawn_failure", "spawn failed with EACCES"),
            ("context_length", "maximum context length exceeded"),
            ("content_filter", "blocked by safety policy"),
            ("model_not_found", "unknown model claude-4-unknown"),
            ("authorization", "403 forbidden"),
            (
                "config_invalid",
                "settings validation failed: invalid request",
            ),
            ("server_error", "internal server error"),
            ("process_crash", "process exited unexpectedly"),
            ("binary_missing", "command not found: claude"),
            ("permission_denied", "permission denied opening file"),
        ];

        for (expected_pattern, message) in cases {
            let result = classifier.classify(Some(message), Some(message), None, None);
            assert_eq!(
                result.matched_pattern.as_deref(),
                Some(expected_pattern),
                "message `{message}`"
            );
        }
    }

    #[test]
    fn classifier_should_format_agent_guidance() {
        let error_classifier = ClaudeErrorClassifier::new();
        let result =
            error_classifier.classify(Some("invalid request"), Some("invalid request"), None, None);
        let formatted = error_classifier.format_for_agent(&result, "invalid request", 2);

        assert!(formatted.contains("attempt 2"));
        assert!(formatted.contains("Retryable: no."));
    }
}
