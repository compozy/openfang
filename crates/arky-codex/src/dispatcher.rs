//! Notification normalization for Codex app-server streams.

use std::sync::Arc;

use arky_protocol::{InputTokenDetails, OutputTokenDetails, TurnId, Usage};
use arky_tools::ToolIdCodec;
use serde_json::{Map, Value};

use crate::CodexNotification;

/// Normalized provider event produced from one Codex notification.
#[derive(Debug, Clone, PartialEq)]
pub enum NormalizedNotification {
    /// Notification is intentionally ignored.
    Ignored,
    /// Assistant message started.
    MessageStart {
        /// Optional authoritative message text.
        snapshot: Option<String>,
    },
    /// Assistant message delta.
    MessageDelta {
        /// Incremental text.
        delta: String,
    },
    /// Assistant message completed.
    MessageComplete {
        /// Optional authoritative message text.
        snapshot: Option<String>,
    },
    /// Reasoning block started.
    ReasoningStart {
        /// Stable reasoning identifier.
        reasoning_id: String,
    },
    /// Reasoning delta.
    ReasoningDelta {
        /// Stable reasoning identifier.
        reasoning_id: String,
        /// Incremental reasoning text.
        text: String,
    },
    /// Reasoning block completed.
    ReasoningComplete {
        /// Stable reasoning identifier.
        reasoning_id: String,
        /// Optional authoritative reasoning text.
        full_text: Option<String>,
    },
    /// Tool call started.
    ToolStart {
        /// Stable tool call identifier.
        call_id: String,
        /// Normalized tool name.
        tool_name: String,
        /// Initial tool input payload.
        input: Value,
        /// Optional parent call identifier.
        parent_id: Option<String>,
    },
    /// Tool output delta.
    ToolUpdate {
        /// Stable tool call identifier.
        call_id: String,
        /// Normalized tool name.
        tool_name: String,
        /// Partial tool result payload.
        partial_result: Value,
    },
    /// Tool call completed.
    ToolComplete {
        /// Stable tool call identifier.
        call_id: String,
        /// Normalized tool name.
        tool_name: String,
        /// Optional final result payload.
        result: Option<Value>,
        /// Whether the tool completed with an error.
        is_error: bool,
    },
    /// Turn usage snapshot updated out-of-band.
    UsageUpdated {
        /// Usage payload reported for the current turn.
        usage: Usage,
    },
    /// Turn completed successfully.
    TurnCompleted {
        /// Usage payload reported by the completed turn, when available.
        usage: Option<Usage>,
    },
    /// Turn failed.
    TurnFailed {
        /// Failure message.
        message: String,
    },
}

/// Stateful Codex notification dispatcher.
#[derive(Clone)]
pub struct CodexEventDispatcher {
    codec: Arc<dyn ToolIdCodec>,
    active_reasoning_id: Option<String>,
}

impl std::fmt::Debug for CodexEventDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexEventDispatcher")
            .finish_non_exhaustive()
    }
}

impl CodexEventDispatcher {
    /// Creates a dispatcher for one active turn stream.
    #[must_use]
    pub fn new(codec: Arc<dyn ToolIdCodec>) -> Self {
        Self {
            codec,
            active_reasoning_id: None,
        }
    }

