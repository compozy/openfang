//! Arky `Provider` to OpenFang `LlmDriver` bridge.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use arky_config::ProviderConfig;
use arky_protocol::{
    AgentEvent, Message as ArkyMessage, ModelRef, SessionRef, StreamDelta, TurnContext, TurnId,
    Usage,
};
use arky_provider::{Provider, ProviderFamily};
use futures::StreamExt;
use openfang_runtime::llm_driver::{
    CompletionRequest, CompletionResponse, LlmDriver, LlmError, StreamEvent,
};
use openfang_types::{
    message::{ContentBlock, StopReason},
    tool::ToolCall,
};
use thiserror::Error;
use tokio::sync::mpsc;

use crate::{
    adapter::{build_provider_config, AdapterError},
    convert::{
        agent_event_to_stream_event, arky_blocks_to_of, completion_request_to_provider,
        finish_reason_from_payload, finish_reason_to_stop_reason, provider_error_to_llm,
        thinking_blocks, usage_from_payload, usage_to_token_usage, ConvertError,
    },
    instantiate::{instantiate_provider, InstantiateError},
    ProviderBinding,
};

/// Errors returned while turning one compiled binding into a live driver.
#[derive(Debug, Error)]
pub enum BridgeError {
    /// The compiled binding could not be adapted into typed runtime config.
    #[error(transparent)]
    Adapter(#[from] AdapterError),
    /// The typed runtime config could not be instantiated into a live provider.
    #[error(transparent)]
    Instantiate(#[from] InstantiateError),
}

/// Wraps an Arky provider behind the OpenFang `LlmDriver` trait.
pub struct ArkyDriverBridge {
    provider: Arc<dyn Provider>,
    model_ref: ModelRef,
    provider_family: ProviderFamily,
    turn_sequence: AtomicU64,
}

impl ArkyDriverBridge {
    /// Creates a bridge for one live provider instance.
    #[must_use]
    pub fn new(provider: Arc<dyn Provider>, model_ref: ModelRef) -> Self {
        let provider_family = provider.descriptor().family.clone();
        Self {
            provider,
            model_ref,
            provider_family,
            turn_sequence: AtomicU64::new(0),
        }
    }

    fn next_turn(&self) -> TurnContext {
        let sequence = self.turn_sequence.fetch_add(1, Ordering::Relaxed) + 1;
        TurnContext::new(TurnId::new(), sequence)
    }

    async fn collect_response(
        &self,
        request: CompletionRequest,
        stream_tx: Option<mpsc::Sender<StreamEvent>>,
    ) -> Result<CompletionResponse, LlmError> {
        let provider_request = completion_request_to_provider(
            &request,
            &self.model_ref,
            &self.provider_family,
            SessionRef::new(None),
            self.next_turn(),
        )
        .map_err(convert_error_to_llm)?;
        let mut provider_stream = self
            .provider
            .stream(provider_request)
            .await
            .map_err(provider_error_to_llm)?;

        let mut collector = TurnCollector::new(self.provider_family.clone());
        let mut stream_tx = stream_tx;

        while let Some(item) = provider_stream.next().await {
            let event = item.map_err(provider_error_to_llm)?;
            let stream_events = collector.observe(&event);
            if let Some(sender) = stream_tx.as_mut() {
                for stream_event in stream_events {
                    if sender.send(stream_event).await.is_err() {
                        stream_tx = None;
                        break;
                    }
                }
            }
        }

        if let Some(sender) = stream_tx.as_mut() {
            for stream_event in collector.flush_pending_tool_use_end_events() {
                if sender.send(stream_event).await.is_err() {
                    stream_tx = None;
                    break;
                }
            }
        }

        let response = collector.into_completion_response()?;
        if let Some(sender) = stream_tx.as_mut() {
            for stream_event in collector_terminal_stream_events(&response) {
                if sender.send(stream_event).await.is_err() {
                    break;
                }
            }
        }

        Ok(response)
    }
}

#[async_trait::async_trait]
impl LlmDriver for ArkyDriverBridge {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.collect_response(request, None).await
    }

    async fn stream(
        &self,
        request: CompletionRequest,
        tx: mpsc::Sender<StreamEvent>,
    ) -> Result<CompletionResponse, LlmError> {
        self.collect_response(request, Some(tx)).await
    }
}

/// Completes the compiled-binding pipeline by returning a live OpenFang driver.
pub fn binding_to_driver(
    binding: &ProviderBinding,
    install: &ProviderConfig,
) -> Result<Arc<dyn LlmDriver>, BridgeError> {
    let config = build_provider_config(binding, install)?;
    let provider = instantiate_provider(config)?;
    Ok(Arc::new(ArkyDriverBridge::new(
        provider,
        binding.model.clone(),
    )))
}

#[derive(Debug, Clone)]
struct CollectedToolCall {
    id: String,
    name: String,
    input: serde_json::Value,
    input_fragments: String,
    emitted_end: bool,
}

impl CollectedToolCall {
    fn new(id: String, name: String, input: serde_json::Value) -> Self {
        Self {
            id,
            name,
            input,
            input_fragments: String::new(),
            emitted_end: false,
        }
    }

