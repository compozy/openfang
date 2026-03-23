//! Integration tests for the shared provider contract suite.

use arky_protocol::{
    AgentEvent, EventMetadata, Message, ModelRef, ProviderId, SessionId, SessionRef, TurnContext,
    TurnId,
};
use arky_provider::{
    Provider, ProviderCapabilities, ProviderContractCase, ProviderContractTests,
    ProviderDescriptor, ProviderError, ProviderEventStream, ProviderFamily, ProviderRequest,
};
use futures::stream;

struct MockProvider {
    descriptor: ProviderDescriptor,
}

#[async_trait::async_trait]
impl Provider for MockProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    async fn stream(&self, request: ProviderRequest) -> Result<ProviderEventStream, ProviderError> {
        let session_id = request
            .session
            .id
            .clone()
            .expect("contract case provides a session id");
        let turn_id = request.turn.id.clone();
        let provider_id = self.descriptor.id.clone();
        let message = Message::assistant("done");

        Ok(Box::pin(stream::iter(vec![
            Ok(AgentEvent::TurnStart {
                meta: EventMetadata::new(1, 1)
                    .with_session_id(session_id.clone())
                    .with_turn_id(turn_id.clone())
                    .with_provider_id(provider_id.clone()),
            }),
            Ok(AgentEvent::MessageEnd {
                meta: EventMetadata::new(2, 2)
                    .with_session_id(session_id)
                    .with_turn_id(turn_id)
                    .with_provider_id(provider_id),
                message: message.clone(),
            }),
            Ok(AgentEvent::TurnEnd {
                meta: EventMetadata::new(3, 3)
                    .with_session_id(request.session.id.expect("session id should exist"))
                    .with_turn_id(request.turn.id)
                    .with_provider_id(self.descriptor.id.clone()),
                message,
                tool_results: Vec::new(),
                usage: None,
            }),
        ])))
    }
}

#[tokio::test]
async fn provider_contract_tests_should_apply_to_a_mock_provider() {
    let provider = MockProvider {
        descriptor: ProviderDescriptor::new(
            ProviderId::new("mock"),
            ProviderFamily::Custom("mock".to_owned()),
            ProviderCapabilities::new()
                .with_streaming(true)
                .with_generate(true),
        ),
    };
    let request = ProviderRequest::new(
        SessionRef::new(Some(SessionId::new())),
        TurnContext::new(TurnId::new(), 1),
        ModelRef::new("mock-model"),
        vec![Message::user("hello")],
    );
    let case = ProviderContractCase {
        request,
        expected_descriptor: provider.descriptor().clone(),
        expected_message: Message::assistant("done"),
        min_event_count: 3,
    };

    ProviderContractTests::assert_provider(&provider, &case)
        .await
        .expect("contract suite should pass");
}