    /// Normalizes one raw Codex notification.
    #[must_use]
    pub fn normalize(&mut self, notification: &CodexNotification) -> NormalizedNotification {
        let method = canonical_method(&notification.method);
        let params = notification.params.as_object();
        let item = params
            .and_then(|params| params.get("item"))
            .and_then(Value::as_object);
        let item_type = item
            .and_then(|item| item.get("type"))
            .and_then(Value::as_str)
            .map(canonical_item_type);

        match method.as_str() {
            "thread/tokenusage/updated" | "codex/event/token_count" => {
                let Some(usage) = extract_incremental_usage(params) else {
                    return NormalizedNotification::Ignored;
                };
                NormalizedNotification::UsageUpdated { usage }
            }
            "turn/completed" => NormalizedNotification::TurnCompleted {
                usage: extract_usage(params),
            },
            "turn/failed" | "error" => NormalizedNotification::TurnFailed {
                message: extract_error_message(params)
                    .unwrap_or_else(|| "Codex turn failed".to_owned()),
            },
            "plan/delta"
            | "reasoning/content/delta"
            | "reasoning/raw/content/delta"
            | "reasoning/summary/part/added"
            | "item/reasoning/delta"
            | "item/reasoning/content/part/added"
            | "item/reasoning/summary/part/added"
            | "agent/reasoning/raw/content"
            | "agent/reasoning/raw/content/delta" => self.normalize_reasoning_delta(params, item),
            "item/agentmessage/delta"
            | "item/agent/message/delta"
            | "item/assistant/message/delta"
            | "item/assistantmessage/delta"
            | "item/agent/message/content/part/added"
            | "item/assistant/message/content/part/added" => {
                let Some(delta) = extract_text_delta(params, item) else {
                    return NormalizedNotification::Ignored;
                };
                NormalizedNotification::MessageDelta { delta }
            }
            "item/commandexecution/outputdelta"
            | "item/filechange/outputdelta"
            | "item/mcptoolcall/outputdelta"
            | "item/collabtoolcall/outputdelta"
            | "exec/command/output/delta"
            | "item/command/execution/output/delta"
            | "file/change/output/delta"
            | "item/file/change/output/delta"
            | "exec/command/terminal/interaction"
            | "item/command/execution/terminal/interaction" => {
                normalize_tool_update(params, item, self.codec.as_ref())
            }
            "item/started" | "item.started" => match item_type.as_deref() {
                Some("agentmessage" | "assistantmessage") => NormalizedNotification::MessageStart {
                    snapshot: extract_text_snapshot(item),
                },
                Some("reasoning" | "reasoningsummary") => {
                    self.normalize_reasoning_start(params, item)
                }
                Some("commandexecution" | "filechange" | "mcptoolcall" | "collabtoolcall") => {
                    normalize_tool_start(item, self.codec.as_ref())
                }
                _ => NormalizedNotification::Ignored,
            },
            "item/completed" | "item.completed" => match item_type.as_deref() {
                Some("agentmessage" | "assistantmessage") => {
                    NormalizedNotification::MessageComplete {
                        snapshot: extract_text_snapshot(item),
                    }
                }
                Some("reasoning" | "reasoningsummary") => {
                    self.normalize_reasoning_complete(params, item)
                }
                Some("commandexecution" | "filechange" | "mcptoolcall" | "collabtoolcall") => {
                    normalize_tool_complete(item, self.codec.as_ref())
                }
                _ => NormalizedNotification::Ignored,
            },
            _ => NormalizedNotification::Ignored,
        }
    }

    fn normalize_reasoning_start(
        &mut self,
        params: Option<&Map<String, Value>>,
        item: Option<&Map<String, Value>>,
    ) -> NormalizedNotification {
        let reasoning_id = self.resolve_reasoning_id(params, item, false);
        self.active_reasoning_id = Some(reasoning_id.clone());
        NormalizedNotification::ReasoningStart { reasoning_id }
    }

    fn normalize_reasoning_delta(
        &mut self,
        params: Option<&Map<String, Value>>,
        item: Option<&Map<String, Value>>,
    ) -> NormalizedNotification {
        let Some(text) = extract_reasoning_text(params, item) else {
            return NormalizedNotification::Ignored;
        };

        let reasoning_id = self.resolve_reasoning_id(params, item, true);
        self.active_reasoning_id = Some(reasoning_id.clone());
        NormalizedNotification::ReasoningDelta { reasoning_id, text }
    }

