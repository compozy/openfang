//! Codex provider orchestration over the shared subprocess layer.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use arky_protocol::{
    AgentEvent, ContentBlock, EventMetadata, ProviderId, Role, StreamDelta, ToolContent,
    ToolResult, Usage,
};
use arky_provider::{
    Provider, ProviderCapabilities, ProviderDescriptor, ProviderError, ProviderEventStream,
    ProviderFamily, ProviderRequest,
};
use arky_tools::{create_codex_tool_id_codec, ToolIdCodec};
use async_stream::try_stream;
use futures::StreamExt;
use serde_json::{json, Map, Value};
use tokio::sync::Mutex;

use crate::{
    accumulator::ToolTracker, CodexAppServer, CodexEventDispatcher, CodexModelDescriptor,
    CodexProviderConfig, CodexServerLease, CodexServerRegistry, CodexStreamPipeline,
    CompactThreadParams, NormalizedNotification, Scheduler, TextAccumulator, TurnStartParams,
};

/// Concrete `Provider` implementation backed by the Codex app-server.
#[derive(Clone)]
pub struct CodexProvider {
    descriptor: ProviderDescriptor,
    config: CodexProviderConfig,
    scheduler: Scheduler,
    sessions: Arc<Mutex<HashMap<String, String>>>,
    codec: Arc<dyn ToolIdCodec>,
    registry: Arc<CodexServerRegistry>,
}

