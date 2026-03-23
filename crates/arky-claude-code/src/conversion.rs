//! Claude-specific request/response conversion helpers.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use arky_protocol::{ContentBlock, FinishReason, Message, ProviderSettings, Role};
use arky_provider::ProviderError;
use serde_json::{Value, json};
use tokio::sync::{Notify, mpsc};

const MAX_PROMPT_WARNING_LENGTH: usize = 100_000;
const DEFAULT_INJECTION_BUFFER: usize = 16;
const SUPPORTED_IMAGE_MEDIA_TYPES: [&str; 4] =
    ["image/jpeg", "image/png", "image/gif", "image/webp"];

/// Callback notified when an injected prompt message is or is not delivered.
#[derive(Clone)]
pub struct ClaudeMessageDeliveryCallback(Arc<dyn Fn(bool) + Send + Sync>);

impl ClaudeMessageDeliveryCallback {
    /// Creates a callback wrapper from any `Fn(bool)` closure.
    #[must_use]
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(bool) + Send + Sync + 'static,
    {
        Self(Arc::new(callback))
    }

    fn call(&self, delivered: bool) {
        (self.0)(delivered);
    }
}

impl std::fmt::Debug for ClaudeMessageDeliveryCallback {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ClaudeMessageDeliveryCallback(..)")
    }
}

impl PartialEq for ClaudeMessageDeliveryCallback {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ClaudeMessageDeliveryCallback {}

#[derive(Debug)]
struct QueuedInjection {
    content: String,
    on_result: Option<ClaudeMessageDeliveryCallback>,
}

/// Handle used to inject follow-up user messages into an active prompt stream.
#[derive(Debug, Clone)]
pub struct ClaudeMessageInjector {
    sender: mpsc::Sender<QueuedInjection>,
    closed: Arc<AtomicBool>,
    wake: Arc<Notify>,
}

impl ClaudeMessageInjector {
    /// Queues a follow-up user message.
    pub fn inject(
        &self,
        content: impl Into<String>,
        on_result: Option<ClaudeMessageDeliveryCallback>,
    ) {
        if self.closed.load(Ordering::Acquire) {
            if let Some(callback) = on_result {
                callback.call(false);
            }
            return;
        }

        match self.sender.try_send(QueuedInjection {
            content: content.into(),
            on_result,
        }) {
            Ok(()) => self.wake.notify_waiters(),
            Err(
                mpsc::error::TrySendError::Full(queued) | mpsc::error::TrySendError::Closed(queued),
            ) => {
                if let Some(callback) = queued.on_result {
                    callback.call(false);
                }
            }
        }
    }

    /// Closes the injector and unblocks any pending stream waiters.
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.wake.notify_waiters();
    }
}

/// Minimal async-iterator-style prompt stream used for injected user messages.
#[derive(Debug)]
pub struct ClaudeInjectedPromptStream {
    initial_message: Option<Value>,
    session_ended: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    wake: Arc<Notify>,
    receiver: mpsc::Receiver<QueuedInjection>,
}

impl ClaudeInjectedPromptStream {
    /// Creates a prompt stream and injector pair.
    #[must_use]
    pub fn new(
        prompt: impl Into<String>,
        session_id: Option<String>,
        content_parts: Option<Vec<Value>>,
    ) -> (Self, ClaudeMessageInjector) {
        let (sender, receiver) = mpsc::channel(DEFAULT_INJECTION_BUFFER);
        let closed = Arc::new(AtomicBool::new(false));
        let session_ended = Arc::new(AtomicBool::new(false));
        let wake = Arc::new(Notify::new());
        let initial_message = json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": content_parts.unwrap_or_else(|| {
                    vec![json!({
                        "type": "text",
                        "text": prompt.into(),
                    })]
                }),
            },
            "parent_tool_use_id": Value::Null,
            "session_id": session_id.unwrap_or_default(),
        });

        (
            Self {
                initial_message: Some(initial_message),
                session_ended,
                closed: Arc::clone(&closed),
                wake: Arc::clone(&wake),
                receiver,
            },
            ClaudeMessageInjector {
                sender,
                closed,
                wake,
            },
        )
    }

    /// Notifies the stream that the active Claude session has ended.
    pub fn notify_session_ended(&self) {
        self.session_ended.store(true, Ordering::Release);
        self.closed.store(true, Ordering::Release);
        self.wake.notify_waiters();
    }

    /// Returns the next prompt message to send to Claude.
    pub async fn next_message(&mut self) -> Option<Value> {
        if let Some(initial_message) = self.initial_message.take() {
            return Some(initial_message);
        }

        loop {
            if self.closed.load(Ordering::Acquire) && self.receiver.is_empty() {
                return None;
            }

            tokio::select! {
                message = self.receiver.recv() => {
                    let message = message?;
                    let payload = json!({
                        "type": "user",
                        "message": {
                            "role": "user",
                            "content": [{
                                "type": "text",
                                "text": message.content,
                            }],
                        },
                        "parent_tool_use_id": Value::Null,
                    });
                    if let Some(callback) = message.on_result {
                        callback.call(true);
                    }
                    return Some(payload);
                }
                () = self.wake.notified() => {
                    if self.session_ended.load(Ordering::Acquire)
                        || (self.closed.load(Ordering::Acquire) && self.receiver.is_empty())
                    {
                        return None;
                    }
                }
            }
        }
    }
}

