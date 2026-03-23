//! Integration coverage for multi-hook merge behavior.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use arky_hooks::{
    AfterToolCallContext, BeforeToolCallContext, HookChain, Hooks, PromptSubmitContext,
    PromptUpdate, SessionEndContext, SessionStartContext, SessionStartSource, SessionStartUpdate,
    StopContext, StopDecision, ToolResultOverride, Verdict,
};
use arky_protocol::{Message, SessionRef, ToolCall, ToolContent, ToolResult};
use async_trait::async_trait;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct CompositeHook {
    id: &'static str,
    session_end_counter: Arc<AtomicUsize>,
}

#[async_trait]
impl Hooks for CompositeHook {
    async fn before_tool_call(
        &self,
        _ctx: &BeforeToolCallContext,
        _cancel: CancellationToken,
    ) -> Result<Verdict, arky_hooks::HookError> {
        let verdict = match self.id {
            "two" => Verdict::block("policy block"),
            _ => Verdict::Allow,
        };

        Ok(verdict)
    }

    async fn after_tool_call(
        &self,
        _ctx: &AfterToolCallContext,
        _cancel: CancellationToken,
    ) -> Result<Option<ToolResultOverride>, arky_hooks::HookError> {
        let update = match self.id {
            "one" => Some(
                ToolResultOverride::new().with_content(vec![ToolContent::text("first content")]),
            ),
            "two" => Some(ToolResultOverride::new().with_is_error(true)),
            "three" => Some(
                ToolResultOverride::new().with_content(vec![ToolContent::text("final content")]),
            ),
            _ => None,
        };

        Ok(update)
    }

    async fn session_start(
        &self,
        _ctx: &SessionStartContext,
        _cancel: CancellationToken,
    ) -> Result<Option<SessionStartUpdate>, arky_hooks::HookError> {
        let mut env = std::collections::BTreeMap::new();
        let mut settings = std::collections::BTreeMap::new();
        let _ = env.insert(
            format!("ENV_{}", self.id.to_uppercase()),
            self.id.to_owned(),
        );
        let _ = settings.insert(self.id.to_owned(), json!(self.id));

        Ok(Some(
            SessionStartUpdate::new()
                .with_env(env)
                .with_settings(settings)
                .with_messages(vec![Message::system(format!("session {}", self.id))]),
        ))
    }

    async fn session_end(&self, _ctx: &SessionEndContext) -> Result<(), arky_hooks::HookError> {
        let _ = self.session_end_counter.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn on_stop(
        &self,
        _ctx: &StopContext,
        _cancel: CancellationToken,
    ) -> Result<StopDecision, arky_hooks::HookError> {
        let decision = if self.id == "two" {
            StopDecision::continue_with("needs more work")
        } else {
            StopDecision::Stop
        };

        Ok(decision)
    }

    async fn user_prompt_submit(
        &self,
        _ctx: &PromptSubmitContext,
        _cancel: CancellationToken,
    ) -> Result<Option<PromptUpdate>, arky_hooks::HookError> {
        let update = match self.id {
            "one" => Some(PromptUpdate::new().with_messages(vec![Message::system("prompt one")])),
            "two" => Some(
                PromptUpdate::new()
                    .rewrite("second rewrite")
                    .with_messages(vec![Message::system("prompt two")]),
            ),
            "three" => Some(
                PromptUpdate::new()
                    .rewrite("final rewrite")
                    .with_messages(vec![Message::system("prompt three")]),
            ),
            _ => None,
        };

        Ok(update)
    }
}

fn before_context() -> BeforeToolCallContext {
    BeforeToolCallContext::new(
        SessionRef::default(),
        ToolCall::new(
            "call-1",
            "mcp/local/read_file",
            json!({ "path": "Cargo.toml" }),
        ),
    )
}

fn after_context() -> AfterToolCallContext {
    let tool_call = ToolCall::new(
        "call-1",
        "mcp/local/read_file",
        json!({ "path": "Cargo.toml" }),
    );
    let result = ToolResult::success(
        "call-1",
        "mcp/local/read_file",
        vec![ToolContent::text("done")],
    );

    AfterToolCallContext::new(SessionRef::default(), tool_call, result)
}

#[tokio::test]
async fn hook_chain_should_merge_three_hooks_across_all_events() {
    let session_end_counter = Arc::new(AtomicUsize::new(0));
    let chain = HookChain::new()
        .with_hook(CompositeHook {
            id: "one",
            session_end_counter: Arc::clone(&session_end_counter),
        })
        .with_hook(CompositeHook {
            id: "two",
            session_end_counter: Arc::clone(&session_end_counter),
        })
        .with_hook(CompositeHook {
            id: "three",
            session_end_counter: Arc::clone(&session_end_counter),
        });

    let before = chain
        .before_tool_call(&before_context(), CancellationToken::new())
        .await
        .expect("before tool call should merge");
    assert_eq!(before, Verdict::block("policy block"));

    let after = chain
        .after_tool_call(&after_context(), CancellationToken::new())
        .await
        .expect("after tool call should merge")
        .expect("after update should exist");
    assert_eq!(
        after,
        ToolResultOverride::new()
            .with_content(vec![ToolContent::text("final content")])
            .with_is_error(true),
    );

    let session_start = chain
        .session_start(
            &SessionStartContext::new(SessionRef::default(), SessionStartSource::Startup),
            CancellationToken::new(),
        )
        .await
        .expect("session start should merge")
        .expect("session start update should exist");
    assert_eq!(
        session_start.messages,
        vec![
            Message::system("session one"),
            Message::system("session two"),
            Message::system("session three"),
        ],
    );
    assert_eq!(session_start.env.get("ENV_ONE"), Some(&"one".to_owned()));
    assert_eq!(session_start.env.get("ENV_TWO"), Some(&"two".to_owned()));
    assert_eq!(
        session_start.env.get("ENV_THREE"),
        Some(&"three".to_owned())
    );
    assert_eq!(session_start.settings.get("one"), Some(&json!("one")));
    assert_eq!(session_start.settings.get("two"), Some(&json!("two")));
    assert_eq!(session_start.settings.get("three"), Some(&json!("three")));

    let stop = chain
        .on_stop(
            &StopContext::new(SessionRef::default()),
            CancellationToken::new(),
        )
        .await
        .expect("stop should merge");
    assert_eq!(stop, StopDecision::continue_with("needs more work"));

    let prompt = chain
        .user_prompt_submit(
            &PromptSubmitContext::new(SessionRef::default(), "original prompt"),
            CancellationToken::new(),
        )
        .await
        .expect("prompt submit should merge")
        .expect("prompt update should exist");
    assert_eq!(
        prompt,
        PromptUpdate::new()
            .rewrite("final rewrite")
            .with_messages(vec![
                Message::system("prompt one"),
                Message::system("prompt two"),
                Message::system("prompt three"),
            ]),
    );

    chain
        .session_end(&SessionEndContext::new(SessionRef::default(), "finished"))
        .await
        .expect("session end should run all hooks");
    assert_eq!(session_end_counter.load(Ordering::SeqCst), 3);
}
