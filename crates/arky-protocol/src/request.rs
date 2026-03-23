//! Shared request, usage, and response DTOs.

use std::{collections::BTreeMap, time::Duration};

use arky_error::ClassifiedError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    AgentEvent, Message, ProviderId, ReplayCursor, SessionId, ToolCall, ToolResult, TurnId,
};

/// Requested reasoning budget or depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    /// Minimal reasoning budget.
    Low,
    /// Default reasoning budget.
    Medium,
    /// Higher reasoning budget.
    High,
    /// Extended reasoning budget.
    XHigh,
}

/// Normalized finish reason reported by a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Provider stopped normally.
    Stop,
    /// Output terminated due to length or token limits.
    Length,
    /// Completion paused for a tool call.
    ToolUse,
    /// Completion stopped due to content filtering.
    ContentFilter,
    /// Provider terminated with an error.
    Error,
    /// Provider returned an unknown finish reason.
    Unknown,
}

/// Token accounting details for prompt-side usage.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct InputTokenDetails {
    /// Tokens read from a prompt cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<u64>,
    /// Tokens written to a prompt cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<u64>,
    /// Tokens processed without cache participation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_cache: Option<u64>,
}

/// Token accounting details for completion-side usage.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OutputTokenDetails {
    /// Tokens emitted as user-visible text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<u64>,
    /// Tokens emitted as reasoning or hidden thought.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<u64>,
}

/// Provider usage information accumulated during a run.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Prompt-side token count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// Completion-side token count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// Total token count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    /// Prompt-side token details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_details: Option<InputTokenDetails>,
    /// Completion-side token details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_details: Option<OutputTokenDetails>,
    /// Estimated dollar cost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Total wall-clock duration in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<f64>,
}

/// A stable reference to the active session.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionRef {
    /// SDK session identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<SessionId>,
    /// Provider-native session identifier used for resume.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    /// Replay state, when a session is being resumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_cursor: Option<ReplayCursor>,
}

impl SessionRef {
    /// Creates a session reference from an optional SDK session identifier.
    #[must_use]
    pub const fn new(id: Option<SessionId>) -> Self {
        Self {
            id,
            provider_session_id: None,
            replay_cursor: None,
        }
    }

    /// Stores a provider-native session identifier.
    #[must_use]
    pub fn with_provider_session_id(mut self, provider_session_id: impl Into<String>) -> Self {
        self.provider_session_id = Some(provider_session_id.into());
        self
    }

    /// Stores replay state.
    #[must_use]
    pub const fn with_replay_cursor(mut self, replay_cursor: ReplayCursor) -> Self {
        self.replay_cursor = Some(replay_cursor);
        self
    }
}

/// Per-turn execution context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TurnContext {
    /// Stable turn identifier.
    pub id: TurnId,
    /// Parent turn when a provider models nested turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<TurnId>,
    /// Monotonic turn sequence within the session.
    pub sequence: u64,
}

impl TurnContext {
    /// Creates turn context with no parent turn.
    #[must_use]
    pub const fn new(id: TurnId, sequence: u64) -> Self {
        Self {
            id,
            parent_id: None,
            sequence,
        }
    }

    /// Stores the parent turn identifier.
    #[must_use]
    pub const fn with_parent_id(mut self, parent_id: TurnId) -> Self {
        self.parent_id = Some(parent_id);
        self
    }
}

/// A normalized reference to the selected model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ModelRef {
    /// Model identifier selected by the caller.
    pub model_id: String,
    /// Provider family that resolves the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<ProviderId>,
    /// Provider-native model identifier after any mapping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_model_id: Option<String>,
}

impl ModelRef {
    /// Creates a model reference.
    #[must_use]
    pub fn new(model_id: impl Into<String>) -> Self {
        Self {
            model_id: model_id.into(),
            provider_id: None,
            provider_model_id: None,
        }
    }

    /// Stores the provider identifier.
    #[must_use]
    pub fn with_provider_id(mut self, provider_id: ProviderId) -> Self {
        self.provider_id = Some(provider_id);
        self
    }

    /// Stores a provider-native model identifier.
    #[must_use]
    pub fn with_provider_model_id(mut self, provider_model_id: impl Into<String>) -> Self {
        self.provider_model_id = Some(provider_model_id.into());
        self
    }
}

/// A serializable tool definition snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Canonical tool name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON schema for tool input.
    pub input_schema: Value,
}

impl ToolDefinition {
    /// Creates a tool definition snapshot.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
        }
    }
}

/// Tool-related context passed to providers.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToolContext {
    /// Available tools for the current call.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub definitions: Vec<ToolDefinition>,
    /// Active tool calls being tracked for the request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_calls: Vec<ToolCall>,
    /// Whether the tool registrations should be cleaned up after the call.
    #[serde(default)]
    pub call_scoped: bool,
}

impl ToolContext {
    /// Creates an empty tool context.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds tool definitions.
    #[must_use]
    pub fn with_definitions(mut self, definitions: Vec<ToolDefinition>) -> Self {
        self.definitions = definitions;
        self
    }

