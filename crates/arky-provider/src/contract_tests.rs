//! Shared provider contract test helpers.

use crate::{Provider, ProviderDescriptor, ProviderRequest, traits::terminal_message_from_event};
use arky_protocol::{AgentEvent, Message};
use futures::StreamExt;

/// Inputs required to exercise the shared provider contract tests.
#[derive(Debug, Clone)]
pub struct ProviderContractCase {
    /// Request used for both streaming and direct generation assertions.
    pub request: ProviderRequest,
    /// Expected immutable descriptor.
    pub expected_descriptor: ProviderDescriptor,
    /// Expected terminal assistant message.
    pub expected_message: Message,
    /// Minimum number of events that should be emitted during streaming.
    pub min_event_count: usize,
}

/// Shared behavioral assertions every provider implementation must satisfy.
pub struct ProviderContractTests;

impl ProviderContractTests {
    /// Runs the shared provider contract assertions.
    pub async fn assert_provider<P>(provider: &P, case: &ProviderContractCase) -> Result<(), String>
    where
        P: Provider + ?Sized,
    {
        if provider.descriptor() != &case.expected_descriptor {
            return Err("provider descriptor did not match the expected descriptor".to_owned());
        }

        let mut stream = provider
            .stream(case.request.clone())
            .await
            .map_err(|error| format!("provider stream should construct: {error}"))?;
        let mut events = Vec::new();
        while let Some(item) = stream.next().await {
            events.push(item.map_err(|error| format!("stream event should be valid: {error}"))?);
        }

        if events.len() < case.min_event_count {
            return Err(format!(
                "provider emitted {} events but contract requires at least {}",
                events.len(),
                case.min_event_count
            ));
        }

        Self::assert_event_metadata(&events, case)?;
        if extract_terminal_message(&events) != Some(case.expected_message.clone()) {
            return Err("provider stream did not emit the expected terminal message".to_owned());
        }

        let generated = provider
            .generate(case.request.clone())
            .await
            .map_err(|error| format!("generate should succeed: {error}"))?;
        if generated.session != case.request.session {
            return Err("generate response session did not match the request".to_owned());
        }
        if generated.turn != case.request.turn {
            return Err("generate response turn did not match the request".to_owned());
        }
        if generated.message != case.expected_message {
            return Err("generate response message did not match the expected message".to_owned());
        }

        Ok(())
    }

    fn assert_event_metadata(
        events: &[AgentEvent],
        case: &ProviderContractCase,
    ) -> Result<(), String> {
        let session_id = case.request.session.id.clone();
        let provider_id = case.expected_descriptor.id.clone();
        let mut previous_sequence = None;

        for event in events {
            let metadata = event.metadata();
            if let Some(expected_session_id) = &session_id
                && metadata.session_id.as_ref() != Some(expected_session_id)
            {
                return Err("event metadata must carry the request session id".to_owned());
            }
            if metadata.provider_id.as_ref() != Some(&provider_id) {
                return Err(
                    "provider-originated events must advertise the descriptor id".to_owned(),
                );
            }
            if let Some(previous_sequence) = previous_sequence
                && metadata.sequence <= previous_sequence
            {
                return Err("event sequence must be strictly monotonic".to_owned());
            }
            previous_sequence = Some(metadata.sequence);
        }

        Ok(())
    }
}

fn extract_terminal_message(events: &[AgentEvent]) -> Option<Message> {
    events.iter().rev().find_map(terminal_message_from_event)
}
