//! Re-exported provider request and response DTOs.

pub use arky_protocol::{
    GenerateResponse, HookContext, ModelRef, ProviderRequest, ProviderSettings, SessionRef,
    ToolContext, TurnContext,
};

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{
        HookContext, ModelRef, ProviderRequest, ProviderSettings, SessionRef, ToolContext,
        TurnContext,
    };
    use arky_protocol::{Message, ReplayCursor, SessionId, ToolCall, ToolDefinition, TurnId};

    #[test]
    fn provider_request_should_support_full_context_population() {
        let session_id = SessionId::new();
        let turn_id = TurnId::new();
        let tool_call = ToolCall::new(
            "call-1",
            "mcp/files/read_file",
            json!({
                "path": "Cargo.toml",
            }),
        );
        let request = ProviderRequest::new(
            SessionRef::new(Some(session_id.clone()))
                .with_provider_session_id("provider-session")
                .with_replay_cursor(ReplayCursor::from_checkpoint(11)),
            TurnContext::new(turn_id.clone(), 3).with_parent_id(TurnId::new()),
            ModelRef::new("gpt-5")
                .with_provider_id(arky_protocol::ProviderId::new("codex"))
                .with_provider_model_id("gpt-5-high"),
            vec![Message::system("system"), Message::user("hello")],
        )
        .with_tools(
            ToolContext::new()
                .with_definitions(vec![ToolDefinition::new(
                    "mcp/files/read_file",
                    "Read a file",
                    json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" }
                        },
                        "required": ["path"],
                    }),
                )])
                .with_active_calls(vec![tool_call.clone()])
                .call_scoped(true),
        )
        .with_hooks(
            HookContext::new()
                .with_metadata(BTreeMap::from([("mode".to_owned(), json!("strict"))])),
        )
        .with_settings({
            let mut settings = ProviderSettings::new();
            settings.temperature = Some(0.2);
            settings.max_tokens = Some(1_024);
            settings.stop_sequences = vec!["STOP".to_owned()];
            settings.extra = BTreeMap::from([("reasoning".to_owned(), json!("medium"))]);
            settings
        });

        assert_eq!(request.session.id, Some(session_id));
        assert_eq!(request.turn.id, turn_id);
        assert_eq!(request.tools.active_calls, vec![tool_call]);
        assert_eq!(request.hooks.metadata.get("mode"), Some(&json!("strict")));
        assert_eq!(request.settings.max_tokens, Some(1_024));
    }
}