    /// Adds active tool calls.
    #[must_use]
    pub fn with_active_calls(mut self, active_calls: Vec<ToolCall>) -> Self {
        self.active_calls = active_calls;
        self
    }

    /// Marks the context as call scoped.
    #[must_use]
    pub const fn call_scoped(mut self, call_scoped: bool) -> Self {
        self.call_scoped = call_scoped;
        self
    }
}

/// Hook-related context passed to providers.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct HookContext {
    /// Enabled hook event names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enabled_events: Vec<String>,
    /// Arbitrary metadata used by downstream hook systems.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}

impl HookContext {
    /// Creates an empty hook context.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores enabled hook event names.
    #[must_use]
    pub fn with_enabled_events(mut self, enabled_events: Vec<String>) -> Self {
        self.enabled_events = enabled_events;
        self
    }

    /// Stores arbitrary hook metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: BTreeMap<String, Value>) -> Self {
        self.metadata = metadata;
        self
    }
}

/// Provider-specific settings that remain serializable across crate boundaries.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ProviderSettings {
    /// Sampling temperature override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Maximum token budget override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Stop sequences requested by the caller.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop_sequences: Vec<String>,
    /// Requested reasoning effort for capable providers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
    /// Provider-specific extension payload.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, Value>,
}

impl ProviderSettings {
    /// Creates empty provider settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// A shared provider request DTO.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ProviderRequest {
    /// Session execution context.
    pub session: SessionRef,
    /// Turn execution context.
    pub turn: TurnContext,
    /// Selected model reference.
    pub model: ModelRef,
    /// Prompt and conversation history.
    pub messages: Vec<Message>,
    /// Available tools.
    pub tools: ToolContext,
    /// Hook execution context.
    pub hooks: HookContext,
    /// Provider-specific settings.
    pub settings: ProviderSettings,
}

impl ProviderRequest {
    /// Creates a provider request with empty tool, hook, and settings context.
    #[must_use]
    pub fn new(
        session: SessionRef,
        turn: TurnContext,
        model: ModelRef,
        messages: Vec<Message>,
    ) -> Self {
        Self {
            session,
            turn,
            model,
            messages,
            tools: ToolContext::default(),
            hooks: HookContext::default(),
            settings: ProviderSettings::default(),
        }
    }

    /// Stores the tool context.
    #[must_use]
    pub fn with_tools(mut self, tools: ToolContext) -> Self {
        self.tools = tools;
        self
    }

    /// Stores the hook context.
    #[must_use]
    pub fn with_hooks(mut self, hooks: HookContext) -> Self {
        self.hooks = hooks;
        self
    }

    /// Stores provider settings.
    #[must_use]
    pub fn with_settings(mut self, settings: ProviderSettings) -> Self {
        self.settings = settings;
        self
    }
}

/// A serializable error projection derived from [`ClassifiedError`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorPayload {
    /// Stable machine-readable error code.
    pub error_code: String,
    /// Human-readable error message.
    pub message: String,
    /// Whether the operation can be retried safely.
    pub is_retryable: bool,
    /// Recommended retry delay in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    /// HTTP-style status code.
    pub http_status: u16,
    /// Structured self-correction payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction_context: Option<Value>,
}

impl ErrorPayload {
    /// Creates a serializable payload from any classified error.
    #[must_use]
    pub fn from_error<E>(error: &E) -> Self
    where
        E: ClassifiedError + ?Sized,
    {
        Self {
            error_code: error.error_code().to_owned(),
            message: error.to_string(),
            is_retryable: error.is_retryable(),
            retry_after_ms: error.retry_after().map(duration_to_millis),
            http_status: error.http_status(),
            correction_context: error.correction_context(),
        }
    }
}

/// Aggregated result returned by direct provider generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct GenerateResponse {
    /// Session execution context.
    pub session: SessionRef,
    /// Turn execution context.
    pub turn: TurnContext,
    /// Final message generated by the provider.
    pub message: Message,
    /// Provider finish reason, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,
    /// Token usage reported by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Error projection when generation completed with a classified failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorPayload>,
}

impl GenerateResponse {
    /// Creates a successful generation response.
    #[must_use]
    pub const fn new(session: SessionRef, turn: TurnContext, message: Message) -> Self {
        Self {
            session,
            turn,
            message,
            finish_reason: None,
            usage: None,
            error: None,
        }
    }

    /// Stores a provider finish reason.
    #[must_use]
    pub const fn with_finish_reason(mut self, finish_reason: FinishReason) -> Self {
        self.finish_reason = Some(finish_reason);
        self
    }

    /// Stores usage information.
    #[must_use]
    pub const fn with_usage(mut self, usage: Usage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Stores a serializable error projection.
    #[must_use]
    pub fn with_error<E>(mut self, error: &E) -> Self
    where
        E: ClassifiedError + ?Sized,
    {
        self.error = Some(ErrorPayload::from_error(error));
        self
    }
}

/// Aggregated result returned by the high-level agent layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AgentResponse {
    /// Session execution context.
    pub session: SessionRef,
    /// Turn execution context.
    pub turn: TurnContext,
    /// Final assistant message.
    pub message: Message,
    /// Tool results captured while assembling the response.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_results: Vec<ToolResult>,
    /// Token usage reported by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Event log captured during the turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<AgentEvent>,
    /// Error projection when the response carries a classified failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorPayload>,
}

