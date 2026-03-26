//! Shared looper domain types for durable subtask execution.

use crate::task::{SubtaskId, TaskId};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

macro_rules! string_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(pub String);

        impl $name {
            /// Create a new typed identifier from any string-like value.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.0.as_str())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.0.as_str()
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }
    };
}

string_newtype!(LooperRunId);
string_newtype!(LooperSubtaskId);

/// Durable lifecycle states for one looper run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LooperRunStatus {
    /// The run exists but dispatching has not started.
    Pending,
    /// The run is actively selecting or observing subtasks.
    Running,
    /// The run is paused and will not dispatch new subtasks.
    Paused,
    /// The run completed successfully.
    Completed,
    /// The run failed.
    Failed,
    /// The run was cancelled.
    Cancelled,
}

impl LooperRunStatus {
    /// Return the stable SQLite / JSON encoding.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Return whether the run is terminal.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    fn parse_db_text(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "paused" => Some(Self::Paused),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

impl std::fmt::Display for LooperRunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for LooperRunStatus {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_db_text(value).ok_or("invalid looper run status")
    }
}

/// Durable lifecycle states for one subtask inside a looper run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LooperSubtaskStatus {
    /// The subtask has not yet been dispatched for this looper run.
    Pending,
    /// The subtask dispatch is active.
    Running,
    /// The subtask completed successfully.
    Completed,
    /// The subtask failed.
    Failed,
    /// The subtask was cancelled.
    Cancelled,
}

impl LooperSubtaskStatus {
    /// Return the stable SQLite / JSON encoding.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Return whether the status is terminal for the looper subtask view.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    fn parse_db_text(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

impl std::fmt::Display for LooperSubtaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for LooperSubtaskStatus {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_db_text(value).ok_or("invalid looper subtask status")
    }
}

/// Explicit looper execution modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LooperExecutionMode {
    /// Dispatch one subtask at a time.
    Sequential,
    /// Dispatch multiple subtasks up to the configured limit.
    Parallel,
}

impl LooperExecutionMode {
    /// Return the stable SQLite / JSON encoding.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sequential => "sequential",
            Self::Parallel => "parallel",
        }
    }

    fn parse_db_text(value: &str) -> Option<Self> {
        match value {
            "sequential" => Some(Self::Sequential),
            "parallel" => Some(Self::Parallel),
            _ => None,
        }
    }
}

impl std::fmt::Display for LooperExecutionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for LooperExecutionMode {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_db_text(value).ok_or("invalid looper execution mode")
    }
}

/// Explicit subtask selection strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LooperSelectionStrategy {
    /// Select eligible subtasks by their stable ordering fields.
    #[default]
    Priority,
}

impl LooperSelectionStrategy {
    /// Return the stable SQLite / JSON encoding.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Priority => "priority",
        }
    }

    fn parse_db_text(value: &str) -> Option<Self> {
        match value {
            "priority" => Some(Self::Priority),
            _ => None,
        }
    }
}

impl std::fmt::Display for LooperSelectionStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for LooperSelectionStrategy {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_db_text(value).ok_or("invalid looper selection strategy")
    }
}

/// Explicit looper execution policy stored on every durable run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LooperExecutionPolicy {
    /// Concurrency mode for the run.
    pub mode: LooperExecutionMode,
    /// Maximum number of subtasks that may be in-flight at once.
    pub max_parallelism: u32,
    /// Ordering strategy for eligible subtasks.
    pub selection: LooperSelectionStrategy,
}

/// Durable looper progress summary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LooperProgress {
    /// Total subtasks tracked by this looper run.
    pub total: usize,
    /// Number of completed subtasks.
    pub completed: usize,
    /// Number of failed subtasks.
    pub failed: usize,
}

/// Durable looper run record used by persistence and control-plane layers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LooperRunRecord {
    /// Stable looper run identifier.
    #[serde(rename = "id")]
    pub looper_run_id: LooperRunId,
    /// Owning task identifier.
    pub task_id: TaskId,
    /// Producing workflow run identifier, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_run_id: Option<String>,
    /// Current looper lifecycle state.
    pub status: LooperRunStatus,
    /// Explicit execution policy.
    pub execution_policy: LooperExecutionPolicy,
    /// Currently active subtask identifier, when one is in-flight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_subtask_id: Option<SubtaskId>,
    /// Aggregated progress counters.
    pub progress: LooperProgress,
    /// Structured terminal or runtime error payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonValue>,
    /// Run start timestamp in RFC 3339 UTC format.
    pub started_at: String,
    /// Last update timestamp in RFC 3339 UTC format.
    pub updated_at: String,
    /// Completion timestamp for terminal states.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
}

