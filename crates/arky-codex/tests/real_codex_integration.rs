//! Optional real Codex App Server integration test.
#![cfg(feature = "integration")]

use std::time::Duration;

use arky_codex::{ApprovalMode, CodexProcessConfig, CodexProvider, CodexProviderConfig};
use arky_protocol::{ContentBlock, Message, ModelRef, SessionRef, TurnContext, TurnId};
use arky_provider::{Provider, ProviderRequest};
use pretty_assertions::assert_eq;

#[tokio::test]
async fn codex_provider_should_work_with_a_real_app_server_when_opted_in() {
    if std::env::var_os("ARKY_CODEX_RUN_REAL_INTEGRATION").is_none() {
        eprintln!(
            "skipping real codex integration test; set ARKY_CODEX_RUN_REAL_INTEGRATION=1 to enable it"
        );
        return;
    }

    let binary = std::env::var("ARKY_CODEX_REAL_BINARY").unwrap_or_else(|_| "codex".to_owned());
    let provider = CodexProvider::with_config(CodexProviderConfig {
        binary,
        process: CodexProcessConfig {
            allow_npx: false,
            ..CodexProcessConfig::default()
        },
        request_timeout: Duration::from_secs(120),
        scheduler_timeout: Duration::from_secs(120),
        approval_mode: ApprovalMode::AutoApprove,
        ..CodexProviderConfig::default()
    });

    let response = provider
        .generate(ProviderRequest::new(
            SessionRef::new(None),
            TurnContext::new(TurnId::new(), 1),
            ModelRef::new("gpt-5"),
            vec![Message::user("Reply with the single word READY.")],
        ))
        .await
        .expect("real codex app-server request should succeed");

    assert_eq!(response.message.role, arky_protocol::Role::Assistant);
    assert!(!response.message.content.is_empty());
    assert!(matches!(
        &response.message.content[0],
        ContentBlock::Text { text } if !text.trim().is_empty()
    ));
}