impl CodexProvider {
    /// Creates a provider with the default runtime configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(CodexProviderConfig::default())
    }

    /// Creates a provider with an explicit configuration.
    #[must_use]
    pub fn with_config(config: CodexProviderConfig) -> Self {
        Self {
            descriptor: ProviderDescriptor::new(
                ProviderId::new("codex"),
                ProviderFamily::Codex,
                ProviderCapabilities::new()
                    .with_streaming(true)
                    .with_generate(true)
                    .with_tool_calls(true)
                    .with_mcp_passthrough(true)
                    .with_session_resume(true)
                    .with_steering(true)
                    .with_follow_up(true),
            ),
            scheduler: Scheduler::with_limits(
                config.max_in_flight_requests,
                config.scheduler_timeout,
                config.max_queued_requests,
            ),
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            codec: Arc::new(create_codex_tool_id_codec()),
            registry: Arc::new(CodexServerRegistry::new()),
        }
    }

    async fn resolve_thread_id(&self, request: &ProviderRequest) -> Option<String> {
        if let Some(provider_session_id) = &request.session.provider_session_id {
            return Some(provider_session_id.clone());
        }

        let session_key = request.session.id.as_ref()?.to_string();
        self.sessions.lock().await.get(&session_key).cloned()
    }

    async fn remember_thread_id(&self, request: &ProviderRequest, thread_id: &str) {
        if let Some(session_id) = &request.session.id {
            self.sessions
                .lock()
                .await
                .insert(session_id.to_string(), thread_id.to_owned());
        }
    }

    fn build_turn_params(
        &self,
        request: &ProviderRequest,
        config_overrides: Option<Map<String, Value>>,
    ) -> TurnStartParams {
        TurnStartParams {
            scope_id: Some(request.turn.id.to_string()),
            prompt: Some(render_prompt(request)),
            input: None,
            model: Some(
                request
                    .model
                    .provider_model_id
                    .clone()
                    .unwrap_or_else(|| request.model.model_id.clone()),
            ),
            config_overrides,
            output_schema: request.settings.extra.get("output_schema").cloned(),
            approval_policy: self.config.approval_policy.clone(),
        }
    }

    fn build_config_overrides(request: &ProviderRequest) -> Option<Map<String, Value>> {
        let mut overrides = Map::new();
        copy_extra_override(
            &request.settings.extra,
            "reasoning",
            "model_reasoning_effort",
            &mut overrides,
        );
        copy_extra_override(
            &request.settings.extra,
            "reasoning_summary",
            "model_reasoning_summary",
            &mut overrides,
        );
        copy_extra_override(
            &request.settings.extra,
            "model_verbosity",
            "model_verbosity",
            &mut overrides,
        );

        if let Some(developer_instructions) = request
            .settings
            .extra
            .get("developer_instructions")
            .cloned()
            .or_else(|| developer_instructions_from_messages(&request.messages))
        {
            overrides.insert("developer_instructions".to_owned(), developer_instructions);
        }
        if !request.tools.definitions.is_empty() {
            overrides.insert(
                "tools.available".to_owned(),
                Value::Array(
                    request
                        .tools
                        .definitions
                        .iter()
                        .map(tool_definition_to_value)
                        .collect(),
                ),
            );
        }
        if let Some(tool_choice) = request
            .settings
            .extra
            .get("tools.choice")
            .cloned()
            .or_else(|| request.settings.extra.get("tool_choice").cloned())
        {
            overrides.insert("tools.choice".to_owned(), tool_choice);
        }
        if let Some(config_override_object) = request
            .settings
            .extra
            .get("config_overrides")
            .and_then(Value::as_object)
        {
            for (key, value) in config_override_object {
                overrides.insert(key.clone(), value.clone());
            }
        }
        if let Some(mcp_servers) = request
            .settings
            .extra
            .get("mcp_servers")
            .and_then(Value::as_object)
        {
            flatten_override_object("mcp_servers", mcp_servers, &mut overrides);
        }
        if let Some(rmcp_client) = request
            .settings
            .extra
            .get("rmcp_client")
            .cloned()
            .or_else(|| request.settings.extra.get("rmcpClient").cloned())
        {
            overrides.insert("features.rmcp_client".to_owned(), rmcp_client);
        }

        (!overrides.is_empty()).then_some(overrides)
    }

    fn build_stream(
        &self,
        request: ProviderRequest,
        app_server: Arc<CodexAppServer>,
        lease: CodexServerLease,
        permit: crate::SchedulerPermit,
    ) -> ProviderEventStream {
        let provider = self.clone();
        let descriptor = self.descriptor.clone();

        let stream: ProviderEventStream = Box::pin(try_stream! {
            let config_overrides = Self::build_config_overrides(&request);
            let turn_params =
                provider.build_turn_params(&request, config_overrides.clone());
            let model = request
                .model
                .provider_model_id
                .clone()
                .unwrap_or_else(|| request.model.model_id.clone());
            let known_thread_id = provider.resolve_thread_id(&request).await;
            let (thread_id, mut turn_stream) = app_server
                .open_turn(
                    known_thread_id.as_deref(),
                    model,
                    config_overrides,
                    turn_params,
                )
                .await?;
            provider.remember_thread_id(&request, &thread_id).await;
            let mut runtime = StreamRuntime::new(&request, descriptor.id.clone());
            let mut dispatcher =
                CodexEventDispatcher::new(provider.codec.clone());
            let mut pipeline = CodexStreamPipeline::new();
            pipeline.record_response_metadata(Some(thread_id.clone()), None);
            let response_id = request.turn.id.to_string();

            yield runtime.turn_start();
            yield pipeline.stream_start_event(runtime.next_meta(), &descriptor.id);
            yield pipeline.response_metadata_event(
                runtime.next_meta(),
                request.model.model_id.as_str(),
                &response_id,
            );

            while let Some(item) = turn_stream.next().await {
                let notification = match item {
                    Ok(notification) => notification,
                    Err(error) => {
                        let mapped_error =
                            app_server.map_stream_error(error).await?;
                        Err(mapped_error)?;
                        unreachable!("error branch should have returned from the stream")
                    }
                };
                if !pipeline.record_notification(&notification) {
                    continue;
                }
                let normalized = dispatcher.normalize(&notification);
                match &normalized {
                    NormalizedNotification::UsageUpdated { usage } => {
                        if pipeline.state().last_usage.as_ref() != Some(usage) {
                            pipeline.record_response_metadata(
                                Some(thread_id.clone()),
                                Some(usage.clone()),
                            );
                            yield pipeline.response_metadata_event(
                                runtime.next_meta(),
                                request.model.model_id.as_str(),
                                &response_id,
                            );
                        }
                    }
                    NormalizedNotification::TurnCompleted { usage } if usage.is_some() => {
                        if pipeline.state().last_usage.as_ref() != usage.as_ref() {
                            pipeline.record_response_metadata(
                                Some(thread_id.clone()),
                                usage.clone(),
                            );
                            yield pipeline.response_metadata_event(
                                runtime.next_meta(),
                                request.model.model_id.as_str(),
                                &response_id,
                            );
                        }
                    }
                    _ => {}
                }
                let events = runtime.handle_notification(normalized)?;
                for event in events {
                    yield event;
                }
                if runtime.finished() {
                    break;
                }
            }

            for event in runtime.finalize_open_state() {
                yield event;
            }
            if !runtime.finished() {
                let mapped_error = app_server
                    .map_stream_error(ProviderError::stream_interrupted(
                        "codex stream ended without a terminal turn notification",
                    ))
                    .await?;
                Err(mapped_error)?;
            }

            let _keep_alive = (&app_server, &lease, &permit);
        });

        stream
    }

    /// Lists available models from the shared Codex app-server.
    pub async fn list_models(&self) -> Result<Vec<CodexModelDescriptor>, ProviderError> {
        let lease = self.registry.acquire(self.config.clone()).await?;
        lease.server().list_models().await
    }

    /// Requests history compaction for an existing provider thread.
    pub async fn compact_thread(&self, thread_id: &str) -> Result<(), ProviderError> {
        let lease = self.registry.acquire(self.config.clone()).await?;
        let mut payload = Map::new();
        payload.insert("scopeId".to_owned(), Value::String("default".to_owned()));
        if let Some(token_threshold) = self.config.compaction_token_limit {
            payload.insert("tokenThreshold".to_owned(), json!(token_threshold));
        }
        if let Some(prompt) = &self.config.compact_prompt {
            payload.insert("prompt".to_owned(), Value::String(prompt.clone()));
        }

        lease
            .server()
            .compact_thread(
                thread_id,
                CompactThreadParams {
                    scope_id: None,
                    payload,
                },
            )
            .await
    }
}