impl AgentResponse {
    /// Creates a successful agent response.
    #[must_use]
    pub const fn new(session: SessionRef, turn: TurnContext, message: Message) -> Self {
        Self {
            session,
            turn,
            message,
            tool_results: Vec::new(),
            usage: None,
            events: Vec::new(),
            error: None,
        }
    }

    /// Stores tool results.
    #[must_use]
    pub fn with_tool_results(mut self, tool_results: Vec<ToolResult>) -> Self {
        self.tool_results = tool_results;
        self
    }

    /// Stores usage information.
    #[must_use]
    pub const fn with_usage(mut self, usage: Usage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Stores the event log.
    #[must_use]
    pub fn with_events(mut self, events: Vec<AgentEvent>) -> Self {
        self.events = events;
        self
    }

    /// Stores a serializable error projection.
    #[must_use]
    pub fn with_error<E>(mut self, error: &E) -> Self
    where
        E: ClassifiedError + ?Sized,
    {
        self.error = Some(ErrorPayload::from_error(error));
        self
    }
}

fn duration_to_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{
        FinishReason, GenerateResponse, HookContext, ModelRef, ProviderRequest, ProviderSettings,
        ReasoningEffort, SessionRef, ToolContext, ToolDefinition, TurnContext,
    };
    use crate::{Message, ProviderId, ReplayCursor, SessionId, ToolCall, TurnId};

    #[test]
    fn provider_request_should_support_full_context_population() {
        let session_id = SessionId::new();
        let turn_id = TurnId::new();
        let tool_call = ToolCall::new(
            "call-1",
            "mcp/files/read_file",
            json!({
                "path": "Cargo.toml",
            }),
        );
        let request = ProviderRequest::new(
            SessionRef::new(Some(session_id.clone()))
                .with_provider_session_id("provider-session")
                .with_replay_cursor(ReplayCursor::from_checkpoint(11)),
            TurnContext::new(turn_id.clone(), 3).with_parent_id(TurnId::new()),
            ModelRef::new("gpt-5")
                .with_provider_id(ProviderId::new("codex"))
                .with_provider_model_id("gpt-5-high"),
            vec![Message::system("system"), Message::user("hello")],
        )
        .with_tools(
            ToolContext::new()
                .with_definitions(vec![ToolDefinition::new(
                    "mcp/files/read_file",
                    "Read a file",
                    json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" }
                        },
                        "required": ["path"],
                    }),
                )])
                .with_active_calls(vec![tool_call.clone()])
                .call_scoped(true),
        )
        .with_hooks(
            HookContext::new()
                .with_metadata(BTreeMap::from([("mode".to_owned(), json!("strict"))])),
        )
        .with_settings({
            let mut settings = ProviderSettings::new();
            settings.temperature = Some(0.2);
            settings.max_tokens = Some(1_024);
            settings.stop_sequences = vec!["STOP".to_owned()];
            settings.reasoning_effort = Some(ReasoningEffort::Medium);
            settings.extra = BTreeMap::from([("reasoning".to_owned(), json!("medium"))]);
            settings
        });

        assert_eq!(request.session.id, Some(session_id));
        assert_eq!(request.turn.id, turn_id);
        assert_eq!(request.tools.active_calls, vec![tool_call]);
        assert_eq!(request.hooks.metadata.get("mode"), Some(&json!("strict")));
        assert_eq!(request.settings.max_tokens, Some(1_024));
        assert_eq!(
            request.settings.reasoning_effort,
            Some(ReasoningEffort::Medium)
        );
    }

    #[test]
    fn finish_reason_and_reasoning_effort_should_support_serde_round_trip() {
        let finish_reason = FinishReason::ToolUse;
        let reasoning_effort = ReasoningEffort::XHigh;

        let finish_json =
            serde_json::to_string(&finish_reason).expect("finish reason should serialize");
        let reasoning_json =
            serde_json::to_string(&reasoning_effort).expect("reasoning effort should serialize");

        let decoded_finish: FinishReason =
            serde_json::from_str(&finish_json).expect("finish reason should deserialize");
        let decoded_reasoning: ReasoningEffort =
            serde_json::from_str(&reasoning_json).expect("reasoning effort should deserialize");

        assert_eq!(decoded_finish, FinishReason::ToolUse);
        assert_eq!(decoded_reasoning, ReasoningEffort::XHigh);
    }

    #[test]
    fn generate_response_should_store_typed_finish_reason() {
        let response = GenerateResponse::new(
            SessionRef::new(Some(SessionId::new())),
            TurnContext::new(TurnId::new(), 1),
            Message::assistant("done"),
        )
        .with_finish_reason(FinishReason::Stop);

        assert_eq!(response.finish_reason, Some(FinishReason::Stop));
    }
}
