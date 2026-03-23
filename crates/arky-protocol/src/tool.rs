//! Shared tool call and result types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A normalized tool invocation request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Stable tool call identifier.
    pub id: String,
    /// Canonical tool name.
    pub name: String,
    /// JSON input passed to the tool.
    pub input: Value,
    /// Parent tool call when providers emit nested tool execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
}

impl ToolCall {
    /// Creates a tool call without a parent relationship.
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>, input: Value) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            input,
            parent_id: None,
        }
    }

    /// Associates this call with a parent tool call.
    #[must_use]
    pub fn with_parent_id(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }
}

/// A single content fragment returned by a tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolContent {
    /// Plain text output.
    Text {
        /// The returned text.
        text: String,
    },
    /// Binary image output.
    Image {
        /// Raw image bytes.
        data: Vec<u8>,
        /// MIME type describing the image encoding.
        media_type: String,
    },
    /// Structured JSON output.
    Json {
        /// Arbitrary JSON payload.
        value: Value,
    },
}

impl ToolContent {
    /// Creates a text fragment.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// Creates an image fragment.
    #[must_use]
    pub fn image(data: impl Into<Vec<u8>>, media_type: impl Into<String>) -> Self {
        Self::Image {
            data: data.into(),
            media_type: media_type.into(),
        }
    }

    /// Creates a JSON fragment.
    #[must_use]
    pub const fn json(value: Value) -> Self {
        Self::Json { value }
    }
}

/// A normalized tool execution result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResult {
    /// Stable tool call identifier.
    pub id: String,
    /// Canonical tool name.
    pub name: String,
    /// Ordered output fragments.
    pub content: Vec<ToolContent>,
    /// Whether the tool reported an error outcome.
    pub is_error: bool,
    /// Parent tool call when providers emit nested tool execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
}

impl ToolResult {
    /// Creates a tool result with explicit error state.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        content: Vec<ToolContent>,
        is_error: bool,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            content,
            is_error,
            parent_id: None,
        }
    }

    /// Creates a successful tool result.
    #[must_use]
    pub fn success(
        id: impl Into<String>,
        name: impl Into<String>,
        content: Vec<ToolContent>,
    ) -> Self {
        Self::new(id, name, content, false)
    }

    /// Creates a failed tool result.
    #[must_use]
    pub fn failure(
        id: impl Into<String>,
        name: impl Into<String>,
        content: Vec<ToolContent>,
    ) -> Self {
        Self::new(id, name, content, true)
    }

    /// Associates this result with a parent tool call.
    #[must_use]
    pub fn with_parent_id(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }
}