#[async_trait::async_trait]
impl Provider for CodexProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    async fn stream(&self, request: ProviderRequest) -> Result<ProviderEventStream, ProviderError> {
        let permit = self.scheduler.acquire("codex stream").await?;
        let lease = self.registry.acquire(self.config.clone()).await?;
        let server = lease.server();
        Ok(self.build_stream(request, server, lease, permit))
    }
}

impl Default for CodexProvider {
    fn default() -> Self {
        Self::new()
    }
}

fn copy_extra_override(
    extra: &std::collections::BTreeMap<String, Value>,
    source_key: &str,
    target_key: &str,
    overrides: &mut Map<String, Value>,
) {
    if let Some(value) = extra.get(source_key).cloned() {
        overrides.insert(target_key.to_owned(), value);
    }
}

fn developer_instructions_from_messages(messages: &[arky_protocol::Message]) -> Option<Value> {
    let instructions = messages
        .iter()
        .filter(|message| matches!(message.role, Role::System))
        .map(message_text)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    if instructions.is_empty() {
        return None;
    }

    Some(Value::String(instructions))
}

fn flatten_override_object(
    prefix: &str,
    object: &Map<String, Value>,
    overrides: &mut Map<String, Value>,
) {
    for (key, value) in object {
        let dotted_key = format!("{prefix}.{key}");
        match value {
            Value::Object(nested) => {
                flatten_override_object(&dotted_key, nested, overrides);
            }
            other => {
                overrides.insert(dotted_key, other.clone());
            }
        }
    }
}

fn tool_definition_to_value(definition: &arky_protocol::ToolDefinition) -> Value {
    json!({
        "name": definition.name,
        "description": definition.description,
        "input_schema": definition.input_schema,
    })
}

fn render_prompt(request: &ProviderRequest) -> String {
    let mut prompt = String::new();

    for message in &request.messages {
        let role = match message.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            Role::System => "System",
            Role::Tool => "Tool",
        };
        let content = message
            .content
            .iter()
            .map(content_block_text)
            .collect::<Vec<_>>()
            .join("\n");

        if !prompt.is_empty() {
            prompt.push_str("\n\n");
        }
        prompt.push_str(role);
        prompt.push_str(": ");
        prompt.push_str(&content);
    }

    if !request.tools.definitions.is_empty() {
        prompt.push_str("\n\nAvailable tools:\n");
        prompt.push_str(
            &serde_json::to_string(&request.tools.definitions).unwrap_or_else(|_| "[]".to_owned()),
        );
    }

    prompt
}

