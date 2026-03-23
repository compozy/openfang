//! Provider contracts, registries, and shared subprocess infrastructure.
//!
//! Concrete provider crates implement the traits and reuse the shared
//! subprocess, transport, replay, and stream-to-response helpers defined here.

mod contract_tests;
mod descriptor;
mod discovery;
mod error;
mod family;
mod process;
mod reasoning;
mod registry;
mod replay;
mod request;
mod traits;
mod transport;

pub use crate::{
    contract_tests::{ProviderContractCase, ProviderContractTests},
    descriptor::{
        CapabilityWarning, ProviderCapabilities, ProviderDescriptor, ProviderFamily,
        messages_have_image_inputs, validate_capabilities,
    },
    discovery::{ModelCost, ModelDiscoveryService, ModelInfo},
    error::ProviderError,
    family::{ResolvedProviderFamily, resolve_provider_family},
    process::{ManagedProcess, ProcessConfig, ProcessManager, RestartPolicy},
    reasoning::{
        XHIGH_CAPABLE_MODEL_IDS, map_max_thinking_tokens_to_reasoning_effort,
        resolve_claude_max_thinking_tokens, resolve_reasoning_for_provider,
        supports_xhigh_reasoning,
    },
    registry::{ProviderRegistry, infer_provider_id},
    replay::{ReplayWriter, ReplayWriterConfig},
    request::{
        GenerateResponse, HookContext, ModelRef, ProviderRequest, ProviderSettings, SessionRef,
        ToolContext, TurnContext,
    },
    traits::{Provider, ProviderEventStream, generate_response_from_stream},
    transport::{StdioTransport, StdioTransportConfig},
};
pub use arky_hooks::Hooks;
pub use arky_session::SessionStore;
pub use arky_tools::{
    ParsedProviderToolName, StaticToolIdCodec, ToolIdCodec, create_claude_code_tool_id_codec,
    create_claude_compatible_tool_id_codec, create_codex_tool_id_codec,
    create_opencode_tool_id_codec,
};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arky_protocol::{Message, ProviderId, SessionId, TurnId};
    use futures::stream;

    use crate::{
        Provider, ProviderCapabilities, ProviderDescriptor, ProviderError, ProviderEventStream,
        ProviderFamily, ProviderRequest,
    };

    fn assert_send_sync<T: Send + Sync>() {}

    struct SendSyncProvider {
        descriptor: ProviderDescriptor,
    }

    #[async_trait::async_trait]
    impl Provider for SendSyncProvider {
        fn descriptor(&self) -> &ProviderDescriptor {
            &self.descriptor
        }

        async fn stream(
            &self,
            _request: ProviderRequest,
        ) -> Result<ProviderEventStream, ProviderError> {
            Ok(Box::pin(stream::empty()))
        }
    }

    #[test]
    fn provider_trait_and_registry_types_should_be_send_and_sync() {
        assert_send_sync::<Arc<dyn Provider>>();
        assert_send_sync::<crate::ProviderRegistry>();
        assert_send_sync::<crate::ProcessManager>();
        assert_send_sync::<crate::ReplayWriter>();
        assert_send_sync::<crate::StdioTransport>();
    }

    #[test]
    fn provider_event_stream_should_support_mid_stream_failures() {
        fn assert_stream(_: &ProviderEventStream) {}

        let descriptor = ProviderDescriptor::new(
            ProviderId::new("mock"),
            ProviderFamily::Custom("mock".to_owned()),
            ProviderCapabilities::default(),
        );
        let provider = SendSyncProvider { descriptor };
        let request = ProviderRequest::new(
            arky_protocol::SessionRef::new(Some(SessionId::new())),
            arky_protocol::TurnContext::new(TurnId::new(), 1),
            arky_protocol::ModelRef::new("mock-model"),
            vec![Message::user("hi")],
        );
        let stream = tokio::runtime::Runtime::new()
            .expect("runtime should build")
            .block_on(provider.stream(request))
            .expect("stream should construct");

        assert_stream(&stream);
    }
}