    fn resolved_input(&self) -> serde_json::Value {
        if self.input.is_null() && !self.input_fragments.is_empty() {
            return serde_json::from_str(&self.input_fragments)
                .unwrap_or_else(|_| serde_json::Value::String(self.input_fragments.clone()));
        }

        self.input.clone()
    }
}

#[derive(Debug)]
struct TurnCollector {
    provider_family: ProviderFamily,
    terminal_message: Option<ArkyMessage>,
    reasoning_order: Vec<String>,
    reasoning: BTreeMap<String, String>,
    tool_call_order: Vec<String>,
    tool_calls: HashMap<String, CollectedToolCall>,
    tool_results: Vec<arky_protocol::ToolResult>,
    usage: Option<Usage>,
    finish_reason: Option<arky_protocol::FinishReason>,
}

impl TurnCollector {
    fn new(provider_family: ProviderFamily) -> Self {
        Self {
            provider_family,
            terminal_message: None,
            reasoning_order: Vec::new(),
            reasoning: BTreeMap::new(),
            tool_call_order: Vec::new(),
            tool_calls: HashMap::new(),
            tool_results: Vec::new(),
            usage: None,
            finish_reason: None,
        }
    }

    fn observe(&mut self, event: &AgentEvent) -> Vec<StreamEvent> {
        let mut emitted = Vec::new();
        match event {
            AgentEvent::MessageStart { message, .. } | AgentEvent::MessageEnd { message, .. } => {
                self.terminal_message = Some(message.clone());
            }
            AgentEvent::MessageUpdate { message, delta, .. } => {
                self.terminal_message = Some(message.clone());
                match delta {
                    StreamDelta::ToolUse { call } => {
                        self.record_tool_call(
                            call.id.clone(),
                            call.name.clone(),
                            call.input.clone(),
                        );
                        if let Some(call) = self.tool_calls.get_mut(&call.id) {
                            if !call.emitted_end {
                                call.emitted_end = true;
                                emitted.push(StreamEvent::ToolUseEnd {
                                    id: call.id.clone(),
                                    name: call.name.clone(),
                                    input: call.resolved_input(),
                                });
                            }
                        }
                    }
                    StreamDelta::ToolResult { result } => {
                        self.tool_results.push(result.clone());
                    }
                    StreamDelta::Replace { content } => {
                        self.terminal_message =
                            Some(ArkyMessage::new(message.role, content.clone()));
                    }
                    StreamDelta::ToolUseInput {
                        tool_call_id,
                        delta,
                    } => {
                        self.record_tool_input_delta(tool_call_id, delta);
                        if let Some(stream_event) = agent_event_to_stream_event(event) {
                            emitted.push(stream_event);
                        }
                    }
                    StreamDelta::Text { .. } => {
                        if let Some(stream_event) = agent_event_to_stream_event(event) {
                            emitted.push(stream_event);
                        }
                    }
                    StreamDelta::Image { .. } => {}
                }
            }
            AgentEvent::TurnEnd {
                message,
                tool_results,
                usage,
                ..
            } => {
                self.terminal_message = Some(message.clone());
                if !tool_results.is_empty() {
                    self.tool_results = tool_results.clone();
                }
                if usage.is_some() {
                    self.usage = usage.clone();
                }
            }
            AgentEvent::ReasoningStart { reasoning_id, .. } => {
                self.ensure_reasoning_slot(reasoning_id);
            }
            AgentEvent::ReasoningDelta {
                reasoning_id,
                delta,
                ..
            } => {
                self.ensure_reasoning_slot(reasoning_id);
                self.reasoning
                    .entry(reasoning_id.clone())
                    .or_default()
                    .push_str(delta);
                if let Some(stream_event) = agent_event_to_stream_event(event) {
                    emitted.push(stream_event);
                }
            }
            AgentEvent::ReasoningComplete {
                reasoning_id,
                full_text,
                ..
            } => {
                self.ensure_reasoning_slot(reasoning_id);
                self.reasoning
                    .insert(reasoning_id.clone(), full_text.clone());
            }
            AgentEvent::ToolExecutionStart {
                tool_call_id,
                tool_name,
                args,
                ..
            } => {
                self.record_tool_call(tool_call_id.clone(), tool_name.clone(), args.clone());
                emitted.push(StreamEvent::ToolUseStart {
                    id: tool_call_id.clone(),
                    name: tool_name.clone(),
                });
                if matches!(self.provider_family, ProviderFamily::Codex) {
                    if let Some(call) = self.tool_calls.get_mut(tool_call_id) {
                        if !call.emitted_end {
                            call.emitted_end = true;
                            emitted.push(StreamEvent::ToolUseEnd {
                                id: call.id.clone(),
                                name: call.name.clone(),
                                input: call.resolved_input(),
                            });
                        }
                    }
                }
            }
            AgentEvent::ToolExecutionEnd {
                tool_name,
                result,
                is_error,
                ..
            } => emitted.push(StreamEvent::ToolExecutionResult {
                name: tool_name.clone(),
                result_preview: tool_execution_preview(result),
                is_error: *is_error,
            }),
            AgentEvent::Custom { payload, .. } => {
                if let Some(finish_reason) = finish_reason_from_payload(payload) {
                    self.finish_reason = Some(finish_reason);
                }
                if let Some(usage) = usage_from_payload(payload) {
                    self.usage = Some(usage);
                }
            }
            AgentEvent::AgentStart { .. }
            | AgentEvent::AgentEnd { .. }
            | AgentEvent::TurnStart { .. }
            | AgentEvent::ToolExecutionUpdate { .. }
            | _ => {}
        }

        emitted
    }

