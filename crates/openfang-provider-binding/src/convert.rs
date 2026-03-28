//! Conversion helpers between OpenFang runtime types and Arky provider types.

use std::collections::BTreeMap;

use arky_protocol::{
    ContentBlock as ArkyContentBlock, FinishReason, GenerateResponse, Message as ArkyMessage,
    ModelRef, ProviderRequest, ProviderSettings, Role as ArkyRole, SessionRef, StreamDelta,
    ToolCall as ArkyToolCall, ToolContent as ArkyToolContent, ToolContext as ArkyToolContext,
    ToolDefinition as ArkyToolDefinition, ToolResult as ArkyToolResult, TurnContext, Usage,
};
use arky_provider::{map_max_thinking_tokens_to_reasoning_effort, ProviderError, ProviderFamily};
use base64::{engine::general_purpose::STANDARD, Engine};
use openfang_runtime::llm_driver::{CompletionRequest, CompletionResponse, LlmError, StreamEvent};
use openfang_types::{
    message::{
        ContentBlock as OfContentBlock, Message as OfMessage, MessageContent, Role as OfRole,
        StopReason, TokenUsage,
    },
    tool::{ToolCall as OfToolCall, ToolDefinition as OfToolDefinition},
};
use serde_json::Value;
use thiserror::Error;

/// Failures while converting between OpenFang and Arky DTOs.
#[derive(Debug, Error)]
pub enum ConvertError {
    /// OpenFang image data is not valid base64.
    #[error("failed to decode base64 image data for `{media_type}`: {source}")]
    InvalidImageData {
        /// MIME type attached to the image block.
        media_type: String,
        /// Base64 decoding failure.
        #[source]
        source: base64::DecodeError,
    },
    /// The provider returned a serialized classified error instead of a usable completion.
    #[error("provider returned serialized error `{error_code}`: {message}")]
    GenerateResponseError {
        /// Stable classified error code.
        error_code: String,
        /// Human-readable provider error.
        message: String,
    },
}

/// Converts one OpenFang completion request into an Arky provider request.
pub fn completion_request_to_provider(
    request: &CompletionRequest,
    model_ref: &ModelRef,
    provider_family: &ProviderFamily,
    session: SessionRef,
    turn: TurnContext,
) -> Result<ProviderRequest, ConvertError> {
    let mut model = model_ref.clone();
    if !request.model.trim().is_empty() {
        model.model_id = request.model.clone();
    }

    let mut messages =
        Vec::with_capacity(request.messages.len() + usize::from(request.system.is_some()));
    if let Some(system) = request.system.as_ref().filter(|text| !text.is_empty()) {
        messages.push(ArkyMessage::system(system.clone()));
    }
    for message in &request.messages {
        messages.push(of_message_to_arky(message)?);
    }

    let tool_definitions = request
        .tools
        .iter()
        .map(of_tool_to_arky)
        .collect::<Vec<_>>();

    let mut settings = ProviderSettings::new();
    settings.max_tokens = Some(request.max_tokens);
    settings.temperature = Some(f64::from(request.temperature));
    settings.reasoning_effort = request.thinking.as_ref().and_then(|thinking| {
        map_max_thinking_tokens_to_reasoning_effort(provider_family, Some(thinking.budget_tokens))
    });

    Ok(ProviderRequest::new(session, turn, model, messages)
        .with_tools(ArkyToolContext::new().with_definitions(tool_definitions))
        .with_settings(settings))
}

/// Converts one OpenFang message into the Arky message DTO.
pub fn of_message_to_arky(message: &OfMessage) -> Result<ArkyMessage, ConvertError> {
    let role = match message.role {
        OfRole::System => ArkyRole::System,
        OfRole::User => ArkyRole::User,
        OfRole::Assistant => ArkyRole::Assistant,
    };

    let mut content = Vec::new();
    match &message.content {
        MessageContent::Text(text) => {
            content.push(ArkyContentBlock::text(text.clone()));
        }
        MessageContent::Blocks(blocks) => {
            for block in blocks {
                if let Some(converted) = of_block_to_arky(block)? {
                    content.push(converted);
                }
            }
        }
    }

    Ok(ArkyMessage::new(role, content))
}

