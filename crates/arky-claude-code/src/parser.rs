//! Claude CLI stream-json parsing and normalization.

use std::collections::{HashMap, HashSet};

use arky_protocol::{InputTokenDetails, OutputTokenDetails, ToolContent, Usage};
use arky_provider::ProviderError;
use serde_json::{json, Map, Value};

/// Origin of a normalized Claude event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeEventSource {
    /// Event emitted from `stream_event`.
    StreamEvent,
    /// Event emitted from an `assistant` snapshot.
    Assistant,
    /// Event emitted from a `system` record.
    System,
    /// Event emitted from a `user` record.
    User,
    /// Event emitted from a `result` record.
    Result,
    /// Event emitted from `tool_progress`.
    ToolProgress,
}

#[derive(Debug, Clone, PartialEq)]
struct ActiveToolBlock {
    tool_call_id: String,
    tool_name: String,
    input_snapshot: String,
    parent_tool_call_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveReasoningBlock {
    reasoning_id: String,
    full_text: String,
}

/// Normalized text delta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeTextDeltaEvent {
    /// Event source.
    pub source: ClaudeEventSource,
    /// Text delta content.
    pub text: String,
}

/// Normalized reasoning-start event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeReasoningStartEvent {
    /// Event source.
    pub source: ClaudeEventSource,
    /// Stable reasoning identifier.
    pub reasoning_id: String,
}

/// Normalized reasoning delta event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeReasoningDeltaEvent {
    /// Event source.
    pub source: ClaudeEventSource,
    /// Stable reasoning identifier.
    pub reasoning_id: String,
    /// Incremental reasoning text.
    pub text: String,
}

/// Normalized reasoning completion event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeReasoningCompleteEvent {
    /// Event source.
    pub source: ClaudeEventSource,
    /// Stable reasoning identifier.
    pub reasoning_id: String,
    /// Fully accumulated reasoning text.
    pub full_text: String,
}

/// Normalized tool-start event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeToolUseStartEvent {
    /// Event source.
    pub source: ClaudeEventSource,
    /// Stable tool call identifier.
    pub tool_call_id: String,
    /// Provider-emitted tool name.
    pub tool_name: String,
    /// Parsed tool input.
    pub input: Value,
    /// Serialized input snapshot.
    pub input_snapshot: String,
    /// Optional parent tool call identifier.
    pub parent_tool_call_id: Option<String>,
}

/// Normalized tool-input delta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeToolUseInputDeltaEvent {
    /// Event source.
    pub source: ClaudeEventSource,
    /// Stable tool call identifier.
    pub tool_call_id: String,
    /// Raw input fragment.
    pub delta: String,
    /// Optional parent tool call identifier.
    pub parent_tool_call_id: Option<String>,
}

/// Normalized tool-input completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeToolUseCompleteEvent {
    /// Event source.
    pub source: ClaudeEventSource,
    /// Stable tool call identifier.
    pub tool_call_id: String,
    /// Provider-emitted tool name.
    pub tool_name: String,
    /// Final serialized input.
    pub final_input: String,
    /// Optional parent tool call identifier.
    pub parent_tool_call_id: Option<String>,
}

/// Normalized tool result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeToolResultEvent {
    /// Stable tool call identifier.
    pub tool_call_id: String,
    /// Provider-emitted tool name.
    pub tool_name: String,
    /// Parsed tool content fragments.
    pub content: Vec<ToolContent>,
    /// JSON projection used for `AgentEvent::ToolExecutionEnd`.
    pub result_json: Value,
    /// Whether the tool completed with an error.
    pub is_error: bool,
    /// Optional parent tool call identifier.
    pub parent_tool_call_id: Option<String>,
}

/// Normalized tool progress update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeToolProgressEvent {
    /// Stable tool call identifier.
    pub tool_call_id: String,
    /// Provider-emitted tool name.
    pub tool_name: String,
    /// Human-readable progress text.
    pub progress_text: String,
    /// Optional parent tool call identifier.
    pub parent_tool_call_id: Option<String>,
}

/// Metadata emitted by Claude outside the assistant content stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeMetadataEvent {
    /// Event source.
    pub source: ClaudeEventSource,
    /// Provider-native Claude session identifier.
    pub session_id: Option<String>,
    /// Model identifier when Claude reports it.
    pub model_id: Option<String>,
}

