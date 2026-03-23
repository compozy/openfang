//! Provider trait and stream-to-response helpers.

use std::pin::Pin;

use async_trait::async_trait;
use futures::{Stream, StreamExt};

use crate::{
    ProviderDescriptor, ProviderError, ProviderRequest,
    request::{GenerateResponse, SessionRef, TurnContext},
};
use arky_protocol::{AgentEvent, FinishReason, Message, Usage};

/// Standard provider stream type that can surface mid-stream failures in-band.
pub type ProviderEventStream =
    Pin<Box<dyn Stream<Item = Result<AgentEvent, ProviderError>> + Send>>;

/// Low-level provider contract implemented by concrete provider crates.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Returns immutable provider metadata.
    fn descriptor(&self) -> &ProviderDescriptor;

    /// Starts streaming a provider response for the supplied request.
    async fn stream(&self, request: ProviderRequest) -> Result<ProviderEventStream, ProviderError>;

    /// Generates a full response directly.
    ///
    /// Providers may override this to use native non-streaming APIs. The
    /// default implementation drains [`Self::stream`] and synthesizes a final
    /// response from the terminal message events.
    async fn generate(&self, request: ProviderRequest) -> Result<GenerateResponse, ProviderError> {
        let session = request.session.clone();
        let turn = request.turn.clone();
        let stream = self.stream(request).await?;
        generate_response_from_stream(session, turn, stream).await
    }
}

/// Drains a provider event stream into a [`GenerateResponse`].
pub async fn generate_response_from_stream(
    session: SessionRef,
    turn: TurnContext,
    mut stream: ProviderEventStream,
) -> Result<GenerateResponse, ProviderError> {
    let mut terminal_message: Option<Message> = None;
    let mut finish_reason: Option<FinishReason> = None;
    let mut usage: Option<Usage> = None;

    while let Some(item) = stream.next().await {
        let event = item?;
        if let Some(message) = terminal_message_from_event(&event) {
            terminal_message = Some(message);
        }
        if let Some(event_usage) = usage_from_event(&event) {
            usage = Some(event_usage);
        }
        if let Some(event_finish_reason) = finish_reason_from_event(&event) {
            finish_reason = Some(event_finish_reason);
        }
    }

    let message = terminal_message.ok_or_else(|| {
        ProviderError::protocol_violation(
            "provider stream completed without emitting a terminal assistant message",
            None,
        )
    })?;

    let mut response = GenerateResponse::new(session, turn, message);
    if let Some(finish_reason) = finish_reason {
        response = response.with_finish_reason(finish_reason);
    }
    if let Some(usage) = usage {
        response = response.with_usage(usage);
    }

    Ok(response)
}

pub fn terminal_message_from_event(event: &AgentEvent) -> Option<Message> {
    match event {
        AgentEvent::MessageStart { message, .. }
        | AgentEvent::MessageUpdate { message, .. }
        | AgentEvent::TurnEnd { message, .. }
        | AgentEvent::MessageEnd { message, .. } => Some(message.clone()),
        AgentEvent::AgentEnd { messages, .. } => messages
            .iter()
            .rev()
            .find(|message| matches!(message.role, arky_protocol::Role::Assistant))
            .cloned(),
        AgentEvent::AgentStart { .. }
        | AgentEvent::TurnStart { .. }
        | AgentEvent::ToolExecutionStart { .. }
        | AgentEvent::ToolExecutionUpdate { .. }
        | AgentEvent::ToolExecutionEnd { .. }
        | AgentEvent::Custom { .. }
        | _ => None,
    }
}

fn usage_from_event(event: &AgentEvent) -> Option<Usage> {
    match event {
        AgentEvent::Custom { payload, .. } => payload
            .get("usage")
            .cloned()
            .and_then(|usage| serde_json::from_value(usage).ok()),
        _ => None,
    }
}

fn finish_reason_from_event(event: &AgentEvent) -> Option<FinishReason> {
    let value = match event {
        AgentEvent::Custom { payload, .. } => payload
            .get("finish_reason")
            .or_else(|| payload.get("stop_reason"))
            .and_then(serde_json::Value::as_str),
        _ => None,
    }?;

    Some(match value {
        "end_turn" | "stop" | "stop_sequence" => FinishReason::Stop,
        "max_tokens" | "length" => FinishReason::Length,
        "tool_use" | "tool_calls" => FinishReason::ToolUse,
        "content_filter" => FinishReason::ContentFilter,
        "error" => FinishReason::Error,
        _ => FinishReason::Unknown,
    })
}

#[cfg(test)]
mod tests {
    use futures::stream;
    use pretty_assertions::assert_eq;

    use super::generate_response_from_stream;
    use crate::ProviderError;
    use arky_protocol::{
        AgentEvent, EventMetadata, FinishReason, Message, SessionId, SessionRef, TurnContext,
        TurnId,
    };

    #[tokio::test]
    async fn generate_response_from_stream_should_use_terminal_message_event() {
        let session = SessionRef::new(Some(SessionId::new()));
        let turn = TurnContext::new(TurnId::new(), 1);
        let message = Message::assistant("done");
        let stream = Box::pin(stream::iter(vec![
            Ok(AgentEvent::MessageStart {
                meta: EventMetadata::new(1, 1),
                message: Message::assistant("partial"),
            }),
            Ok(AgentEvent::MessageEnd {
                meta: EventMetadata::new(2, 2),
                message: message.clone(),
            }),
        ]));

        let response = generate_response_from_stream(session, turn, stream)
            .await
            .expect("response should be synthesized");

        assert_eq!(response.message, message);
    }

    #[tokio::test]
    async fn generate_response_from_stream_should_collect_usage_and_finish_reason() {
        let session = SessionRef::new(Some(SessionId::new()));
        let turn = TurnContext::new(TurnId::new(), 1);
        let stream = Box::pin(stream::iter(vec![
            Ok(AgentEvent::MessageUpdate {
                meta: EventMetadata::new(1, 1),
                message: Message::assistant("partial"),
                delta: arky_protocol::StreamDelta::text("partial"),
            }),
            Ok(AgentEvent::Custom {
                meta: EventMetadata::new(2, 2),
                event_type: "provider.finish".to_owned(),
                payload: serde_json::json!({
                    "finish_reason": "tool_use",
                    "usage": {
                        "input_tokens": 5,
                        "output_tokens": 7,
                        "total_tokens": 12,
                    }
                }),
            }),
            Ok(AgentEvent::MessageEnd {
                meta: EventMetadata::new(3, 3),
                message: Message::assistant("final"),
            }),
        ]));

        let response = generate_response_from_stream(session, turn, stream)
            .await
            .expect("response should be synthesized");

        assert_eq!(response.finish_reason, Some(FinishReason::ToolUse));
        assert_eq!(
            response.usage.as_ref().and_then(|usage| usage.total_tokens),
            Some(12)
        );
    }

    #[tokio::test]
    async fn generate_response_from_stream_should_reject_missing_terminal_message() {
        let session = SessionRef::new(Some(SessionId::new()));
        let turn = TurnContext::new(TurnId::new(), 1);
        let stream = Box::pin(stream::iter(vec![Ok(AgentEvent::TurnStart {
            meta: EventMetadata::new(1, 1),
        })]));

        let error = generate_response_from_stream(session, turn, stream)
            .await
            .expect_err("missing terminal message should fail");

        assert!(matches!(error, ProviderError::ProtocolViolation { .. }));
    }
}