/// Result of converting Arky messages into Claude-friendly prompt structures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeMessageConversion {
    /// Flattened prompt text for Claude's print-oriented CLI mode.
    pub messages_prompt: String,
    /// Optional system prompt extracted from system-role messages.
    pub system_prompt: Option<String>,
    /// Non-fatal conversion warnings.
    pub warnings: Vec<String>,
    /// User-content parts that can be forwarded to Claude's streaming SDK.
    pub streaming_content_parts: Vec<Value>,
    /// Whether any image content was encountered.
    pub has_image_parts: bool,
}

/// Converts normalized Arky messages into Claude prompt structures.
pub fn convert_messages(messages: &[Message]) -> ClaudeMessageConversion {
    let mut prompt_sections = Vec::new();
    let mut system_sections = Vec::new();
    let mut warnings = Vec::new();
    let mut streaming_content_parts = Vec::new();
    let mut has_image_parts = false;

    for message in messages {
        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Tool => "tool",
        };
        let mut text_parts = Vec::new();

        for block in &message.content {
            match block {
                ContentBlock::Text { text } => {
                    text_parts.push(text.clone());
                    if message.role == Role::User {
                        streaming_content_parts.push(json!({ "type": "text", "text": text }));
                    }
                }
                ContentBlock::Image { data, media_type } => {
                    has_image_parts = true;
                    if message.role == Role::User {
                        if let Some(image_part) = image_part_from_bytes(data, media_type) {
                            streaming_content_parts.push(image_part);
                        } else {
                            warnings.push(format!(
                                "unsupported image media type `{media_type}` for Claude Code"
                            ));
                        }
                    } else {
                        warnings.push(format!(
                            "image content from `{role}` messages is not streamed to Claude"
                        ));
                    }
                }
                ContentBlock::ToolUse { call } => {
                    text_parts.push(format!("ToolUse {} {} {}", call.id, call.name, call.input));
                }
                ContentBlock::ToolResult { result } => {
                    text_parts.push(format!(
                        "ToolResult {} {} {}",
                        result.id,
                        result.name,
                        stringify_json(&json!(result.content))
                    ));
                }
            }
        }

        if text_parts.is_empty() {
            continue;
        }

        let joined = text_parts.join("\n");
        if message.role == Role::System {
            system_sections.push(joined);
        } else {
            prompt_sections.push(format!("{role}: {joined}"));
        }
    }

    ClaudeMessageConversion {
        messages_prompt: prompt_sections.join("\n\n"),
        system_prompt: (!system_sections.is_empty()).then(|| system_sections.join("\n\n")),
        warnings,
        streaming_content_parts,
        has_image_parts,
    }
}

/// Builds a Claude image content part from raw bytes when the MIME type is supported.
#[must_use]
pub fn image_part_from_bytes(data: &[u8], media_type: &str) -> Option<Value> {
    let media_type = media_type.trim().to_ascii_lowercase();
    if !SUPPORTED_IMAGE_MEDIA_TYPES.contains(&media_type.as_str()) {
        return None;
    }

    Some(json!({
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": media_type,
            "data": encode_base64(data),
        },
    }))
}

