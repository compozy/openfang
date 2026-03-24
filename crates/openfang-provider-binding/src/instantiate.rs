//! Provider instantiation helpers for compiled Compozy bindings.

use std::sync::Arc;

use arky_claude_code::{
    BedrockProvider, ClaudeCodeProvider, ClaudeCompatibleProviderKind, ClaudeProviderProfile,
    MinimaxProvider, MoonshotProvider, OllamaProvider, OpenRouterProvider, VercelProvider,
    VertexProvider, ZaiProvider,
};
use arky_codex::CodexProvider;
use arky_provider::Provider;
use thiserror::Error;

use crate::adapter::CompozyProviderConfig;

/// Failures while instantiating a live provider from the typed adapter config.
#[derive(Debug, Error)]
pub enum InstantiateError {
    /// The wrapper kind and profile payload do not match.
    #[error("claude-compatible provider kind `{kind}` does not match profile payload `{profile}`")]
    ProfileMismatch {
        /// Wrapper kind requested by the adapter layer.
        kind: &'static str,
        /// Concrete profile payload provided by the adapter layer.
        profile: &'static str,
    },
}

/// Instantiates a concrete Arky provider from one typed Compozy provider config.
pub fn instantiate_provider(
    config: CompozyProviderConfig,
) -> Result<Arc<dyn Provider>, InstantiateError> {
    match config {
        CompozyProviderConfig::Codex(config) => Ok(Arc::new(CodexProvider::with_config(config))),
        CompozyProviderConfig::ClaudeCode(config) => {
            Ok(Arc::new(ClaudeCodeProvider::with_config(config)))
        }
        CompozyProviderConfig::ClaudeCompatible { kind, profile } => {
            instantiate_claude_compatible(kind, profile)
        }
    }
}

fn instantiate_claude_compatible(
    kind: ClaudeCompatibleProviderKind,
    profile: ClaudeProviderProfile,
) -> Result<Arc<dyn Provider>, InstantiateError> {
    match (kind, profile) {
        (ClaudeCompatibleProviderKind::Bedrock, ClaudeProviderProfile::Bedrock(config)) => {
            Ok(Arc::new(BedrockProvider::with_config(config)))
        }
        (ClaudeCompatibleProviderKind::Zai, ClaudeProviderProfile::Zai(config)) => {
            Ok(Arc::new(ZaiProvider::with_config(config)))
        }
        (ClaudeCompatibleProviderKind::OpenRouter, ClaudeProviderProfile::OpenRouter(config)) => {
            Ok(Arc::new(OpenRouterProvider::with_config(config)))
        }
        (ClaudeCompatibleProviderKind::Vercel, ClaudeProviderProfile::Vercel(config)) => {
            Ok(Arc::new(VercelProvider::with_config(config)))
        }
        (ClaudeCompatibleProviderKind::Moonshot, ClaudeProviderProfile::Moonshot(config)) => {
            Ok(Arc::new(MoonshotProvider::with_config(config)))
        }
        (ClaudeCompatibleProviderKind::Minimax, ClaudeProviderProfile::Minimax(config)) => {
            Ok(Arc::new(MinimaxProvider::with_config(config)))
        }
        (ClaudeCompatibleProviderKind::Vertex, ClaudeProviderProfile::Vertex(config)) => {
            Ok(Arc::new(VertexProvider::with_config(config)))
        }
        (ClaudeCompatibleProviderKind::Ollama, ClaudeProviderProfile::Ollama(config)) => {
            Ok(Arc::new(OllamaProvider::with_config(config)))
        }
        (kind, profile) => Err(InstantiateError::ProfileMismatch {
            kind: kind.as_str(),
            profile: profile_name(&profile),
        }),
    }
}

fn profile_name(profile: &ClaudeProviderProfile) -> &'static str {
    match profile {
        ClaudeProviderProfile::ClaudeCode => "claude-code",
        ClaudeProviderProfile::Bedrock(_) => "bedrock",
        ClaudeProviderProfile::Vertex(_) => "vertex",
        ClaudeProviderProfile::Zai(_) => "zai",
        ClaudeProviderProfile::OpenRouter(_) => "openrouter",
        ClaudeProviderProfile::Vercel(_) => "vercel",
        ClaudeProviderProfile::Moonshot(_) => "moonshot",
        ClaudeProviderProfile::Minimax(_) => "minimax",
        ClaudeProviderProfile::Ollama(_) => "ollama",
    }
}

#[cfg(test)]
mod tests {
    use arky_claude_code::{
        BedrockProviderConfig, ClaudeCodeProviderConfig, ClaudeCompatibleProviderConfig,
        ClaudeCompatibleProviderKind, ClaudeProviderProfile, OpenRouterProviderConfig,
        ZaiProviderConfig,
    };
    use pretty_assertions::assert_eq;

    use super::{instantiate_provider, InstantiateError};
    use crate::adapter::CompozyProviderConfig;

    #[test]
    fn instantiate_provider_should_build_codex_provider() {
        let provider = instantiate_provider(CompozyProviderConfig::Codex(
            arky_codex::CodexProviderConfig::default(),
        ))
        .expect("codex provider should instantiate");

        assert_eq!(provider.descriptor().id.as_str(), "codex");
    }

    #[test]
    fn instantiate_provider_should_build_claude_code_provider() {
        let provider = instantiate_provider(CompozyProviderConfig::ClaudeCode(
            ClaudeCodeProviderConfig::default(),
        ))
        .expect("claude-code provider should instantiate");

        assert_eq!(provider.descriptor().id.as_str(), "claude-code");
    }

    #[test]
    fn instantiate_provider_should_build_wrapper_providers() {
        let bedrock = instantiate_provider(CompozyProviderConfig::ClaudeCompatible {
            kind: ClaudeCompatibleProviderKind::Bedrock,
            profile: ClaudeProviderProfile::Bedrock(BedrockProviderConfig::default()),
        })
        .expect("bedrock provider should instantiate");
        let openrouter = instantiate_provider(CompozyProviderConfig::ClaudeCompatible {
            kind: ClaudeCompatibleProviderKind::OpenRouter,
            profile: ClaudeProviderProfile::OpenRouter(OpenRouterProviderConfig {
                base: ClaudeCompatibleProviderConfig::default(),
                api_key: "test-openrouter-key".to_owned(),
                selected_model: "openrouter/anthropic/claude-sonnet".to_owned(),
            }),
        })
        .expect("openrouter provider should instantiate");

        assert_eq!(bedrock.descriptor().id.as_str(), "bedrock");
        assert_eq!(openrouter.descriptor().id.as_str(), "openrouter");
    }

    #[test]
    fn instantiate_provider_should_reject_profile_kind_mismatches() {
        let result = instantiate_provider(CompozyProviderConfig::ClaudeCompatible {
            kind: ClaudeCompatibleProviderKind::Bedrock,
            profile: ClaudeProviderProfile::Zai(ZaiProviderConfig::new(
                "test-zai-key",
                "zai/claude-sonnet",
            )),
        });

        assert!(matches!(
            result,
            Err(InstantiateError::ProfileMismatch {
                kind: "bedrock",
                profile: "zai",
            })
        ));
    }
}
