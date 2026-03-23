//! Claude Code generate helpers with truncation-aware retry logic.

use arky_provider::{
    generate_response_from_stream, GenerateResponse, Provider, ProviderError, ProviderRequest,
};

/// Runs a Claude generation request and retries once when the first attempt
/// terminates with a truncated structured-output error.
pub async fn generate_with_recovery<P>(
    provider: &P,
    request: ProviderRequest,
) -> Result<GenerateResponse, ProviderError>
where
    P: Provider + ?Sized,
{
    let session = request.session.clone();
    let turn = request.turn.clone();

    let first_stream = match provider.stream(request.clone()).await {
        Ok(stream) => stream,
        Err(error) if should_retry_for_truncation(&error) => {
            let retry_stream = provider.stream(request).await?;
            return generate_response_from_stream(session, turn, retry_stream).await;
        }
        Err(error) => return Err(error),
    };
    match generate_response_from_stream(session.clone(), turn.clone(), first_stream).await {
        Ok(response) => Ok(response),
        Err(error) if should_retry_for_truncation(&error) => {
            let retry_stream = provider.stream(request).await?;
            generate_response_from_stream(session, turn, retry_stream).await
        }
        Err(error) => Err(error),
    }
}

fn should_retry_for_truncation(error: &ProviderError) -> bool {
    let lower = error.to_string().to_ascii_lowercase();
    lower.contains("truncated")
        || lower.contains("incomplete json")
        || lower.contains("unexpected eof")
        || lower.contains("unterminated string")
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use async_trait::async_trait;
    use futures::stream;
    use pretty_assertions::assert_eq;

    use super::generate_with_recovery;
    use arky_protocol::{
        AgentEvent, EventMetadata, Message, ProviderId, ProviderSettings, SessionRef, TurnContext,
    };
    use arky_provider::{
        ModelRef, Provider, ProviderCapabilities, ProviderDescriptor, ProviderError,
        ProviderEventStream, ProviderFamily, ProviderRequest,
    };

    struct RetryingProvider {
        descriptor: ProviderDescriptor,
        responses: Mutex<VecDeque<Result<ProviderEventStream, ProviderError>>>,
    }

    #[async_trait]
    impl Provider for RetryingProvider {
        fn descriptor(&self) -> &ProviderDescriptor {
            &self.descriptor
        }

        async fn stream(
            &self,
            _request: ProviderRequest,
        ) -> Result<ProviderEventStream, ProviderError> {
            self.responses
                .lock()
                .expect("responses mutex should be available")
                .pop_front()
                .expect("test provider should have a queued response")
        }
    }

    fn make_request() -> ProviderRequest {
        ProviderRequest::new(
            SessionRef::default(),
            TurnContext::new(arky_protocol::TurnId::new(), 1),
            ModelRef::new("claude-sonnet"),
            vec![Message::user("hello")],
        )
        .with_settings(ProviderSettings::new())
    }

    #[tokio::test]
    async fn generate_with_recovery_should_retry_truncated_streams() {
        let mut responses = VecDeque::new();
        responses.push_back(Err(ProviderError::protocol_violation(
            "incomplete json",
            None,
        )));
        let recovered_stream: ProviderEventStream =
            Box::pin(stream::iter(vec![Ok(AgentEvent::MessageEnd {
                meta: EventMetadata::new(1, 1).with_provider_id(ProviderId::new("claude-code")),
                message: Message::assistant("recovered"),
            })]));
        let provider = RetryingProvider {
            descriptor: ProviderDescriptor::new(
                ProviderId::new("claude-code"),
                ProviderFamily::ClaudeCode,
                ProviderCapabilities::new()
                    .with_streaming(true)
                    .with_generate(true),
            ),
            responses: Mutex::new({
                responses.push_back(Ok(recovered_stream));
                responses
            }),
        };

        let response = generate_with_recovery(&provider, make_request())
            .await
            .expect("retry should recover");

        assert_eq!(response.message, Message::assistant("recovered"));
    }
}