fn message_text(message: &arky_protocol::Message) -> String {
    message
        .content
        .iter()
        .map(content_block_text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn content_block_text(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Text { text } => text.clone(),
        ContentBlock::ToolUse { call } => {
            format!("ToolUse {} {} {}", call.id, call.name, call.input)
        }
        ContentBlock::ToolResult { result } => format!(
            "ToolResult {} {} {}",
            result.id,
            result.name,
            tool_contents_to_text(&result.content)
        ),
        ContentBlock::Image { media_type, .. } => format!("Image<{media_type}>"),
    }
}

fn tool_contents_to_text(content: &[ToolContent]) -> String {
    content
        .iter()
        .map(|item| match item {
            ToolContent::Text { text } => text.clone(),
            ToolContent::Json { value } => value.to_string(),
            ToolContent::Image { media_type, .. } => format!("image:{media_type}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn tool_result_to_value(result: &ToolResult) -> Value {
    let content = result
        .content
        .iter()
        .map(|item| match item {
            ToolContent::Text { text } => Value::String(text.clone()),
            ToolContent::Json { value } => value.clone(),
            ToolContent::Image { media_type, .. } => Value::String(format!("image:{media_type}")),
        })
        .collect::<Vec<_>>();

    json!({
        "id": result.id,
        "name": result.name,
        "content": content,
        "isError": result.is_error,
    })
}

struct StreamRuntime {
    emitter: EventEmitter,
    text: TextAccumulator,
    tools: ToolTracker,
    tool_results: Vec<ToolResult>,
    reasoning: HashMap<String, String>,
    usage: Option<Usage>,
    message_started: bool,
    message_finished: bool,
    finished: bool,
}

impl std::fmt::Debug for StreamRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamRuntime")
            .field("emitter", &self.emitter)
            .field("text", &self.text)
            .field("tools", &self.tools)
            .field("tool_results", &self.tool_results)
            .field("message_started", &self.message_started)
            .field("message_finished", &self.message_finished)
            .field("finished", &self.finished)
            .finish_non_exhaustive()
    }
}

impl StreamRuntime {
    fn new(request: &ProviderRequest, provider_id: ProviderId) -> Self {
        Self {
            emitter: EventEmitter::new(request, provider_id),
            text: TextAccumulator::new(),
            tools: ToolTracker::new(),
            tool_results: Vec::new(),
            reasoning: HashMap::new(),
            usage: None,
            message_started: false,
            message_finished: false,
            finished: false,
        }
    }

    fn turn_start(&mut self) -> AgentEvent {
        AgentEvent::TurnStart {
            meta: self.emitter.next(),
        }
    }

    fn next_meta(&mut self) -> EventMetadata {
        self.emitter.next()
    }

    const fn finished(&self) -> bool {
        self.finished
    }

    fn finalize_open_state(&mut self) -> Vec<AgentEvent> {
        if self.finished {
            return Vec::new();
        }

        let mut events = Vec::new();
        for result in self.tools.fail_open_tools() {
            events.push(AgentEvent::ToolExecutionEnd {
                meta: self.emitter.next(),
                tool_call_id: result.id.clone(),
                tool_name: result.name.clone(),
                result: tool_result_to_value(&result),
                is_error: true,
            });
            self.tool_results.push(result);
        }
        if self.message_started && !self.message_finished {
            events.push(AgentEvent::MessageEnd {
                meta: self.emitter.next(),
                message: self.text.message(),
            });
            self.message_finished = true;
        }
        events
    }

    fn handle_notification(
        &mut self,
        notification: NormalizedNotification,
    ) -> Result<Vec<AgentEvent>, ProviderError> {
        match notification {
            NormalizedNotification::Ignored => Ok(Vec::new()),
            NormalizedNotification::MessageStart { snapshot } => {
                Ok(self.handle_message_start(snapshot))
            }
            NormalizedNotification::MessageDelta { delta } => Ok(self.handle_message_delta(delta)),
            NormalizedNotification::MessageComplete { snapshot } => {
                Ok(self.handle_message_complete(snapshot))
            }
            NormalizedNotification::ReasoningStart { reasoning_id } => {
                Ok(self.handle_reasoning_start(reasoning_id))
            }
            NormalizedNotification::ReasoningDelta { reasoning_id, text } => {
                Ok(self.handle_reasoning_delta(reasoning_id, text))
            }
            NormalizedNotification::ReasoningComplete {
                reasoning_id,
                full_text,
            } => Ok(self.handle_reasoning_complete(reasoning_id, full_text)),
            NormalizedNotification::ToolStart {
                call_id,
                tool_name,
                input,
                parent_id,
            } => Ok(vec![
                self.handle_tool_start(call_id, tool_name, input, parent_id)
            ]),
            NormalizedNotification::ToolUpdate {
                call_id,
                tool_name,
                partial_result,
            } => Ok(vec![self.handle_tool_update(
                &call_id,
                tool_name,
                partial_result,
            )]),
            NormalizedNotification::ToolComplete {
                call_id,
                tool_name,
                result,
                is_error,
            } => {
                Ok(vec![self.handle_tool_complete(
                    &call_id, tool_name, result, is_error,
                )])
            }
            NormalizedNotification::UsageUpdated { usage } => {
                self.usage = Some(usage);
                Ok(Vec::new())
            }
            NormalizedNotification::TurnCompleted { usage } => {
                Ok(self.handle_turn_completed(usage))
            }
            NormalizedNotification::TurnFailed { message } => {
                Err(ProviderError::protocol_violation(message, None))
            }
        }
    }

    fn handle_message_start(&mut self, snapshot: Option<String>) -> Vec<AgentEvent> {
        if let Some(snapshot) = snapshot {
            self.text.apply_snapshot(&snapshot);
        }
        if self.message_started {
            return Vec::new();
        }

        self.message_started = true;
        vec![AgentEvent::MessageStart {
            meta: self.emitter.next(),
            message: self.text.message_with_part_id(),
        }]
    }

    fn handle_message_delta(&mut self, delta: String) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        if !self.message_started {
            self.message_started = true;
            events.push(AgentEvent::MessageStart {
                meta: self.emitter.next(),
                message: self.text.message_with_part_id(),
            });
        }

        self.text.push_delta(&delta);
        events.push(AgentEvent::MessageUpdate {
            meta: self.emitter.next(),
            message: self.text.message_with_part_id(),
            delta: StreamDelta::text(delta),
        });
        events
    }

    fn handle_message_complete(&mut self, snapshot: Option<String>) -> Vec<AgentEvent> {
        if let Some(snapshot) = snapshot {
            self.text.apply_snapshot(&snapshot);
        }

        let mut events = Vec::new();
        if !self.message_started {
            self.message_started = true;
            events.push(AgentEvent::MessageStart {
                meta: self.emitter.next(),
                message: self.text.message_with_part_id(),
            });
        }
        if !self.message_finished {
            self.message_finished = true;
            events.push(AgentEvent::MessageEnd {
                meta: self.emitter.next(),
                message: self.text.message(),
            });
        }
        events
    }

    fn handle_reasoning_start(&mut self, reasoning_id: String) -> Vec<AgentEvent> {
        self.reasoning.entry(reasoning_id.clone()).or_default();
        vec![AgentEvent::ReasoningStart {
            meta: self.emitter.next(),
            reasoning_id,
        }]
    }

    fn handle_reasoning_delta(&mut self, reasoning_id: String, text: String) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        let entry = self
            .reasoning
            .entry(reasoning_id.clone())
            .or_insert_with(|| {
                events.push(AgentEvent::ReasoningStart {
                    meta: self.emitter.next(),
                    reasoning_id: reasoning_id.clone(),
                });
                String::new()
            });
        entry.push_str(&text);
        events.push(AgentEvent::ReasoningDelta {
            meta: self.emitter.next(),
            reasoning_id,
            delta: text,
        });
        events
    }

    fn handle_reasoning_complete(
        &mut self,
        reasoning_id: String,
        full_text: Option<String>,
    ) -> Vec<AgentEvent> {
        let accumulated = self.reasoning.remove(&reasoning_id).unwrap_or_default();
        let full_text = full_text.unwrap_or(accumulated);
        vec![AgentEvent::ReasoningComplete {
            meta: self.emitter.next(),
            reasoning_id,
            full_text,
        }]
    }

    fn handle_tool_start(
        &mut self,
        call_id: String,
        tool_name: String,
        input: Value,
        parent_id: Option<String>,
    ) -> AgentEvent {
        let call = self.tools.start(call_id, tool_name, input, parent_id);
        AgentEvent::ToolExecutionStart {
            meta: self.emitter.next(),
            tool_call_id: call.id,
            tool_name: call.name,
            args: call.input,
        }
    }

    fn handle_tool_update(
        &mut self,
        call_id: &str,
        tool_name: String,
        partial_result: Value,
    ) -> AgentEvent {
        self.tools.push_output(
            call_id,
            tool_name.clone(),
            &stringify_value(&partial_result),
        );
        AgentEvent::ToolExecutionUpdate {
            meta: self.emitter.next(),
            tool_call_id: call_id.to_owned(),
            tool_name,
            partial_result,
        }
    }

    fn handle_tool_complete(
        &mut self,
        call_id: &str,
        tool_name: String,
        result: Option<Value>,
        is_error: bool,
    ) -> AgentEvent {
        let completed = self.tools.complete(call_id, tool_name, result, is_error);
        self.tool_results.push(completed.clone());
        AgentEvent::ToolExecutionEnd {
            meta: self.emitter.next(),
            tool_call_id: completed.id.clone(),
            tool_name: completed.name.clone(),
            result: tool_result_to_value(&completed),
            is_error: completed.is_error,
        }
    }

    fn handle_turn_completed(&mut self, usage: Option<Usage>) -> Vec<AgentEvent> {
        if usage.is_some() {
            self.usage = usage;
        }
        let mut events = self.finalize_open_state();
        events.push(AgentEvent::TurnEnd {
            meta: self.emitter.next(),
            message: self.text.message(),
            tool_results: self.tool_results.clone(),
            usage: self.usage.clone(),
        });
        self.finished = true;
        events
    }
}

