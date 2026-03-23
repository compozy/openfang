//! Pattern-based error classification and agent-facing recovery messages.

use std::{collections::BTreeMap, sync::LazyLock, time::Duration};

use regex::Regex;

/// Coarse-grained category used to normalize provider errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Authentication or API key failures.
    Authentication,
    /// Authorization or permission failures.
    Authorization,
    /// Rate-limit throttling from the upstream provider.
    RateLimit,
    /// Timeout or deadline exhaustion.
    Timeout,
    /// Network, DNS, or transport connectivity failures.
    NetworkError,
    /// Payload parsing or decoding failures.
    ParseError,
    /// Stream framing or corruption failures.
    StreamCorruption,
    /// Tool execution failed within the provider.
    ToolExecution,
    /// Process spawn or binary boot failure.
    SpawnFailure,
    /// Invalid configuration supplied to the provider.
    ConfigInvalid,
    /// Requested model does not exist or is not available.
    ModelNotFound,
    /// Request exceeded the model context window.
    ContextLength,
    /// Safety or content filtering blocked the request.
    ContentFilter,
    /// Provider or backend server is overloaded.
    ServerOverloaded,
    /// Generic provider/internal server error.
    InternalError,
    /// No known category matched.
    Unknown,
}

impl ErrorCategory {
    const fn retry_guidance(self) -> &'static str {
        match self {
            Self::Authentication => "Check API credentials or login state before retrying.",
            Self::Authorization => "Check permissions, workspace access, or approval settings.",
            Self::RateLimit => "Wait for the retry window to elapse, then retry with backoff.",
            Self::Timeout => "Retry the same request, or lower the workload and timeout budget.",
            Self::NetworkError => "Retry after connectivity stabilizes and verify network access.",
            Self::ParseError => "Inspect malformed JSON or structured payloads before retrying.",
            Self::StreamCorruption => {
                "Restart the stream and verify the provider emitted valid frames."
            }
            Self::ToolExecution => {
                "Fix the failing tool arguments or tool-side runtime error first."
            }
            Self::SpawnFailure => {
                "Check the provider binary path, execution permissions, and environment."
            }
            Self::ConfigInvalid => {
                "Fix the invalid config fields and retry with a valid configuration."
            }
            Self::ModelNotFound => "Select a supported model identifier for the active provider.",
            Self::ContextLength => {
                "Reduce prompt/tool output size or switch to a larger-context model."
            }
            Self::ContentFilter => {
                "Revise the request so it complies with the provider's safety policy."
            }
            Self::ServerOverloaded => {
                "Retry later; the upstream service is temporarily overloaded."
            }
            Self::InternalError => {
                "Retry once and inspect provider diagnostics if the error repeats."
            }
            Self::Unknown => "Inspect the raw error details before retrying blindly.",
        }
    }
}

/// One regex-backed error pattern registered by a provider.
#[derive(Debug, Clone)]
pub struct ErrorPattern {
    /// Human-readable pattern name used in tests and diagnostics.
    pub name: &'static str,
    /// Regex used for matching raw stderr or provider messages.
    pub regex: Regex,
    /// Normalized error category.
    pub category: ErrorCategory,
    /// Whether a retry is safe after this failure.
    pub is_retryable: bool,
    /// Suggested retry delay when the provider implies one.
    pub retry_after_hint: Option<Duration>,
}

impl ErrorPattern {
    /// Builds a pattern from a regex string.
    pub fn new(
        name: &'static str,
        regex: &str,
        category: ErrorCategory,
        is_retryable: bool,
        retry_after_hint: Option<Duration>,
    ) -> Result<Self, regex::Error> {
        Ok(Self {
            name,
            regex: Regex::new(regex)?,
            category,
            is_retryable,
            retry_after_hint,
        })
    }
}

/// Classification input collected from provider failures.
#[derive(Debug, Clone, Copy, Default)]
pub struct ErrorInput<'a> {
    /// Provider stderr, when present.
    pub stderr: Option<&'a str>,
    /// User-facing error message, when present.
    pub message: Option<&'a str>,
    /// Transport or HTTP status code.
    pub status_code: Option<u16>,
    /// Process exit code when a subprocess crashed.
    pub exit_code: Option<i32>,
    /// Stable provider error code when already available.
    pub error_code: Option<&'a str>,
}

impl<'a> ErrorInput<'a> {
    fn search_haystacks(self) -> impl Iterator<Item = &'a str> {
        self.stderr.into_iter().chain(self.message)
    }
}

