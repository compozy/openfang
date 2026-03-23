//! Stateful helpers for running one Codex turn stream.

use arky_protocol::{AgentEvent, EventMetadata, Message, ProviderId, ToolResult, Usage};
use tokio_util::sync::CancellationToken;

use crate::{CodexNotification, FingerprintDeduper, payload_has_error};

/// Mutable lifecycle state for one Codex turn stream.
#[derive(Debug, Clone, Default)]
pub struct CodexStreamState {
    /// Whether the pipeline has already emitted a terminal event.
    pub closed: bool,
    /// Most recent usage payload observed during the stream.
    pub last_usage: Option<Usage>,
    /// Terminal failure message when the turn failed before completion.
    pub turn_failure: Option<String>,
    /// Latest provider-native session identifier.
    pub session_id: Option<String>,
    /// Deduplication fingerprints collected for this turn.
    pub fingerprints: FingerprintDeduper,
}

/// Small pipeline wrapper for deduplication, metadata capture, and finalization.
#[derive(Debug, Clone, Default)]
pub struct CodexStreamPipeline {
    state: CodexStreamState,
}

impl CodexStreamPipeline {
    /// Creates an empty pipeline.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns immutable access to the current state snapshot.
    #[must_use]
    pub const fn state(&self) -> &CodexStreamState {
        &self.state
    }

    /// Records one raw notification and returns `true` if it should be processed.
    pub fn record_notification(&mut self, notification: &CodexNotification) -> bool {
        self.state.fingerprints.record(notification)
    }

    /// Stores response metadata emitted during the turn.
    pub fn record_response_metadata(&mut self, session_id: Option<String>, usage: Option<Usage>) {
        if session_id.is_some() {
            self.state.session_id = session_id;
        }
        if usage.is_some() {
            self.state.last_usage = usage;
        }
    }

    /// Records a terminal tool payload and flags the turn on tool failure.
    pub fn record_tool_payload(&mut self, payload: &serde_json::Value) {
        if payload_has_error(payload) && self.state.turn_failure.is_none() {
            self.state.turn_failure = Some("tool execution failed".to_owned());
        }
    }

    /// Returns an error when cancellation was requested.
    pub fn check_cancelled(
        &self,
        cancel: &CancellationToken,
    ) -> Result<(), arky_provider::ProviderError> {
        if cancel.is_cancelled() {
            return Err(arky_provider::ProviderError::stream_interrupted(
                "codex turn was cancelled",
            ));
        }

        Ok(())
    }

    /// Finalizes the turn and returns the terminal event set.
    #[must_use]
    pub fn finalize(
        &mut self,
        meta: EventMetadata,
        message: Message,
        tool_results: Vec<ToolResult>,
    ) -> Vec<AgentEvent> {
        self.state.closed = true;
        let mut events = Vec::new();

        if let Some(turn_failure) = &self.state.turn_failure {
            events.push(AgentEvent::Custom {
                meta: meta.clone(),
                event_type: "codex.turn_failed".to_owned(),
                payload: serde_json::json!({
                    "message": turn_failure,
                    "session_id": self.state.session_id,
                }),
            });
        }

        events.push(AgentEvent::TurnEnd {
            meta,
            message,
            tool_results,
            usage: self.state.last_usage.clone(),
        });
        events
    }

    /// Builds an initial stream-start custom event.
    #[must_use]
    pub fn stream_start_event(&self, meta: EventMetadata, provider_id: &ProviderId) -> AgentEvent {
        AgentEvent::Custom {
            meta,
            event_type: "stream-start".to_owned(),
            payload: serde_json::json!({
                "provider_id": provider_id,
                "session_id": self.state.session_id,
            }),
        }
    }

    /// Builds a response-metadata custom event for downstream reconciliation.
    #[must_use]
    pub fn response_metadata_event(
        &self,
        meta: EventMetadata,
        model_id: &str,
        response_id: &str,
    ) -> AgentEvent {
        AgentEvent::Custom {
            meta,
            event_type: "response-metadata".to_owned(),
            payload: serde_json::json!({
                "id": response_id,
                "model_id": model_id,
                "session_id": self.state.session_id,
                "usage": self.state.last_usage,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use arky_error::ClassifiedError;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use tokio_util::sync::CancellationToken;

    use super::CodexStreamPipeline;
    use crate::CodexNotification;
    use arky_protocol::{AgentEvent, EventMetadata, Message, ProviderId, Usage};

    #[test]
    fn pipeline_should_emit_stream_start_and_dedup_notifications() {
        let mut pipeline = CodexStreamPipeline::new();
        let notification = CodexNotification {
            method: "turn.started".to_owned(),
            params: json!({ "id": "turn-1" }),
        };

        let event =
            pipeline.stream_start_event(EventMetadata::new(1, 1), &ProviderId::new("codex"));

        assert_eq!(
            matches!(event, AgentEvent::Custom { event_type, .. } if event_type == "stream-start"),
            true,
        );
        assert_eq!(pipeline.record_notification(&notification), true);
        assert_eq!(pipeline.record_notification(&notification), false);
    }

    #[test]
    fn pipeline_should_emit_response_metadata_payload() {
        let mut pipeline = CodexStreamPipeline::new();
        pipeline.record_response_metadata(Some("thread-1".to_owned()), None);

        let event =
            pipeline.response_metadata_event(EventMetadata::new(1, 2), "gpt-5", "response-1");

        assert_eq!(
            matches!(
                event,
                AgentEvent::Custom { event_type, payload, .. }
                    if event_type == "response-metadata"
                        && payload["model_id"] == "gpt-5"
                        && payload["session_id"] == "thread-1"
                        && payload["id"] == "response-1"
            ),
            true,
        );
    }

    #[test]
    fn pipeline_should_finalize_with_usage_and_failure_metadata() {
        let mut pipeline = CodexStreamPipeline::new();
        pipeline.record_response_metadata(
            Some("session-1".to_owned()),
            Some(Usage {
                total_tokens: Some(12),
                ..Usage::default()
            }),
        );
        pipeline.record_tool_payload(&json!({ "exitCode": 1 }));

        let events = pipeline.finalize(
            EventMetadata::new(2, 2),
            Message::assistant("done"),
            Vec::new(),
        );

        assert_eq!(events.len(), 2);
        assert_eq!(matches!(events[0], AgentEvent::Custom { .. }), true,);
        assert_eq!(
            matches!(&events[1], AgentEvent::TurnEnd { usage, .. } if usage.as_ref().and_then(|usage| usage.total_tokens) == Some(12)),
            true,
        );
        assert_eq!(pipeline.state().closed, true);
    }

    #[test]
    fn pipeline_should_respect_cancellation() {
        let pipeline = CodexStreamPipeline::new();
        let cancel = CancellationToken::new();
        cancel.cancel();

        let error = pipeline
            .check_cancelled(&cancel)
            .expect_err("cancelled pipeline should fail");

        assert_eq!(error.error_code(), "PROVIDER_STREAM_INTERRUPTED");
    }
}