#[derive(Debug)]
struct EventEmitter {
    session_id: Option<arky_protocol::SessionId>,
    turn_id: arky_protocol::TurnId,
    provider_id: ProviderId,
    sequence: u64,
}

impl EventEmitter {
    fn new(request: &ProviderRequest, provider_id: ProviderId) -> Self {
        Self {
            session_id: request.session.id.clone(),
            turn_id: request.turn.id.clone(),
            provider_id,
            sequence: 0,
        }
    }

    fn next(&mut self) -> EventMetadata {
        self.sequence = self.sequence.saturating_add(1);
        let mut meta = EventMetadata::new(now_ms(), self.sequence)
            .with_turn_id(self.turn_id.clone())
            .with_provider_id(self.provider_id.clone());
        if let Some(session_id) = &self.session_id {
            meta = meta.with_session_id(session_id.clone());
        }
        meta
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(u64::MAX)
}

fn stringify_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use arky_protocol::{
        Message, ModelRef, SessionRef, ToolContext, ToolDefinition, TurnContext, TurnId,
    };
    use arky_provider::Provider;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{CodexEventDispatcher, CodexProvider, StreamRuntime};
    use crate::NormalizedNotification;

    #[test]
    fn normalize_notification_should_detect_message_and_tool_events() {
        let provider = CodexProvider::new();
        let mut dispatcher = CodexEventDispatcher::new(provider.codec.clone());

        let message_delta = dispatcher.normalize(&crate::CodexNotification {
            method: "item/agentMessage/delta".to_owned(),
            params: json!({
                "delta": "hello",
            }),
        });
        let tool_complete = dispatcher.normalize(&crate::CodexNotification {
            method: "item/completed".to_owned(),
            params: json!({
                "item": {
                    "id": "tool-1",
                    "type": "commandExecution",
                    "status": "completed",
                    "aggregatedOutput": "done",
                },
            }),
        });

        assert!(matches!(
            message_delta,
            NormalizedNotification::MessageDelta { delta } if delta == "hello"
        ));
        assert!(matches!(
            tool_complete,
            NormalizedNotification::ToolComplete { call_id, .. } if call_id == "tool-1"
        ));
    }