/// Public looper run resource shape returned by the control plane.
///
/// Unlike `LooperRunRecord`, nullable fields are always serialized as `null`
/// so the HTTP API can match the exact payload contract in `API-SPEC.md`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LooperRunResource {
    /// Stable looper run identifier.
    pub id: LooperRunId,
    /// Owning task identifier.
    pub task_id: TaskId,
    /// Producing workflow run identifier, when applicable.
    pub source_run_id: Option<String>,
    /// Current looper lifecycle state.
    pub status: LooperRunStatus,
    /// Explicit execution policy.
    pub execution_policy: LooperExecutionPolicy,
    /// Currently active subtask identifier, when one is in-flight.
    pub current_subtask_id: Option<SubtaskId>,
    /// Aggregated progress counters.
    pub progress: LooperProgress,
    /// Structured terminal or runtime error payload.
    pub error: Option<JsonValue>,
    /// Run start timestamp in RFC 3339 UTC format.
    pub started_at: String,
    /// Last update timestamp in RFC 3339 UTC format.
    pub updated_at: String,
    /// Completion timestamp for terminal states.
    pub completed_at: Option<String>,
}

/// One looper-run subtask execution view row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LooperSubtaskRecord {
    /// Stable looper-subtask identifier.
    #[serde(rename = "id")]
    pub looper_subtask_id: LooperSubtaskId,
    /// Owning looper run identifier.
    pub looper_run_id: LooperRunId,
    /// Canonical subtask identifier.
    pub subtask_id: SubtaskId,
    /// Current execution-view status.
    pub status: LooperSubtaskStatus,
    /// Linked durable dispatch identifier, when dispatched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatch_id: Option<String>,
    /// Structured result payload, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<JsonValue>,
    /// Structured error payload, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonValue>,
    /// Last update timestamp in RFC 3339 UTC format.
    pub updated_at: String,
}

/// Public looper-subtask execution view exposed by the control plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LooperSubtaskView {
    /// Stable canonical subtask identifier.
    pub id: SubtaskId,
    /// Human-facing subtask title.
    pub title: String,
    /// Current looper execution status.
    pub status: LooperSubtaskStatus,
    /// Last update timestamp in RFC 3339 UTC format.
    pub updated_at: String,
}

/// Filter set for listing durable looper runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LooperRunListQuery {
    /// Optional task filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    /// Optional source run filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_run_id: Option<String>,
    /// Optional looper status filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<LooperRunStatus>,
    /// Optional execution mode filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_mode: Option<LooperExecutionMode>,
}

impl From<&LooperRunRecord> for LooperRunResource {
    fn from(record: &LooperRunRecord) -> Self {
        Self {
            id: record.looper_run_id.clone(),
            task_id: record.task_id.clone(),
            source_run_id: record.source_run_id.clone(),
            status: record.status,
            execution_policy: record.execution_policy.clone(),
            current_subtask_id: record.current_subtask_id.clone(),
            progress: record.progress,
            error: record.error.clone(),
            started_at: record.started_at.clone(),
            updated_at: record.updated_at.clone(),
            completed_at: record.completed_at.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn looper_run_record_should_round_trip_through_json() {
        let record = LooperRunRecord {
            looper_run_id: LooperRunId::new("loop_001"),
            task_id: TaskId::new("task_001"),
            source_run_id: Some("run_123".to_string()),
            status: LooperRunStatus::Running,
            execution_policy: LooperExecutionPolicy {
                mode: LooperExecutionMode::Parallel,
                max_parallelism: 3,
                selection: LooperSelectionStrategy::Priority,
            },
            current_subtask_id: Some(SubtaskId::new("subtask_001")),
            progress: LooperProgress {
                total: 5,
                completed: 2,
                failed: 1,
            },
            error: None,
            started_at: "2026-03-25T10:00:00Z".to_string(),
            updated_at: "2026-03-25T10:02:00Z".to_string(),
            completed_at: None,
        };

        let value = serde_json::to_value(&record).expect("serialize looper run");
        let round_trip: LooperRunRecord =
            serde_json::from_value(value).expect("deserialize looper run");

        assert_eq!(round_trip, record);
    }

    #[test]
    fn looper_subtask_record_should_round_trip_through_json() {
        let record = LooperSubtaskRecord {
            looper_subtask_id: LooperSubtaskId::new("loop_subtask_001"),
            looper_run_id: LooperRunId::new("loop_001"),
            subtask_id: SubtaskId::new("subtask_001"),
            status: LooperSubtaskStatus::Completed,
            dispatch_id: Some("dispatch_001".to_string()),
            result: Some(serde_json::json!({ "ok": true })),
            error: None,
            updated_at: "2026-03-25T10:03:00Z".to_string(),
        };

        let value = serde_json::to_value(&record).expect("serialize looper subtask");
        let round_trip: LooperSubtaskRecord =
            serde_json::from_value(value).expect("deserialize looper subtask");

        assert_eq!(round_trip, record);
    }
}