    fn normalize_reasoning_complete(
        &mut self,
        params: Option<&Map<String, Value>>,
        item: Option<&Map<String, Value>>,
    ) -> NormalizedNotification {
        let reasoning_id = self.resolve_reasoning_id(params, item, true);
        if self.active_reasoning_id.as_deref() == Some(reasoning_id.as_str()) {
            self.active_reasoning_id = None;
        }
        NormalizedNotification::ReasoningComplete {
            reasoning_id,
            full_text: extract_reasoning_text(params, item),
        }
    }

    fn resolve_reasoning_id(
        &self,
        params: Option<&Map<String, Value>>,
        item: Option<&Map<String, Value>>,
        allow_active: bool,
    ) -> String {
        params
            .and_then(|params| params.get("reasoningId"))
            .and_then(Value::as_str)
            .or_else(|| {
                params
                    .and_then(|params| params.get("itemId"))
                    .and_then(Value::as_str)
            })
            .or_else(|| item.and_then(|item| item.get("id")).and_then(Value::as_str))
            .map(ToOwned::to_owned)
            .or_else(|| {
                allow_active
                    .then(|| self.active_reasoning_id.clone())
                    .flatten()
            })
            .unwrap_or_else(|| TurnId::new().to_string())
    }
}

fn extract_usage(params: Option<&Map<String, Value>>) -> Option<Usage> {
    params
        .and_then(|params| params.get("usage"))
        .cloned()
        .and_then(|usage| serde_json::from_value(usage).ok())
}

fn extract_incremental_usage(params: Option<&Map<String, Value>>) -> Option<Usage> {
    params
        .and_then(extract_thread_usage_snapshot)
        .or_else(|| params.and_then(extract_token_count_snapshot))
        .and_then(parse_usage_snapshot)
}

fn extract_thread_usage_snapshot(params: &Map<String, Value>) -> Option<&Value> {
    params
        .get("tokenUsage")
        .or_else(|| params.get("token_usage"))
        .and_then(Value::as_object)
        .and_then(|token_usage| {
            token_usage
                .get("last")
                .or_else(|| token_usage.get("lastTokenUsage"))
                .or_else(|| token_usage.get("total"))
                .or_else(|| token_usage.get("totalTokenUsage"))
        })
}

fn extract_token_count_snapshot(params: &Map<String, Value>) -> Option<&Value> {
    params
        .get("msg")
        .and_then(Value::as_object)
        .and_then(|msg| msg.get("info"))
        .and_then(Value::as_object)
        .and_then(|info| {
            info.get("last_token_usage")
                .or_else(|| info.get("lastTokenUsage"))
                .or_else(|| info.get("total_token_usage"))
                .or_else(|| info.get("totalTokenUsage"))
        })
}

fn parse_usage_snapshot(snapshot: &Value) -> Option<Usage> {
    let snapshot = snapshot.as_object()?;
    let input_tokens = u64_field(snapshot, "input_tokens", "inputTokens");
    let output_tokens = u64_field(snapshot, "output_tokens", "outputTokens");
    let total_tokens = u64_field(snapshot, "total_tokens", "totalTokens").or_else(|| {
        input_tokens.and_then(|input| output_tokens.map(|output| input.saturating_add(output)))
    });
    let cached_input_tokens = u64_field(snapshot, "cached_input_tokens", "cachedInputTokens");
    let reasoning_output_tokens =
        u64_field(snapshot, "reasoning_output_tokens", "reasoningOutputTokens");
    let text_output_tokens = output_tokens
        .map(|output| output.saturating_sub(reasoning_output_tokens.unwrap_or_default()));
    let input_details =
        (cached_input_tokens.is_some() || input_tokens.is_some()).then_some(InputTokenDetails {
            cache_read: cached_input_tokens,
            cache_write: None,
            no_cache: input_tokens
                .map(|input| input.saturating_sub(cached_input_tokens.unwrap_or_default())),
        });
    let output_details = (reasoning_output_tokens.is_some() || output_tokens.is_some()).then_some(
        OutputTokenDetails {
            text: text_output_tokens,
            reasoning: reasoning_output_tokens,
        },
    );

    Some(Usage {
        input_tokens,
        output_tokens,
        total_tokens,
        input_details,
        output_details,
        cost_usd: None,
        duration_ms: None,
    })
}