    #[test]
    fn codex_descriptor_should_expose_expected_capabilities() {
        let provider = CodexProvider::new();
        let descriptor = provider.descriptor();

        assert_eq!(descriptor.id.as_str(), "codex");
        assert_eq!(descriptor.capabilities.streaming, true);
        assert_eq!(descriptor.capabilities.generate, true);
        assert_eq!(descriptor.capabilities.tool_calls, true);
        assert_eq!(descriptor.capabilities.session_resume, true);
    }

    #[test]
    fn build_config_overrides_should_include_tools_developer_text_and_mcp_settings() {
        let mut extra = BTreeMap::new();
        extra.insert("reasoning".to_owned(), json!("high"));
        extra.insert("reasoning_summary".to_owned(), json!("none"));
        extra.insert("tool_choice".to_owned(), json!("auto"));
        extra.insert(
            "mcp_servers".to_owned(),
            json!({
                "runtime": {
                    "url": "http://127.0.0.1:7777/mcp",
                },
            }),
        );

        let mut settings = arky_provider::ProviderSettings::new();
        settings.extra = extra;
        let request = arky_provider::ProviderRequest::new(
            SessionRef::new(None),
            TurnContext::new(TurnId::new(), 1),
            ModelRef::new("gpt-5"),
            vec![Message::system("Use the contract"), Message::user("hello")],
        )
        .with_tools(
            ToolContext::new().with_definitions(vec![ToolDefinition::new(
                "search_docs",
                "Search docs",
                json!({
                    "type": "object",
                }),
            )]),
        )
        .with_settings(settings);

        let overrides =
            CodexProvider::build_config_overrides(&request).expect("config overrides should exist");

        assert_eq!(overrides["model_reasoning_effort"], "high");
        assert_eq!(overrides["model_reasoning_summary"], "none");
        assert_eq!(overrides["tools.choice"], "auto");
        assert_eq!(overrides["developer_instructions"], "Use the contract");
        assert_eq!(
            overrides["mcp_servers.runtime.url"],
            "http://127.0.0.1:7777/mcp"
        );
        assert_eq!(overrides["tools.available"][0]["name"], "search_docs");
    }