/// Rate-limit metadata emitted by Claude outside the assistant content stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeRateLimitEvent {
    /// Provider-native Claude session identifier.
    pub session_id: Option<String>,
    /// Raw rate-limit info payload.
    pub rate_limit_info: Value,
}

/// Final turn metadata emitted by Claude.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeFinishEvent {
    /// Raw provider finish reason.
    pub finish_reason: String,
    /// Usage projection.
    pub usage: Usage,
    /// Provider-native Claude session identifier.
    pub session_id: Option<String>,
    /// Whether Claude marked the terminal result as an error.
    pub is_error: bool,
    /// Provider-declared terminal result text when present.
    pub result: Option<String>,
    /// Provider-declared terminal error code when present.
    pub error_code: Option<String>,
}

/// All normalized events emitted by the parser.
#[derive(Debug, Clone, PartialEq)]
pub enum ClaudeNormalizedEvent {
    /// Plain text delta.
    TextDelta(ClaudeTextDeltaEvent),
    /// Reasoning stream start.
    ReasoningStart(ClaudeReasoningStartEvent),
    /// Reasoning text delta.
    ReasoningDelta(ClaudeReasoningDeltaEvent),
    /// Reasoning stream completion.
    ReasoningComplete(ClaudeReasoningCompleteEvent),
    /// Tool invocation start.
    ToolUseStart(ClaudeToolUseStartEvent),
    /// Tool input fragment.
    ToolUseInputDelta(ClaudeToolUseInputDeltaEvent),
    /// Tool input finalized.
    ToolUseComplete(ClaudeToolUseCompleteEvent),
    /// Tool result or error.
    ToolResult(ClaudeToolResultEvent),
    /// Tool progress update.
    ToolProgress(ClaudeToolProgressEvent),
    /// Session/model metadata.
    Metadata(ClaudeMetadataEvent),
    /// Rate-limit metadata.
    RateLimit(ClaudeRateLimitEvent),
    /// Final usage + finish reason.
    Finish(ClaudeFinishEvent),
}

/// Stateful parser for Claude `stream-json` lines.
#[derive(Debug, Default, Clone)]
pub struct ClaudeEventParser {
    active_tools_by_block: HashMap<u64, ActiveToolBlock>,
    active_reasoning_by_block: HashMap<u64, ActiveReasoningBlock>,
    seen_stream_tool_call_ids: HashSet<String>,
    has_seen_stream_events: bool,
    last_error_code: Option<String>,
}

impl ClaudeEventParser {
    /// Creates an empty parser.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses one `stream-json` line into normalized events.
    pub fn parse_line(&mut self, line: &str) -> Result<Vec<ClaudeNormalizedEvent>, ProviderError> {
        let value = serde_json::from_str::<Value>(line).map_err(|error| {
            ProviderError::protocol_violation(
                format!("failed to parse Claude stream-json line: {error}"),
                Some(json!({
                    "line": line,
                    "line_number": error.line(),
                    "column": error.column(),
                })),
            )
        })?;

        self.parse_value(&value)
    }

    /// Parses one already-deserialized JSON record.
    pub fn parse_value(
        &mut self,
        value: &Value,
    ) -> Result<Vec<ClaudeNormalizedEvent>, ProviderError> {
        let record = as_object(value)?;
        let message_type = record.get("type").and_then(Value::as_str).ok_or_else(|| {
            protocol_error("Claude stream message is missing a `type` field", value)
        })?;

        match message_type {
            "system" => Ok(Self::parse_system(record)),
            "stream_event" => self.parse_stream_event(record),
            "assistant" => self.parse_assistant(record),
            "user" => Self::parse_user(record),
            "rate_limit_event" => Ok(Self::parse_rate_limit_event(record)),
            "result" => Ok(self.parse_result(record)),
            "tool_progress" => Self::parse_tool_progress(record),
            other => Err(protocol_error(
                format!("unsupported Claude stream message type `{other}`"),
                value,
            )),
        }
    }

    fn parse_system(record: &Map<String, Value>) -> Vec<ClaudeNormalizedEvent> {
        let session_id = normalize_optional_string(record.get("session_id"));
        let subtype = normalize_optional_string(record.get("subtype"));
        if subtype.as_deref() == Some("init") || session_id.is_some() {
            return vec![ClaudeNormalizedEvent::Metadata(ClaudeMetadataEvent {
                source: ClaudeEventSource::System,
                session_id,
                model_id: None,
            })];
        }

        Vec::new()
    }

