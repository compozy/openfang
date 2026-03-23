//! Result types merged by the hook chain.

use std::collections::BTreeMap;

use arky_protocol::{Message, ToolContent};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Verdict returned by `before_tool_call`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Verdict {
    /// Allow the tool call to continue.
    Allow,
    /// Block the tool call with a reason.
    Block {
        /// Human-readable block reason.
        reason: String,
    },
}

impl Verdict {
    /// Creates a blocking verdict.
    #[must_use]
    pub fn block(reason: impl Into<String>) -> Self {
        Self::Block {
            reason: reason.into(),
        }
    }
}

/// Override returned by `after_tool_call`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToolResultOverride {
    /// Replacement tool content when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<ToolContent>>,
    /// Replacement error flag when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

impl ToolResultOverride {
    /// Creates an empty override.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces tool content.
    #[must_use]
    pub fn with_content(mut self, content: Vec<ToolContent>) -> Self {
        self.content = Some(content);
        self
    }

    /// Replaces the error flag.
    #[must_use]
    pub const fn with_is_error(mut self, is_error: bool) -> Self {
        self.is_error = Some(is_error);
        self
    }

    /// Creates a text-only override used by plain-text shell hooks.
    #[must_use]
    pub fn from_text(text: impl Into<String>) -> Self {
        Self::new().with_content(vec![ToolContent::text(text)])
    }

    /// Merges another override into this one using last-write-wins semantics.
    pub fn merge_from(&mut self, other: Self) {
        if let Some(content) = other.content {
            self.content = Some(content);
        }
        if let Some(is_error) = other.is_error {
            self.is_error = Some(is_error);
        }
    }

    /// Returns whether the override changes anything.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.content.is_none() && self.is_error.is_none()
    }
}

/// Decision returned by `on_stop`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StopDecision {
    /// Proceed with stopping.
    Stop,
    /// Continue execution and block the current stop attempt.
    Continue {
        /// Human-readable reason for continuing.
        reason: String,
    },
}

impl StopDecision {
    /// Creates a continuation decision.
    #[must_use]
    pub fn continue_with(reason: impl Into<String>) -> Self {
        Self::Continue {
            reason: reason.into(),
        }
    }
}

/// Update returned by `session_start`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionStartUpdate {
    /// Environment variables injected for the session.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Shallow settings overrides injected for the session.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, Value>,
    /// Messages appended in registration order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<Message>,
}

impl SessionStartUpdate {
    /// Creates an empty session-start update.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores environment overrides.
    #[must_use]
    pub fn with_env(mut self, env: BTreeMap<String, String>) -> Self {
        self.env = env;
        self
    }

    /// Stores settings overrides.
    #[must_use]
    pub fn with_settings(mut self, settings: BTreeMap<String, Value>) -> Self {
        self.settings = settings;
        self
    }

    /// Stores injected messages.
    #[must_use]
    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    /// Merges another update using shallow-map merge and message append semantics.
    pub fn merge_from(&mut self, other: Self) {
        self.env.extend(other.env);
        self.settings.extend(other.settings);
        self.messages.extend(other.messages);
    }

    /// Returns whether the update changes anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.env.is_empty() && self.settings.is_empty() && self.messages.is_empty()
    }
}

/// Update returned by `user_prompt_submit`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PromptUpdate {
    /// Replacement prompt text when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Messages appended in registration order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<Message>,
}

impl PromptUpdate {
    /// Creates an empty prompt update.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores a prompt rewrite.
    #[must_use]
    pub fn rewrite(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    /// Stores injected messages.
    #[must_use]
    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    /// Merges another update using last-prompt-wins and message append semantics.
    pub fn merge_from(&mut self, other: Self) {
        if let Some(prompt) = other.prompt {
            self.prompt = Some(prompt);
        }
        self.messages.extend(other.messages);
    }

    /// Returns whether the update changes anything.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.prompt.is_none() && self.messages.is_empty()
    }
}