    #[test]
    fn stream_runtime_should_attach_uuid_part_ids_to_message_updates() {
        let request = arky_provider::ProviderRequest::new(
            SessionRef::new(None),
            TurnContext::new(TurnId::new(), 1),
            ModelRef::new("gpt-5"),
            vec![Message::user("hello")],
        );
        let mut runtime = StreamRuntime::new(&request, arky_protocol::ProviderId::new("codex"));

        let events = runtime.handle_message_delta("hello".to_owned());

        assert_eq!(events.len(), 2);
        let message = match &events[1] {
            arky_protocol::AgentEvent::MessageUpdate { message, .. } => message,
            other => panic!("expected message update, got {other:?}"),
        };
        let part_id = message
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.id.as_deref())
            .expect("part id should be attached");
        assert_eq!(TurnId::parse_str(part_id).is_ok(), true);
    }

    #[test]
    fn stream_runtime_should_preserve_usage_reported_before_turn_completion() {
        let request = arky_provider::ProviderRequest::new(
            SessionRef::new(None),
            TurnContext::new(TurnId::new(), 1),
            ModelRef::new("gpt-5"),
            vec![Message::user("hello")],
        );
        let mut runtime = StreamRuntime::new(&request, arky_protocol::ProviderId::new("codex"));

        let usage = arky_protocol::Usage {
            input_tokens: Some(10),
            output_tokens: Some(5),
            total_tokens: Some(15),
            ..arky_protocol::Usage::default()
        };

        let no_events = runtime
            .handle_notification(NormalizedNotification::UsageUpdated {
                usage: usage.clone(),
            })
            .expect("usage update should succeed");
        assert_eq!(no_events.is_empty(), true);

        let events = runtime
            .handle_notification(NormalizedNotification::TurnCompleted { usage: None })
            .expect("turn completion should succeed");

        let final_usage = events.iter().find_map(|event| match event {
            arky_protocol::AgentEvent::TurnEnd { usage, .. } => usage.clone(),
            _ => None,
        });
        assert_eq!(final_usage, Some(usage));
    }
}