    fn parse_rate_limit_event(record: &Map<String, Value>) -> Vec<ClaudeNormalizedEvent> {
        vec![ClaudeNormalizedEvent::RateLimit(ClaudeRateLimitEvent {
            session_id: normalize_optional_string(record.get("session_id")),
            rate_limit_info: record
                .get("rate_limit_info")
                .cloned()
                .unwrap_or(Value::Null),
        })]
    }

    fn parse_stream_event(
        &mut self,
        record: &Map<String, Value>,
    ) -> Result<Vec<ClaudeNormalizedEvent>, ProviderError> {
        self.has_seen_stream_events = true;
        let event = record.get("event").ok_or_else(|| {
            protocol_error(
                "stream_event payload is missing `event`",
                &Value::Object(record.clone()),
            )
        })?;
        let event_record = as_object(event)?;
        let event_type = event_record
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol_error("stream event is missing `type`", event))?;
        let parent_tool_call_id = extract_parent_tool_call_id(event_record)
            .or_else(|| extract_parent_tool_call_id(record));

        match event_type {
            "content_block_start" => {
                self.parse_stream_content_block_start(event, event_record, parent_tool_call_id)
            }
            "content_block_delta" => self.parse_stream_content_block_delta(event, event_record),
            "content_block_stop" => self.parse_stream_content_block_stop(event, event_record),
            _ => Ok(Vec::new()),
        }
    }

    fn parse_assistant(
        &mut self,
        record: &Map<String, Value>,
    ) -> Result<Vec<ClaudeNormalizedEvent>, ProviderError> {
        self.last_error_code = normalize_optional_string(record.get("error"));
        let message = record.get("message").ok_or_else(|| {
            protocol_error(
                "assistant message is missing `message`",
                &Value::Object(record.clone()),
            )
        })?;
        let message_record = as_object(message)?;
        let content = message_record
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol_error("assistant message is missing `content`", message))?;

        let mut events = Vec::new();
        for block in content {
            let block_record = as_object(block)?;
            match block_record
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
            {
                "text" => Self::push_assistant_text(block_record, &mut events),
                "thinking" => {
                    self.push_assistant_reasoning(block_record, &mut events);
                }
                "tool_use" => {
                    self.push_assistant_tool_use(record, block_record, block, &mut events)?;
                }
                _ => {}
            }
        }

        Ok(events)
    }

    fn push_assistant_text(
        block_record: &Map<String, Value>,
        events: &mut Vec<ClaudeNormalizedEvent>,
    ) {
        let text = block_record
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        events.push(ClaudeNormalizedEvent::TextDelta(ClaudeTextDeltaEvent {
            source: ClaudeEventSource::Assistant,
            text,
        }));
    }

    fn push_assistant_reasoning(
        &self,
        block_record: &Map<String, Value>,
        events: &mut Vec<ClaudeNormalizedEvent>,
    ) {
        if self.has_seen_stream_events {
            return;
        }

        let text = block_record
            .get("thinking")
            .and_then(Value::as_str)
            .or_else(|| block_record.get("text").and_then(Value::as_str))
            .unwrap_or_default()
            .to_owned();
        let reasoning_id = normalize_optional_string(block_record.get("id"))
            .unwrap_or_else(|| format!("reasoning-{}", events.len() + 1));
        events.push(ClaudeNormalizedEvent::ReasoningStart(
            ClaudeReasoningStartEvent {
                source: ClaudeEventSource::Assistant,
                reasoning_id: reasoning_id.clone(),
            },
        ));
        if !text.is_empty() {
            events.push(ClaudeNormalizedEvent::ReasoningDelta(
                ClaudeReasoningDeltaEvent {
                    source: ClaudeEventSource::Assistant,
                    reasoning_id: reasoning_id.clone(),
                    text: text.clone(),
                },
            ));
        }
        events.push(ClaudeNormalizedEvent::ReasoningComplete(
            ClaudeReasoningCompleteEvent {
                source: ClaudeEventSource::Assistant,
                reasoning_id,
                full_text: text,
            },
        ));
    }

    fn push_assistant_tool_use(
        &self,
        record: &Map<String, Value>,
        block_record: &Map<String, Value>,
        block: &Value,
        events: &mut Vec<ClaudeNormalizedEvent>,
    ) -> Result<(), ProviderError> {
        let tool_call_id = required_string(block_record, "id", block)?;
        if self.has_seen_stream_events && self.seen_stream_tool_call_ids.contains(&tool_call_id) {
            return Ok(());
        }

        let tool_name = required_string(block_record, "name", block)?;
        let input = block_record
            .get("input")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let input_snapshot = serialize_json(&input);
        let parent_tool_call_id = extract_parent_tool_call_id(block_record)
            .or_else(|| extract_parent_tool_call_id(record));

        events.push(ClaudeNormalizedEvent::ToolUseStart(
            ClaudeToolUseStartEvent {
                source: ClaudeEventSource::Assistant,
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                input,
                input_snapshot: input_snapshot.clone(),
                parent_tool_call_id: parent_tool_call_id.clone(),
            },
        ));
        events.push(ClaudeNormalizedEvent::ToolUseComplete(
            ClaudeToolUseCompleteEvent {
                source: ClaudeEventSource::Assistant,
                tool_call_id,
                tool_name,
                final_input: input_snapshot,
                parent_tool_call_id,
            },
        ));
        Ok(())
    }

    fn parse_user(
        record: &Map<String, Value>,
    ) -> Result<Vec<ClaudeNormalizedEvent>, ProviderError> {
        let message = record.get("message").ok_or_else(|| {
            protocol_error(
                "user message is missing `message`",
                &Value::Object(record.clone()),
            )
        })?;
        let message_record = as_object(message)?;
        let content = message_record
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol_error("user message is missing `content`", message))?;

        let mut events = Vec::new();
        for block in content {
            let block_record = as_object(block)?;
            if block_record
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                != "tool_result"
            {
                continue;
            }

            let tool_call_id =
                required_any_string(block_record, &["tool_use_id", "tool_call_id"], block)?;
            let tool_name = normalize_optional_string(block_record.get("name"))
                .unwrap_or_else(|| "unknown".to_owned());
            let raw_content = block_record.get("content").cloned().unwrap_or(Value::Null);
            let content = parse_tool_content(&raw_content);
            let result_json = tool_content_to_value(&content);
            let is_error = block_record
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let parent_tool_call_id = extract_parent_tool_call_id(block_record)
                .or_else(|| extract_parent_tool_call_id(record));

            events.push(ClaudeNormalizedEvent::ToolResult(ClaudeToolResultEvent {
                tool_call_id,
                tool_name,
                content,
                result_json,
                is_error,
                parent_tool_call_id,
            }));
        }

        Ok(events)
    }

    fn parse_result(&mut self, record: &Map<String, Value>) -> Vec<ClaudeNormalizedEvent> {
        let session_id = normalize_optional_string(record.get("session_id"));
        let finish_reason = normalize_optional_string(record.get("stop_reason"))
            .or_else(|| normalize_optional_string(record.get("subtype")))
            .unwrap_or_else(|| "unknown".to_owned());
        let usage = parse_usage(record.get("usage"));
        let is_error = record
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let result = normalize_optional_string(record.get("result"));
        let error_code = self.last_error_code.take();

        vec![
            ClaudeNormalizedEvent::Metadata(ClaudeMetadataEvent {
                source: ClaudeEventSource::Result,
                session_id: session_id.clone(),
                model_id: normalize_optional_string(record.get("model")),
            }),
            ClaudeNormalizedEvent::Finish(ClaudeFinishEvent {
                finish_reason,
                usage,
                session_id,
                is_error,
                result,
                error_code,
            }),
        ]
    }

    fn parse_tool_progress(
        record: &Map<String, Value>,
    ) -> Result<Vec<ClaudeNormalizedEvent>, ProviderError> {
        let tool_call_id = required_any_string(
            record,
            &["tool_call_id", "tool_use_id", "id"],
            &Value::Object(record.clone()),
        )?;
        let tool_name =
            normalize_optional_string(record.get("name")).unwrap_or_else(|| "unknown".to_owned());
        let progress_text = normalize_optional_string(record.get("message"))
            .or_else(|| normalize_optional_string(record.get("progress")))
            .or_else(|| normalize_optional_string(record.get("status")))
            .unwrap_or_else(|| "running".to_owned());

        Ok(vec![ClaudeNormalizedEvent::ToolProgress(
            ClaudeToolProgressEvent {
                tool_call_id,
                tool_name,
                progress_text,
                parent_tool_call_id: extract_parent_tool_call_id(record),
            },
        )])
    }

    fn parse_stream_content_block_start(
        &mut self,
        event: &Value,
        event_record: &Map<String, Value>,
        parent_tool_call_id: Option<String>,
    ) -> Result<Vec<ClaudeNormalizedEvent>, ProviderError> {
        let index = event_record
            .get("index")
            .and_then(Value::as_u64)
            .ok_or_else(|| protocol_error("content_block_start is missing `index`", event))?;
        let block = event_record.get("content_block").ok_or_else(|| {
            protocol_error("content_block_start is missing `content_block`", event)
        })?;
        let block_record = as_object(block)?;
        match block_record
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "tool_use" => {
                let tool_call_id = required_string(block_record, "id", block)?;
                let tool_name = required_string(block_record, "name", block)?;
                let input = block_record
                    .get("input")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let input_snapshot = serialize_json(&input);
                self.active_tools_by_block.insert(
                    index,
                    ActiveToolBlock {
                        tool_call_id: tool_call_id.clone(),
                        tool_name: tool_name.clone(),
                        input_snapshot: input_snapshot.clone(),
                        parent_tool_call_id: parent_tool_call_id.clone(),
                    },
                );
                self.seen_stream_tool_call_ids.insert(tool_call_id.clone());

                Ok(vec![ClaudeNormalizedEvent::ToolUseStart(
                    ClaudeToolUseStartEvent {
                        source: ClaudeEventSource::StreamEvent,
                        tool_call_id,
                        tool_name,
                        input,
                        input_snapshot,
                        parent_tool_call_id,
                    },
                )])
            }
            "thinking" => {
                let reasoning_id = normalize_optional_string(block_record.get("id"))
                    .unwrap_or_else(|| format!("reasoning-{index}"));
                self.active_reasoning_by_block.insert(
                    index,
                    ActiveReasoningBlock {
                        reasoning_id: reasoning_id.clone(),
                        full_text: String::new(),
                    },
                );

                Ok(vec![ClaudeNormalizedEvent::ReasoningStart(
                    ClaudeReasoningStartEvent {
                        source: ClaudeEventSource::StreamEvent,
                        reasoning_id,
                    },
                )])
            }
            _ => Ok(Vec::new()),
        }
    }

    fn parse_stream_content_block_delta(
        &mut self,
        event: &Value,
        event_record: &Map<String, Value>,
    ) -> Result<Vec<ClaudeNormalizedEvent>, ProviderError> {
        let index = event_record
            .get("index")
            .and_then(Value::as_u64)
            .ok_or_else(|| protocol_error("content_block_delta is missing `index`", event))?;
        let delta = event_record
            .get("delta")
            .ok_or_else(|| protocol_error("content_block_delta is missing `delta`", event))?;
        let delta_record = as_object(delta)?;
        match delta_record
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "text_delta" => Ok(vec![ClaudeNormalizedEvent::TextDelta(
                ClaudeTextDeltaEvent {
                    source: ClaudeEventSource::StreamEvent,
                    text: delta_record
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                },
            )]),
            "input_json_delta" => {
                let partial_json = delta_record
                    .get("partial_json")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let tool = self.active_tools_by_block.get_mut(&index).ok_or_else(|| {
                    protocol_error(
                        "input_json_delta referenced an unknown Claude tool block",
                        delta,
                    )
                })?;
                tool.input_snapshot.push_str(&partial_json);

                Ok(vec![ClaudeNormalizedEvent::ToolUseInputDelta(
                    ClaudeToolUseInputDeltaEvent {
                        source: ClaudeEventSource::StreamEvent,
                        tool_call_id: tool.tool_call_id.clone(),
                        delta: partial_json,
                        parent_tool_call_id: tool.parent_tool_call_id.clone(),
                    },
                )])
            }
            "thinking_delta" => {
                let delta_text = delta_record
                    .get("thinking")
                    .and_then(Value::as_str)
                    .or_else(|| delta_record.get("text").and_then(Value::as_str))
                    .unwrap_or_default()
                    .to_owned();
                let Some(reasoning) = self.active_reasoning_by_block.get_mut(&index) else {
                    return Ok(Vec::new());
                };
                reasoning.full_text.push_str(&delta_text);

                Ok(vec![ClaudeNormalizedEvent::ReasoningDelta(
                    ClaudeReasoningDeltaEvent {
                        source: ClaudeEventSource::StreamEvent,
                        reasoning_id: reasoning.reasoning_id.clone(),
                        text: delta_text,
                    },
                )])
            }
            _ => Ok(Vec::new()),
        }
    }

    fn parse_stream_content_block_stop(
        &mut self,
        event: &Value,
        event_record: &Map<String, Value>,
    ) -> Result<Vec<ClaudeNormalizedEvent>, ProviderError> {
        let index = event_record
            .get("index")
            .and_then(Value::as_u64)
            .ok_or_else(|| protocol_error("content_block_stop is missing `index`", event))?;
        if let Some(tool) = self.active_tools_by_block.remove(&index) {
            return Ok(vec![ClaudeNormalizedEvent::ToolUseComplete(
                ClaudeToolUseCompleteEvent {
                    source: ClaudeEventSource::StreamEvent,
                    tool_call_id: tool.tool_call_id,
                    tool_name: tool.tool_name,
                    final_input: tool.input_snapshot,
                    parent_tool_call_id: tool.parent_tool_call_id,
                },
            )]);
        }

        let Some(reasoning) = self.active_reasoning_by_block.remove(&index) else {
            return Ok(Vec::new());
        };

        Ok(vec![ClaudeNormalizedEvent::ReasoningComplete(
            ClaudeReasoningCompleteEvent {
                source: ClaudeEventSource::StreamEvent,
                reasoning_id: reasoning.reasoning_id,
                full_text: reasoning.full_text,
            },
        )])
    }
}

