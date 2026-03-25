//! Shared task and subtask domain types for Compozy.

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

        impl std::str::FromStr for $name {
            type Err = std::convert::Infallible;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Ok(Self::from(value))
            }
        }
    };
}

string_newtype!(TaskId);
string_newtype!(SubtaskId);
string_newtype!(LabelRef);

/// Stable task source kinds used by the public API and repository filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskSourceKind {
    /// The task originated from a workflow run.
    Workflow,
    /// The task was created directly as a manual durable work item.
    Manual,
    /// The task was created through the public API.
    Api,
}

impl TaskSourceKind {
    /// Return the stable SQLite / JSON encoding.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Workflow => "workflow",
            Self::Manual => "manual",
            Self::Api => "api",
        }
    }

    fn parse_db_text(value: &str) -> Option<Self> {
        match value {
            "workflow" => Some(Self::Workflow),
            "manual" => Some(Self::Manual),
            "api" => Some(Self::Api),
            _ => None,
        }
    }
}

impl std::fmt::Display for TaskSourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for TaskSourceKind {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_db_text(value).ok_or("invalid task source kind")
    }
}

/// Stable task source payload used by the public task shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskSource {
    /// The task originated from a workflow run.
    Workflow {
        /// Workflow definition identifier.
        workflow_id: String,
        /// Producing workflow run identifier.
        run_id: String,
    },
    /// The task was created manually outside workflow execution.
    Manual,
    /// The task was created through the public API.
    Api,
}

impl TaskSource {
    /// Return the stable source kind discriminator.
    pub const fn kind(&self) -> TaskSourceKind {
        match self {
            Self::Workflow { .. } => TaskSourceKind::Workflow,
            Self::Manual => TaskSourceKind::Manual,
            Self::Api => TaskSourceKind::Api,
        }
    }

    /// Return the workflow run identifier when the source is workflow-backed.
    pub fn run_id(&self) -> Option<&str> {
        match self {
            Self::Workflow { run_id, .. } => Some(run_id.as_str()),
            Self::Manual | Self::Api => None,
        }
    }

    /// Return the workflow definition identifier when the source is workflow-backed.
    pub fn workflow_id(&self) -> Option<&str> {
        match self {
            Self::Workflow { workflow_id, .. } => Some(workflow_id.as_str()),
            Self::Manual | Self::Api => None,
        }
    }
}

/// Stable actor kinds used for task ownership and subtask assignees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    /// A single agent identity.
    Agent,
    /// A named agent group.
    AgentGroup,
    /// A human user identity.
    User,
}

impl ActorKind {
    /// Return the stable SQLite / JSON encoding.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::AgentGroup => "agent_group",
            Self::User => "user",
        }
    }

    fn parse_db_text(value: &str) -> Option<Self> {
        match value {
            "agent" => Some(Self::Agent),
            "agent_group" => Some(Self::AgentGroup),
            "user" => Some(Self::User),
            _ => None,
        }
    }
}

impl std::fmt::Display for ActorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ActorKind {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_db_text(value).ok_or("invalid actor kind")
    }
}

/// Owner payload used by top-level tasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerRef {
    /// Owner category.
    pub kind: ActorKind,
    /// Stable referenced identifier.
    #[serde(rename = "ref")]
    pub ref_id: String,
}

/// Assignee payload used by subtasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssigneeRef {
    /// Assignee category.
    pub kind: ActorKind,
    /// Stable referenced identifier.
    #[serde(rename = "ref")]
    pub ref_id: String,
}

/// Repository link attached to a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryRef {
    /// Stable repository identifier.
    pub repository_id: String,
    /// Repository role inside the task context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

/// Artifact link attached to a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    /// Stable artifact identifier.
    pub artifact_id: String,
    /// Artifact classification.
    #[serde(rename = "type")]
    pub type_name: String,
    /// Optional current version identifier projected by linked-context views.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_version_id: Option<String>,
}