fn of_block_to_arky(block: &OfContentBlock) -> Result<Option<ArkyContentBlock>, ConvertError> {
    match block {
        OfContentBlock::Text { text, .. } => Ok(Some(ArkyContentBlock::text(text.clone()))),
        OfContentBlock::Image { media_type, data } => Ok(Some(ArkyContentBlock::image(
            STANDARD
                .decode(data)
                .map_err(|source| ConvertError::InvalidImageData {
                    media_type: media_type.clone(),
                    source,
                })?,
            media_type.clone(),
        ))),
        OfContentBlock::ToolUse {
            id, name, input, ..
        } => Ok(Some(ArkyContentBlock::tool_use(ArkyToolCall::new(
            id.clone(),
            name.clone(),
            input.clone(),
        )))),
        OfContentBlock::ToolResult {
            tool_use_id,
            tool_name,
            content,
            is_error,
        } => {
            let result = if *is_error {
                ArkyToolResult::failure(
                    tool_use_id.clone(),
                    tool_name.clone(),
                    vec![ArkyToolContent::text(content.clone())],
                )
            } else {
                ArkyToolResult::success(
                    tool_use_id.clone(),
                    tool_name.clone(),
                    vec![ArkyToolContent::text(content.clone())],
                )
            };
            Ok(Some(ArkyContentBlock::tool_result(result)))
        }
        // Arky does not have a first-class reasoning block in `Message.content`.
        // Dropping these on the request path is the least-lossy truthful mapping:
        // they are response-side metadata, not prompt content the provider should see again.
        OfContentBlock::Thinking { .. } | OfContentBlock::Unknown => Ok(None),
    }
}

/// Converts one Arky message into the OpenFang message DTO.
#[must_use]
pub fn arky_message_to_of(message: &ArkyMessage) -> OfMessage {
    let role = match message.role {
        ArkyRole::System => OfRole::System,
        ArkyRole::User => OfRole::User,
        ArkyRole::Assistant => OfRole::Assistant,
        ArkyRole::Tool => OfRole::User,
    };

    let blocks = arky_blocks_to_of(&message.content);
    let content = blocks_to_message_content(blocks);

    OfMessage { role, content }
}

/// Converts Arky content blocks into OpenFang content blocks.
#[must_use]
pub fn arky_blocks_to_of(blocks: &[ArkyContentBlock]) -> Vec<OfContentBlock> {
    blocks.iter().map(arky_block_to_of).collect()
}

fn arky_block_to_of(block: &ArkyContentBlock) -> OfContentBlock {
    match block {
        ArkyContentBlock::Text { text } => OfContentBlock::Text {
            text: text.clone(),
            provider_metadata: None,
        },
        ArkyContentBlock::Image { data, media_type } => OfContentBlock::Image {
            media_type: media_type.clone(),
            data: STANDARD.encode(data),
        },
        ArkyContentBlock::ToolUse { call } => OfContentBlock::ToolUse {
            id: call.id.clone(),
            name: call.name.clone(),
            input: call.input.clone(),
            provider_metadata: None,
        },
        ArkyContentBlock::ToolResult { result } => OfContentBlock::ToolResult {
            tool_use_id: result.id.clone(),
            tool_name: result.name.clone(),
            content: flatten_tool_result_content(&result.content),
            is_error: result.is_error,
        },
    }
}

fn blocks_to_message_content(blocks: Vec<OfContentBlock>) -> MessageContent {
    match blocks.as_slice() {
        [OfContentBlock::Text {
            text,
            provider_metadata: None,
        }] => MessageContent::Text(text.clone()),
        _ => MessageContent::Blocks(blocks),
    }
}

/// Converts one OpenFang tool definition into Arky's tool definition snapshot.
#[must_use]
pub fn of_tool_to_arky(tool: &OfToolDefinition) -> ArkyToolDefinition {
    ArkyToolDefinition::new(
        tool.name.clone(),
        tool.description.clone(),
        tool.input_schema.clone(),
    )
}