const MIN_TRUNCATION_BUFFER_LEN: usize = 512;

/// Detects whether a parser/protocol error looks like an in-flight Claude truncation.
#[must_use]
pub fn is_claude_truncation_error(error: &ProviderError, buffered_text: impl AsRef<str>) -> bool {
    let buffered_text = buffered_text.as_ref();
    if buffered_text.len() < MIN_TRUNCATION_BUFFER_LEN {
        return false;
    }

    let lower = error.to_string().to_ascii_lowercase();
    [
        "unexpected eof",
        "unexpected end of input",
        "unexpected end of string",
        "unterminated string",
        "eof while parsing",
        "end of file while parsing",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn parse_usage(value: Option<&Value>) -> Usage {
    let Some(usage) = value.and_then(Value::as_object) else {
        return Usage::default();
    };

    let input_tokens = usage.get("input_tokens").and_then(Value::as_u64);
    let output_tokens = usage.get("output_tokens").and_then(Value::as_u64);
    let cache_read = usage.get("cache_read_input_tokens").and_then(Value::as_u64);
    let cache_write = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64);

    Usage {
        input_tokens: Some(
            input_tokens.unwrap_or(0) + cache_read.unwrap_or(0) + cache_write.unwrap_or(0),
        ),
        output_tokens,
        total_tokens: Some(
            input_tokens.unwrap_or(0)
                + cache_read.unwrap_or(0)
                + cache_write.unwrap_or(0)
                + output_tokens.unwrap_or(0),
        ),
        input_details: Some(InputTokenDetails {
            cache_read,
            cache_write,
            no_cache: input_tokens,
        }),
        output_details: Some(OutputTokenDetails {
            text: output_tokens,
            reasoning: None,
        }),
        cost_usd: None,
        duration_ms: None,
    }
}

fn parse_tool_content(raw_content: &Value) -> Vec<ToolContent> {
    match raw_content {
        Value::String(text) => vec![ToolContent::text(text.clone())],
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                let record = item.as_object()?;
                match record
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                {
                    "text" => Some(ToolContent::text(
                        record
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    )),
                    _ => None,
                }
            })
            .collect::<Vec<_>>(),
        Value::Null => Vec::new(),
        other => vec![ToolContent::json(other.clone())],
    }
}