/// Document link attached to a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocRef {
    /// Stable document identifier.
    pub doc_id: String,
    /// Document classification.
    #[serde(rename = "type")]
    pub type_name: String,
    /// Optional current version identifier projected by linked-context views.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_version_id: Option<String>,
}

/// File link attached to a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    /// Workspace-relative or external file path.
    pub path: String,
    /// File location kind.
    pub kind: String,
    /// Optional human-facing description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Durable task lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Planned but not yet started.
    Planned,
    /// Actively in progress.
    InProgress,
    /// Completed successfully.
    Completed,
    /// Completed with failure semantics.
    Failed,
    /// Explicitly cancelled.
    Cancelled,
}

impl TaskStatus {
    /// Return the stable SQLite / JSON encoding.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Whether this state is terminal.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    fn parse_db_text(value: &str) -> Option<Self> {
        match value {
            "planned" => Some(Self::Planned),
            "in_progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for TaskStatus {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_db_text(value).ok_or("invalid task status")
    }
}

/// Durable subtask lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtaskStatus {
    /// Not yet eligible for execution.
    Planned,
    /// All dependencies are satisfied and execution may start.
    Ready,
    /// Actively executing.
    InProgress,
    /// Completed successfully.
    Completed,
    /// Completed with failure semantics.
    Failed,
    /// Explicitly cancelled.
    Cancelled,
}

impl SubtaskStatus {
    /// Return the stable SQLite / JSON encoding.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Ready => "ready",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Whether this state is terminal.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    fn parse_db_text(value: &str) -> Option<Self> {
        match value {
            "planned" => Some(Self::Planned),
            "ready" => Some(Self::Ready),
            "in_progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

impl std::fmt::Display for SubtaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for SubtaskStatus {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_db_text(value).ok_or("invalid subtask status")
    }
}

/// Stable subtask kinds exposed through the public domain model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtaskKind {
    /// A documentation-oriented change.
    DocChange,
    /// A code-oriented change.
    CodeChange,
    /// A research or investigation item.
    Research,
    /// A review-driven follow-up item.
    ReviewItem,
    /// A validation or QA item.
    Validation,
    /// Fallback for future extensions.
    Other,
}

impl SubtaskKind {
    /// Return the stable SQLite / JSON encoding.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DocChange => "doc_change",
            Self::CodeChange => "code_change",
            Self::Research => "research",
            Self::ReviewItem => "review_item",
            Self::Validation => "validation",
            Self::Other => "other",
        }
    }

    fn parse_db_text(value: &str) -> Option<Self> {
        match value {
            "doc_change" => Some(Self::DocChange),
            "code_change" => Some(Self::CodeChange),
            "research" => Some(Self::Research),
            "review_item" => Some(Self::ReviewItem),
            "validation" => Some(Self::Validation),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}

impl std::fmt::Display for SubtaskKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for SubtaskKind {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_db_text(value).ok_or("invalid subtask kind")
    }
}

/// Task priority used for ordering and selection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// Lowest priority.
    Low,
    /// Default priority.
    #[default]
    Medium,
    /// High priority.
    High,
    /// Critical priority.
    Critical,
}

impl Priority {
    /// Return the stable SQLite / JSON encoding.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    fn parse_db_text(value: &str) -> Option<Self> {
        match value {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Priority {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_db_text(value).ok_or("invalid priority")
    }
}

/// Execution complexity classification.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Complexity {
    /// Small or simple work.
    Low,
    /// Default work complexity.
    #[default]
    Medium,
    /// Large or difficult work.
    High,
}

impl Complexity {
    /// Return the stable SQLite / JSON encoding.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    fn parse_db_text(value: &str) -> Option<Self> {
        match value {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }
}

impl std::fmt::Display for Complexity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Complexity {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_db_text(value).ok_or("invalid complexity")
    }
}

/// Sort direction for cursor-based list queries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    /// Ascending order.
    #[default]
    Asc,
    /// Descending order.
    Desc,
}

/// Supported task list sort fields.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskSortField {
    /// Sort by the stable task position.
    #[default]
    Position,
}

/// Supported subtask list sort fields.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtaskSortField {
    /// Sort by the stable subtask position.
    #[default]
    Position,
}

