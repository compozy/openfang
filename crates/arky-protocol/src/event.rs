//! Shared event and streaming update types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ContentBlock, Message, ProviderId, SessionId, ToolCall, ToolResult, TurnId, Usage};

/// Metadata attached to every emitted event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventMetadata {
    /// Millisecond timestamp when the event was emitted.
    pub timestamp_ms: u64,
    /// Strictly monotonic sequence number within a session.
    pub sequence: u64,
    /// Owning session identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    /// Owning turn identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<TurnId>,
    /// Provider that originated the event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<ProviderId>,
}

impl EventMetadata {
    /// Creates metadata with no attached routing identifiers.
    #[must_use]
    pub const fn new(timestamp_ms: u64, sequence: u64) -> Self {
        Self {
            timestamp_ms,
            sequence,
            session_id: None,
            turn_id: None,
            provider_id: None,
        }
    }

    /// Stores the session identifier.
    #[must_use]
    pub const fn with_session_id(mut self, session_id: SessionId) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Stores the turn identifier.
    #[must_use]
    pub const fn with_turn_id(mut self, turn_id: TurnId) -> Self {
        self.turn_id = Some(turn_id);
        self
    }

    /// Stores the provider identifier.
    #[must_use]
    pub fn with_provider_id(mut self, provider_id: ProviderId) -> Self {
        self.provider_id = Some(provider_id);
        self
    }

    /// Checks whether this metadata can legally follow the previous event.
    ///
    /// The contract is strict: both events must belong to the same session and
    /// the current sequence must be greater than the previous sequence.
    #[must_use]
    pub fn is_strictly_after(&self, previous: &Self) -> bool {
        self.session_id == previous.session_id && self.sequence > previous.sequence
    }
}

/// An incremental update emitted while constructing a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamDelta {
    /// Appended text content.
    Text {
        /// The appended text.
        text: String,
    },
    /// A complete tool-use block became available.
    ToolUse {
        /// Flattened tool call metadata.
        #[serde(flatten)]
        call: ToolCall,
    },
    /// A streaming fragment for tool input JSON.
    ToolUseInput {
        /// Tool call identifier receiving the input fragment.
        tool_call_id: String,
        /// The raw input fragment.
        delta: String,
    },
    /// A complete tool-result block became available.
    ToolResult {
        /// Flattened tool result metadata.
        #[serde(flatten)]
        result: ToolResult,
    },
    /// An appended image block.
    Image {
        /// Raw image bytes.
        data: Vec<u8>,
        /// MIME type describing the image encoding.
        media_type: String,
    },
    /// A full replacement for the current message content.
    Replace {
        /// Replacement content blocks.
        content: Vec<ContentBlock>,
    },
}

impl StreamDelta {
    /// Creates a text delta.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// Creates a tool-use delta.
    #[must_use]
    pub const fn tool_use(call: ToolCall) -> Self {
        Self::ToolUse { call }
    }

    /// Creates a tool-input delta.
    #[must_use]
    pub fn tool_use_input(tool_call_id: impl Into<String>, delta: impl Into<String>) -> Self {
        Self::ToolUseInput {
            tool_call_id: tool_call_id.into(),
            delta: delta.into(),
        }
    }

    /// Creates a tool-result delta.
    #[must_use]
    pub const fn tool_result(result: ToolResult) -> Self {
        Self::ToolResult { result }
    }

    /// Creates an image delta.
    #[must_use]
    pub fn image(data: impl Into<Vec<u8>>, media_type: impl Into<String>) -> Self {
        Self::Image {
            data: data.into(),
            media_type: media_type.into(),
        }
    }

    /// Creates a replace delta.
    #[must_use]
    pub const fn replace(content: Vec<ContentBlock>) -> Self {
        Self::Replace { content }
    }
}