fn u64_field(object: &Map<String, Value>, snake_case: &str, camel_case: &str) -> Option<u64> {
    object
        .get(snake_case)
        .or_else(|| object.get(camel_case))
        .and_then(Value::as_u64)
}

fn normalize_tool_start(
    item: Option<&Map<String, Value>>,
    codec: &dyn ToolIdCodec,
) -> NormalizedNotification {
    let Some(item) = item else {
        return NormalizedNotification::Ignored;
    };
    let call_id = item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_owned();
    let tool_name = canonical_tool_name(item, codec);
    let input = tool_input(item);
    let parent_id = item
        .get("parentId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    NormalizedNotification::ToolStart {
        call_id,
        tool_name,
        input,
        parent_id,
    }
}

fn normalize_tool_update(
    params: Option<&Map<String, Value>>,
    item: Option<&Map<String, Value>>,
    codec: &dyn ToolIdCodec,
) -> NormalizedNotification {
    let call_id = params
        .and_then(|params| params.get("itemId"))
        .and_then(Value::as_str)
        .or_else(|| item.and_then(|item| item.get("id")).and_then(Value::as_str))
        .unwrap_or("tool")
        .to_owned();
    let tool_name = tool_name_or_default(item, codec);
    let partial = params
        .and_then(|params| params.get("delta").cloned())
        .or_else(|| params.and_then(|params| params.get("output").cloned()))
        .unwrap_or_else(|| Value::String(String::new()));

    NormalizedNotification::ToolUpdate {
        call_id,
        tool_name,
        partial_result: partial,
    }
}

fn normalize_tool_complete(
    item: Option<&Map<String, Value>>,
    codec: &dyn ToolIdCodec,
) -> NormalizedNotification {
    let Some(item) = item else {
        return NormalizedNotification::Ignored;
    };
    let call_id = item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_owned();
    let tool_name = canonical_tool_name(item, codec);
    let status = item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let is_error = matches!(status, "failed" | "declined") || item.get("error").is_some();
    let result = item
        .get("result")
        .cloned()
        .or_else(|| item.get("aggregatedOutput").cloned())
        .or_else(|| item.get("changes").cloned());

    NormalizedNotification::ToolComplete {
        call_id,
        tool_name,
        result,
        is_error,
    }
}

fn canonical_method(method: &str) -> String {
    method.to_ascii_lowercase().replace('.', "/")
}