/// Durable task record used by the repository layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskRecord {
    /// Stable task identifier.
    #[serde(rename = "id")]
    pub task_id: TaskId,
    /// Human-meaningful slug unique within the database.
    pub slug: String,
    /// Public source metadata for the task.
    pub source: TaskSource,
    /// Human-facing title.
    pub title: String,
    /// Human-facing description.
    pub description: String,
    /// Current task lifecycle state.
    pub status: TaskStatus,
    /// Priority classification.
    pub priority: Priority,
    /// Complexity classification.
    pub complexity: Complexity,
    /// Stable ordering key inside task lists.
    pub position: i64,
    /// Task owner reference.
    pub owner: OwnerRef,
    /// Task creator reference.
    pub created_by: OwnerRef,
    /// Repository links attached to the task.
    pub repository_refs: Vec<RepositoryRef>,
    /// Label links attached to the task.
    pub label_refs: Vec<LabelRef>,
    /// Artifact links attached to the task.
    pub artifact_refs: Vec<ArtifactRef>,
    /// Document links attached to the task.
    pub doc_refs: Vec<DocRef>,
    /// File links attached to the task.
    pub file_refs: Vec<FileRef>,
    /// Arbitrary task metadata, excluding reserved system keys.
    pub metadata: JsonValue,
    /// Creation timestamp in RFC 3339 UTC format.
    pub created_at: String,
    /// Last update timestamp in RFC 3339 UTC format.
    pub updated_at: String,
    /// Completion timestamp for terminal states.
    pub completed_at: Option<String>,
}

/// Durable subtask record used by the repository layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubtaskRecord {
    /// Stable subtask identifier.
    #[serde(rename = "id")]
    pub subtask_id: SubtaskId,
    /// Owning task identifier.
    pub task_id: TaskId,
    /// Human-facing title.
    pub title: String,
    /// Human-facing description.
    pub description: String,
    /// Subtask kind.
    pub kind: SubtaskKind,
    /// Current lifecycle state.
    pub status: SubtaskStatus,
    /// Complexity classification.
    pub complexity: Complexity,
    /// Stable ordering key inside the parent task.
    pub position: i64,
    /// Optional assignee reference.
    pub assignee: Option<AssigneeRef>,
    /// Ordered dependency list.
    pub depends_on: Vec<SubtaskId>,
    /// Whether the subtask can execute concurrently with peers.
    pub parallelizable: bool,
    /// Opaque structured input payload.
    pub input: JsonValue,
    /// Opaque structured result payload.
    pub result: Option<JsonValue>,
    /// Arbitrary subtask metadata.
    pub metadata: JsonValue,
    /// Creation timestamp in RFC 3339 UTC format.
    pub created_at: String,
    /// Last update timestamp in RFC 3339 UTC format.
    pub updated_at: String,
    /// Completion timestamp for terminal states.
    pub completed_at: Option<String>,
}

/// Cursor-backed task list query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskListQuery {
    /// Maximum number of items to return.
    pub limit: usize,
    /// Opaque pagination cursor returned by the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Stable sort field.
    #[serde(default)]
    pub sort: TaskSortField,
    /// Sort direction for `position`.
    #[serde(default)]
    pub order: SortOrder,
    /// Optional lifecycle-state filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskStatus>,
    /// Optional priority filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    /// Optional created-by reference filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    /// Optional source kind filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<TaskSourceKind>,
    /// Optional label reference filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Optional repository reference filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// Optional free-text search filter.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "q")]
    pub search: Option<String>,
}

impl Default for TaskListQuery {
    fn default() -> Self {
        Self {
            limit: 50,
            cursor: None,
            sort: TaskSortField::Position,
            order: SortOrder::Asc,
            status: None,
            priority: None,
            created_by: None,
            source_kind: None,
            label: None,
            repository: None,
            search: None,
        }
    }
}