    fn ensure_reasoning_slot(&mut self, reasoning_id: &str) {
        if self.reasoning.contains_key(reasoning_id) {
            return;
        }

        self.reasoning_order.push(reasoning_id.to_owned());
        self.reasoning
            .insert(reasoning_id.to_owned(), String::new());
    }

    fn record_tool_call(&mut self, id: String, name: String, input: serde_json::Value) {
        if !self.tool_calls.contains_key(&id) {
            self.tool_call_order.push(id.clone());
        }
        self.tool_calls
            .entry(id.clone())
            .and_modify(|call| {
                call.name = name.clone();
                call.input = input.clone();
            })
            .or_insert_with(|| CollectedToolCall::new(id, name, input));
    }

    fn record_tool_input_delta(&mut self, tool_call_id: &str, delta: &str) {
        if !self.tool_calls.contains_key(tool_call_id) {
            self.tool_call_order.push(tool_call_id.to_owned());
        }
        self.tool_calls
            .entry(tool_call_id.to_owned())
            .and_modify(|call| call.input_fragments.push_str(delta))
            .or_insert_with(|| {
                let mut call = CollectedToolCall::new(
                    tool_call_id.to_owned(),
                    "unknown".to_owned(),
                    serde_json::Value::Null,
                );
                call.input_fragments.push_str(delta);
                call
            });
    }

