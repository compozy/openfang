//! Provider-family resolution helpers for direct and gateway providers.

use serde::{Deserialize, Serialize};

/// Resolved provider family used by cross-provider usage and routing logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedProviderFamily {
    /// Direct Claude Code or Claude-family routing.
    ClaudeCode,
    /// Direct Codex or `OpenAI` GPT-family routing.
    Codex,
    /// OpenCode-compatible routing.
    OpenCode,
    /// Gateway or proxy routing such as `OpenRouter` or `LiteLLM`.
    Gateway,
    /// Family could not be resolved confidently.
    Unknown,
}

const GATEWAY_PROVIDER_IDS: &[&str] = &[
    "bedrock",
    "litellm",
    "minimax",
    "moonshot",
    "ollama",
    "openrouter",
    "vercel",
    "vertex",
    "zai",
];

const CLAUDE_PROVIDER_IDS: &[&str] = &["claude-code", "claude", "anthropic"];
const CODEX_PROVIDER_IDS: &[&str] = &["codex", "openai"];
const OPENCODE_PROVIDER_IDS: &[&str] = &["opencode"];

/// Resolves the provider family from either a provider identifier or a model-like string.
#[must_use]
pub fn resolve_provider_family(value: &str) -> ResolvedProviderFamily {
    let normalized = value.trim().to_lowercase();
    if normalized.is_empty() {
        return ResolvedProviderFamily::Unknown;
    }

    let provider_id = match normalized.split_once('/') {
        Some((provider_id, _)) => provider_id,
        None => normalized.as_str(),
    };

    if GATEWAY_PROVIDER_IDS.contains(&provider_id) {
        return ResolvedProviderFamily::Gateway;
    }

    if CLAUDE_PROVIDER_IDS.contains(&provider_id) || normalized.starts_with("claude-") {
        return ResolvedProviderFamily::ClaudeCode;
    }

    if CODEX_PROVIDER_IDS.contains(&provider_id)
        || normalized.starts_with("gpt-")
        || normalized.starts_with("o1-")
        || normalized.starts_with("o3-")
        || normalized.starts_with("o4-")
        || normalized.starts_with("codex-")
    {
        return ResolvedProviderFamily::Codex;
    }

    if OPENCODE_PROVIDER_IDS.contains(&provider_id) {
        return ResolvedProviderFamily::OpenCode;
    }

    ResolvedProviderFamily::Unknown
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{resolve_provider_family, ResolvedProviderFamily};

    #[test]
    fn resolve_provider_family_should_detect_gateway_models() {
        let family = resolve_provider_family("openrouter/claude-3.5-sonnet");

        assert_eq!(family, ResolvedProviderFamily::Gateway);
    }

    #[test]
    fn resolve_provider_family_should_detect_claude_models_without_provider_id() {
        let family = resolve_provider_family("claude-3.5-sonnet");

        assert_eq!(family, ResolvedProviderFamily::ClaudeCode);
    }

    #[test]
    fn resolve_provider_family_should_detect_codex_gpt_models() {
        let family = resolve_provider_family("gpt-4o");

        assert_eq!(family, ResolvedProviderFamily::Codex);
    }
}
