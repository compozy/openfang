//! Claude Code provider orchestration over the shared subprocess layer.

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use arky_protocol::{
    AgentEvent, ContentBlock, EventMetadata, Message, ProviderId, Role, StreamDelta, ToolCall,
    ToolContent, ToolResult,
};
use arky_provider::{
    ManagedProcess, ProcessConfig, ProcessManager, Provider, ProviderDescriptor, ProviderError,
    ProviderEventStream, ProviderRequest, StdioTransport, StdioTransportConfig,
};
use arky_tools::{create_claude_compatible_tool_id_codec, ToolIdCodec};
use async_stream::try_stream;
use serde_json::{json, Value};
use tokio::{
    io::{AsyncReadExt, AsyncWrite, AsyncWriteExt, BufWriter},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::{
    config::{
        validate_claude_model_id, validate_prompt_length, validate_session_id_format,
        ClaudeInputFormat,
    },
    conversion::{collect_warning_messages, ClaudeInjectedPromptStream},
    cooldown::SpawnFailureTracker,
    dedup::{TextDeduplicator, TextSource},
    generate::generate_with_recovery,
    nested::NestedToolTracker,
    parser::{
        is_claude_truncation_error, ClaudeEventParser, ClaudeEventSource, ClaudeNormalizedEvent,
    },
    profile::ClaudeProviderProfile,
    session::SessionManager,
    tool_fsm::{ToolLifecycleState, ToolLifecycleTracker},
    ClaudeCodeProviderConfig, ClaudeErrorClassifier,
};

/// Concrete `Provider` implementation wrapping the Claude CLI.
#[derive(Clone)]
pub struct ClaudeCodeProvider {
    descriptor: ProviderDescriptor,
    config: ClaudeCodeProviderConfig,
    profile: ClaudeProviderProfile,
    sessions: SessionManager,
    cooldown: SpawnFailureTracker,
    codec: Arc<dyn ToolIdCodec>,
    classifier: ClaudeErrorClassifier,
    validated_version: Arc<tokio::sync::Mutex<Option<String>>>,
}

impl ClaudeCodeProvider {
    /// Creates a provider with the default Claude configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(ClaudeCodeProviderConfig::default())
    }

    /// Creates a provider with an explicit runtime configuration.
    #[must_use]
    pub fn with_config(config: ClaudeCodeProviderConfig) -> Self {
        Self::with_profile_config(ClaudeProviderProfile::ClaudeCode, config)
    }

    /// Creates a provider with an explicit compatible-provider profile plus base config.
    #[must_use]
    pub fn with_profile_config(
        profile: ClaudeProviderProfile,
        config: ClaudeCodeProviderConfig,
    ) -> Self {
        let descriptor = profile.descriptor();
        Self {
            descriptor: descriptor.clone(),
            cooldown: SpawnFailureTracker::new(config.spawn_failure_policy),
            config,
            profile,
            sessions: SessionManager::new(),
            codec: Arc::new(create_claude_compatible_tool_id_codec(descriptor.id)),
            classifier: ClaudeErrorClassifier::new(),
            validated_version: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    async fn ensure_binary_validated(&self) -> Result<String, ProviderError> {
        let cached_version = self.validated_version.lock().await.clone();
        if let Some(version) = cached_version {
            return Ok(version);
        }

        let manager = ProcessManager::new(self.version_process_config());
        let mut process = manager.spawn()?;
        let mut stdout = process.take_stdout()?;
        let mut stderr = process.take_stderr()?;
        let mut stdout_buffer = String::new();
        let mut stderr_buffer = String::new();
        tokio::try_join!(
            stdout.read_to_string(&mut stdout_buffer),
            stderr.read_to_string(&mut stderr_buffer),
        )
        .map_err(|error| {
            ProviderError::stream_interrupted(format!(
                "failed to read Claude version output: {error}"
            ))
        })?;
        if let Some(callback) = &self.config.cli_behavior.stderr_callback {
            if !stderr_buffer.trim().is_empty() {
                callback.call(stderr_buffer.trim());
            }
        }
        process.wait().await?;

        let version = stdout_buffer.trim();
        if version.is_empty() {
            return Err(ProviderError::protocol_violation(
                "Claude version command returned empty stdout",
                Some(json!({
                    "stderr": stderr_buffer.trim(),
                })),
            ));
        }

        *self.validated_version.lock().await = Some(version.to_owned());
        Ok(version.to_owned())
    }

    fn version_process_config(&self) -> ProcessConfig {
        let mut config = ProcessConfig::new(&self.config.binary)
            .with_args(self.config.version_args.clone())
            .with_kill_on_drop(true);
        if let Some(cwd) = &self.config.cwd {
            config = config.with_cwd(cwd.clone());
        }
        for (key, value) in merged_env_layers(&[&self.config.env]) {
            config = config.with_env(key.clone(), value.clone());
        }
        config
    }

    async fn build_process_config(
        &self,
        request: &ProviderRequest,
    ) -> Result<ClaudeProcessPlan, ProviderError> {
        let prompt = render_prompt(request);
        let session_id = self.sessions.resolve(&request.session).await;
        let input_format =
            resolve_input_format(self.config.cli_behavior.streaming_input.as_deref())?;
        let prompt_stream = if input_format == ClaudeInputFormat::StreamJson {
            let (stream, injector) =
                ClaudeInjectedPromptStream::new(prompt.clone(), session_id.clone(), None);
            injector.close();
            Some(stream)
        } else {
            None
        };
        let args = self.config.cli_args_with_input_format(
            (input_format == ClaudeInputFormat::Text).then_some(prompt),
            self.profile.runtime_model(request),
            session_id.as_deref(),
            input_format,
        )?;

        let mut config = ProcessConfig::new(&self.config.binary)
            .with_args(args)
            .with_kill_on_drop(true);
        if let Some(cwd) = &self.config.cwd {
            config = config.with_cwd(cwd.clone());
        }
        let request_env = request_env_overrides(&request.settings.extra);
        let profile_env = self.profile.env_overrides(request, &request_env);
        for (key, value) in merged_env_layers(&[&profile_env, &self.config.env, &request_env]) {
            config = config.with_env(key.clone(), value.clone());
        }
        Ok(ClaudeProcessPlan {
            process_config: config,
            prompt_stream,
        })
    }

    fn canonical_tool_name(&self, tool_name: &str) -> String {
        match self.codec.decode(tool_name) {
            Ok(parsed) => parsed.canonical_name,
            Err(_) => tool_name.to_owned(),
        }
    }

    fn build_stream(
        &self,
        request: ProviderRequest,
        mut process: ManagedProcess,
        transport: StdioTransport,
        prompt_writer: Option<JoinHandle<Result<(), ProviderError>>>,
    ) -> Result<ProviderEventStream, ProviderError> {
        let stderr = process.take_stderr()?;
        let provider = self.clone();
        let descriptor = self.descriptor.clone();
        let sessions = self.sessions.clone();
        let error_classifier = self.classifier.clone();
        let binary = self.config.binary.clone();
        let stderr_callback = self.config.cli_behavior.stderr_callback.clone();
        let prompt = render_prompt(&request);
        let request_warnings = collect_request_warnings(
            &request,
            &prompt,
            request.session.provider_session_id.as_deref(),
        );

        let stream: ProviderEventStream = Box::pin(try_stream! {
            let stderr_task = tokio::spawn(async move {
                let mut stderr = stderr;
                let mut buffer = String::new();
                let _ = stderr.read_to_string(&mut buffer).await;
                buffer
            });
            let mut prompt_writer = prompt_writer;
            let mut transport = transport;
            let cancel = CancellationToken::new();
            let mut parser = ClaudeEventParser::new();
            let mut runtime =
                StreamRuntime::new(&request, descriptor.id.clone(), provider, sessions);
            let mut recovered_from_truncation = false;

            yield runtime.turn_start();
            for warning in request_warnings {
                yield runtime.warning_event(&warning);
            }

            while let Some(line) = transport.recv_frame(cancel.child_token()).await? {
                let normalized_events = match parser.parse_line(&line) {
                    Ok(events) => events,
                    Err(error)
                        if is_claude_truncation_error(&error, runtime.buffered_text()) =>
                    {
                        for emitted in runtime.recover_from_truncation(&error) {
                            yield emitted;
                        }
                        recovered_from_truncation = true;
                        break;
                    }
                    Err(error) => Err(error)?,
                };
                for event in normalized_events {
                    for emitted in runtime.handle_event(event).await? {
                        yield emitted;
                    }
                }
            }

            drop(transport);
            let stderr_excerpt = stderr_task.await.unwrap_or_default();
            if let Some(callback) = &stderr_callback {
                if !stderr_excerpt.trim().is_empty() {
                    callback.call(stderr_excerpt.trim());
                }
            }
            if recovered_from_truncation {
                if let Some(prompt_writer) = prompt_writer.take() {
                    let _ = prompt_writer.await;
                }
                let _ = process.graceful_shutdown().await;
                return;
            }
            if let Some(prompt_writer) = prompt_writer.take() {
                let prompt_writer_result = prompt_writer.await.map_err(|error| {
                    ProviderError::stream_interrupted(format!(
                        "failed to join Claude stdin writer task: {error}"
                    ))
                })?;
                prompt_writer_result?;
            }
            finish_stream_process(
                &mut process,
                &runtime,
                &error_classifier,
                &binary,
                &stderr_excerpt,
            ).await?;
        });

        Ok(stream)
    }
}

#[async_trait::async_trait]
impl Provider for ClaudeCodeProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    async fn stream(&self, request: ProviderRequest) -> Result<ProviderEventStream, ProviderError> {
        let _version = self.ensure_binary_validated().await?;
        self.cooldown.wait_until_ready().await;

        let process_plan = self.build_process_config(&request).await?;
        let manager = ProcessManager::new(process_plan.process_config);
        let mut process = match manager.spawn() {
            Ok(process) => process,
            Err(error) => {
                let _ = self.cooldown.record_failure().await;
                return Err(error);
            }
        };
        self.cooldown.record_success().await;

        let stdin = process.take_stdin()?;
        let prompt_writer = if let Some(prompt_stream) = process_plan.prompt_stream {
            Some(spawn_prompt_stream_writer(stdin, prompt_stream))
        } else {
            drop(stdin);
            None
        };
        let stdout = process.take_stdout()?;
        let transport = StdioTransport::new(
            stdout,
            tokio::io::sink(),
            StdioTransportConfig {
                max_frame_len: self.config.max_frame_len,
                ..StdioTransportConfig::default()
            },
        );

        self.build_stream(request, process, transport, prompt_writer)
    }

    async fn generate(
        &self,
        request: ProviderRequest,
    ) -> Result<arky_provider::GenerateResponse, ProviderError> {
        generate_with_recovery(self, request).await
    }
}

impl Default for ClaudeCodeProvider {
    fn default() -> Self {
        Self::new()
    }
}

struct ClaudeProcessPlan {
    process_config: ProcessConfig,
    prompt_stream: Option<ClaudeInjectedPromptStream>,
}

fn resolve_input_format(configured: Option<&str>) -> Result<ClaudeInputFormat, ProviderError> {
    match configured.unwrap_or("auto") {
        "auto" | "text" => Ok(ClaudeInputFormat::Text),
        "stream-json" => Ok(ClaudeInputFormat::StreamJson),
        other => Err(ProviderError::protocol_violation(
            format!("unsupported Claude input format `{other}`"),
            None,
        )),
    }
}

fn spawn_prompt_stream_writer<W>(
    writer: W,
    mut prompt_stream: ClaudeInjectedPromptStream,
) -> JoinHandle<Result<(), ProviderError>>
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut writer = BufWriter::new(writer);
        while let Some(message) = prompt_stream.next_message().await {
            let encoded = serde_json::to_string(&message).map_err(|error| {
                ProviderError::protocol_violation(
                    format!("failed to serialize Claude prompt message: {error}"),
                    None,
                )
            })?;
            writer
                .write_all(encoded.as_bytes())
                .await
                .map_err(|error| {
                    ProviderError::stream_interrupted(format!(
                        "failed to write Claude prompt message: {error}"
                    ))
                })?;
            writer.write_all(b"\n").await.map_err(|error| {
                ProviderError::stream_interrupted(format!(
                    "failed to write Claude prompt newline: {error}"
                ))
            })?;
            writer.flush().await.map_err(|error| {
                ProviderError::stream_interrupted(format!(
                    "failed to flush Claude prompt writer: {error}"
                ))
            })?;
        }

        writer.flush().await.map_err(|error| {
            ProviderError::stream_interrupted(format!(
                "failed to finalize Claude prompt writer: {error}"
            ))
        })?;
        Ok(())
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
            .map(|block| match block {
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
            })
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

fn collect_request_warnings(
    request: &ProviderRequest,
    prompt: &str,
    runtime_session_id: Option<&str>,
) -> Vec<String> {
    let mut warnings = collect_warning_messages(&request.settings);
    let model_id = request
        .model
        .provider_model_id
        .as_deref()
        .unwrap_or(&request.model.model_id);

    if let Some(warning) = validate_claude_model_id(model_id) {
        warnings.push(warning);
    }
    if let Some(warning) = validate_prompt_length(prompt) {
        warnings.push(warning);
    }
    if let Some(session_id) = runtime_session_id {
        if let Some(warning) = validate_session_id_format(session_id) {
            warnings.push(warning);
        }
    }

    warnings
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

fn append_text_block(message: &mut Message, text: &str) {
    if let Some(ContentBlock::Text { text: current }) = message.content.last_mut() {
        current.push_str(text);
        return;
    }

    message.content.push(ContentBlock::text(text.to_owned()));
}

fn append_tool_use_block(message: &mut Message, tool_call: &ToolCall) {
    if message
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolUse { call } if call.id == tool_call.id))
    {
        return;
    }

    message
        .content
        .push(ContentBlock::tool_use(tool_call.clone()));
}