/// Converts one aggregated Arky response into an OpenFang completion response.
pub fn generate_response_to_completion(
    response: GenerateResponse,
) -> Result<CompletionResponse, ConvertError> {
    if let Some(error) = response.error {
        return Err(ConvertError::GenerateResponseError {
            error_code: error.error_code,
            message: error.message,
        });
    }

    let content = arky_blocks_to_of(&response.message.content);
    let tool_calls = extract_tool_calls(&content);

    Ok(CompletionResponse {
        content,
        stop_reason: response
            .finish_reason
            .map(finish_reason_to_stop_reason)
            .unwrap_or_else(|| {
                if tool_calls.is_empty() {
                    StopReason::EndTurn
                } else {
                    StopReason::ToolUse
                }
            }),
        tool_calls,
        usage: usage_to_token_usage(response.usage.as_ref()),
        metadata: None,
    })
}

/// Converts one Arky provider error into the OpenFang runtime error type.
#[must_use]
pub fn provider_error_to_llm(error: ProviderError) -> LlmError {
    match error {
        ProviderError::AuthFailed { message } => LlmError::AuthenticationFailed(message),
        ProviderError::RateLimited {
            retry_after,
            message: _,
        } => LlmError::RateLimited {
            retry_after_ms: retry_after
                .and_then(|duration| u64::try_from(duration.as_millis()).ok())
                .unwrap_or(0),
        },
        ProviderError::ProtocolViolation { message, .. } => LlmError::Parse(message),
        ProviderError::ProcessCrashed {
            command,
            exit_code,
            stderr,
        } => {
            let detail =
                stderr
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| match exit_code {
                        Some(code) => format!("{command} exited with status {code}"),
                        None => format!("{command} exited unexpectedly"),
                    });
            LlmError::Http(format!("provider process crashed: {detail}"))
        }
        ProviderError::BinaryNotFound { binary } => {
            LlmError::MissingApiKey(format!("provider binary not found: {binary}"))
        }
        ProviderError::StreamInterrupted { message } => {
            LlmError::Http(format!("stream interrupted: {message}"))
        }
        ProviderError::NotFound { provider_id } => LlmError::ModelNotFound(provider_id.to_string()),
    }
}

/// Maps Arky finish reasons into the OpenFang stop-reason enum.
#[must_use]
pub const fn finish_reason_to_stop_reason(finish_reason: FinishReason) -> StopReason {
    match finish_reason {
        FinishReason::Stop
        | FinishReason::ContentFilter
        | FinishReason::Error
        | FinishReason::Unknown => StopReason::EndTurn,
        FinishReason::Length => StopReason::MaxTokens,
        FinishReason::ToolUse => StopReason::ToolUse,
    }
}

/// Converts Arky usage accounting into OpenFang token usage.
#[must_use]
pub fn usage_to_token_usage(usage: Option<&Usage>) -> TokenUsage {
    TokenUsage {
        input_tokens: usage
            .and_then(|value| value.input_tokens)
            .unwrap_or_default(),
        output_tokens: usage
            .and_then(|value| value.output_tokens)
            .unwrap_or_default(),
    }
}

/// Extracts OpenFang tool-call requests from converted content blocks.
#[must_use]
pub fn extract_tool_calls(blocks: &[OfContentBlock]) -> Vec<OfToolCall> {
    blocks
        .iter()
        .filter_map(|block| match block {
            OfContentBlock::ToolUse {
                id, name, input, ..
            } => Some(OfToolCall {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            }),
            _ => None,
        })
        .collect()
}