    fn tool_calls(&self) -> Vec<ToolCall> {
        self.tool_call_order
            .iter()
            .filter_map(|tool_call_id| self.tool_calls.get(tool_call_id))
            .map(|call| ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                input: call.resolved_input(),
            })
            .collect()
    }

    fn flush_pending_tool_use_end_events(&mut self) -> Vec<StreamEvent> {
        let pending_ids = self
            .tool_call_order
            .iter()
            .filter_map(|tool_call_id| {
                self.tool_calls
                    .get(tool_call_id)
                    .filter(|call| !call.emitted_end)
                    .map(|call| call.id.clone())
            })
            .collect::<Vec<_>>();

        let mut events = Vec::new();
        for tool_call_id in pending_ids {
            if let Some(call) = self.tool_calls.get_mut(&tool_call_id) {
                call.emitted_end = true;
                events.push(StreamEvent::ToolUseEnd {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    input: call.resolved_input(),
                });
            }
        }
        events
    }

    fn into_completion_response(self) -> Result<CompletionResponse, LlmError> {
        let collected_tool_calls = self.tool_calls();
        let terminal_message = self.terminal_message.clone().ok_or_else(|| {
            LlmError::Parse(
                "provider stream completed without emitting a terminal assistant message"
                    .to_owned(),
            )
        })?;

        let mut content = thinking_blocks(&self.reasoning, &self.reasoning_order);
        content.extend(arky_blocks_to_of(&terminal_message.content));

        let stop_reason = self.resolve_stop_reason(&content, &collected_tool_calls);
        let tool_calls = if stop_reason == StopReason::ToolUse {
            collected_tool_calls
        } else {
            Vec::new()
        };

        if stop_reason == StopReason::ToolUse && !contains_tool_use_block(&content) {
            content.extend(tool_use_blocks(&tool_calls));
        }

        Ok(CompletionResponse {
            content,
            stop_reason,
            tool_calls,
            usage: usage_to_token_usage(self.usage.as_ref()),
        })
    }

    fn resolve_stop_reason(&self, content: &[ContentBlock], tool_calls: &[ToolCall]) -> StopReason {
        if let Some(finish_reason) = self.finish_reason {
            return finish_reason_to_stop_reason(finish_reason);
        }

        if contains_tool_use_block(content) || !tool_calls.is_empty() {
            return StopReason::ToolUse;
        }

        StopReason::EndTurn
    }
}

fn convert_error_to_llm(error: ConvertError) -> LlmError {
    LlmError::Parse(error.to_string())
}

fn collector_terminal_stream_events(response: &CompletionResponse) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    for block in &response.content {
        if let ContentBlock::Thinking { thinking } = block {
            if !thinking.is_empty() {
                // The collector already streamed reasoning deltas when present.
                // Only emit these blocks at completion when there were no deltas.
                continue;
            }
        }
    }
    events.push(StreamEvent::ContentComplete {
        stop_reason: response.stop_reason,
        usage: response.usage,
    });
    events
}

fn contains_tool_use_block(content: &[ContentBlock]) -> bool {
    content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolUse { .. }))
}

fn tool_use_blocks(tool_calls: &[ToolCall]) -> Vec<ContentBlock> {
    tool_calls
        .iter()
        .map(|tool_call| ContentBlock::ToolUse {
            id: tool_call.id.clone(),
            name: tool_call.name.clone(),
            input: tool_call.input.clone(),
            provider_metadata: None,
        })
        .collect()
}

