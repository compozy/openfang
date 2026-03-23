//! Context objects passed into hook invocations.

use std::{collections::BTreeMap, path::PathBuf};

use arky_protocol::{Message, SessionRef, ToolCall, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Lifecycle events handled by the hook system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    /// Before a tool call executes.
    BeforeToolCall,
    /// After a tool call completes.
    AfterToolCall,
    /// When a session starts.
    SessionStart,
    /// When a session ends.
    SessionEnd,
    /// When the agent decides whether to stop.
    OnStop,
    /// When the user submits a prompt.
    UserPromptSubmit,
}

impl HookEvent {
    /// Returns the stable event name used in diagnostics and shell payloads.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BeforeToolCall => "before_tool_call",
            Self::AfterToolCall => "after_tool_call",
            Self::SessionStart => "session_start",
            Self::SessionEnd => "session_end",
            Self::OnStop => "on_stop",
            Self::UserPromptSubmit => "user_prompt_submit",
        }
    }
}

/// Shared environment attached to all hook contexts.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct HookExecutionScope {
    /// Session reference owned by the current run.
    pub session: SessionRef,
    /// Transcript path when one exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<PathBuf>,
    /// Working directory associated with the current run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    /// Provider-specific permission mode when one is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    /// Arbitrary metadata reserved for higher layers.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}

impl HookExecutionScope {
    /// Creates a scope for a session.
    #[must_use]
    pub fn new(session: SessionRef) -> Self {
        Self {
            session,
            ..Self::default()
        }
    }

    /// Stores a transcript path.
    #[must_use]
    pub fn with_transcript_path(mut self, transcript_path: impl Into<PathBuf>) -> Self {
        self.transcript_path = Some(transcript_path.into());
        self
    }

    /// Stores a working directory.
    #[must_use]
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Stores a permission mode.
    #[must_use]
    pub fn with_permission_mode(mut self, permission_mode: impl Into<String>) -> Self {
        self.permission_mode = Some(permission_mode.into());
        self
    }

    /// Stores arbitrary metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: BTreeMap<String, Value>) -> Self {
        self.metadata = metadata;
        self
    }
}

/// Context passed to `before_tool_call`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BeforeToolCallContext {
    /// Shared execution scope.
    #[serde(flatten)]
    pub scope: HookExecutionScope,
    /// Tool call about to execute.
    pub tool_call: ToolCall,
}

impl BeforeToolCallContext {
    /// Creates a before-tool-call context.
    #[must_use]
    pub fn new(session: SessionRef, tool_call: ToolCall) -> Self {
        Self {
            scope: HookExecutionScope::new(session),
            tool_call,
        }
    }

    /// Replaces the shared scope.
    #[must_use]
    pub fn with_scope(mut self, scope: HookExecutionScope) -> Self {
        self.scope = scope;
        self
    }
}

/// Context passed to `after_tool_call`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AfterToolCallContext {
    /// Shared execution scope.
    #[serde(flatten)]
    pub scope: HookExecutionScope,
    /// Tool call that completed.
    pub tool_call: ToolCall,
    /// Result emitted by the tool.
    pub result: ToolResult,
}

impl AfterToolCallContext {
    /// Creates an after-tool-call context.
    #[must_use]
    pub fn new(session: SessionRef, tool_call: ToolCall, result: ToolResult) -> Self {
        Self {
            scope: HookExecutionScope::new(session),
            tool_call,
            result,
        }
    }

    /// Replaces the shared scope.
    #[must_use]
    pub fn with_scope(mut self, scope: HookExecutionScope) -> Self {
        self.scope = scope;
        self
    }
}

/// Session-start sources carried over from the upstream provider stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStartSource {
    /// Fresh startup.
    Startup,
    /// Resume from persisted state.
    Resume,
    /// Start after a clear/reset.
    Clear,
    /// Start after compaction.
    Compact,
}

