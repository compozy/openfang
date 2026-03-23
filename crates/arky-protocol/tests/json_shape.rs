//! JSON compatibility tests for externally consumed protocol events.

use pretty_assertions::assert_eq;
use serde_json::json;

use arky_protocol::{
    AgentEvent, ContentBlock, EventMetadata, Message, ProviderId, SessionId, StreamDelta, TurnId,
};

#[test]
fn agent_event_json_shape_should_match_downstream_expectations() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let message = Message::assistant("hello");
    let event = AgentEvent::MessageUpdate {
        meta: EventMetadata::new(1_717_171_717, 42)
            .with_session_id(session_id.clone())
            .with_turn_id(turn_id.clone())
            .with_provider_id(ProviderId::new("codex")),
        message,
        delta: StreamDelta::replace(vec![
            ContentBlock::text("hello"),
            ContentBlock::text(" world"),
        ]),
    };

    let actual = serde_json::to_value(&event).expect("event should serialize");
    let expected = json!({
        "type": "message_update",
        "meta": {
            "timestamp_ms": 1_717_171_717_u64,
            "sequence": 42_u64,
            "session_id": session_id.to_string(),
            "turn_id": turn_id.to_string(),
            "provider_id": "codex",
        },
        "message": {
            "role": "assistant",
            "content": [{
                "type": "text",
                "text": "hello",
            }],
        },
        "delta": {
            "type": "replace",
            "content": [
                {
                    "type": "text",
                    "text": "hello",
                },
                {
                    "type": "text",
                    "text": " world",
                }
            ]
        }
    });

    assert_eq!(actual, expected);
}