fn tool_execution_preview(result: &serde_json::Value) -> String {
    match result {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use arky_claude_code::{ClaudeFilesystemConfig, ClaudeSessionConfig};
    use arky_config::{
        AgentConfigBuilder, ArkyConfigBuilder, ProviderConfigBuilder, ProviderRequestDefaults,
        ResolvedClaudeCodeBehaviorConfig, ResolvedClaudeCompatibleBehaviorConfig,
        ResolvedCodexBehaviorConfig, ResolvedProviderBehaviorConfig,
    };
    use arky_protocol::ReasoningEffort;
    use arky_protocol::{
        AgentEvent, ContentBlock as ArkyContentBlock, EventMetadata, Message as ArkyMessage,
        ModelRef, ProviderId, StreamDelta, ToolCall as ArkyToolCall, Usage,
    };
    use arky_provider::{
        Provider, ProviderCapabilities, ProviderDescriptor, ProviderEventStream, ProviderFamily,
    };
    use futures::stream;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{binding_to_driver, ArkyDriverBridge};
    use crate::ProviderBinding;
    use openfang_runtime::llm_driver::{CompletionRequest, LlmDriver, LlmError, StreamEvent};
    use openfang_types::{
        message::{ContentBlock, Message, StopReason},
        tool::ToolDefinition,
    };
    use tokio::sync::mpsc;

    #[derive(Debug)]
    struct FakeProvider {
        descriptor: ProviderDescriptor,
        events: Vec<Result<AgentEvent, arky_provider::ProviderError>>,
        seen_requests: Mutex<Vec<arky_protocol::ProviderRequest>>,
    }

    impl FakeProvider {
        fn new(
            family: ProviderFamily,
            events: Vec<Result<AgentEvent, arky_provider::ProviderError>>,
        ) -> Self {
            Self {
                descriptor: ProviderDescriptor::new(
                    ProviderId::new("fake"),
                    family,
                    ProviderCapabilities::new()
                        .with_streaming(true)
                        .with_generate(true)
                        .with_tool_calls(true)
                        .with_extended_thinking(true),
                ),
                events,
                seen_requests: Mutex::new(Vec::new()),
            }
        }

        fn recorded_turn_sequences(&self) -> Vec<u64> {
            self.seen_requests
                .lock()
                .expect("request recording mutex should not poison")
                .iter()
                .map(|request| request.turn.sequence)
                .collect()
        }
    }

    #[async_trait::async_trait]
    impl Provider for FakeProvider {
        fn descriptor(&self) -> &ProviderDescriptor {
            &self.descriptor
        }

        async fn stream(
            &self,
            request: arky_protocol::ProviderRequest,
        ) -> Result<ProviderEventStream, arky_provider::ProviderError> {
            self.seen_requests
                .lock()
                .expect("request recording mutex should not poison")
                .push(request);
            Ok(Box::pin(stream::iter(self.events.clone())))
        }
    }

    #[tokio::test]
    async fn complete_should_preserve_text_thinking_tool_calls_and_usage() {
        let bridge = ArkyDriverBridge::new(
            Arc::new(FakeProvider::new(
                ProviderFamily::ClaudeCode,
                claude_tool_use_events(),
            )),
            ModelRef::new("claude-sonnet").with_provider_id(ProviderId::new("claude-code")),
        );

        let response = bridge
            .complete(sample_request())
            .await
            .expect("completion should succeed");

        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert_eq!(response.text(), "Hello from bridge");
        assert_eq!(response.tool_calls.len(), 1);
        assert!(matches!(
            response.content.first(),
            Some(ContentBlock::Thinking { thinking }) if thinking == "Need tool context"
        ));
        assert_eq!(response.usage.input_tokens, 12);
        assert_eq!(response.usage.output_tokens, 8);
    }

    #[tokio::test]
    async fn stream_should_forward_deltas_and_synthesize_response() {
        let bridge = ArkyDriverBridge::new(
            Arc::new(FakeProvider::new(
                ProviderFamily::ClaudeCode,
                claude_tool_use_events(),
            )),
            ModelRef::new("claude-sonnet").with_provider_id(ProviderId::new("claude-code")),
        );
        let (tx, mut rx) = mpsc::channel(32);

        let response = bridge
            .stream(sample_request(), tx)
            .await
            .expect("streaming completion should succeed");

        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
            if matches!(events.last(), Some(StreamEvent::ContentComplete { .. })) {
                break;
            }
        }

        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert!(events.iter().any(|event| matches!(
            event,
            StreamEvent::ThinkingDelta { text } if text == "Need tool context"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            StreamEvent::TextDelta { text } if text == "Hello from bridge"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            StreamEvent::ToolUseStart { id, name } if id == "call-1" && name == "read_file"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            StreamEvent::ToolInputDelta { text } if text.contains("\"path\"")
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            StreamEvent::ToolUseEnd { id, name, .. } if id == "call-1" && name == "read_file"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            StreamEvent::ToolExecutionResult { name, is_error, .. }
            if name == "read_file" && !is_error
        )));
        assert!(matches!(
            events.last(),
            Some(StreamEvent::ContentComplete {
                stop_reason: StopReason::ToolUse,
                usage
            }) if usage.input_tokens == 12 && usage.output_tokens == 8
        ));
    }

    #[tokio::test]
    async fn complete_should_fail_without_terminal_message() {
        let bridge = ArkyDriverBridge::new(
            Arc::new(FakeProvider::new(
                ProviderFamily::Codex,
                vec![Ok(AgentEvent::TurnStart {
                    meta: EventMetadata::new(1, 1),
                })],
            )),
            ModelRef::new("gpt-5").with_provider_id(ProviderId::new("codex")),
        );

        let error = bridge
            .complete(sample_request())
            .await
            .expect_err("missing terminal message should fail");

        assert!(
            matches!(error, LlmError::Parse(message) if message.contains("terminal assistant message"))
        );
    }

    #[tokio::test]
    async fn complete_should_increment_turn_sequence() {
        let provider = Arc::new(FakeProvider::new(
            ProviderFamily::Codex,
            simple_end_turn_events(),
        ));
        let bridge = ArkyDriverBridge::new(
            provider.clone(),
            ModelRef::new("gpt-5").with_provider_id(ProviderId::new("codex")),
        );

        bridge
            .complete(sample_request())
            .await
            .expect("first completion should succeed");
        bridge
            .complete(sample_request())
            .await
            .expect("second completion should succeed");

        assert_eq!(provider.recorded_turn_sequences(), vec![1, 2]);
    }

    #[tokio::test]
    async fn complete_should_infer_tool_use_when_finish_reason_is_missing() {
        let bridge = ArkyDriverBridge::new(
            Arc::new(FakeProvider::new(
                ProviderFamily::Codex,
                vec![
                    Ok(AgentEvent::ToolExecutionStart {
                        meta: EventMetadata::new(1, 1),
                        tool_call_id: "call-1".to_owned(),
                        tool_name: "read_file".to_owned(),
                        args: json!({ "path": "Cargo.toml" }),
                    }),
                    Ok(AgentEvent::TurnEnd {
                        meta: EventMetadata::new(2, 2),
                        message: ArkyMessage::assistant(""),
                        tool_results: Vec::new(),
                        usage: Some(sample_usage(3, 2)),
                    }),
                ],
            )),
            ModelRef::new("gpt-5").with_provider_id(ProviderId::new("codex")),
        );

        let response = bridge
            .complete(sample_request())
            .await
            .expect("completion should succeed");

        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert_eq!(response.tool_calls.len(), 1);
        assert!(response.content.iter().any(
            |block| matches!(block, ContentBlock::ToolUse { name, .. } if name == "read_file")
        ));
    }

    #[test]
    fn binding_to_driver_should_build_real_bridge_for_supported_bindings() {
        let codex_driver = binding_to_driver(&codex_binding(), &install_config("codex"));
        let claude_driver =
            binding_to_driver(&claude_code_binding(), &install_config("claude-code"));
        let bedrock_driver = binding_to_driver(&bedrock_binding(), &install_config("bedrock"));

        assert!(codex_driver.is_ok());
        assert!(claude_driver.is_ok());
        assert!(bedrock_driver.is_ok());
    }

    fn sample_request() -> CompletionRequest {
        CompletionRequest {
            model: "fixture-model".to_owned(),
            messages: vec![Message::user("hello")],
            tools: vec![ToolDefinition {
                name: "read_file".to_owned(),
                description: "Read a file".to_owned(),
                input_schema: json!({ "type": "object" }),
            }],
            max_tokens: 256,
            temperature: 0.2,
            system: Some("You are helpful".to_owned()),
            thinking: None,
        }
    }

    fn simple_end_turn_events() -> Vec<Result<AgentEvent, arky_provider::ProviderError>> {
        vec![
            Ok(AgentEvent::MessageStart {
                meta: EventMetadata::new(1, 1),
                message: ArkyMessage::assistant("hello"),
            }),
            Ok(AgentEvent::MessageEnd {
                meta: EventMetadata::new(2, 2),
                message: ArkyMessage::assistant("hello"),
            }),
            Ok(AgentEvent::TurnEnd {
                meta: EventMetadata::new(3, 3),
                message: ArkyMessage::assistant("hello"),
                tool_results: Vec::new(),
                usage: Some(sample_usage(2, 1)),
            }),
            Ok(AgentEvent::Custom {
                meta: EventMetadata::new(4, 4),
                event_type: "provider.finish".to_owned(),
                payload: json!({
                    "finish_reason": "stop",
                    "usage": {
                        "input_tokens": 2,
                        "output_tokens": 1,
                        "total_tokens": 3,
                    }
                }),
            }),
        ]
    }

    fn claude_tool_use_events() -> Vec<Result<AgentEvent, arky_provider::ProviderError>> {
        let assistant_message = ArkyMessage::new(
            arky_protocol::Role::Assistant,
            vec![
                ArkyContentBlock::text("Hello from bridge"),
                ArkyContentBlock::tool_use(ArkyToolCall::new(
                    "call-1",
                    "read_file",
                    json!({ "path": "Cargo.toml" }),
                )),
            ],
        );

        vec![
            Ok(AgentEvent::MessageStart {
                meta: EventMetadata::new(1, 1),
                message: ArkyMessage::assistant(""),
            }),
            Ok(AgentEvent::ReasoningStart {
                meta: EventMetadata::new(2, 2),
                reasoning_id: "think-1".to_owned(),
            }),
            Ok(AgentEvent::ReasoningDelta {
                meta: EventMetadata::new(3, 3),
                reasoning_id: "think-1".to_owned(),
                delta: "Need tool context".to_owned(),
            }),
            Ok(AgentEvent::ReasoningComplete {
                meta: EventMetadata::new(4, 4),
                reasoning_id: "think-1".to_owned(),
                full_text: "Need tool context".to_owned(),
            }),
            Ok(AgentEvent::MessageUpdate {
                meta: EventMetadata::new(5, 5),
                message: ArkyMessage::assistant("Hello from bridge"),
                delta: StreamDelta::text("Hello from bridge"),
            }),
            Ok(AgentEvent::ToolExecutionStart {
                meta: EventMetadata::new(6, 6),
                tool_call_id: "call-1".to_owned(),
                tool_name: "read_file".to_owned(),
                args: json!({ "path": "Cargo.toml" }),
            }),
            Ok(AgentEvent::MessageUpdate {
                meta: EventMetadata::new(7, 7),
                message: ArkyMessage::assistant("Hello from bridge"),
                delta: StreamDelta::tool_use_input("call-1", "{\"path\":\"Cargo.toml\"}"),
            }),
            Ok(AgentEvent::MessageUpdate {
                meta: EventMetadata::new(8, 8),
                message: assistant_message.clone(),
                delta: StreamDelta::tool_use(ArkyToolCall::new(
                    "call-1",
                    "read_file",
                    json!({ "path": "Cargo.toml" }),
                )),
            }),
            Ok(AgentEvent::ToolExecutionEnd {
                meta: EventMetadata::new(9, 9),
                tool_call_id: "call-1".to_owned(),
                tool_name: "read_file".to_owned(),
                result: json!({ "content": "workspace manifest" }),
                is_error: false,
            }),
            Ok(AgentEvent::TurnEnd {
                meta: EventMetadata::new(10, 10),
                message: assistant_message,
                tool_results: Vec::new(),
                usage: Some(sample_usage(12, 8)),
            }),
            Ok(AgentEvent::Custom {
                meta: EventMetadata::new(11, 11),
                event_type: "claude_code.finish".to_owned(),
                payload: json!({
                    "finish_reason": "tool_use",
                    "usage": {
                        "input_tokens": 12,
                        "output_tokens": 8,
                        "total_tokens": 20,
                    }
                }),
            }),
        ]
    }

    fn sample_usage(input_tokens: u64, output_tokens: u64) -> Usage {
        Usage {
            input_tokens: Some(input_tokens),
            output_tokens: Some(output_tokens),
            total_tokens: Some(input_tokens + output_tokens),
            input_details: None,
            output_details: None,
            cost_usd: None,
            duration_ms: None,
        }
    }

    fn install_config(driver: &str) -> arky_config::ProviderConfig {
        ArkyConfigBuilder::new()
            .provider(
                "default",
                ProviderConfigBuilder::new()
                    .driver(driver)
                    .binary(format!("missing-{driver}"))
                    .args(["--stdio"])
                    .env("ANTHROPIC_API_KEY", "install-anthropic-key")
                    .env("OPENROUTER_API_KEY", "install-openrouter-key")
                    .env("OLLAMA_BASE_URL", "http://localhost:11434"),
            )
            .agent(
                "writer",
                AgentConfigBuilder::new()
                    .provider("default")
                    .model("fixture-model"),
            )
            .build()
            .expect("fixture config should build")
            .resolve_agent_provider("writer")
            .expect("fixture provider should resolve")
            .install
    }

    fn codex_binding() -> ProviderBinding {
        ProviderBinding {
            driver: "codex".to_owned(),
            provider_id: ProviderId::new("codex"),
            model: ModelRef::new("gpt-5").with_provider_id(ProviderId::new("codex")),
            profile: None,
            defaults: ProviderRequestDefaults {
                max_tokens: Some(1_024),
                reasoning_effort: Some(ReasoningEffort::Medium),
            },
            config: Some(ResolvedProviderBehaviorConfig::Codex(
                ResolvedCodexBehaviorConfig {
                    sandbox: arky_codex::CodexSandboxConfig::default(),
                    workspace: arky_codex::CodexWorkspaceConfig::default(),
                    web_search: false,
                    rmcp_client: false,
                    reasoning_summary: None,
                    model_verbosity: None,
                },
            )),
        }
    }

    fn claude_code_binding() -> ProviderBinding {
        ProviderBinding {
            driver: "claude-code".to_owned(),
            provider_id: ProviderId::new("claude-code"),
            model: ModelRef::new("sonnet").with_provider_id(ProviderId::new("claude-code")),
            profile: None,
            defaults: ProviderRequestDefaults {
                max_tokens: Some(1_024),
                reasoning_effort: None,
            },
            config: Some(ResolvedProviderBehaviorConfig::ClaudeCode(
                ResolvedClaudeCodeBehaviorConfig {
                    session: ClaudeSessionConfig::default(),
                    filesystem: ClaudeFilesystemConfig::default(),
                    allowed_tools: Vec::new(),
                    disallowed_tools: Vec::new(),
                    mcp_servers: Default::default(),
                    max_budget_usd: None,
                    fallback_model: None,
                },
            )),
        }
    }

    fn bedrock_binding() -> ProviderBinding {
        ProviderBinding {
            driver: "bedrock".to_owned(),
            provider_id: ProviderId::new("bedrock"),
            model: ModelRef::new("sonnet").with_provider_id(ProviderId::new("bedrock")),
            profile: None,
            defaults: ProviderRequestDefaults::default(),
            config: Some(ResolvedProviderBehaviorConfig::ClaudeCompatible(Box::new(
                ResolvedClaudeCompatibleBehaviorConfig {
                    base: ResolvedClaudeCodeBehaviorConfig {
                        session: ClaudeSessionConfig::default(),
                        filesystem: ClaudeFilesystemConfig::default(),
                        allowed_tools: Vec::new(),
                        disallowed_tools: Vec::new(),
                        mcp_servers: Default::default(),
                        max_budget_usd: None,
                        fallback_model: None,
                    },
                    selected_model: Some("anthropic.claude-sonnet-4-20250514-v1:0".to_owned()),
                    region: Some("us-east-1".to_owned()),
                    project_id: None,
                },
            ))),
        }
    }
}