fn tool_content_to_value(content: &[ToolContent]) -> Value {
    if content.is_empty() {
        return Value::Null;
    }

    if content.len() == 1 {
        match &content[0] {
            ToolContent::Text { text } => return Value::String(text.clone()),
            ToolContent::Json { value } => return value.clone(),
            ToolContent::Image { data, media_type } => {
                return json!({
                    "type": "image",
                    "media_type": media_type,
                    "size_bytes": data.len(),
                });
            }
        }
    }

    Value::Array(
        content
            .iter()
            .map(|item| match item {
                ToolContent::Text { text } => json!({
                    "type": "text",
                    "text": text,
                }),
                ToolContent::Json { value } => json!({
                    "type": "json",
                    "value": value,
                }),
                ToolContent::Image { data, media_type } => json!({
                    "type": "image",
                    "media_type": media_type,
                    "size_bytes": data.len(),
                }),
            })
            .collect(),
    )
}

fn serialize_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned())
}

fn required_string(
    record: &Map<String, Value>,
    field: &str,
    context: &Value,
) -> Result<String, ProviderError> {
    normalize_optional_string(record.get(field))
        .ok_or_else(|| protocol_error(format!("Claude payload is missing `{field}`"), context))
}

fn required_any_string(
    record: &Map<String, Value>,
    fields: &[&str],
    context: &Value,
) -> Result<String, ProviderError> {
    fields
        .iter()
        .find_map(|field| normalize_optional_string(record.get(*field)))
        .ok_or_else(|| {
            protocol_error(
                format!("Claude payload is missing one of: {}", fields.join(", ")),
                context,
            )
        })
}

