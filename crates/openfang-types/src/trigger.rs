//! Public Trigger v2 definition and compiled types.
//!
//! These types define the Compozy-owned trigger control-plane surface. They
//! are intentionally separate from the legacy agent-centric trigger model in
//! `openfang-kernel::triggers`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

fn default_trigger_enabled() -> bool {
    true
}

fn default_empty_object() -> JsonValue {
    JsonValue::Object(serde_json::Map::new())
}

/// Severity for machine-readable trigger validation issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerValidationSeverity {
    /// The definition is invalid and compilation must stop.
    Error,
    /// The definition is accepted, but the author should review the issue.
    Warning,
}

impl TriggerValidationSeverity {
    /// Returns `true` when the severity blocks compilation.
    #[must_use]
    pub const fn is_error(self) -> bool {
        matches!(self, Self::Error)
    }
}

/// Machine-readable validation issue returned by the trigger compile pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerValidationIssue {
    /// Severity of the issue.
    pub severity: TriggerValidationSeverity,
    /// Stable machine-readable issue code.
    pub code: String,
    /// Path to the invalid field or binding.
    pub path: String,
    /// Human-readable message explaining the problem.
    pub message: String,
}

impl TriggerValidationIssue {
    /// Creates a validation issue.
    #[must_use]
    pub fn new(
        severity: TriggerValidationSeverity,
        code: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            code: code.into(),
            path: path.into(),
            message: message.into(),
        }
    }

    /// Creates an error issue.
    #[must_use]
    pub fn error(
        code: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(TriggerValidationSeverity::Error, code, path, message)
    }

    /// Creates a warning issue.
    #[must_use]
    pub fn warning(
        code: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(TriggerValidationSeverity::Warning, code, path, message)
    }
}

/// Match expression for one trigger definition.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TriggerMatch {
    /// Exact event name, such as `issue.created`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    /// Exact event source, such as `api`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Case-insensitive substring search against the serialized event payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contains: Option<String>,
    /// Simple dotted-path equality filters applied to the event payload.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub filters: BTreeMap<String, JsonValue>,
}

/// Structured text input payload for trigger-dispatched agent messages.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TriggerInputPayload {
    /// Ordered message items delivered to the target agent.
    #[serde(default)]
    pub items: Vec<TriggerInputItem>,
}

/// One structured item inside a trigger agent-message payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TriggerInputItem {
    /// Item kind. Currently only `text` is supported.
    #[serde(rename = "type")]
    pub item_type: String,
    /// Text body for `type = "text"` items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Destination selector for workflow signal targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TriggerWorkflowSignalSelector {
    /// Stable workflow definition identifier that owns the waiting runs.
    pub workflow_id: String,
}

/// What happens when one trigger fires.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TriggerTarget {
    /// Deliver a structured message to one agent definition.
    AgentMessage {
        /// Agent definition identifier or name.
        agent: String,
        /// Structured message input payload.
        #[serde(default)]
        input: TriggerInputPayload,
        /// Optional metadata forwarded to the dispatch layer.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<JsonValue>,
    },
    /// Start a workflow definition with the provided input.
    WorkflowStart {
        /// Workflow definition identifier or name.
        workflow: String,
        /// Initial workflow input.
        #[serde(
            default = "default_empty_object",
            skip_serializing_if = "JsonValue::is_null"
        )]
        input: JsonValue,
    },
    /// Deliver a durable signal to waiting workflow runs.
    WorkflowSignal {
        /// Signal name expected by the waiting workflow step.
        signal: String,
        /// Target workflow selector.
        selector: TriggerWorkflowSignalSelector,
        /// Structured signal payload.
        #[serde(
            default = "default_empty_object",
            skip_serializing_if = "JsonValue::is_null"
        )]
        payload: JsonValue,
    },
}

impl TriggerTarget {
    /// Returns the stable public target kind.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::AgentMessage { .. } => "agent_message",
            Self::WorkflowStart { .. } => "workflow_start",
            Self::WorkflowSignal { .. } => "workflow_signal",
        }
    }

    /// Returns the dispatch action class used by the runtime.
    #[must_use]
    pub const fn dispatch_action(&self) -> TriggerDispatchAction {
        match self {
            Self::AgentMessage { .. } => TriggerDispatchAction::AgentMessageSend,
            Self::WorkflowStart { .. } => TriggerDispatchAction::WorkflowRunCreate,
            Self::WorkflowSignal { .. } => TriggerDispatchAction::WorkflowSignalDispatch,
        }
    }
}