/// Structured classification result produced by [`ErrorClassifier`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedResult {
    /// Provider registry that contributed the matching pattern.
    pub provider_id: Option<String>,
    /// Matching pattern name, when any pattern matched.
    pub matched_pattern: Option<String>,
    /// Normalized category.
    pub category: ErrorCategory,
    /// Whether retrying is safe.
    pub is_retryable: bool,
    /// Suggested retry delay.
    pub retry_after_hint: Option<Duration>,
    /// Raw error code when one was supplied.
    pub error_code: Option<String>,
}

impl ClassifiedResult {
    /// Creates an unknown fallback result.
    #[must_use]
    pub fn unknown(input: &ErrorInput<'_>) -> Self {
        Self {
            provider_id: None,
            matched_pattern: None,
            category: ErrorCategory::Unknown,
            is_retryable: false,
            retry_after_hint: None,
            error_code: input.error_code.map(ToOwned::to_owned),
        }
    }
}

/// Registry-based classifier shared by provider crates.
#[derive(Debug, Default, Clone)]
pub struct ErrorClassifier {
    patterns_by_provider: BTreeMap<String, Vec<ErrorPattern>>,
}

impl ErrorClassifier {
    /// Creates an empty pattern registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers patterns for one provider identifier.
    pub fn register_patterns(
        &mut self,
        provider_id: impl Into<String>,
        patterns: Vec<ErrorPattern>,
    ) {
        self.patterns_by_provider
            .insert(provider_id.into(), patterns);
    }

    /// Classifies a provider failure using the registered patterns.
    #[must_use]
    pub fn classify(&self, input: &ErrorInput<'_>) -> ClassifiedResult {
        for (provider_id, patterns) in &self.patterns_by_provider {
            for pattern in patterns {
                if input
                    .search_haystacks()
                    .any(|haystack| pattern.regex.is_match(haystack))
                {
                    return ClassifiedResult {
                        provider_id: Some(provider_id.clone()),
                        matched_pattern: Some(pattern.name.to_owned()),
                        category: pattern.category,
                        is_retryable: pattern.is_retryable,
                        retry_after_hint: pattern.retry_after_hint,
                        error_code: input.error_code.map(ToOwned::to_owned),
                    };
                }
            }
        }

        classify_fallback(input)
    }

    /// Formats a structured agent-facing recovery message.
    #[must_use]
    pub fn format_for_agent(
        &self,
        result: &ClassifiedResult,
        original_error: &str,
        attempt: u32,
    ) -> String {
        let mut suggestions = extract_field_suggestions(original_error);
        if suggestions.is_empty() {
            suggestions.push(result.category.retry_guidance().to_owned());
        }

        let retry_line = match (result.is_retryable, result.retry_after_hint) {
            (false, _) => "Retryable: no.".to_owned(),
            (true, Some(delay)) => {
                format!("Retryable: yes. Suggested delay: {} ms.", delay.as_millis())
            }
            (true, None) => "Retryable: yes.".to_owned(),
        };

        let pattern = result.matched_pattern.as_deref().unwrap_or("unmatched");
        let provider = result.provider_id.as_deref().unwrap_or("unknown");

        let suggestion_lines = suggestions
            .iter()
            .enumerate()
            .map(|(index, suggestion)| format!("{}. {suggestion}", index + 1))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "Tool execution failed (attempt {attempt}).\n\
Category: {:?}\n\
Provider: {provider}\n\
Pattern: {pattern}\n\
{retry_line}\n\
Original error: {original_error}\n\n\
Suggestions:\n{suggestion_lines}",
            result.category
        )
    }
}

fn classify_fallback(input: &ErrorInput<'_>) -> ClassifiedResult {
    if matches!(input.status_code, Some(401)) {
        return ClassifiedResult {
            category: ErrorCategory::Authentication,
            is_retryable: false,
            retry_after_hint: None,
            error_code: input.error_code.map(ToOwned::to_owned),
            matched_pattern: Some("status_code_401".to_owned()),
            provider_id: None,
        };
    }
    if matches!(input.status_code, Some(403)) {
        return ClassifiedResult {
            category: ErrorCategory::Authorization,
            is_retryable: false,
            retry_after_hint: None,
            error_code: input.error_code.map(ToOwned::to_owned),
            matched_pattern: Some("status_code_403".to_owned()),
            provider_id: None,
        };
    }
    if matches!(input.status_code, Some(408 | 504)) {
        return ClassifiedResult {
            category: ErrorCategory::Timeout,
            is_retryable: true,
            retry_after_hint: Some(Duration::from_secs(1)),
            error_code: input.error_code.map(ToOwned::to_owned),
            matched_pattern: Some("status_code_timeout".to_owned()),
            provider_id: None,
        };
    }
    if matches!(input.status_code, Some(429)) {
        return ClassifiedResult {
            category: ErrorCategory::RateLimit,
            is_retryable: true,
            retry_after_hint: Some(Duration::from_secs(5)),
            error_code: input.error_code.map(ToOwned::to_owned),
            matched_pattern: Some("status_code_rate_limit".to_owned()),
            provider_id: None,
        };
    }
    if matches!(input.status_code, Some(502 | 503)) || matches!(input.exit_code, Some(137 | 143)) {
        return ClassifiedResult {
            category: ErrorCategory::ServerOverloaded,
            is_retryable: true,
            retry_after_hint: Some(Duration::from_secs(2)),
            error_code: input.error_code.map(ToOwned::to_owned),
            matched_pattern: Some("status_or_exit_fallback".to_owned()),
            provider_id: None,
        };
    }

    ClassifiedResult::unknown(input)
}