fn canonical_item_type(item_type: &str) -> String {
    item_type
        .chars()
        .filter(|character| *character != '_' && *character != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

fn extract_text_delta(
    params: Option<&Map<String, Value>>,
    item: Option<&Map<String, Value>>,
) -> Option<String> {
    params
        .and_then(|params| params.get("delta"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            item.and_then(|item| item.get("delta"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn extract_text_snapshot(item: Option<&Map<String, Value>>) -> Option<String> {
    item.and_then(|item| item.get("text"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn extract_reasoning_text(
    params: Option<&Map<String, Value>>,
    item: Option<&Map<String, Value>>,
) -> Option<String> {
    params
        .and_then(|params| params.get("delta"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            params
                .and_then(|params| params.get("text"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            params
                .and_then(|params| params.get("content"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            item.and_then(|item| item.get("text"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            item.and_then(|item| item.get("content"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            item.and_then(|item| item.get("summary"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn extract_error_message(params: Option<&Map<String, Value>>) -> Option<String> {
    params
        .and_then(|params| params.get("message"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            params
                .and_then(|params| params.get("error"))
                .and_then(Value::as_object)
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            params
                .and_then(|params| params.get("turn"))
                .and_then(Value::as_object)
                .and_then(|turn| turn.get("error"))
                .and_then(Value::as_object)
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn canonical_tool_name(item: &Map<String, Value>, codec: &dyn ToolIdCodec) -> String {
    let item_type = item
        .get("type")
        .and_then(Value::as_str)
        .map(canonical_item_type);

    let direct_name = item
        .get("tool")
        .and_then(Value::as_str)
        .or_else(|| item.get("name").and_then(Value::as_str))
        .or_else(|| item.get("command").and_then(Value::as_str));

    if let Some(name) = direct_name {
        if let Ok(decoded) = codec.decode(name) {
            return decoded.canonical_name;
        }
        return name.to_owned();
    }

    match item_type.as_deref() {
        Some("commandexecution") => "command_execution".to_owned(),
        Some("filechange") => "file_change".to_owned(),
        Some("mcptoolcall") => {
            let server = item
                .get("server")
                .and_then(Value::as_str)
                .unwrap_or("server");
            let tool = item.get("tool").and_then(Value::as_str).unwrap_or("tool");
            format!("mcp/{server}/{tool}")
        }
        Some("collabtoolcall") => item
            .get("tool")
            .and_then(Value::as_str)
            .unwrap_or("collab_tool")
            .to_owned(),
        _ => "tool".to_owned(),
    }
}

fn tool_name_or_default(item: Option<&Map<String, Value>>, codec: &dyn ToolIdCodec) -> String {
    let Some(item) = item else {
        return "tool".to_owned();
    };

    canonical_tool_name(item, codec)
}

fn tool_input(item: &Map<String, Value>) -> Value {
    item.get("arguments")
        .cloned()
        .or_else(|| item.get("input").cloned())
        .or_else(|| item.get("command").cloned())
        .unwrap_or_else(|| Value::Object(item.clone()))
}

#[cfg(test)]
mod tests {
    use arky_tools::create_codex_tool_id_codec;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::sync::Arc;

    use super::{CodexEventDispatcher, NormalizedNotification};
    use crate::CodexNotification;

    #[test]
    fn dispatcher_should_handle_message_tool_reasoning_and_turn_events() {
        let mut dispatcher = CodexEventDispatcher::new(Arc::new(create_codex_tool_id_codec()));

        for (method, params, expected_kind) in dispatcher_cases() {
            let normalized = dispatcher.normalize(&CodexNotification {
                method: method.to_owned(),
                params,
            });

            let actual_kind = notification_kind(&normalized);

            assert_eq!(actual_kind, expected_kind, "method `{method}`");
        }
    }

    #[test]
    fn dispatcher_should_generate_and_reuse_reasoning_ids_when_events_omit_them() {
        let mut dispatcher = CodexEventDispatcher::new(Arc::new(create_codex_tool_id_codec()));

        let started = dispatcher.normalize(&CodexNotification {
            method: "plan.delta".to_owned(),
            params: json!({
                "delta": "think",
            }),
        });
        let completed = dispatcher.normalize(&CodexNotification {
            method: "item/completed".to_owned(),
            params: json!({
                "item": {
                    "type": "reasoning",
                    "text": "done",
                },
            }),
        });

        let started_id = match started {
            NormalizedNotification::ReasoningDelta { reasoning_id, .. } => reasoning_id,
            other => panic!("unexpected normalized event: {other:?}"),
        };
        let completed_id = match completed {
            NormalizedNotification::ReasoningComplete { reasoning_id, .. } => reasoning_id,
            other => panic!("unexpected normalized event: {other:?}"),
        };

        assert_eq!(started_id, completed_id);
        assert_eq!(started_id.len(), 36);
    }

    fn notification_kind(notification: &NormalizedNotification) -> &'static str {
        match notification {
            NormalizedNotification::Ignored => "ignored",
            NormalizedNotification::MessageStart { .. } => "message_start",
            NormalizedNotification::MessageDelta { .. } => "message_delta",
            NormalizedNotification::MessageComplete { .. } => "message_complete",
            NormalizedNotification::ReasoningStart { .. } => "reasoning_start",
            NormalizedNotification::ReasoningDelta { .. } => "reasoning_delta",
            NormalizedNotification::ReasoningComplete { .. } => "reasoning_complete",
            NormalizedNotification::ToolStart { .. } => "tool_start",
            NormalizedNotification::ToolUpdate { .. } => "tool_update",
            NormalizedNotification::ToolComplete { .. } => "tool_complete",
            NormalizedNotification::UsageUpdated { .. } => "usage_updated",
            NormalizedNotification::TurnCompleted { .. } => "turn_completed",
            NormalizedNotification::TurnFailed { .. } => "turn_failed",
        }
    }

    fn dispatcher_cases() -> Vec<(&'static str, Value, &'static str)> {
        let mut cases = Vec::new();
        cases.extend(message_and_reasoning_cases());
        cases.extend(tool_and_turn_cases());
        cases.extend(ignored_cases());
        cases
    }

    fn message_and_reasoning_cases() -> Vec<(&'static str, Value, &'static str)> {
        vec![
            (
                "item.started",
                json!({"item": {"id": "message-1", "type": "agentMessage", "text": "hi"}}),
                "message_start",
            ),
            (
                "item/agentMessage/delta",
                json!({"delta": "hello"}),
                "message_delta",
            ),
            (
                "item.agent.message.delta",
                json!({"delta": "hello"}),
                "message_delta",
            ),
            (
                "item/completed",
                json!({"item": {"id": "message-1", "type": "assistantMessage", "text": "done"}}),
                "message_complete",
            ),
            ("plan.delta", json!({"delta": "step 1"}), "reasoning_delta"),
            (
                "reasoning.content.delta",
                json!({"text": "step 2"}),
                "reasoning_delta",
            ),
            (
                "reasoning.raw.content.delta",
                json!({"content": "step 3"}),
                "reasoning_delta",
            ),
            (
                "reasoning.summary.part.added",
                json!({"text": "summary"}),
                "reasoning_delta",
            ),
            (
                "item.started",
                json!({"item": {"id": "reasoning-1", "type": "reasoning", "text": ""}}),
                "reasoning_start",
            ),
            (
                "item.reasoNing.delta",
                json!({"itemId": "reasoning-1", "delta": "delta"}),
                "reasoning_delta",
            ),
            (
                "item/reasoning/content/part/added",
                json!({"itemId": "reasoning-1", "delta": "more"}),
                "reasoning_delta",
            ),
            (
                "item/reasoning/summary/part/added",
                json!({"itemId": "reasoning-1", "text": "summary"}),
                "reasoning_delta",
            ),
            (
                "agent/reasoning/raw/content",
                json!({"text": "raw"}),
                "reasoning_delta",
            ),
            (
                "agent/reasoning/raw/content.delta",
                json!({"text": "raw"}),
                "reasoning_delta",
            ),
            (
                "item.completed",
                json!({"item": {"id": "reasoning-1", "type": "reasoning", "text": "done"}}),
                "reasoning_complete",
            ),
        ]
    }

    fn tool_and_turn_cases() -> Vec<(&'static str, Value, &'static str)> {
        vec![
            (
                "item.started",
                json!({"item": {"id": "tool-1", "type": "commandExecution", "command": "ls"}}),
                "tool_start",
            ),
            (
                "item/commandExecution/outputDelta",
                json!({"itemId": "tool-1", "delta": "listing"}),
                "tool_update",
            ),
            (
                "item/fileChange/outputDelta",
                json!({"itemId": "tool-1", "output": "changed"}),
                "tool_update",
            ),
            (
                "item/mcpToolCall/outputDelta",
                json!({"itemId": "tool-1", "delta": "mcp"}),
                "tool_update",
            ),
            (
                "item/collabToolCall/outputDelta",
                json!({"itemId": "tool-1", "delta": "collab"}),
                "tool_update",
            ),
            (
                "exec.command.output.delta",
                json!({"itemId": "tool-1", "delta": "stdout"}),
                "tool_update",
            ),
            (
                "item.command.execution.output.delta",
                json!({"itemId": "tool-1", "delta": "stdout"}),
                "tool_update",
            ),
            (
                "file.change.output.delta",
                json!({"itemId": "tool-1", "delta": "stdout"}),
                "tool_update",
            ),
            (
                "item.file.change.output.delta",
                json!({"itemId": "tool-1", "delta": "stdout"}),
                "tool_update",
            ),
            (
                "exec.command.terminal.interaction",
                json!({"itemId": "tool-1", "delta": "stdin"}),
                "tool_update",
            ),
            (
                "item.command.execution.terminal.interaction",
                json!({"itemId": "tool-1", "delta": "stdin"}),
                "tool_update",
            ),
            (
                "item/completed",
                json!({"item": {"id": "tool-1", "type": "commandExecution", "status": "completed", "aggregatedOutput": "done"}}),
                "tool_complete",
            ),
            (
                "thread/tokenUsage/updated",
                json!({
                    "tokenUsage": {
                        "last": {
                            "cachedInputTokens": 11,
                            "inputTokens": 30,
                            "outputTokens": 7,
                            "reasoningOutputTokens": 5,
                            "totalTokens": 37,
                        }
                    }
                }),
                "usage_updated",
            ),
            ("turn.completed", json!({}), "turn_completed"),
            ("turn/failed", json!({"message": "boom"}), "turn_failed"),
            ("error", json!({"error": {"message": "bad"}}), "turn_failed"),
        ]
    }

    fn ignored_cases() -> Vec<(&'static str, Value, &'static str)> {
        vec![
            ("thread.started", json!({"threadId": "thread-1"}), "ignored"),
            (
                "session.created",
                json!({"sessionId": "session-1"}),
                "ignored",
            ),
            ("token.count", json!({"outputTokens": 12}), "ignored"),
            ("plan.update", json!({"message": "updated"}), "ignored"),
            ("status", json!({"message": "still working"}), "ignored"),
            ("context.compacted", json!({"message": "done"}), "ignored"),
            (
                "context.compaction.started",
                json!({"message": "done"}),
                "ignored",
            ),
            (
                "context.compaction.completed",
                json!({"message": "done"}),
                "ignored",
            ),
            ("thread.compacted", json!({"message": "done"}), "ignored"),
            (
                "item.started",
                json!({"item": {"id": "x", "type": "context_compaction"}}),
                "ignored",
            ),
            ("account.changed", json!({"message": "done"}), "ignored"),
        ]
    }

    #[test]
    fn dispatcher_should_extract_usage_from_thread_token_usage_updates() {
        let mut dispatcher = CodexEventDispatcher::new(Arc::new(create_codex_tool_id_codec()));

        let normalized = dispatcher.normalize(&CodexNotification {
            method: "thread/tokenUsage/updated".to_owned(),
            params: json!({
                "threadId": "thread-1",
                "tokenUsage": {
                    "last": {
                        "cachedInputTokens": 10,
                        "inputTokens": 23,
                        "outputTokens": 9,
                        "reasoningOutputTokens": 4,
                        "totalTokens": 32
                    }
                }
            }),
        });

        let usage = match normalized {
            NormalizedNotification::UsageUpdated { usage } => usage,
            other => panic!("unexpected normalized event: {other:?}"),
        };

        assert_eq!(usage.input_tokens, Some(23));
        assert_eq!(usage.output_tokens, Some(9));
        assert_eq!(usage.total_tokens, Some(32));
        assert_eq!(
            usage.input_details.and_then(|details| details.cache_read),
            Some(10)
        );
        assert_eq!(
            usage.output_details.and_then(|details| details.reasoning),
            Some(4)
        );
    }
}