/// One file-backed Trigger v2 definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TriggerV2Definition {
    /// Stable trigger definition identifier.
    pub id: String,
    /// Human-readable trigger name.
    pub name: String,
    /// Human-readable description of the automation.
    pub description: String,
    /// Whether the trigger participates in active matching.
    #[serde(default = "default_trigger_enabled")]
    pub enabled: bool,
    /// Maximum number of fires before auto-disable. Zero means unlimited.
    #[serde(default)]
    pub max_fires: u64,
    /// Cooldown window in seconds between dispatches. Zero disables cooldown.
    #[serde(default)]
    pub cooldown_secs: u64,
    /// Event match expression.
    #[serde(rename = "match")]
    pub trigger_match: TriggerMatch,
    /// Explicit dispatch target.
    pub target: TriggerTarget,
}

/// Normalized trigger definition after reference resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NormalizedTrigger {
    /// Stable trigger definition identifier.
    pub id: String,
    /// Human-readable trigger name.
    pub name: String,
    /// Human-readable description of the automation.
    pub description: String,
    /// Whether the trigger participates in active matching.
    pub enabled: bool,
    /// Maximum number of fires before auto-disable. Zero means unlimited.
    pub max_fires: u64,
    /// Cooldown window in seconds between dispatches. Zero disables cooldown.
    pub cooldown_secs: u64,
    /// Canonicalized event match expression.
    #[serde(rename = "match")]
    pub trigger_match: TriggerMatch,
    /// Canonicalized dispatch target.
    pub target: TriggerTarget,
}

/// Runtime dispatch class derived from a trigger target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerDispatchAction {
    /// Send one agent message.
    AgentMessageSend,
    /// Create one durable workflow run.
    WorkflowRunCreate,
    /// Deliver one durable workflow signal.
    WorkflowSignalDispatch,
}

/// Compiled IR returned by trigger compile endpoints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TriggerIr {
    /// Stable trigger definition identifier.
    pub trigger_id: String,
    /// Canonicalized event match expression.
    #[serde(rename = "match")]
    pub trigger_match: TriggerMatch,
    /// Canonicalized dispatch target.
    pub target: TriggerTarget,
    /// Runtime action class selected from the target kind.
    pub dispatch_action: TriggerDispatchAction,
    /// Whether the trigger is currently enabled.
    pub enabled: bool,
    /// Maximum number of fires before auto-disable. Zero means unlimited.
    pub max_fires: u64,
    /// Cooldown window in seconds between dispatches.
    pub cooldown_secs: u64,
}

/// Persisted runtime state for one trigger definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TriggerRuntimeStatus {
    /// Stable trigger definition identifier.
    pub trigger_id: String,
    /// Whether the trigger is currently enabled.
    pub enabled: bool,
    /// Number of successful dispatches recorded so far.
    pub fire_count: u64,
    /// Maximum number of fires before auto-disable. Zero means unlimited.
    pub max_fires: u64,
    /// Cooldown window in seconds between dispatches.
    pub cooldown_secs: u64,
    /// Timestamp of the last successful fire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired_at: Option<String>,
}

/// Synthetic event payload used by trigger test and compile helpers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TriggerEvent {
    /// Event name, such as `issue.created`.
    pub event: String,
    /// Event source, such as `api`.
    pub source: String,
    /// Structured event payload.
    #[serde(default = "default_empty_object")]
    pub payload: JsonValue,
}

#[cfg(test)]
mod tests {
    use super::{
        TriggerDispatchAction, TriggerInputItem, TriggerInputPayload, TriggerTarget,
        TriggerWorkflowSignalSelector,
    };

    #[test]
    fn target_kind_should_map_to_dispatch_actions() {
        assert_eq!(
            TriggerTarget::AgentMessage {
                agent: "writer".to_string(),
                input: TriggerInputPayload {
                    items: vec![TriggerInputItem {
                        item_type: "text".to_string(),
                        text: Some("hello".to_string()),
                    }],
                },
                metadata: None,
            }
            .dispatch_action(),
            TriggerDispatchAction::AgentMessageSend
        );
        assert_eq!(
            TriggerTarget::WorkflowStart {
                workflow: "sdlc".to_string(),
                input: serde_json::json!({}),
            }
            .dispatch_action(),
            TriggerDispatchAction::WorkflowRunCreate
        );
        assert_eq!(
            TriggerTarget::WorkflowSignal {
                signal: "resume".to_string(),
                selector: TriggerWorkflowSignalSelector {
                    workflow_id: "sdlc".to_string(),
                },
                payload: serde_json::json!({}),
            }
            .dispatch_action(),
            TriggerDispatchAction::WorkflowSignalDispatch
        );
    }
}