fn extract_field_suggestions(original_error: &str) -> Vec<String> {
    let mut suggestions = Vec::new();
    for (pattern, template) in FIELD_SUGGESTION_PATTERNS.iter() {
        for captures in pattern.captures_iter(original_error) {
            let replacement = captures
                .get(1)
                .map(|value| value.as_str().to_owned())
                .unwrap_or_default();
            suggestions.push(template.replace("$1", &replacement));
        }
    }
    suggestions
}

static FIELD_SUGGESTION_PATTERNS: LazyLock<[(Regex, &str); 4]> = LazyLock::new(|| {
    [
        (
            Regex::new("unknown field `([^`]+)`").expect("unknown-field regex should compile"),
            "Remove or rename unsupported field `$1`.",
        ),
        (
            Regex::new("missing field `([^`]+)`").expect("missing-field regex should compile"),
            "Provide required field `$1`.",
        ),
        (
            Regex::new("invalid type: .*?, expected ([^\\s]+)")
                .expect("invalid-type regex should compile"),
            "Provide the expected type `$1`.",
        ),
        (
            Regex::new("model [`'\"]?([^`'\"\\s]+)[`'\"]? not found")
                .expect("model-not-found regex should compile"),
            "Select an available model instead of `$1`.",
        ),
    ]
});

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{ErrorCategory, ErrorClassifier, ErrorInput, ErrorPattern};

    #[test]
    fn classifier_should_match_registered_patterns() {
        let mut classifier = ErrorClassifier::new();
        classifier.register_patterns(
            "claude-code",
            vec![
                ErrorPattern::new(
                    "authentication",
                    "(?i)api key",
                    ErrorCategory::Authentication,
                    false,
                    None,
                )
                .expect("pattern should compile"),
            ],
        );

        let result = classifier.classify(&ErrorInput {
            stderr: Some("invalid API key"),
            ..ErrorInput::default()
        });

        assert_eq!(result.category, ErrorCategory::Authentication);
        assert_eq!(result.is_retryable, false);
        assert_eq!(result.matched_pattern.as_deref(), Some("authentication"));
        assert_eq!(result.provider_id.as_deref(), Some("claude-code"));
    }

    #[test]
    fn classifier_should_use_status_fallbacks_when_no_pattern_matches() {
        let classifier = ErrorClassifier::new();

        let result = classifier.classify(&ErrorInput {
            status_code: Some(429),
            ..ErrorInput::default()
        });

        assert_eq!(result.category, ErrorCategory::RateLimit);
        assert_eq!(result.is_retryable, true);
    }

    #[test]
    fn classifier_should_default_to_unknown_when_unmatched() {
        let classifier = ErrorClassifier::new();

        let result = classifier.classify(&ErrorInput {
            message: Some("unexpected failure"),
            ..ErrorInput::default()
        });

        assert_eq!(result.category, ErrorCategory::Unknown);
        assert_eq!(result.is_retryable, false);
        assert_eq!(result.matched_pattern, None);
    }

    #[test]
    fn format_for_agent_should_include_attempt_and_field_suggestions() {
        let mut classifier = ErrorClassifier::new();
        classifier.register_patterns(
            "claude-code",
            vec![
                ErrorPattern::new(
                    "config_invalid",
                    "(?i)unknown field",
                    ErrorCategory::ConfigInvalid,
                    false,
                    None,
                )
                .expect("pattern should compile"),
            ],
        );
        let original_error = "unknown field `permissionMode`, expected one of `permission_mode`";
        let result = classifier.classify(&ErrorInput {
            message: Some(original_error),
            ..ErrorInput::default()
        });

        let formatted = classifier.format_for_agent(&result, original_error, 2);

        assert_eq!(formatted.contains("attempt 2"), true);
        assert_eq!(
            formatted.contains("Remove or rename unsupported field `permissionMode`."),
            true
        );
        assert_eq!(formatted.contains("Retryable: no."), true);
    }
}
