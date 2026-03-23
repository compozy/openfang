//! Persistence-oriented protocol types shared with session storage.

use serde::{Deserialize, Serialize};

use crate::{AgentEvent, Message, ProviderId, ToolResult, TurnId};

/// A persisted event record stored for replay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedEvent {
    /// Cached sequence number for efficient lookup.
    pub sequence: u64,
    /// Millisecond timestamp captured when the event was persisted.
    pub recorded_at_ms: u64,
    /// The event payload.
    pub event: AgentEvent,
}

impl PersistedEvent {
    /// Creates a persisted event by mirroring sequence and timestamp from the
    /// embedded event metadata.
    #[must_use]
    pub const fn new(event: AgentEvent) -> Self {
        Self {
            sequence: event.sequence(),
            recorded_at_ms: event.metadata().timestamp_ms,
            event,
        }
    }

    /// Creates a persisted event with explicit indexing metadata.
    #[must_use]
    pub const fn from_parts(sequence: u64, recorded_at_ms: u64, event: AgentEvent) -> Self {
        Self {
            sequence,
            recorded_at_ms,
            event,
        }
    }
}

/// A resumable checkpoint for a single turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnCheckpoint {
    /// Turn identifier captured by the checkpoint.
    pub turn_id: TurnId,
    /// Highest replay sequence incorporated by the checkpoint.
    pub sequence: u64,
    /// Final assistant message, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
    /// Tool results captured for the turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_results: Vec<ToolResult>,
    /// Provider that produced the checkpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<ProviderId>,
    /// Provider-native session identifier used for resume.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    /// Completion timestamp for the turn, when it finished successfully.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at_ms: Option<u64>,
}

impl TurnCheckpoint {
    /// Creates an empty checkpoint for the given turn and replay sequence.
    #[must_use]
    pub const fn new(turn_id: TurnId, sequence: u64) -> Self {
        Self {
            turn_id,
            sequence,
            message: None,
            tool_results: Vec::new(),
            provider_id: None,
            provider_session_id: None,
            completed_at_ms: None,
        }
    }

    /// Stores the final assistant message.
    #[must_use]
    pub fn with_message(mut self, message: Message) -> Self {
        self.message = Some(message);
        self
    }

    /// Stores the tool results captured during the turn.
    #[must_use]
    pub fn with_tool_results(mut self, tool_results: Vec<ToolResult>) -> Self {
        self.tool_results = tool_results;
        self
    }

    /// Stores the provider identifier.
    #[must_use]
    pub fn with_provider_id(mut self, provider_id: ProviderId) -> Self {
        self.provider_id = Some(provider_id);
        self
    }

    /// Stores the provider-native session identifier.
    #[must_use]
    pub fn with_provider_session_id(mut self, provider_session_id: impl Into<String>) -> Self {
        self.provider_session_id = Some(provider_session_id.into());
        self
    }

    /// Marks the turn as completed at the provided timestamp.
    #[must_use]
    pub const fn mark_completed(mut self, completed_at_ms: u64) -> Self {
        self.completed_at_ms = Some(completed_at_ms);
        self
    }
}

/// A cursor describing the next replay sequence to consume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayCursor {
    /// Sequence already covered by the latest checkpoint, when one exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_sequence: Option<u64>,
    /// Next event sequence to replay.
    pub next_sequence: u64,
}

impl ReplayCursor {
    /// Creates a cursor that starts replay at `next_sequence`.
    #[must_use]
    pub const fn new(next_sequence: u64) -> Self {
        Self {
            checkpoint_sequence: None,
            next_sequence,
        }
    }

    /// Creates a cursor anchored to a checkpoint sequence.
    #[must_use]
    pub const fn from_checkpoint(checkpoint_sequence: u64) -> Self {
        Self {
            checkpoint_sequence: Some(checkpoint_sequence),
            next_sequence: checkpoint_sequence.saturating_add(1),
        }
    }

    /// Advances the cursor to the sequence after `sequence`.
    pub const fn advance_to(&mut self, sequence: u64) {
        self.checkpoint_sequence = Some(sequence);
        self.next_sequence = sequence.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{PersistedEvent, ReplayCursor, TurnCheckpoint};
    use crate::{
        AgentEvent, EventMetadata, Message, ProviderId, SessionId, ToolContent, ToolResult, TurnId,
    };

    #[test]
    fn replay_cursor_should_advance_monotonically() {
        let mut cursor = ReplayCursor::new(3);
        cursor.advance_to(7);

        assert_eq!(
            cursor,
            ReplayCursor {
                checkpoint_sequence: Some(7),
                next_sequence: 8,
            }
        );
    }

    #[test]
    fn persisted_event_should_support_serde_round_trip() {
        let event = AgentEvent::AgentStart {
            meta: EventMetadata::new(100, 2).with_session_id(SessionId::new()),
        };
        let persisted_event = PersistedEvent::new(event);
        let encoded =
            serde_json::to_string(&persisted_event).expect("persisted event should serialize");
        let decoded: PersistedEvent =
            serde_json::from_str(&encoded).expect("persisted event should deserialize");

        assert_eq!(decoded, persisted_event);
    }

    #[test]
    fn turn_checkpoint_should_support_serde_round_trip() {
        let checkpoint = TurnCheckpoint::new(TurnId::new(), 12)
            .with_message(Message::assistant("done"))
            .with_tool_results(vec![ToolResult::success(
                "call-1",
                "read_file",
                vec![ToolContent::json(json!({ "path": "Cargo.toml" }))],
            )])
            .with_provider_id(ProviderId::new("codex"))
            .with_provider_session_id("provider-session-1")
            .mark_completed(2_000);
        let encoded = serde_json::to_string(&checkpoint).expect("turn checkpoint should serialize");
        let decoded: TurnCheckpoint =
            serde_json::from_str(&encoded).expect("turn checkpoint should deserialize");

        assert_eq!(decoded, checkpoint);
    }
}