/// Builds a Claude image content part from an explicit base64 payload.
#[must_use]
pub fn image_part_from_base64(media_type: &str, data: &str) -> Option<Value> {
    let media_type = media_type.trim().to_ascii_lowercase();
    if !SUPPORTED_IMAGE_MEDIA_TYPES.contains(&media_type.as_str()) {
        return None;
    }

    let normalized = data.split_whitespace().collect::<String>();
    if normalized.is_empty() {
        return None;
    }

    Some(json!({
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": media_type,
            "data": normalized,
        },
    }))
}

/// Parses a `data:<mime>;base64,<payload>` image URL into a Claude content part.
#[must_use]
pub fn image_part_from_data_url(data_url: &str) -> Option<Value> {
    let prefix = "data:";
    let suffix = ";base64,";
    let payload = data_url.trim();
    if !payload.starts_with(prefix) {
        return None;
    }
    let (header, encoded) = payload.split_once(suffix)?;
    let media_type = header.strip_prefix(prefix)?;
    image_part_from_base64(media_type, encoded)
}

/// Maps Claude stop reasons onto the shared protocol finish-reason enum.
pub fn map_finish_reason(stop_reason: Option<&str>) -> FinishReason {
    match stop_reason {
        Some("end_turn" | "stop" | "stop_sequence") => FinishReason::Stop,
        Some("max_tokens" | "length") => FinishReason::Length,
        Some("tool_use" | "tool_calls") => FinishReason::ToolUse,
        Some("content_filter") => FinishReason::ContentFilter,
        Some("error") => FinishReason::Error,
        Some(_) | None => FinishReason::Unknown,
    }
}

/// Maps Claude permission mode aliases to the CLI's expected values.
pub fn map_permission_mode(permission_mode: &str) -> String {
    if permission_mode == "delegate" {
        return "dontAsk".to_owned();
    }

    permission_mode.to_owned()
}

/// Returns warnings for request settings that Claude Code ignores.
pub fn collect_warning_messages(settings: &ProviderSettings) -> Vec<String> {
    let mut warnings = Vec::new();

    if settings.temperature.is_some() {
        warnings.push("temperature is unsupported by Claude Code and will be ignored".to_owned());
    }
    if settings.extra.contains_key("topP") || settings.extra.contains_key("top_p") {
        warnings.push("topP is unsupported by Claude Code and will be ignored".to_owned());
    }
    if settings.extra.contains_key("topK") || settings.extra.contains_key("top_k") {
        warnings.push("topK is unsupported by Claude Code and will be ignored".to_owned());
    }
    if !settings.stop_sequences.is_empty() {
        warnings
            .push("stop sequences are unsupported by Claude Code and will be ignored".to_owned());
    }

    warnings
}

/// Returns richer Claude runtime warnings, including model and prompt checks.
#[must_use]
pub fn collect_runtime_warning_messages(
    settings: &ProviderSettings,
    model_id: Option<&str>,
    prompt_text: Option<&str>,
    session_id: Option<&str>,
) -> Vec<String> {
    let mut warnings = collect_warning_messages(settings);

    if let Some(model_id) = model_id.map(str::trim)
        && (model_id.is_empty() || model_id.chars().any(char::is_whitespace))
    {
        warnings.push(format!(
            "model id `{model_id}` looks invalid for Claude Code and may be rejected"
        ));
    }

    if let Some(prompt_text) = prompt_text
        && prompt_text.len() > MAX_PROMPT_WARNING_LENGTH
    {
        warnings.push(format!(
            "Very long prompt detected ({} characters). Claude Code performance may degrade.",
            prompt_text.len()
        ));
    }

    if let Some(session_id) = session_id.map(str::trim)
        && !session_id.is_empty()
        && !session_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | ':')
        })
    {
        warnings.push(format!(
            "session id `{session_id}` contains characters outside [A-Za-z0-9-_:]"
        ));
    }

    warnings
}

/// Builds CLI flags for Claude structured-output mode.
pub fn structured_output_args(schema: &Value) -> Result<Vec<String>, ProviderError> {
    let encoded = serde_json::to_string(schema).map_err(|error| {
        ProviderError::protocol_violation(
            "failed to serialize Claude structured output schema",
            Some(json!({
                "reason": error.to_string(),
            })),
        )
    })?;

    Ok(vec![
        "--output-format".to_owned(),
        "json_schema".to_owned(),
        "--schema".to_owned(),
        encoded,
    ])
}

