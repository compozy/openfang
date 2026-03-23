//! Shared message and content-block types.

use serde::{Deserialize, Serialize};

use crate::{ProviderId, SessionId, ToolCall, ToolResult, TurnId};

/// The role associated with a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// A user-authored message.
    User,
    /// An assistant-authored message.
    Assistant,
    /// A system-level control message.
    System,
    /// A tool-authored message.
    Tool,
}

/// Additional metadata attached to a message.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MessageMetadata {
    /// Stable message identifier, when one exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Provider-native message identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_message_id: Option<String>,
    /// Session that owns the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    /// Turn that produced the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<TurnId>,
    /// Provider that emitted the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<ProviderId>,
    /// Millisecond timestamp associated with the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<u64>,
}

impl MessageMetadata {
    /// Creates empty message metadata.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores a stable message identifier.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Stores a provider-native message identifier.
    #[must_use]
    pub fn with_provider_message_id(mut self, provider_message_id: impl Into<String>) -> Self {
        self.provider_message_id = Some(provider_message_id.into());
        self
    }

    /// Stores the session identifier.
    #[must_use]
    pub const fn with_session_id(mut self, session_id: SessionId) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Stores the turn identifier.
    #[must_use]
    pub const fn with_turn_id(mut self, turn_id: TurnId) -> Self {
        self.turn_id = Some(turn_id);
        self
    }

    /// Stores the provider identifier.
    #[must_use]
    pub fn with_provider_id(mut self, provider_id: ProviderId) -> Self {
        self.provider_id = Some(provider_id);
        self
    }

    /// Stores the message timestamp.
    #[must_use]
    pub const fn with_timestamp_ms(mut self, timestamp_ms: u64) -> Self {
        self.timestamp_ms = Some(timestamp_ms);
        self
    }
}

/// A single content fragment inside a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text content.
    Text {
        /// The message text.
        text: String,
    },
    /// A tool invocation emitted as message content.
    ToolUse {
        /// Flattened tool call metadata.
        #[serde(flatten)]
        call: ToolCall,
    },
    /// A tool result emitted as message content.
    ToolResult {
        /// Flattened tool result metadata.
        #[serde(flatten)]
        result: ToolResult,
    },
    /// Binary image content.
    Image {
        /// Raw image bytes.
        data: Vec<u8>,
        /// MIME type describing the image encoding.
        media_type: String,
    },
}

impl ContentBlock {
    /// Creates a text block.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// Creates a tool-use block.
    #[must_use]
    pub const fn tool_use(call: ToolCall) -> Self {
        Self::ToolUse { call }
    }

    /// Creates a tool-result block.
    #[must_use]
    pub const fn tool_result(result: ToolResult) -> Self {
        Self::ToolResult { result }
    }

    /// Creates an image block.
    #[must_use]
    pub fn image(data: impl Into<Vec<u8>>, media_type: impl Into<String>) -> Self {
        Self::Image {
            data: data.into(),
            media_type: media_type.into(),
        }
    }
}

/// A normalized conversation message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// The message role.
    pub role: Role,
    /// Ordered content fragments.
    pub content: Vec<ContentBlock>,
    /// Optional message metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MessageMetadata>,
}

impl Message {
    /// Creates a message with explicit content.
    #[must_use]
    pub const fn new(role: Role, content: Vec<ContentBlock>) -> Self {
        Self {
            role,
            content,
            metadata: None,
        }
    }

    /// Starts a builder for the provided role.
    #[must_use]
    pub const fn builder(role: Role) -> MessageBuilder {
        MessageBuilder::new(role)
    }

    /// Creates a user message containing a single text block.
    #[must_use]
    pub fn user(text: impl Into<String>) -> Self {
        Self::builder(Role::User).text(text).build()
    }

    /// Creates an assistant message containing a single text block.
    #[must_use]
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::builder(Role::Assistant).text(text).build()
    }

    /// Creates a system message containing a single text block.
    #[must_use]
    pub fn system(text: impl Into<String>) -> Self {
        Self::builder(Role::System).text(text).build()
    }

    /// Creates a tool message containing a single tool-result block.
    #[must_use]
    pub fn tool(result: ToolResult) -> Self {
        Self::builder(Role::Tool)
            .block(ContentBlock::tool_result(result))
            .build()
    }

    /// Stores metadata on the message.
    #[must_use]
    pub fn with_metadata(mut self, metadata: MessageMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Fluent builder used to assemble [`Message`] values.
#[derive(Debug, Clone)]
pub struct MessageBuilder {
    role: Role,
    content: Vec<ContentBlock>,
    metadata: Option<MessageMetadata>,
}

impl MessageBuilder {
    /// Creates a builder for the provided role.
    #[must_use]
    pub const fn new(role: Role) -> Self {
        Self {
            role,
            content: Vec::new(),
            metadata: None,
        }
    }

    /// Appends a text block.
    #[must_use]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.content.push(ContentBlock::text(text));
        self
    }

    /// Appends an arbitrary content block.
    #[must_use]
    pub fn block(mut self, block: ContentBlock) -> Self {
        self.content.push(block);
        self
    }

    /// Stores message metadata.
    #[must_use]
    pub fn metadata(mut self, metadata: MessageMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Finalizes the message.
    #[must_use]
    pub fn build(self) -> Message {
        Message {
            role: self.role,
            content: self.content,
            metadata: self.metadata,
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{ContentBlock, Message, MessageMetadata, Role};
    use crate::{ProviderId, SessionId, ToolCall, ToolContent, ToolResult, TurnId};

    #[test]
    fn content_block_constructors_should_build_each_variant() {
        let tool_call = ToolCall::new("call-1", "read_file", json!({ "path": "Cargo.toml" }));
        let tool_result = ToolResult::success(
            "call-1",
            "read_file",
            vec![ToolContent::text("workspace manifest")],
        );

        let actual = vec![
            ContentBlock::text("hello"),
            ContentBlock::tool_use(tool_call.clone()),
            ContentBlock::tool_result(tool_result.clone()),
            ContentBlock::image([1, 2, 3], "image/png"),
        ];

        let expected = vec![
            ContentBlock::Text {
                text: "hello".to_owned(),
            },
            ContentBlock::ToolUse { call: tool_call },
            ContentBlock::ToolResult {
                result: tool_result,
            },
            ContentBlock::Image {
                data: vec![1, 2, 3],
                media_type: "image/png".to_owned(),
            },
        ];

        assert_eq!(actual, expected);
    }

    #[test]
    fn message_builder_should_assemble_messages_with_metadata() {
        let session_id = SessionId::new();
        let turn_id = TurnId::new();
        let metadata = MessageMetadata::new()
            .with_id("message-1")
            .with_provider_message_id("provider-message-1")
            .with_session_id(session_id)
            .with_turn_id(turn_id)
            .with_provider_id(ProviderId::new("claude-code"))
            .with_timestamp_ms(1_717_171_717);

        let actual = Message::builder(Role::Assistant)
            .text("first chunk")
            .block(ContentBlock::text("second chunk"))
            .metadata(metadata.clone())
            .build();

        let expected = Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::text("first chunk"),
                ContentBlock::text("second chunk"),
            ],
            metadata: Some(metadata),
        };

        assert_eq!(actual, expected);
    }

    #[test]
    fn message_should_support_serde_round_trip() {
        let message = Message::user("inspect the repository")
            .with_metadata(MessageMetadata::new().with_session_id(SessionId::new()));
        let encoded = serde_json::to_string(&message).expect("message should serialize");
        let decoded: Message = serde_json::from_str(&encoded).expect("message should deserialize");

        assert_eq!(decoded, message);
    }
}