/// Cursor-backed task list response used by repository callers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskListPage {
    /// Page of task records.
    pub items: Vec<TaskRecord>,
    /// Cursor for the next page, or `None` when exhausted.
    pub next_cursor: Option<String>,
}

/// Filter set for listing subtasks inside one parent task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtaskListQuery {
    /// Maximum number of items to return.
    pub limit: usize,
    /// Opaque pagination cursor returned by the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Stable sort field.
    #[serde(default)]
    pub sort: SubtaskSortField,
    /// Sort direction for the selected sort field.
    #[serde(default)]
    pub order: SortOrder,
    /// Optional owning task filter for global lists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    /// Optional lifecycle-state filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<SubtaskStatus>,
    /// Optional assignee-kind filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee_kind: Option<ActorKind>,
    /// Optional assignee-ref filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee_ref: Option<String>,
    /// Optional kind filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<SubtaskKind>,
    /// Optional ready filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ready: Option<bool>,
    /// Optional blocked filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<bool>,
}

impl Default for SubtaskListQuery {
    fn default() -> Self {
        Self {
            limit: 50,
            cursor: None,
            sort: SubtaskSortField::Position,
            order: SortOrder::Asc,
            task_id: None,
            status: None,
            assignee_kind: None,
            assignee_ref: None,
            kind: None,
            ready: None,
            blocked: None,
        }
    }
}

/// Cursor-backed subtask list response used by repository callers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubtaskListPage {
    /// Page of subtask records.
    pub items: Vec<SubtaskRecord>,
    /// Cursor for the next page, or `None` when exhausted.
    pub next_cursor: Option<String>,
}

/// Replan request applied atomically to one task.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskReplanRequest {
    /// Human-facing reason for the replan.
    pub reason: String,
    /// Ordered set of operations to apply.
    pub operations: Vec<TaskReplanOperation>,
    /// Structured metadata for the replan request.
    #[serde(default)]
    pub metadata: JsonValue,
}

/// One ordered replan operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum TaskReplanOperation {
    /// Cancel existing subtasks.
    CancelSubtasks {
        /// Stable subtask identifiers to cancel.
        subtask_ids: Vec<SubtaskId>,
    },
    /// Create new subtasks.
    CreateSubtasks {
        /// New subtask definitions.
        items: Vec<PlannedSubtask>,
    },
    /// Patch existing subtasks.
    UpdateSubtasks {
        /// Partial updates to apply.
        items: Vec<SubtaskPatch>,
    },
}

/// A new subtask definition used by replan/create flows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedSubtask {
    /// Optional explicit subtask identifier. When absent, the caller generates one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "id",
        alias = "subtask_id"
    )]
    pub subtask_id: Option<SubtaskId>,
    /// Human-facing title.
    pub title: String,
    /// Human-facing description.
    pub description: String,
    /// Subtask kind.
    pub kind: SubtaskKind,
    /// Optional starting status. Defaults are repository-defined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<SubtaskStatus>,
    /// Optional complexity classification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity: Option<Complexity>,
    /// Ordering key.
    pub position: i64,
    /// Optional assignee.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<AssigneeRef>,
    /// Ordered dependency list.
    #[serde(default)]
    pub depends_on: Vec<SubtaskId>,
    /// Whether the subtask can execute concurrently with peers.
    #[serde(default)]
    pub parallelizable: bool,
    /// Structured input payload.
    #[serde(default = "empty_object")]
    pub input: JsonValue,
    /// Optional structured result payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<JsonValue>,
    /// Optional metadata payload.
    #[serde(default = "empty_object")]
    pub metadata: JsonValue,
}

/// Partial update payload applied during replan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubtaskPatch {
    /// Stable subtask identifier being updated.
    pub id: SubtaskId,
    /// Optional title patch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional description patch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional kind patch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<SubtaskKind>,
    /// Optional status patch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<SubtaskStatus>,
    /// Optional complexity patch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity: Option<Complexity>,
    /// Optional position patch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<i64>,
    /// Optional assignee patch. `Some(None)` clears the assignee.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Option<AssigneeRef>>,
    /// Optional dependency patch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<Vec<SubtaskId>>,
    /// Optional concurrency patch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallelizable: Option<bool>,
    /// Optional input patch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<JsonValue>,
    /// Optional result patch. `Some(None)` clears the stored result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Option<JsonValue>>,
    /// Optional metadata patch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
}