/// All events emitted by the provider and agent layers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgentEvent {
    /// The agent started processing work.
    AgentStart {
        /// Shared event metadata.
        meta: EventMetadata,
    },
    /// The agent finished processing work.
    AgentEnd {
        /// Shared event metadata.
        meta: EventMetadata,
        /// Messages accumulated by the agent.
        messages: Vec<Message>,
    },
    /// A new turn started.
    TurnStart {
        /// Shared event metadata.
        meta: EventMetadata,
    },
    /// A turn finished.
    TurnEnd {
        /// Shared event metadata.
        meta: EventMetadata,
        /// Final assistant message for the turn.
        message: Message,
        /// Tool results emitted during the turn.
        tool_results: Vec<ToolResult>,
        /// Usage accumulated for the completed turn.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
    },
    /// A message started streaming.
    MessageStart {
        /// Shared event metadata.
        meta: EventMetadata,
        /// Current state of the message.
        message: Message,
    },
    /// A message received an incremental update.
    MessageUpdate {
        /// Shared event metadata.
        meta: EventMetadata,
        /// Current state of the message.
        message: Message,
        /// Incremental change since the previous message update.
        delta: StreamDelta,
    },
    /// A message finished streaming.
    MessageEnd {
        /// Shared event metadata.
        meta: EventMetadata,
        /// Final message state.
        message: Message,
    },
    /// Reasoning or extended-thinking stream started.
    ReasoningStart {
        /// Shared event metadata.
        meta: EventMetadata,
        /// Stable reasoning block identifier.
        reasoning_id: String,
    },
    /// One reasoning delta arrived.
    ReasoningDelta {
        /// Shared event metadata.
        meta: EventMetadata,
        /// Stable reasoning block identifier.
        reasoning_id: String,
        /// Incremental reasoning text.
        delta: String,
    },
    /// One reasoning stream completed.
    ReasoningComplete {
        /// Shared event metadata.
        meta: EventMetadata,
        /// Stable reasoning block identifier.
        reasoning_id: String,
        /// Fully accumulated reasoning text.
        full_text: String,
    },
    /// Tool execution started.
    ToolExecutionStart {
        /// Shared event metadata.
        meta: EventMetadata,
        /// Tool call identifier.
        tool_call_id: String,
        /// Canonical tool name.
        tool_name: String,
        /// JSON arguments passed to the tool.
        args: Value,
    },
    /// Tool execution produced an intermediate result.
    ToolExecutionUpdate {
        /// Shared event metadata.
        meta: EventMetadata,
        /// Tool call identifier.
        tool_call_id: String,
        /// Canonical tool name.
        tool_name: String,
        /// Partial result payload.
        partial_result: Value,
    },
    /// Tool execution finished.
    ToolExecutionEnd {
        /// Shared event metadata.
        meta: EventMetadata,
        /// Tool call identifier.
        tool_call_id: String,
        /// Canonical tool name.
        tool_name: String,
        /// Final result payload.
        result: Value,
        /// Whether the final result represents an error.
        is_error: bool,
    },
    /// A caller-defined extensibility hook.
    Custom {
        /// Shared event metadata.
        meta: EventMetadata,
        /// Stable caller-defined event type.
        event_type: String,
        /// Arbitrary event payload.
        payload: Value,
    },
}

impl AgentEvent {
    /// Returns shared metadata for the event.
    #[must_use]
    pub const fn metadata(&self) -> &EventMetadata {
        match self {
            Self::AgentStart { meta }
            | Self::AgentEnd { meta, .. }
            | Self::TurnStart { meta }
            | Self::TurnEnd { meta, .. }
            | Self::MessageStart { meta, .. }
            | Self::MessageUpdate { meta, .. }
            | Self::MessageEnd { meta, .. }
            | Self::ReasoningStart { meta, .. }
            | Self::ReasoningDelta { meta, .. }
            | Self::ReasoningComplete { meta, .. }
            | Self::ToolExecutionStart { meta, .. }
            | Self::ToolExecutionUpdate { meta, .. }
            | Self::ToolExecutionEnd { meta, .. }
            | Self::Custom { meta, .. } => meta,
        }
    }

    /// Returns the event sequence number.
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.metadata().sequence
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{AgentEvent, EventMetadata, StreamDelta};
    use crate::{Message, ProviderId, SessionId, ToolCall, ToolContent, ToolResult, TurnId};

