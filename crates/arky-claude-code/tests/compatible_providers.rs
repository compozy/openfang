//! Public wrapper coverage for Claude-compatible providers.

use std::{collections::BTreeMap, path::PathBuf};

use arky_claude_code::{
    BedrockProvider, BedrockProviderConfig, MinimaxProvider, MoonshotProvider, OllamaProvider,
    OpenRouterProvider, VercelProvider, VertexProvider, ZaiProvider,
};
use arky_provider::Provider;
use pretty_assertions::assert_eq;

fn fixture_binary() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("claude_fixture.sh")
        .display()
        .to_string()
}

fn fixture_base_env() -> BTreeMap<String, String> {
    BTreeMap::from([(
        "CLAUDE_FIXTURE_MODE".to_owned(),
        "contract_basic".to_owned(),
    )])
}

#[test]
fn wrapper_providers_should_expose_expected_descriptor_ids() {
    let bedrock = BedrockProvider::with_config(BedrockProviderConfig {
        base: arky_claude_code::ClaudeCompatibleProviderConfig {
            binary: fixture_binary(),
            env: fixture_base_env(),
            ..arky_claude_code::ClaudeCompatibleProviderConfig::default()
        },
        selected_model: None,
        region: None,
    });
    let zai = ZaiProvider::new("zai-key", "zai/claude-sonnet");
    let openrouter =
        OpenRouterProvider::new("openrouter-key", "openrouter/anthropic/claude-sonnet");
    let vercel = VercelProvider::new("vercel-key", "vercel/claude-sonnet");
    let moonshot = MoonshotProvider::new("moonshot-key", "moonshot-v1");
    let minimax = MinimaxProvider::new("minimax-key");
    let vertex = VertexProvider::new();
    let ollama = OllamaProvider::new("llama3");

    assert_eq!(bedrock.descriptor().id.as_str(), "bedrock");
    assert_eq!(zai.descriptor().id.as_str(), "zai");
    assert_eq!(openrouter.descriptor().id.as_str(), "openrouter");
    assert_eq!(vercel.descriptor().id.as_str(), "vercel");
    assert_eq!(moonshot.descriptor().id.as_str(), "moonshot");
    assert_eq!(minimax.descriptor().id.as_str(), "minimax");
    assert_eq!(vertex.descriptor().id.as_str(), "vertex");
    assert_eq!(ollama.descriptor().id.as_str(), "ollama");
}