/// Extracts structured output from provider metadata payloads when present.
pub fn extract_structured_output(payload: &Value) -> Option<Value> {
    payload
        .get("structured_output")
        .cloned()
        .or_else(|| payload.get("structuredOutput").cloned())
        .or_else(|| {
            payload
                .get("result")
                .and_then(|result| result.get("structured_output"))
                .cloned()
        })
}

fn stringify_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[unserializable]".to_owned())
}

fn encode_base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(data.len().div_ceil(3) * 4);

    let mut index = 0usize;
    while index + 3 <= data.len() {
        let chunk = &data[index..index + 3];
        let combined =
            (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]);
        encoded.push(TABLE[((combined >> 18) & 0x3f) as usize] as char);
        encoded.push(TABLE[((combined >> 12) & 0x3f) as usize] as char);
        encoded.push(TABLE[((combined >> 6) & 0x3f) as usize] as char);
        encoded.push(TABLE[(combined & 0x3f) as usize] as char);
        index += 3;
    }

    let remainder = data.len().saturating_sub(index);
    if remainder == 1 {
        let combined = u32::from(data[index]) << 16;
        encoded.push(TABLE[((combined >> 18) & 0x3f) as usize] as char);
        encoded.push(TABLE[((combined >> 12) & 0x3f) as usize] as char);
        encoded.push('=');
        encoded.push('=');
    } else if remainder == 2 {
        let combined = (u32::from(data[index]) << 16) | (u32::from(data[index + 1]) << 8);
        encoded.push(TABLE[((combined >> 18) & 0x3f) as usize] as char);
        encoded.push(TABLE[((combined >> 12) & 0x3f) as usize] as char);
        encoded.push(TABLE[((combined >> 6) & 0x3f) as usize] as char);
        encoded.push('=');
    }

    encoded
}

/// Parses an image string into Claude's expected base64 image source payload.
#[must_use]
pub fn parse_image_string(value: &str, fallback_media_type: Option<&str>) -> Option<Value> {
    let trimmed = value.trim();
    if let Some(data_url) = trimmed.strip_prefix("data:") {
        let (media_type, payload) = data_url.split_once(";base64,")?;
        if media_type.trim().is_empty() || payload.trim().is_empty() {
            return None;
        }

        return Some(base64_image_source_payload(
            media_type.trim(),
            payload.trim(),
        ));
    }

    fallback_media_type
        .filter(|media_type| !media_type.trim().is_empty() && !trimmed.is_empty())
        .map(|media_type| base64_image_source_payload(media_type.trim(), trimmed))
}

/// Converts raw bytes into Claude's base64 image-source payload.
#[must_use]
pub fn image_source_payload(media_type: impl Into<String>, data: impl AsRef<[u8]>) -> Value {
    base64_image_source_payload(media_type, encode_base64(data.as_ref()))
}