/// Flattens structured Arky tool-result fragments into OpenFang's string-only result field.
#[must_use]
pub fn flatten_tool_result_content(content: &[ArkyToolContent]) -> String {
    content
        .iter()
        .map(|item| match item {
            ArkyToolContent::Text { text } => text.clone(),
            ArkyToolContent::Json { value } => value.to_string(),
            ArkyToolContent::Image { data, media_type } => {
                format!("data:{media_type};base64,{}", STANDARD.encode(data))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Converts the subset of Arky events that map directly to OpenFang stream events.
#[must_use]
pub fn agent_event_to_stream_event(event: &arky_protocol::AgentEvent) -> Option<StreamEvent> {
    match event {
        arky_protocol::AgentEvent::MessageUpdate { delta, .. } => match delta {
            StreamDelta::Text { text } => Some(StreamEvent::TextDelta { text: text.clone() }),
            StreamDelta::ToolUseInput { delta, .. } => Some(StreamEvent::ToolInputDelta {
                text: delta.clone(),
            }),
            _ => None,
        },
        arky_protocol::AgentEvent::ReasoningDelta { delta, .. } => {
            Some(StreamEvent::ThinkingDelta {
                text: delta.clone(),
            })
        }
        arky_protocol::AgentEvent::ToolExecutionStart {
            tool_call_id,
            tool_name,
            ..
        } => Some(StreamEvent::ToolUseStart {
            id: tool_call_id.clone(),
            name: tool_name.clone(),
        }),
        arky_protocol::AgentEvent::ToolExecutionEnd {
            tool_call_id,
            tool_name,
            result,
            is_error,
            ..
        } => Some(StreamEvent::ToolExecutionResult {
            id: tool_call_id.clone(),
            name: tool_name.clone(),
            result_preview: result_preview(result),
            is_error: *is_error,
        }),
        _ => None,
    }
}

/// Parses the normalized finish-reason strings emitted in Arky `Custom` events.
#[must_use]
pub fn finish_reason_from_payload(payload: &Value) -> Option<FinishReason> {
    let value = payload
        .get("finish_reason")
        .or_else(|| payload.get("stop_reason"))
        .and_then(Value::as_str)?;

    Some(match value {
        "end_turn" | "stop" | "stop_sequence" => FinishReason::Stop,
        "max_tokens" | "length" => FinishReason::Length,
        "tool_use" | "tool_calls" => FinishReason::ToolUse,
        "content_filter" => FinishReason::ContentFilter,
        "error" => FinishReason::Error,
        _ => FinishReason::Unknown,
    })
}

/// Extracts serialized usage from one `Custom` payload when available.
#[must_use]
pub fn usage_from_payload(payload: &Value) -> Option<Usage> {
    payload
        .get("usage")
        .cloned()
        .and_then(|usage| serde_json::from_value(usage).ok())
}

/// Creates the OpenFang thinking blocks preserved from collected reasoning streams.
#[must_use]
pub fn thinking_blocks(
    reasoning: &BTreeMap<String, String>,
    reasoning_order: &[String],
) -> Vec<OfContentBlock> {
    reasoning_order
        .iter()
        .filter_map(|reasoning_id| reasoning.get(reasoning_id))
        .filter(|text| !text.is_empty())
        .map(|text| OfContentBlock::Thinking {
            thinking: text.clone(),
        })
        .collect()
}

fn result_preview(result: &Value) -> String {
    match result {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use arky_protocol::{
        ContentBlock as ArkyContentBlock, GenerateResponse, Message as ArkyMessage,
        ProviderId as ArkyProviderId, ToolCall as ArkyToolCall, ToolContent, ToolResult,
    };
    use arky_provider::ProviderError;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{
        agent_event_to_stream_event, arky_message_to_of, completion_request_to_provider,
        extract_tool_calls, finish_reason_from_payload, finish_reason_to_stop_reason,
        flatten_tool_result_content, generate_response_to_completion, of_message_to_arky,
        provider_error_to_llm, usage_from_payload, usage_to_token_usage, ConvertError,
    };
    use arky_protocol::{
        AgentEvent, EventMetadata, FinishReason, MessageMetadata, ModelRef, ReasoningEffort,
        SessionRef, StreamDelta, TurnContext, TurnId, Usage,
    };
    use arky_provider::ProviderFamily;
    use openfang_runtime::llm_driver::{CompletionRequest, LlmError, StreamEvent};
    use openfang_types::{
        config::ThinkingConfig,
        message::{ContentBlock, Message, MessageContent, Role, StopReason},
        tool::ToolDefinition,
    };

    #[test]
    fn completion_request_to_provider_should_prepend_system_and_map_reasoning() {
        let request = CompletionRequest {
            model: "gpt-5.2".to_owned(),
            messages: vec![Message::user("hello")],
            tools: vec![ToolDefinition {
                name: "read_file".to_owned(),
                description: "Read a file".to_owned(),
                input_schema: json!({ "type": "object" }),
            }],
            max_tokens: 321,
            temperature: 0.25,
            system: Some("follow instructions".to_owned()),
            thinking: Some(ThinkingConfig {
                budget_tokens: 70_000,
                stream_thinking: true,
            }),
            session: None,
        };

        let provider_request = completion_request_to_provider(
            &request,
            &ModelRef::new("gpt-5.2").with_provider_id(ArkyProviderId::new("codex")),
            &ProviderFamily::Codex,
            SessionRef::new(None),
            TurnContext::new(TurnId::new(), 1),
        )
        .expect("request should convert");

        assert_eq!(provider_request.messages.len(), 2);
        assert!(matches!(
            provider_request.messages.first(),
            Some(ArkyMessage {
                role: arky_protocol::Role::System,
                ..
            })
        ));
        assert_eq!(provider_request.settings.max_tokens, Some(321));
        assert_eq!(provider_request.settings.temperature, Some(0.25));
        assert_eq!(
            provider_request.settings.reasoning_effort,
            Some(ReasoningEffort::XHigh)
        );
        assert_eq!(provider_request.tools.definitions.len(), 1);
    }

    #[test]
    fn of_message_to_arky_should_decode_images_and_drop_thinking_blocks() {
        let message = Message::user_with_blocks(vec![
            ContentBlock::Text {
                text: "see image".to_owned(),
                provider_metadata: Some(json!({ "opaque": true })),
            },
            ContentBlock::Thinking {
                thinking: "hidden".to_owned(),
            },
            ContentBlock::Image {
                media_type: "image/png".to_owned(),
                data: STANDARD.encode([1_u8, 2, 3]),
            },
        ]);

        let converted = of_message_to_arky(&message).expect("message should convert");

        assert_eq!(converted.content.len(), 2);
        assert!(matches!(
            converted.content[0],
            ArkyContentBlock::Text { ref text } if text == "see image"
        ));
        assert!(matches!(
            converted.content[1],
            ArkyContentBlock::Image { ref data, ref media_type }
            if data == &vec![1, 2, 3] && media_type == "image/png"
        ));
    }

    #[test]
    fn of_message_to_arky_should_fail_for_invalid_base64_images() {
        let message = Message::user_with_blocks(vec![ContentBlock::Image {
            media_type: "image/png".to_owned(),
            data: "%%%".to_owned(),
        }]);

        let error = of_message_to_arky(&message).expect_err("invalid image should fail");

        assert!(matches!(error, ConvertError::InvalidImageData { .. }));
    }

    #[test]
    fn arky_message_to_of_should_map_tool_role_to_user_with_structured_blocks() {
        let message = ArkyMessage::new(
            arky_protocol::Role::Tool,
            vec![
                ArkyContentBlock::tool_result(ToolResult::success(
                    "call-1",
                    "read_file",
                    vec![ToolContent::text("Cargo.toml")],
                )),
                ArkyContentBlock::image([9_u8, 8, 7], "image/png"),
            ],
        )
        .with_metadata(MessageMetadata::new().with_id("ignored"));

        let converted = arky_message_to_of(&message);

        assert_eq!(converted.role, Role::User);
        match converted.content {
            MessageContent::Blocks(blocks) => {
                assert!(matches!(
                    blocks[0],
                    ContentBlock::ToolResult {
                        ref tool_use_id,
                        ref tool_name,
                        ref content,
                        is_error: false
                    } if tool_use_id == "call-1" && tool_name == "read_file" && content == "Cargo.toml"
                ));
                assert!(matches!(
                    blocks[1],
                    ContentBlock::Image { ref media_type, ref data }
                    if media_type == "image/png" && data == &STANDARD.encode([9_u8, 8, 7])
                ));
            }
            other => panic!("expected blocks, got {other:?}"),
        }
    }

    #[test]
    fn flatten_tool_result_content_should_join_text_json_and_images() {
        let flattened = flatten_tool_result_content(&[
            ToolContent::text("hello"),
            ToolContent::json(json!({ "ok": true })),
            ToolContent::image([1_u8, 2, 3], "image/png"),
        ]);

        assert_eq!(
            flattened,
            format!(
                "hello\n{{\"ok\":true}}\ndata:image/png;base64,{}",
                STANDARD.encode([1_u8, 2, 3])
            )
        );
    }

    #[test]
    fn generate_response_to_completion_should_extract_tool_calls_and_usage() {
        let response = GenerateResponse::new(
            SessionRef::new(None),
            TurnContext::new(TurnId::new(), 1),
            ArkyMessage::new(
                arky_protocol::Role::Assistant,
                vec![ArkyContentBlock::tool_use(ArkyToolCall::new(
                    "call-1",
                    "read_file",
                    json!({ "path": "Cargo.toml" }),
                ))],
            ),
        )
        .with_finish_reason(FinishReason::ToolUse)
        .with_usage(Usage {
            input_tokens: Some(11),
            output_tokens: Some(7),
            total_tokens: Some(18),
            input_details: None,
            output_details: None,
            cost_usd: None,
            duration_ms: None,
        });

        let completion =
            generate_response_to_completion(response).expect("response should convert");

        assert_eq!(completion.stop_reason, StopReason::ToolUse);
        assert_eq!(completion.tool_calls.len(), 1);
        assert_eq!(completion.usage.input_tokens, 11);
        assert_eq!(completion.usage.output_tokens, 7);
    }

    #[test]
    fn generate_response_to_completion_should_fail_for_serialized_error_payloads() {
        let provider_error = ProviderError::protocol_violation("boom", None);
        let response = GenerateResponse::new(
            SessionRef::new(None),
            TurnContext::new(TurnId::new(), 1),
            ArkyMessage::assistant("partial"),
        )
        .with_error(&provider_error);

        let error = generate_response_to_completion(response)
            .expect_err("serialized provider errors should fail conversion");

        assert!(matches!(
            error,
            ConvertError::GenerateResponseError { error_code, message }
            if error_code == "PROVIDER_PROTOCOL_VIOLATION" && message == "boom"
        ));
    }

    #[test]
    fn provider_error_to_llm_should_map_variants() {
        let rate_limited = provider_error_to_llm(ProviderError::rate_limited(
            "slow down",
            Some(std::time::Duration::from_millis(250)),
        ));
        let process_crashed = provider_error_to_llm(ProviderError::process_crashed(
            "codex",
            Some(9),
            Some("killed".to_owned()),
        ));
        let not_found =
            provider_error_to_llm(ProviderError::not_found(ArkyProviderId::new("codex")));

        assert!(matches!(
            rate_limited,
            LlmError::RateLimited {
                retry_after_ms: 250
            }
        ));
        assert!(matches!(
            process_crashed,
            LlmError::Http(ref message) if message.contains("provider process crashed: killed")
        ));
        assert!(matches!(
            not_found,
            LlmError::ModelNotFound(ref model) if model == "codex"
        ));
    }

    #[test]
    fn helper_mappings_should_match_runtime_contract() {
        let stream_event = agent_event_to_stream_event(&AgentEvent::MessageUpdate {
            meta: EventMetadata::new(1, 1),
            message: ArkyMessage::assistant("hello"),
            delta: StreamDelta::text("hello"),
        });
        let finish_reason = finish_reason_from_payload(&json!({
            "finish_reason": "tool_use",
            "usage": { "input_tokens": 5, "output_tokens": 3 }
        }));
        let usage = usage_from_payload(&json!({
            "usage": { "input_tokens": 5, "output_tokens": 3 }
        }));
        let token_usage = usage_to_token_usage(usage.as_ref());

        assert!(matches!(
            stream_event,
            Some(StreamEvent::TextDelta { ref text }) if text == "hello"
        ));
        assert_eq!(finish_reason, Some(FinishReason::ToolUse));
        assert_eq!(
            finish_reason_to_stop_reason(FinishReason::Length),
            StopReason::MaxTokens
        );
        assert_eq!(token_usage.input_tokens, 5);
        assert_eq!(token_usage.output_tokens, 3);
    }

    #[test]
    fn extract_tool_calls_should_ignore_non_tool_blocks() {
        let calls = extract_tool_calls(&[
            ContentBlock::Text {
                text: "hello".to_owned(),
                provider_metadata: None,
            },
            ContentBlock::ToolUse {
                id: "call-1".to_owned(),
                name: "read_file".to_owned(),
                input: json!({ "path": "Cargo.toml" }),
                provider_metadata: None,
            },
        ]);

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
    }
}
