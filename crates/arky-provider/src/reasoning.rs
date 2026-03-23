//! Provider-family reasoning normalization helpers.

use arky_protocol::ReasoningEffort;

use crate::ProviderFamily;

const CLAUDE_LOW_BUDGET: u32 = 15_999;
const CLAUDE_MEDIUM_BUDGET: u32 = 31_999;
const CLAUDE_HIGH_BUDGET: u32 = 63_999;

/// Model identifiers with explicit `xhigh` reasoning support.
pub const XHIGH_CAPABLE_MODEL_IDS: &[&str] =
    &["gpt-5.1-codex", "gpt-5.1-codex-mini", "gpt-5.1-codex-max"];

/// Resolves the reasoning effort supported by a provider family.
#[must_use]
pub fn resolve_reasoning_for_provider(
    family: &ProviderFamily,
    effort: Option<ReasoningEffort>,
    model_id: Option<&str>,
) -> Option<ReasoningEffort> {
    let effort = effort?;

    match family {
        ProviderFamily::ClaudeCode => Some(match effort {
            ReasoningEffort::XHigh => ReasoningEffort::High,
            other => other,
        }),
        ProviderFamily::Codex => Some(match effort {
            ReasoningEffort::XHigh if supports_xhigh_reasoning(model_id) => ReasoningEffort::XHigh,
            ReasoningEffort::XHigh => ReasoningEffort::High,
            other => other,
        }),
        ProviderFamily::Custom(_) => Some(effort),
    }
}

/// Resolves Claude's max-thinking-token budget from explicit or logical settings.
#[must_use]
pub fn resolve_claude_max_thinking_tokens(
    max_thinking_tokens: Option<u32>,
    reasoning_effort: Option<ReasoningEffort>,
    extension_max_thinking_tokens: Option<u32>,
    extension_reasoning_effort: Option<ReasoningEffort>,
) -> Option<u32> {
    if let Some(max_thinking_tokens) = max_thinking_tokens {
        return Some(max_thinking_tokens);
    }

    if let Some(reasoning_effort) = reasoning_effort.map(to_claude_reasoning_effort) {
        return Some(claude_reasoning_budget(reasoning_effort));
    }

    if let Some(extension_max_thinking_tokens) = extension_max_thinking_tokens {
        return Some(extension_max_thinking_tokens);
    }

    extension_reasoning_effort
        .map(to_claude_reasoning_effort)
        .map(claude_reasoning_budget)
}

/// Maps a max-thinking-token setting back to a normalized reasoning effort.
#[must_use]
pub fn map_max_thinking_tokens_to_reasoning_effort(
    family: &ProviderFamily,
    max_thinking_tokens: Option<u32>,
) -> Option<ReasoningEffort> {
    let max_thinking_tokens = max_thinking_tokens?;
    if max_thinking_tokens == 0 {
        return None;
    }

    match family {
        ProviderFamily::Codex => Some(if max_thinking_tokens <= 16_000 {
            ReasoningEffort::Low
        } else if max_thinking_tokens <= 32_000 {
            ReasoningEffort::Medium
        } else if max_thinking_tokens <= 64_000 {
            ReasoningEffort::High
        } else {
            ReasoningEffort::XHigh
        }),
        ProviderFamily::ClaudeCode | ProviderFamily::Custom(_) => {
            Some(if max_thinking_tokens <= CLAUDE_LOW_BUDGET {
                ReasoningEffort::Low
            } else if max_thinking_tokens <= CLAUDE_MEDIUM_BUDGET {
                ReasoningEffort::Medium
            } else {
                ReasoningEffort::High
            })
        }
    }
}

/// Returns whether the model supports `xhigh` reasoning.
#[must_use]
pub fn supports_xhigh_reasoning(model_id: Option<&str>) -> bool {
    let Some(normalized) = normalize_reasoning_model_id(model_id) else {
        return false;
    };

    if XHIGH_CAPABLE_MODEL_IDS
        .iter()
        .any(|candidate| *candidate == normalized)
    {
        return true;
    }

    model_matches_capability(normalized.as_str(), "gpt-5.2")
        || model_matches_capability(normalized.as_str(), "gpt-5.3")
}

const fn to_claude_reasoning_effort(effort: ReasoningEffort) -> ReasoningEffort {
    match effort {
        ReasoningEffort::XHigh => ReasoningEffort::High,
        other => other,
    }
}

const fn claude_reasoning_budget(effort: ReasoningEffort) -> u32 {
    match effort {
        ReasoningEffort::Low => CLAUDE_LOW_BUDGET,
        ReasoningEffort::Medium => CLAUDE_MEDIUM_BUDGET,
        ReasoningEffort::High | ReasoningEffort::XHigh => CLAUDE_HIGH_BUDGET,
    }
}

fn normalize_reasoning_model_id(model_id: Option<&str>) -> Option<String> {
    let model_id = model_id?.trim().to_lowercase();
    if model_id.is_empty() {
        return None;
    }

    Some(
        model_id
            .strip_prefix("openai/")
            .unwrap_or(&model_id)
            .to_owned(),
    )
}

fn model_matches_capability(model_id: &str, family: &str) -> bool {
    model_id == family
        || model_id
            .strip_prefix(family)
            .is_some_and(|suffix| suffix.starts_with('-'))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        map_max_thinking_tokens_to_reasoning_effort, resolve_claude_max_thinking_tokens,
        resolve_reasoning_for_provider, supports_xhigh_reasoning,
    };
    use crate::ProviderFamily;
    use arky_protocol::ReasoningEffort;

    #[test]
    fn claude_reasoning_should_clamp_xhigh_to_high() {
        let resolved = resolve_reasoning_for_provider(
            &ProviderFamily::ClaudeCode,
            Some(ReasoningEffort::XHigh),
            Some("claude-3.7-sonnet"),
        );

        assert_eq!(resolved, Some(ReasoningEffort::High));
    }

    #[test]
    fn claude_low_effort_should_resolve_to_expected_budget() {
        let budget =
            resolve_claude_max_thinking_tokens(None, Some(ReasoningEffort::Low), None, None);

        assert_eq!(budget, Some(15_999));
    }

    #[test]
    fn codex_should_gate_xhigh_to_supported_models() {
        assert_eq!(supports_xhigh_reasoning(Some("gpt-5.2")), true);
        assert_eq!(supports_xhigh_reasoning(Some("gpt-5.1")), false);
        assert_eq!(supports_xhigh_reasoning(Some("gpt-5.20")), false);
        assert_eq!(supports_xhigh_reasoning(Some("gpt-4o")), false);
    }

    #[test]
    fn max_thinking_tokens_should_map_back_to_reasoning_effort() {
        let effort =
            map_max_thinking_tokens_to_reasoning_effort(&ProviderFamily::Codex, Some(70_000));

        assert_eq!(effort, Some(ReasoningEffort::XHigh));
    }
}