fn base64_image_source_payload(media_type: impl Into<String>, data: impl Into<String>) -> Value {
    json!({
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": media_type.into(),
            "data": data.into(),
        },
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{
        ClaudeInjectedPromptStream, ClaudeMessageDeliveryCallback,
        collect_runtime_warning_messages, collect_warning_messages, convert_messages,
        extract_structured_output, image_part_from_base64, image_part_from_bytes,
        image_part_from_data_url, image_source_payload, map_finish_reason, map_permission_mode,
        parse_image_string, structured_output_args,
    };
    use arky_protocol::{ContentBlock, FinishReason, Message, ProviderSettings};

    #[test]
    fn convert_messages_should_include_text_and_images() {
        let conversion = convert_messages(&[
            Message::system("system"),
            Message::new(
                arky_protocol::Role::User,
                vec![
                    ContentBlock::text("hello"),
                    ContentBlock::image(vec![1, 2, 3], "image/png"),
                ],
            ),
        ]);

        assert_eq!(conversion.system_prompt.as_deref(), Some("system"));
        assert_eq!(conversion.has_image_parts, true);
        assert_eq!(conversion.streaming_content_parts.len(), 2);
        assert_eq!(conversion.messages_prompt.contains("user: hello"), true);
    }

    #[test]
    fn map_permission_mode_should_remap_delegate() {
        assert_eq!(map_permission_mode("delegate"), "dontAsk");
        assert_eq!(map_permission_mode("acceptEdits"), "acceptEdits");
    }

    #[test]
    fn map_finish_reason_should_map_known_values() {
        assert_eq!(map_finish_reason(Some("end_turn")), FinishReason::Stop);
        assert_eq!(map_finish_reason(Some("tool_use")), FinishReason::ToolUse);
    }

    #[test]
    fn collect_warning_messages_should_report_unsupported_settings() {
        let mut settings = ProviderSettings::new();
        settings.temperature = Some(0.3);
        settings.stop_sequences = vec!["STOP".to_owned()];
        settings.extra.insert("topP".to_owned(), json!(0.9));

        let warnings = collect_warning_messages(&settings);

        assert_eq!(warnings.len(), 3);
        assert_eq!(
            warnings
                .iter()
                .any(|warning| warning.contains("temperature")),
            true
        );
    }

    #[test]
    fn structured_output_helpers_should_encode_and_extract_payloads() {
        let args = structured_output_args(&json!({
            "type": "object",
            "properties": {
                "answer": { "type": "string" }
            },
        }))
        .expect("schema should encode");
        let extracted = extract_structured_output(&json!({
            "result": {
                "structured_output": { "answer": "ok" }
            }
        }));

        assert_eq!(args[1], "json_schema");
        assert_eq!(extracted, Some(json!({ "answer": "ok" })));
    }

    #[test]
    fn collect_runtime_warning_messages_should_include_model_prompt_and_session_checks() {
        let warnings = collect_runtime_warning_messages(
            &ProviderSettings::default(),
            Some("bad model"),
            Some(&"x".repeat(100_001)),
            Some("session/1"),
        );

        assert_eq!(
            warnings
                .iter()
                .any(|warning| warning.contains("model id `bad model`")),
            true
        );
        assert_eq!(
            warnings
                .iter()
                .any(|warning| warning.contains("Very long prompt detected")),
            true
        );
        assert_eq!(
            warnings
                .iter()
                .any(|warning| warning.contains("session id `session/1`")),
            true
        );
    }

    #[test]
    fn image_helpers_should_encode_bytes_and_parse_data_urls() {
        let from_bytes = image_source_payload("image/png", [1_u8, 2, 3]);
        let from_data_url = parse_image_string("data:image/png;base64,AQID", Some("image/png"))
            .expect("data url should parse");

        assert_eq!(from_bytes["source"]["data"], json!("AQID"));
        assert_eq!(from_data_url["source"]["media_type"], json!("image/png"));
        assert_eq!(from_data_url["source"]["data"], json!("AQID"));
    }

    #[test]
    fn image_part_helpers_should_support_base64_data_urls_and_binary_payloads() {
        let from_base64 = image_part_from_base64("image/png", "AQID").expect("base64 should work");
        let from_data_url =
            image_part_from_data_url("data:image/jpeg;base64,BAUG").expect("data url should work");
        let from_bytes =
            image_part_from_bytes(&[7_u8, 8, 9], "image/gif").expect("bytes should work");

        assert_eq!(from_base64["source"]["data"], json!("AQID"));
        assert_eq!(from_data_url["source"]["media_type"], json!("image/jpeg"));
        assert_eq!(from_bytes["source"]["data"], json!("BwgJ"));
    }

    #[tokio::test]
    async fn injected_prompt_stream_should_yield_initial_prompt_and_injections() {
        let deliveries = Arc::new(Mutex::new(Vec::new()));
        let (mut stream, injector) = ClaudeInjectedPromptStream::new(
            "hello",
            Some("session-live".to_owned()),
            Some(vec![json!({
                "type": "text",
                "text": "from-parts",
            })]),
        );

        let first = stream
            .next_message()
            .await
            .expect("initial message should exist");
        injector.inject(
            "follow-up",
            Some(ClaudeMessageDeliveryCallback::new({
                let deliveries = Arc::clone(&deliveries);
                move |delivered| {
                    deliveries
                        .lock()
                        .expect("deliveries mutex should lock")
                        .push(delivered);
                }
            })),
        );
        let second = stream
            .next_message()
            .await
            .expect("injected message should exist");
        injector.close();
        let final_message = stream.next_message().await;

        assert_eq!(first["message"]["content"][0]["text"], json!("from-parts"));
        assert_eq!(second["message"]["content"][0]["text"], json!("follow-up"));
        assert_eq!(final_message, None);
        assert_eq!(
            deliveries
                .lock()
                .expect("deliveries mutex should lock")
                .as_slice(),
            &[true]
        );
    }
}
