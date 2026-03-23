//! Optional real-binary integration test for Claude Code.
#![cfg(feature = "integration")]

use std::process::Command;

use arky_claude_code::ClaudeCodeProvider;
use arky_protocol::{Message, ModelRef, SessionId, SessionRef, TurnContext, TurnId};
use arky_provider::{Provider, ProviderRequest};
use futures::StreamExt;

#[tokio::test]
async fn real_claude_binary_should_stream_events_when_explicitly_enabled() {
    if std::env::var_os("ARKY_REAL_CLAUDE_TEST").is_none() {
        return;
    }

    let version = Command::new("claude").arg("--version").output();
    let Ok(version) = version else {
        return;
    };
    if !version.status.success() {
        return;
    }

    let provider = ClaudeCodeProvider::new();
    let mut stream = provider
        .stream(ProviderRequest::new(
            SessionRef::new(Some(SessionId::new())),
            TurnContext::new(TurnId::new(), 1),
            ModelRef::new("sonnet"),
            vec![Message::user("Reply with the single word: ready")],
        ))
        .await
        .expect("real Claude stream should start");

    let mut saw_terminal_message = false;
    while let Some(item) = stream.next().await {
        if let arky_protocol::AgentEvent::TurnEnd { message, .. } =
            item.expect("real Claude event should be valid")
        {
            saw_terminal_message = !message.content.is_empty();
            break;
        }
    }

    assert!(saw_terminal_message);
}