    fn sample_meta(sequence: u64) -> EventMetadata {
        EventMetadata::new(1_717_171_717, sequence)
            .with_session_id(SessionId::new())
            .with_turn_id(TurnId::new())
            .with_provider_id(ProviderId::new("codex"))
    }

    fn sample_message() -> Message {
        Message::assistant("hello from Arky")
    }

    fn sample_tool_call() -> ToolCall {
        ToolCall::new("call-1", "read_file", json!({ "path": "Cargo.toml" }))
    }

    fn sample_tool_result() -> ToolResult {
        ToolResult::success(
            "call-1",
            "read_file",
            vec![ToolContent::text("workspace manifest")],
        )
    }

    #[test]
    fn event_metadata_should_report_monotonic_order_within_a_session() {
        let session_id = SessionId::new();
        let older = EventMetadata::new(10, 1).with_session_id(session_id.clone());
        let newer = EventMetadata::new(20, 2).with_session_id(session_id);

        assert_eq!(newer.is_strictly_after(&older), true);
    }

    #[test]
    fn stream_delta_should_support_serde_round_trip() {
        let delta = StreamDelta::tool_use_input("call-1", "{\"path\":\"Cargo.toml\"}");
        let encoded = serde_json::to_string(&delta).expect("delta should serialize");
        let decoded: StreamDelta =
            serde_json::from_str(&encoded).expect("delta should deserialize");

        assert_eq!(decoded, delta);
    }

    #[test]
    fn agent_event_should_support_all_variants_and_serde_round_trip() {
        let message = sample_message();
        let tool_call = sample_tool_call();
        let tool_result = sample_tool_result();
        let events = vec![
            AgentEvent::AgentStart {
                meta: sample_meta(1),
            },
            AgentEvent::AgentEnd {
                meta: sample_meta(2),
                messages: vec![message.clone()],
            },
            AgentEvent::TurnStart {
                meta: sample_meta(3),
            },
            AgentEvent::TurnEnd {
                meta: sample_meta(4),
                message: message.clone(),
                tool_results: vec![tool_result],
                usage: None,
            },
            AgentEvent::MessageStart {
                meta: sample_meta(5),
                message: message.clone(),
            },
            AgentEvent::MessageUpdate {
                meta: sample_meta(6),
                message: message.clone(),
                delta: StreamDelta::text(" world"),
            },
            AgentEvent::MessageEnd {
                meta: sample_meta(7),
                message,
            },
            AgentEvent::ReasoningStart {
                meta: sample_meta(8),
                reasoning_id: "reasoning-1".to_owned(),
            },
            AgentEvent::ReasoningDelta {
                meta: sample_meta(9),
                reasoning_id: "reasoning-1".to_owned(),
                delta: "think".to_owned(),
            },
            AgentEvent::ReasoningComplete {
                meta: sample_meta(10),
                reasoning_id: "reasoning-1".to_owned(),
                full_text: "thinking complete".to_owned(),
            },
            AgentEvent::ToolExecutionStart {
                meta: sample_meta(11),
                tool_call_id: tool_call.id.clone(),
                tool_name: tool_call.name.clone(),
                args: tool_call.input.clone(),
            },
            AgentEvent::ToolExecutionUpdate {
                meta: sample_meta(12),
                tool_call_id: tool_call.id.clone(),
                tool_name: tool_call.name.clone(),
                partial_result: json!({ "progress": 50 }),
            },
            AgentEvent::ToolExecutionEnd {
                meta: sample_meta(13),
                tool_call_id: tool_call.id,
                tool_name: tool_call.name,
                result: json!({ "content": "workspace manifest" }),
                is_error: false,
            },
            AgentEvent::Custom {
                meta: sample_meta(14),
                event_type: "provider.custom".to_owned(),
                payload: json!({ "trace_id": "abc123" }),
            },
        ];

        let round_tripped = events
            .iter()
            .map(|event| {
                let encoded = serde_json::to_string(event).expect("event should serialize");
                serde_json::from_str::<AgentEvent>(&encoded).expect("event should deserialize")
            })
            .collect::<Vec<_>>();

        assert_eq!(round_tripped, events);
    }
}
