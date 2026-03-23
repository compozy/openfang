//! Provider contract coverage for the Claude fixture process.

use std::{collections::BTreeMap, path::PathBuf};

use arky_claude_code::{ClaudeCodeProvider, ClaudeCodeProviderConfig};
use arky_protocol::{Message, ModelRef, SessionId, SessionRef, TurnContext, TurnId};
use arky_provider::{
    ProviderCapabilities, ProviderContractCase, ProviderContractTests, ProviderDescriptor,
    ProviderFamily, ProviderRequest,
};

fn fixture_binary() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("claude_fixture.sh")
        .display()
        .to_string()
}

fn fixture_provider(mode: &str) -> ClaudeCodeProvider {
    let mut env = BTreeMap::new();
    env.insert("CLAUDE_FIXTURE_MODE".to_owned(), mode.to_owned());

    ClaudeCodeProvider::with_config(ClaudeCodeProviderConfig {
        binary: fixture_binary(),
        env,
        ..ClaudeCodeProviderConfig::default()
    })
}

#[tokio::test]
async fn claude_provider_should_pass_shared_provider_contract_tests() {
    let provider = fixture_provider("contract_basic");
    let request = ProviderRequest::new(
        SessionRef::new(Some(SessionId::new())),
        TurnContext::new(TurnId::new(), 1),
        ModelRef::new("sonnet"),
        vec![Message::user("hello")],
    );
    let case = ProviderContractCase {
        request,
        expected_descriptor: ProviderDescriptor::new(
            arky_protocol::ProviderId::new("claude-code"),
            ProviderFamily::ClaudeCode,
            ProviderCapabilities::new()
                .with_streaming(true)
                .with_generate(true)
                .with_tool_calls(true)
                .with_mcp_passthrough(true)
                .with_session_resume(true)
                .with_extended_thinking(true),
        ),
        expected_message: Message::assistant("done"),
        min_event_count: 5,
    };

    ProviderContractTests::assert_provider(&provider, &case)
        .await
        .expect("Claude fixture provider should satisfy the shared contract");
}