fn normalize_optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn extract_parent_tool_call_id(record: &Map<String, Value>) -> Option<String> {
    normalize_optional_string(record.get("parent_tool_use_id"))
        .or_else(|| normalize_optional_string(record.get("parent_tool_call_id")))
}

fn as_object(value: &Value) -> Result<&Map<String, Value>, ProviderError> {
    value
        .as_object()
        .ok_or_else(|| protocol_error("Claude stream message is not a JSON object", value))
}

fn protocol_error(message: impl Into<String>, value: &Value) -> ProviderError {
    ProviderError::protocol_violation(message.into(), Some(value.clone()))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use arky_provider::ProviderError;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{is_claude_truncation_error, ClaudeEventParser, ClaudeNormalizedEvent};

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name)
    }

    fn fixture_names() -> Vec<String> {
        let mut names = fs::read_dir(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures"),
        )
        .expect("fixture directory should read")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn parse_fixture(name: &str) -> Vec<ClaudeNormalizedEvent> {
        let mut parser = ClaudeEventParser::new();
        let input = fs::read_to_string(fixture_path(name)).expect("fixture should read");
        input
            .lines()
            .filter(|line| !line.trim().is_empty())
            .flat_map(|line| parser.parse_line(line).expect("fixture line should parse"))
            .collect()
    }

    #[test]
    fn fixture_corpus_should_parse_without_errors() {
        let fixture_count = fixture_names()
            .into_iter()
            .map(|name| parse_fixture(&name).len())
            .sum::<usize>();

        assert!(fixture_count > 0);
    }

    #[test]
    fn parser_should_normalize_text_fixture() {
        let events = parse_fixture("basic_text_stream.jsonl");

        assert_eq!(events.len(), 4);
        assert!(matches!(events[0], ClaudeNormalizedEvent::Metadata(..)));
        assert!(matches!(events[1], ClaudeNormalizedEvent::TextDelta(..)));
        assert!(matches!(events[2], ClaudeNormalizedEvent::Metadata(..)));
        assert!(matches!(events[3], ClaudeNormalizedEvent::Finish(..)));
    }

    #[test]
    fn parser_should_normalize_tool_lifecycle_fixture() {
        let events = parse_fixture("tool_cycle_stream.jsonl");

        assert_eq!(
            events
                .iter()
                .map(|event| match event {
                    ClaudeNormalizedEvent::TextDelta(_) => "text",
                    ClaudeNormalizedEvent::ReasoningStart(_) => "reasoning_start",
                    ClaudeNormalizedEvent::ReasoningDelta(_) => "reasoning_delta",
                    ClaudeNormalizedEvent::ReasoningComplete(_) => "reasoning_complete",
                    ClaudeNormalizedEvent::ToolUseStart(_) => "tool_start",
                    ClaudeNormalizedEvent::ToolUseInputDelta(_) => "tool_input",
                    ClaudeNormalizedEvent::ToolUseComplete(_) => "tool_complete",
                    ClaudeNormalizedEvent::ToolResult(_) => "tool_result",
                    ClaudeNormalizedEvent::Metadata(_) => "metadata",
                    ClaudeNormalizedEvent::RateLimit(_) => "rate_limit",
                    ClaudeNormalizedEvent::Finish(_) => "finish",
                    ClaudeNormalizedEvent::ToolProgress(_) => "tool_progress",
                })
                .collect::<Vec<_>>(),
            vec![
                "metadata",
                "tool_start",
                "tool_input",
                "tool_complete",
                "tool_result",
                "metadata",
                "finish"
            ]
        );
    }

    #[test]
    fn parser_should_preserve_nested_parent_identifiers() {
        let events = parse_fixture("nested_tool_stream.jsonl");

        let nested_start = events
            .iter()
            .find_map(|event| match event {
                ClaudeNormalizedEvent::ToolUseStart(event) if event.tool_call_id == "child-1" => {
                    Some(event)
                }
                _ => None,
            })
            .expect("nested start should exist");

        assert_eq!(
            nested_start.parent_tool_call_id,
            Some("parent-1".to_owned())
        );
    }

    #[test]
    fn parser_should_normalize_tool_progress_fixture() {
        let events = parse_fixture("tool_progress_stream.jsonl");

        let progress = events
            .iter()
            .find_map(|event| match event {
                ClaudeNormalizedEvent::ToolProgress(event) => Some(event),
                _ => None,
            })
            .expect("tool progress event should exist");

        assert_eq!(progress.tool_name, "search");
        assert_eq!(progress.progress_text, "searching docs");
    }

    #[test]
    fn parser_should_reject_malformed_json_lines() {
        let mut parser = ClaudeEventParser::new();
        let error = parser
            .parse_line("{not json")
            .expect_err("malformed JSON should fail");

        assert!(matches!(error, ProviderError::ProtocolViolation { .. }));
    }

    #[test]
    fn parser_should_convert_tool_result_payloads() {
        let mut parser = ClaudeEventParser::new();
        let events = parser
            .parse_value(&json!({
                "type": "user",
                "session_id": "session-1",
                "message": {
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "tool-1",
                        "name": "search",
                        "content": [{ "type": "text", "text": "done" }],
                        "is_error": false
                    }]
                }
            }))
            .expect("user tool result should parse");

        let result = match &events[0] {
            ClaudeNormalizedEvent::ToolResult(result) => result,
            other => panic!("expected tool result, got {other:?}"),
        };

        assert_eq!(result.result_json, json!("done"));
    }

    #[test]
    fn parser_should_normalize_rate_limit_event_fixture() {
        let events = parse_fixture("rate_limit_stream.jsonl");

        let rate_limit = events
            .iter()
            .find_map(|event| match event {
                ClaudeNormalizedEvent::RateLimit(event) => Some(event),
                _ => None,
            })
            .expect("rate limit event should exist");

        assert_eq!(rate_limit.session_id.as_deref(), Some("session-rate"));
        assert_eq!(rate_limit.rate_limit_info["status"], "allowed");
    }

    #[test]
    fn parser_should_normalize_thinking_blocks() {
        let mut parser = ClaudeEventParser::new();
        let events = vec![
            json!({
                "type": "stream_event",
                "event": {
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "thinking",
                        "id": "reasoning-1"
                    }
                }
            }),
            json!({
                "type": "stream_event",
                "event": {
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {
                        "type": "thinking_delta",
                        "thinking": "plan step"
                    }
                }
            }),
            json!({
                "type": "stream_event",
                "event": {
                    "type": "content_block_stop",
                    "index": 0
                }
            }),
        ]
        .into_iter()
        .flat_map(|value| parser.parse_value(&value).expect("value should parse"))
        .collect::<Vec<_>>();

        assert!(matches!(
            &events[0],
            ClaudeNormalizedEvent::ReasoningStart(event)
                if event.reasoning_id == "reasoning-1"
        ));
        assert!(matches!(
            &events[1],
            ClaudeNormalizedEvent::ReasoningDelta(event)
                if event.text == "plan step"
        ));
        assert!(matches!(
            &events[2],
            ClaudeNormalizedEvent::ReasoningComplete(event)
                if event.full_text == "plan step"
        ));
    }

    #[test]
    fn parser_should_detect_truncation_errors_only_with_enough_buffered_text() {
        let error = ProviderError::protocol_violation(
            "failed to parse Claude stream-json line: EOF while parsing a string",
            None,
        );

        assert_eq!(is_claude_truncation_error(&error, "x".repeat(600)), true);
        assert_eq!(is_claude_truncation_error(&error, "short"), false);
    }
}