/// Context passed to `session_start`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionStartContext {
    /// Shared execution scope.
    #[serde(flatten)]
    pub scope: HookExecutionScope,
    /// Why the session is starting.
    pub source: SessionStartSource,
    /// Initial environment visible to the session.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Provider settings exposed as a shallow JSON map.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, Value>,
    /// Messages already present at session start.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<Message>,
}

impl SessionStartContext {
    /// Creates a session-start context.
    #[must_use]
    pub fn new(session: SessionRef, source: SessionStartSource) -> Self {
        Self {
            scope: HookExecutionScope::new(session),
            source,
            env: BTreeMap::new(),
            settings: BTreeMap::new(),
            messages: Vec::new(),
        }
    }

    /// Replaces the shared scope.
    #[must_use]
    pub fn with_scope(mut self, scope: HookExecutionScope) -> Self {
        self.scope = scope;
        self
    }

    /// Stores environment values.
    #[must_use]
    pub fn with_env(mut self, env: BTreeMap<String, String>) -> Self {
        self.env = env;
        self
    }

    /// Stores provider settings.
    #[must_use]
    pub fn with_settings(mut self, settings: BTreeMap<String, Value>) -> Self {
        self.settings = settings;
        self
    }

    /// Stores initial messages.
    #[must_use]
    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }
}

/// Context passed to `session_end`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionEndContext {
    /// Shared execution scope.
    #[serde(flatten)]
    pub scope: HookExecutionScope,
    /// Reason the session ended.
    pub reason: String,
    /// Final messages visible to the hook.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<Message>,
}

impl SessionEndContext {
    /// Creates a session-end context.
    #[must_use]
    pub fn new(session: SessionRef, reason: impl Into<String>) -> Self {
        Self {
            scope: HookExecutionScope::new(session),
            reason: reason.into(),
            messages: Vec::new(),
        }
    }

    /// Replaces the shared scope.
    #[must_use]
    pub fn with_scope(mut self, scope: HookExecutionScope) -> Self {
        self.scope = scope;
        self
    }

    /// Stores final messages.
    #[must_use]
    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }
}

/// Context passed to `on_stop`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct StopContext {
    /// Shared execution scope.
    #[serde(flatten)]
    pub scope: HookExecutionScope,
    /// Optional stop reason proposed by the caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Whether the stop hook is already active upstream.
    #[serde(default)]
    pub stop_hook_active: bool,
    /// Messages visible at the stop decision point.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<Message>,
}

impl StopContext {
    /// Creates a stop context.
    #[must_use]
    pub fn new(session: SessionRef) -> Self {
        Self {
            scope: HookExecutionScope::new(session),
            reason: None,
            stop_hook_active: false,
            messages: Vec::new(),
        }
    }

    /// Replaces the shared scope.
    #[must_use]
    pub fn with_scope(mut self, scope: HookExecutionScope) -> Self {
        self.scope = scope;
        self
    }

    /// Stores the proposed stop reason.
    #[must_use]
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Stores the current stop-hook activation state.
    #[must_use]
    pub const fn with_stop_hook_active(mut self, stop_hook_active: bool) -> Self {
        self.stop_hook_active = stop_hook_active;
        self
    }

    /// Stores visible messages.
    #[must_use]
    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }
}

/// Context passed to `user_prompt_submit`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PromptSubmitContext {
    /// Shared execution scope.
    #[serde(flatten)]
    pub scope: HookExecutionScope,
    /// Prompt text submitted by the user.
    pub prompt: String,
    /// Messages visible to the prompt hook.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<Message>,
}

impl PromptSubmitContext {
    /// Creates a prompt-submit context.
    #[must_use]
    pub fn new(session: SessionRef, prompt: impl Into<String>) -> Self {
        Self {
            scope: HookExecutionScope::new(session),
            prompt: prompt.into(),
            messages: Vec::new(),
        }
    }

    /// Replaces the shared scope.
    #[must_use]
    pub fn with_scope(mut self, scope: HookExecutionScope) -> Self {
        self.scope = scope;
        self
    }

    /// Stores visible messages.
    #[must_use]
    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }
}