/// Replan effect counters returned by the repository layer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskReplanEffects {
    /// Number of subtasks created.
    pub created_subtasks: usize,
    /// Number of subtasks updated.
    pub updated_subtasks: usize,
    /// Number of subtasks cancelled.
    pub cancelled_subtasks: usize,
}

fn empty_object() -> JsonValue {
    JsonValue::Object(serde_json::Map::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn task_resource_should_round_trip_with_nested_refs() {
        let record = TaskRecord {
            task_id: TaskId::new("task_001"),
            slug: "onboarding-revamp-prd".to_string(),
            source: TaskSource::Workflow {
                workflow_id: "sdlc".to_string(),
                run_id: "run_123".to_string(),
            },
            title: "Prepare PRD for onboarding revamp".to_string(),
            description: "Define the new onboarding flow and acceptance criteria".to_string(),
            status: TaskStatus::Planned,
            priority: Priority::High,
            complexity: Complexity::Medium,
            position: 1,
            owner: OwnerRef {
                kind: ActorKind::AgentGroup,
                ref_id: "sdlc".to_string(),
            },
            created_by: OwnerRef {
                kind: ActorKind::Agent,
                ref_id: "planner".to_string(),
            },
            repository_refs: vec![RepositoryRef {
                repository_id: "repo_main".to_string(),
                role: Some("primary".to_string()),
            }],
            label_refs: vec![LabelRef::new("planning"), LabelRef::new("prd")],
            artifact_refs: vec![ArtifactRef {
                artifact_id: "artifact_001".to_string(),
                type_name: "prd".to_string(),
                current_version_id: Some("artifact_v3".to_string()),
            }],
            doc_refs: vec![DocRef {
                doc_id: "doc_001".to_string(),
                type_name: "brief".to_string(),
                current_version_id: Some("doc_v2".to_string()),
            }],
            file_refs: vec![FileRef {
                path: "docs/prd.md".to_string(),
                kind: "workspace".to_string(),
                description: Some("Current PRD draft".to_string()),
            }],
            metadata: serde_json::json!({ "source": "agent" }),
            created_at: "2026-03-21T14:00:00Z".to_string(),
            updated_at: "2026-03-21T14:05:00Z".to_string(),
            completed_at: None,
        };

        let json = serde_json::to_value(&record).expect("serialize task record");
        let round_trip: TaskRecord = serde_json::from_value(json).expect("deserialize task record");

        assert_eq!(round_trip, record);
    }

    #[test]
    fn subtask_resource_should_round_trip_with_dependency_and_result_fields() {
        let record = SubtaskRecord {
            subtask_id: SubtaskId::new("subtask_001"),
            task_id: TaskId::new("task_001"),
            title: "Draft problem statement".to_string(),
            description: "Write the initial problem statement for the PRD".to_string(),
            kind: SubtaskKind::DocChange,
            status: SubtaskStatus::Ready,
            complexity: Complexity::Medium,
            position: 1,
            assignee: Some(AssigneeRef {
                kind: ActorKind::Agent,
                ref_id: "prd-writer".to_string(),
            }),
            depends_on: vec![SubtaskId::new("subtask_000")],
            parallelizable: false,
            input: serde_json::json!({ "artifact_id": "artifact_001" }),
            result: Some(serde_json::json!({ "ok": true })),
            metadata: serde_json::json!({}),
            created_at: "2026-03-21T14:01:00Z".to_string(),
            updated_at: "2026-03-21T14:05:00Z".to_string(),
            completed_at: None,
        };

        let json = serde_json::to_value(&record).expect("serialize subtask record");
        let round_trip: SubtaskRecord =
            serde_json::from_value(json).expect("deserialize subtask record");

        assert_eq!(round_trip, record);
    }
}