fn parse_json_or_string(input: &str) -> Value {
    serde_json::from_str::<Value>(input).unwrap_or_else(|_| Value::String(input.to_owned()))
}

fn request_env_overrides(extra: &BTreeMap<String, Value>) -> BTreeMap<String, String> {
    extra
        .get("env")
        .and_then(Value::as_object)
        .map(|record| {
            record
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn merged_env_layers(layers: &[&BTreeMap<String, String>]) -> BTreeMap<String, String> {
    let mut merged = std::env::vars().collect::<BTreeMap<_, _>>();
    for layer in layers {
        merged.extend((*layer).clone());
    }
    merged
}

fn current_tool_name(tool_fsm: &ToolLifecycleTracker, tool_call_id: &str) -> String {
    match tool_fsm.state(tool_call_id) {
        ToolLifecycleState::Started { tool_name }
        | ToolLifecycleState::InputReceiving { tool_name, .. }
        | ToolLifecycleState::Executing { tool_name, .. }
        | ToolLifecycleState::Completed { tool_name, .. } => tool_name,
        ToolLifecycleState::Idle => "unknown".to_owned(),
    }
}

#[derive(Debug, Clone, PartialEq)]
struct NestedPreviewEntry {
    tool_name: String,
    state: &'static str,
    input: Value,
    output: Option<Value>,
    error: Option<Value>,
}

struct StreamRuntime {
    provider: ClaudeCodeProvider,
    sessions: SessionManager,
    tool_fsm: ToolLifecycleTracker,
    deduplicator: TextDeduplicator,
    reasoning_text: BTreeMap<String, String>,
    nested_tools: NestedToolTracker,
    nested_preview_state: BTreeMap<String, BTreeMap<String, NestedPreviewEntry>>,
    preview_signatures: BTreeMap<String, String>,
    tool_results: Vec<ToolResult>,
    emitter: EventEmitter,
    current_message: Message,
    message_started: bool,
    finished: bool,
    terminal_error: Option<ProviderError>,
}

impl StreamRuntime {
    fn new(
        request: &ProviderRequest,
        provider_id: ProviderId,
        provider: ClaudeCodeProvider,
        sessions: SessionManager,
    ) -> Self {
        Self {
            provider,
            sessions,
            tool_fsm: ToolLifecycleTracker::new(),
            deduplicator: TextDeduplicator::new(),
            reasoning_text: BTreeMap::new(),
            nested_tools: NestedToolTracker::new(),
            nested_preview_state: BTreeMap::new(),
            preview_signatures: BTreeMap::new(),
            tool_results: Vec::new(),
            emitter: EventEmitter::new(request, provider_id),
            current_message: Message::new(Role::Assistant, Vec::new()),
            message_started: false,
            finished: false,
            terminal_error: None,
        }
    }

    fn turn_start(&mut self) -> AgentEvent {
        AgentEvent::TurnStart {
            meta: self.emitter.next(),
        }
    }

    fn warning_event(&mut self, warning: &str) -> AgentEvent {
        AgentEvent::Custom {
            meta: self.emitter.next(),
            event_type: "claude_code.warning".to_owned(),
            payload: json!({
                "message": warning,
            }),
        }
    }

    const fn finished(&self) -> bool {
        self.finished
    }

    fn terminal_error(&self) -> Option<ProviderError> {
        self.terminal_error.clone()
    }

    fn buffered_text(&self) -> String {
        assistant_message_text(&self.current_message)
    }

    fn recover_from_truncation(&mut self, error: &ProviderError) -> Vec<AgentEvent> {
        let mut emitted = Vec::new();
        self.finished = true;
        self.ensure_message_started(&mut emitted);
        emitted.push(AgentEvent::MessageEnd {
            meta: self.emitter.next(),
            message: self.current_message.clone(),
        });
        emitted.push(AgentEvent::TurnEnd {
            meta: self.emitter.next(),
            message: self.current_message.clone(),
            tool_results: self.tool_results.clone(),
            usage: None,
        });
        emitted.push(AgentEvent::Custom {
            meta: self.emitter.next(),
            event_type: "claude_code.stream_retry_marker".to_owned(),
            payload: json!({
                "retry_suggested": true,
                "reason": "truncated_stream",
                "error": error.to_string(),
                "partial_text": self.buffered_text(),
            }),
        });
        emitted
    }

    async fn handle_event(
        &mut self,
        event: ClaudeNormalizedEvent,
    ) -> Result<Vec<AgentEvent>, ProviderError> {
        match event {
            ClaudeNormalizedEvent::Metadata(metadata) => self.handle_metadata(metadata).await,
            ClaudeNormalizedEvent::RateLimit(rate_limit) => {
                self.handle_rate_limit(rate_limit).await
            }
            ClaudeNormalizedEvent::TextDelta(text) => Ok(self.handle_text_delta(&text)),
            ClaudeNormalizedEvent::ReasoningStart(reasoning) => {
                Ok(self.handle_reasoning_start(reasoning))
            }
            ClaudeNormalizedEvent::ReasoningDelta(reasoning) => {
                Ok(self.handle_reasoning_delta(reasoning))
            }
            ClaudeNormalizedEvent::ReasoningComplete(reasoning) => {
                Ok(self.handle_reasoning_complete(reasoning))
            }
            ClaudeNormalizedEvent::ToolUseStart(tool) => self.handle_tool_use_start(tool),
            ClaudeNormalizedEvent::ToolUseInputDelta(input) => {
                self.handle_tool_use_input_delta(input)
            }
            ClaudeNormalizedEvent::ToolUseComplete(tool) => self.handle_tool_use_complete(&tool),
            ClaudeNormalizedEvent::ToolProgress(progress) => {
                Ok(self.handle_tool_progress(progress))
            }
            ClaudeNormalizedEvent::ToolResult(tool) => self.handle_tool_result(tool),
            ClaudeNormalizedEvent::Finish(finish) => Ok(self.handle_finish(&finish)),
        }
    }

    fn ensure_message_started(&mut self, emitted: &mut Vec<AgentEvent>) {
        if self.message_started {
            return;
        }

        self.message_started = true;
        emitted.push(AgentEvent::MessageStart {
            meta: self.emitter.next(),
            message: self.current_message.clone(),
        });
    }

    async fn handle_metadata(
        &mut self,
        metadata: crate::parser::ClaudeMetadataEvent,
    ) -> Result<Vec<AgentEvent>, ProviderError> {
        if let Some(session_id) = metadata.session_id.clone() {
            if let Some(request_session_id) = self.emitter.session_id.clone() {
                self.sessions
                    .record(&request_session_id, session_id.clone())
                    .await;
            }
        }
        if metadata.session_id.is_none() && metadata.model_id.is_none() {
            return Ok(Vec::new());
        }

        Ok(vec![AgentEvent::Custom {
            meta: self.emitter.next(),
            event_type: "claude_code.metadata".to_owned(),
            payload: json!({
                "session_id": metadata.session_id,
                "model_id": metadata.model_id,
            }),
        }])
    }

    async fn handle_rate_limit(
        &mut self,
        rate_limit: crate::parser::ClaudeRateLimitEvent,
    ) -> Result<Vec<AgentEvent>, ProviderError> {
        if let Some(session_id) = rate_limit.session_id.clone() {
            if let Some(request_session_id) = self.emitter.session_id.clone() {
                self.sessions.record(&request_session_id, session_id).await;
            }
        }

        Ok(vec![AgentEvent::Custom {
            meta: self.emitter.next(),
            event_type: "claude_code.rate_limit".to_owned(),
            payload: json!({
                "session_id": rate_limit.session_id,
                "rate_limit_info": rate_limit.rate_limit_info,
            }),
        }])
    }

    fn handle_text_delta(&mut self, text: &crate::parser::ClaudeTextDeltaEvent) -> Vec<AgentEvent> {
        let mut emitted = Vec::new();
        self.ensure_message_started(&mut emitted);
        let deduplicated = self.deduplicator.process(
            match text.source {
                ClaudeEventSource::StreamEvent => TextSource::StreamEvent,
                _ => TextSource::Assistant,
            },
            &text.text,
        );
        if deduplicated.is_empty() {
            return emitted;
        }

        append_text_block(&mut self.current_message, &deduplicated);
        emitted.push(AgentEvent::MessageUpdate {
            meta: self.emitter.next(),
            message: self.current_message.clone(),
            delta: StreamDelta::text(deduplicated),
        });
        emitted
    }

    fn handle_reasoning_start(
        &mut self,
        reasoning: crate::parser::ClaudeReasoningStartEvent,
    ) -> Vec<AgentEvent> {
        self.reasoning_text
            .entry(reasoning.reasoning_id.clone())
            .or_default();
        vec![AgentEvent::ReasoningStart {
            meta: self.emitter.next(),
            reasoning_id: reasoning.reasoning_id,
        }]
    }

    fn handle_reasoning_delta(
        &mut self,
        reasoning: crate::parser::ClaudeReasoningDeltaEvent,
    ) -> Vec<AgentEvent> {
        self.reasoning_text
            .entry(reasoning.reasoning_id.clone())
            .or_default()
            .push_str(&reasoning.text);
        vec![AgentEvent::ReasoningDelta {
            meta: self.emitter.next(),
            reasoning_id: reasoning.reasoning_id,
            delta: reasoning.text,
        }]
    }

    fn handle_reasoning_complete(
        &mut self,
        reasoning: crate::parser::ClaudeReasoningCompleteEvent,
    ) -> Vec<AgentEvent> {
        let full_text = if reasoning.full_text.is_empty() {
            self.reasoning_text
                .remove(&reasoning.reasoning_id)
                .unwrap_or_default()
        } else {
            self.reasoning_text.remove(&reasoning.reasoning_id);
            reasoning.full_text
        };

        vec![AgentEvent::ReasoningComplete {
            meta: self.emitter.next(),
            reasoning_id: reasoning.reasoning_id,
            full_text,
        }]
    }

    fn emit_nested_preview(&mut self, parent_tool_call_id: &str) -> Option<AgentEvent> {
        let tool_calls = self
            .nested_preview_state
            .get(parent_tool_call_id)?
            .iter()
            .map(|(tool_call_id, entry)| {
                json!({
                    "id": tool_call_id,
                    "tool": entry.tool_name,
                    "state": entry.state,
                    "input": entry.input,
                    "output": entry.output,
                    "error": entry.error,
                })
            })
            .collect::<Vec<_>>();
        let payload = json!({
            "preliminary": true,
            "toolCalls": tool_calls,
        });
        let signature = serde_json::to_string(&payload).ok()?;
        if self
            .preview_signatures
            .get(parent_tool_call_id)
            .is_some_and(|previous| previous == &signature)
        {
            return None;
        }
        self.preview_signatures
            .insert(parent_tool_call_id.to_owned(), signature);

        Some(AgentEvent::ToolExecutionUpdate {
            meta: self.emitter.next(),
            tool_call_id: parent_tool_call_id.to_owned(),
            tool_name: current_tool_name(&self.tool_fsm, parent_tool_call_id),
            partial_result: payload,
        })
    }

    fn handle_tool_use_start(
        &mut self,
        tool: crate::parser::ClaudeToolUseStartEvent,
    ) -> Result<Vec<AgentEvent>, ProviderError> {
        let mut emitted = Vec::new();
        let parent_tool_call_id = tool.parent_tool_call_id.clone();
        self.ensure_message_started(&mut emitted);
        self.deduplicator.reset();
        self.tool_fsm.start(&tool.tool_call_id, &tool.tool_name)?;
        if let Some(parent_tool_call_id) = &parent_tool_call_id {
            self.nested_tools.register_start(
                parent_tool_call_id,
                &tool.tool_call_id,
                &tool.tool_name,
                tool.input.clone(),
            );
            self.nested_preview_state
                .entry(parent_tool_call_id.clone())
                .or_default()
                .insert(
                    tool.tool_call_id.clone(),
                    NestedPreviewEntry {
                        tool_name: self.provider.canonical_tool_name(&tool.tool_name),
                        state: "running",
                        input: tool.input.clone(),
                        output: None,
                        error: None,
                    },
                );
        }

        emitted.push(AgentEvent::ToolExecutionStart {
            meta: self.emitter.next(),
            tool_call_id: tool.tool_call_id,
            tool_name: self.provider.canonical_tool_name(&tool.tool_name),
            args: tool.input,
        });
        if let Some(parent_tool_call_id) = parent_tool_call_id {
            if let Some(preview) = self.emit_nested_preview(&parent_tool_call_id) {
                emitted.push(preview);
            }
        }
        Ok(emitted)
    }

    fn handle_tool_use_input_delta(
        &mut self,
        input: crate::parser::ClaudeToolUseInputDeltaEvent,
    ) -> Result<Vec<AgentEvent>, ProviderError> {
        self.tool_fsm
            .input_delta(&input.tool_call_id, &input.delta)?;
        Ok(vec![
            AgentEvent::ToolExecutionUpdate {
                meta: self.emitter.next(),
                tool_call_id: input.tool_call_id.clone(),
                tool_name: current_tool_name(&self.tool_fsm, &input.tool_call_id),
                partial_result: json!({
                    "input_delta": input.delta.clone(),
                }),
            },
            AgentEvent::MessageUpdate {
                meta: self.emitter.next(),
                message: self.current_message.clone(),
                delta: StreamDelta::tool_use_input(input.tool_call_id, input.delta),
            },
        ])
    }

    fn handle_tool_use_complete(
        &mut self,
        tool: &crate::parser::ClaudeToolUseCompleteEvent,
    ) -> Result<Vec<AgentEvent>, ProviderError> {
        self.deduplicator.reset();
        self.tool_fsm
            .complete_input(&tool.tool_call_id, &tool.tool_name, &tool.final_input)?;
        let mut tool_call = ToolCall::new(
            tool.tool_call_id.clone(),
            self.provider.canonical_tool_name(&tool.tool_name),
            parse_json_or_string(&tool.final_input),
        );
        if let Some(parent_tool_call_id) = &tool.parent_tool_call_id {
            tool_call = tool_call.with_parent_id(parent_tool_call_id.clone());
        }
        append_tool_use_block(&mut self.current_message, &tool_call);

        Ok(vec![AgentEvent::MessageUpdate {
            meta: self.emitter.next(),
            message: self.current_message.clone(),
            delta: StreamDelta::tool_use(tool_call),
        }])
    }

    fn handle_tool_progress(
        &mut self,
        progress: crate::parser::ClaudeToolProgressEvent,
    ) -> Vec<AgentEvent> {
        vec![AgentEvent::ToolExecutionUpdate {
            meta: self.emitter.next(),
            tool_call_id: progress.tool_call_id,
            tool_name: self.provider.canonical_tool_name(&progress.tool_name),
            partial_result: json!({
                "progress": progress.progress_text,
            }),
        }]
    }

    fn handle_tool_result(
        &mut self,
        tool: crate::parser::ClaudeToolResultEvent,
    ) -> Result<Vec<AgentEvent>, ProviderError> {
        self.deduplicator.reset();
        self.tool_fsm.result(&tool.tool_call_id, !tool.is_error)?;
        let canonical_tool_name = self.provider.canonical_tool_name(&tool.tool_name);
        let mut emitted = Vec::new();
        if let Some(parent_tool_call_id) = &tool.parent_tool_call_id {
            let previous_entry = self
                .nested_preview_state
                .get(parent_tool_call_id)
                .and_then(|entries| entries.get(&tool.tool_call_id));
            let previous_input = previous_entry
                .cloned()
                .unwrap_or_else(|| NestedPreviewEntry {
                    tool_name: String::new(),
                    state: "running",
                    input: Value::Null,
                    output: None,
                    error: None,
                })
                .input;
            self.nested_preview_state
                .entry(parent_tool_call_id.clone())
                .or_default()
                .insert(
                    tool.tool_call_id.clone(),
                    NestedPreviewEntry {
                        tool_name: canonical_tool_name.clone(),
                        state: if tool.is_error { "error" } else { "completed" },
                        input: previous_input,
                        output: (!tool.is_error).then(|| tool.result_json.clone()),
                        error: tool.is_error.then(|| tool.result_json.clone()),
                    },
                );
            if let Some(preview) = self.emit_nested_preview(parent_tool_call_id) {
                emitted.push(preview);
            }
        } else {
            self.nested_preview_state.remove(&tool.tool_call_id);
            self.preview_signatures.remove(&tool.tool_call_id);
        }
        let result_json = if let Some(parent_tool_call_id) = &tool.parent_tool_call_id {
            self.nested_tools
                .register_result(parent_tool_call_id, &tool);
            tool.result_json.clone()
        } else {
            self.nested_tools
                .merge_into_parent_result(&tool.tool_call_id, tool.result_json.clone())
        };

        let result = if tool.is_error {
            ToolResult::failure(
                tool.tool_call_id.clone(),
                canonical_tool_name.clone(),
                tool.content.clone(),
            )
        } else {
            ToolResult::success(
                tool.tool_call_id.clone(),
                canonical_tool_name.clone(),
                tool.content.clone(),
            )
        };

        if tool.parent_tool_call_id.is_none() {
            self.tool_results.push(result);
        }

        emitted.push(AgentEvent::ToolExecutionEnd {
            meta: self.emitter.next(),
            tool_call_id: tool.tool_call_id,
            tool_name: canonical_tool_name,
            result: result_json,
            is_error: tool.is_error,
        });
        Ok(emitted)
    }

    fn handle_finish(&mut self, finish: &crate::parser::ClaudeFinishEvent) -> Vec<AgentEvent> {
        let mut emitted = Vec::new();
        self.deduplicator.reset();
        self.finished = true;
        self.terminal_error =
            classify_terminal_error(&self.provider.classifier, finish, &self.current_message);
        self.ensure_message_started(&mut emitted);
        emitted.push(AgentEvent::MessageEnd {
            meta: self.emitter.next(),
            message: self.current_message.clone(),
        });
        if self.terminal_error.is_none() {
            emitted.push(AgentEvent::TurnEnd {
                meta: self.emitter.next(),
                message: self.current_message.clone(),
                tool_results: self.tool_results.clone(),
                usage: Some(finish.usage.clone()),
            });
        }
        emitted.push(AgentEvent::Custom {
            meta: self.emitter.next(),
            event_type: "claude_code.finish".to_owned(),
            payload: json!({
                "finish_reason": finish.finish_reason,
                "usage": finish.usage,
                "session_id": finish.session_id,
                "is_error": finish.is_error,
                "result": finish.result,
                "error_code": finish.error_code,
            }),
        });
        emitted
    }
}

async fn finish_stream_process(
    process: &mut ManagedProcess,
    runtime: &StreamRuntime,
    error_classifier: &ClaudeErrorClassifier,
    binary: &str,
    stderr_excerpt: &str,
) -> Result<(), ProviderError> {
    let terminal_error = runtime.terminal_error();
    match process.wait().await {
        Ok(_) if runtime.finished() && terminal_error.is_none() => Ok(()),
        Ok(_) => {
            if let Some(error) = terminal_error {
                return Err(error);
            }

            if stderr_excerpt.trim().is_empty() {
                return Err(ProviderError::protocol_violation(
                    "Claude stream ended without a terminal finish event",
                    Some(json!({
                        "stderr": stderr_excerpt.trim(),
                    })),
                ));
            }

            let classification = error_classifier.classify(
                Some(stderr_excerpt.trim()),
                Some(stderr_excerpt.trim()),
                None,
                None,
            );
            Err(error_classifier.to_provider_error(
                &classification,
                stderr_excerpt.trim().to_owned(),
                Some(binary),
                None,
                Some(stderr_excerpt.trim()),
            ))
        }
        Err(ProviderError::ProcessCrashed {
            command, exit_code, ..
        }) => {
            if let Some(error) = terminal_error {
                return Err(error);
            }

            if stderr_excerpt.trim().is_empty() {
                return Err(ProviderError::process_crashed(&command, exit_code, None));
            }

            let classification = error_classifier.classify(
                Some(stderr_excerpt.trim()),
                Some(stderr_excerpt.trim()),
                exit_code,
                None,
            );
            Err(error_classifier.to_provider_error(
                &classification,
                stderr_excerpt.trim().to_owned(),
                Some(&command),
                exit_code,
                Some(stderr_excerpt.trim()),
            ))
        }
        Err(error) => Err(error),
    }
}

fn classify_terminal_error(
    error_classifier: &ClaudeErrorClassifier,
    finish: &crate::parser::ClaudeFinishEvent,
    message: &Message,
) -> Option<ProviderError> {
    if !finish.is_error {
        return None;
    }

    let detail = finish
        .result
        .clone()
        .unwrap_or_else(|| assistant_message_text(message));

    let classification =
        error_classifier.classify(None, Some(&detail), None, finish.error_code.as_deref());
    Some(error_classifier.to_provider_error(&classification, detail, Some("claude"), None, None))
}

fn assistant_message_text(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(|block| {
            if let ContentBlock::Text { text } = block {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
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
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0);
        let mut metadata = EventMetadata::new(timestamp_ms, self.sequence)
            .with_turn_id(self.turn_id.clone())
            .with_provider_id(self.provider_id.clone());
        if let Some(session_id) = &self.session_id {
            metadata = metadata.with_session_id(session_id.clone());
        }
        metadata
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{merged_env_layers, ClaudeCodeProvider, ClaudeCodeProviderConfig};
    use crate::{
        profile::ClaudeProviderProfile, BedrockProviderConfig, ClaudeCompatibleProviderConfig,
        OllamaProviderConfig,
    };
    use arky_protocol::{Message, ModelRef, ProviderId, SessionRef, TurnContext, TurnId};
    use arky_provider::{Provider, ProviderRequest};

    #[tokio::test]
    async fn provider_should_pass_session_id_to_cli_arguments() {
        let provider = ClaudeCodeProvider::with_config(ClaudeCodeProviderConfig {
            binary: "claude".to_owned(),
            ..ClaudeCodeProviderConfig::default()
        });
        let request = ProviderRequest::new(
            SessionRef::new(None).with_provider_session_id("session-123"),
            TurnContext::new(TurnId::new(), 1),
            ModelRef::new("sonnet"),
            vec![Message::user("hello")],
        );

        let config = provider
            .build_process_config(&request)
            .await
            .expect("process config should build");

        assert_eq!(
            config
                .process_config
                .args
                .windows(2)
                .any(|window| { window == ["--resume".to_owned(), "session-123".to_owned()] }),
            true
        );
    }

    #[test]
    fn merged_env_layers_should_prefer_later_overrides() {
        let request_env = BTreeMap::from([("SHARED_KEY".to_owned(), "request".to_owned())]);
        let provider_env = BTreeMap::from([("SHARED_KEY".to_owned(), "provider".to_owned())]);
        let profile_env = BTreeMap::from([
            ("SHARED_KEY".to_owned(), "profile".to_owned()),
            ("PROFILE_ONLY".to_owned(), "value".to_owned()),
        ]);

        let merged = merged_env_layers(&[&request_env, &provider_env, &profile_env]);

        assert_eq!(
            merged.get("SHARED_KEY").map(String::as_str),
            Some("profile")
        );
        assert_eq!(
            merged.get("PROFILE_ONLY").map(String::as_str),
            Some("value")
        );
    }

    #[tokio::test]
    async fn provider_should_merge_request_env_overrides_into_process_config() {
        let provider = ClaudeCodeProvider::with_config(ClaudeCodeProviderConfig {
            env: BTreeMap::from([("FROM_PROVIDER".to_owned(), "provider".to_owned())]),
            ..ClaudeCodeProviderConfig::default()
        });
        let mut request = ProviderRequest::new(
            SessionRef::new(None),
            TurnContext::new(TurnId::new(), 1),
            ModelRef::new("sonnet"),
            vec![Message::user("hello")],
        );
        request.settings.extra.insert(
            "env".to_owned(),
            json!({
                "FROM_REQUEST": "request"
            }),
        );

        let config = provider
            .build_process_config(&request)
            .await
            .expect("process config should build");

        assert_eq!(
            config
                .process_config
                .env
                .get("FROM_REQUEST")
                .map(String::as_str),
            Some("request")
        );
        assert_eq!(
            config
                .process_config
                .env
                .get("FROM_PROVIDER")
                .map(String::as_str),
            Some("provider")
        );
    }

    #[tokio::test]
    async fn bedrock_profile_should_keep_runtime_model_separate_from_selected_model() {
        let provider = ClaudeCodeProvider::with_profile_config(
            ClaudeProviderProfile::Bedrock(BedrockProviderConfig {
                base: ClaudeCompatibleProviderConfig::default(),
                selected_model: Some("anthropic.claude-3-7-sonnet-v1:0".to_owned()),
                region: Some("us-west-2".to_owned()),
            }),
            ClaudeCompatibleProviderConfig::default(),
        );
        let request = ProviderRequest::new(
            SessionRef::new(None),
            TurnContext::new(TurnId::new(), 1),
            ModelRef::new("sonnet").with_provider_model_id("anthropic.claude-3-5-haiku-v1:0"),
            vec![Message::user("hello")],
        );

        let config = provider
            .build_process_config(&request)
            .await
            .expect("process config should build");

        assert_eq!(
            config
                .process_config
                .args
                .windows(2)
                .any(|window| window == ["--model".to_owned(), "sonnet".to_owned()]),
            true
        );
        assert_eq!(
            config
                .process_config
                .env
                .get("ANTHROPIC_MODEL")
                .map(String::as_str),
            Some("anthropic.claude-3-5-haiku-v1:0")
        );
        assert_eq!(
            config
                .process_config
                .env
                .get("AWS_REGION")
                .map(String::as_str),
            Some("us-west-2")
        );
    }

    #[tokio::test]
    async fn bedrock_profile_should_use_request_model_for_selected_model_when_unmapped() {
        let provider = ClaudeCodeProvider::with_profile_config(
            ClaudeProviderProfile::Bedrock(BedrockProviderConfig {
                base: ClaudeCompatibleProviderConfig::default(),
                selected_model: None,
                region: Some("us-west-2".to_owned()),
            }),
            ClaudeCompatibleProviderConfig::default(),
        );
        let request = ProviderRequest::new(
            SessionRef::new(None),
            TurnContext::new(TurnId::new(), 1),
            ModelRef::new("sonnet"),
            vec![Message::user("hello")],
        );

        let config = provider
            .build_process_config(&request)
            .await
            .expect("process config should build");

        assert_eq!(
            config
                .process_config
                .env
                .get("ANTHROPIC_MODEL")
                .map(String::as_str),
            Some("sonnet")
        );
        assert_eq!(
            config
                .process_config
                .env
                .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
                .map(String::as_str),
            Some("sonnet")
        );
    }

    #[test]
    fn profiled_provider_should_use_actual_provider_id_in_descriptor() {
        let provider = ClaudeCodeProvider::with_profile_config(
            ClaudeProviderProfile::Bedrock(BedrockProviderConfig {
                base: ClaudeCompatibleProviderConfig::default(),
                selected_model: None,
                region: None,
            }),
            ClaudeCompatibleProviderConfig::default(),
        );

        assert_eq!(provider.descriptor().id, ProviderId::new("bedrock"));
    }

    #[tokio::test]
    async fn ollama_profile_should_use_request_model_over_wrapper_default() {
        let provider = ClaudeCodeProvider::with_profile_config(
            ClaudeProviderProfile::Ollama(OllamaProviderConfig {
                base: ClaudeCompatibleProviderConfig::default(),
                base_url: Some("http://localhost:11434".to_owned()),
                selected_model: "llama3".to_owned(),
            }),
            ClaudeCompatibleProviderConfig::default(),
        );
        let request = ProviderRequest::new(
            SessionRef::new(None),
            TurnContext::new(TurnId::new(), 1),
            ModelRef::new("qwen2.5"),
            vec![Message::user("hello")],
        );

        let config = provider
            .build_process_config(&request)
            .await
            .expect("process config should build");

        assert_eq!(
            config
                .process_config
                .env
                .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
                .map(String::as_str),
            Some("qwen2.5")
        );
    }

    #[tokio::test]
    async fn ollama_profile_should_allow_base_env_to_override_profile_settings() {
        let provider = ClaudeCodeProvider::with_profile_config(
            ClaudeProviderProfile::Ollama(OllamaProviderConfig {
                base: ClaudeCompatibleProviderConfig::default(),
                base_url: Some("http://localhost:11434".to_owned()),
                selected_model: "llama3".to_owned(),
            }),
            ClaudeCompatibleProviderConfig {
                env: BTreeMap::from([(
                    "ANTHROPIC_BASE_URL".to_owned(),
                    "http://override-me".to_owned(),
                )]),
                ..ClaudeCompatibleProviderConfig::default()
            },
        );
        let request = ProviderRequest::new(
            SessionRef::new(None),
            TurnContext::new(TurnId::new(), 1),
            ModelRef::new("sonnet"),
            vec![Message::user("hello")],
        );

        let config = provider
            .build_process_config(&request)
            .await
            .expect("process config should build");

        assert_eq!(
            config
                .process_config
                .env
                .get("ANTHROPIC_BASE_URL")
                .map(String::as_str),
            Some("http://override-me")
        );
        assert_eq!(
            config
                .process_config
                .env
                .get("ANTHROPIC_AUTH_TOKEN")
                .map(String::as_str),
            Some("ollama")
        );
    }

    #[tokio::test]
    async fn request_env_should_override_profile_and_base_env() {
        let provider = ClaudeCodeProvider::with_profile_config(
            ClaudeProviderProfile::Ollama(OllamaProviderConfig {
                base: ClaudeCompatibleProviderConfig::default(),
                base_url: Some("http://localhost:11434".to_owned()),
                selected_model: "llama3".to_owned(),
            }),
            ClaudeCompatibleProviderConfig {
                env: BTreeMap::from([(
                    "ANTHROPIC_BASE_URL".to_owned(),
                    "http://from-base".to_owned(),
                )]),
                ..ClaudeCompatibleProviderConfig::default()
            },
        );
        let mut request = ProviderRequest::new(
            SessionRef::new(None),
            TurnContext::new(TurnId::new(), 1),
            ModelRef::new("llama3"),
            vec![Message::user("hello")],
        );
        request.settings.extra.insert(
            "env".to_owned(),
            json!({
                "ANTHROPIC_BASE_URL": "http://from-request"
            }),
        );

        let config = provider
            .build_process_config(&request)
            .await
            .expect("process config should build");

        assert_eq!(
            config
                .process_config
                .env
                .get("ANTHROPIC_BASE_URL")
                .map(String::as_str),
            Some("http://from-request")
        );
    }
}
