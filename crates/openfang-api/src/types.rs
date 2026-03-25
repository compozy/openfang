//! Request/response types for the OpenFang API.

use openfang_agent_definition::{
    AgentDefinition, AgentProductMetadata, ProviderBinding, ValidationIssue,
};
use openfang_types::agent::AgentManifest;
use openfang_types::scheduler::{
    CronAction, CronDefinitionForkedFrom, CronDefinitionOrigin, CronDelivery, CronSchedule,
};
pub use openfang_types::skill::{SkillDetail, SkillSummary};
use openfang_types::task::{
    ActorKind, AssigneeRef, Complexity, FileRef, LabelRef, OwnerRef, Priority, RepositoryRef,
    SortOrder, SubtaskId, SubtaskKind, SubtaskSortField, SubtaskStatus, TaskId, TaskReplanEffects,
    TaskSortField, TaskSource, TaskStatus,
};
use openfang_types::workflow::{
    NormalizedWorkflow, ValidationIssue as WorkflowValidationIssue, WorkflowIr,
    WorkflowV2Definition,
};
use serde::{Deserialize, Serialize};

/// Request to spawn an agent from a TOML manifest string or a template name.
#[derive(Debug, Deserialize)]
pub struct SpawnRequest {
    /// Agent manifest as TOML string (optional if `template` is provided).
    #[serde(default)]
    pub manifest_toml: String,
    /// Template name from `~/.openfang/agents/{template}/agent.toml`.
    /// When provided and `manifest_toml` is empty, the template is loaded automatically.
    #[serde(default)]
    pub template: Option<String>,
    /// Optional Ed25519 signed manifest envelope (JSON).
    /// When present, the signature is verified before spawning.
    #[serde(default)]
    pub signed_manifest: Option<String>,
}

/// Response after spawning an agent.
#[derive(Debug, Serialize)]
pub struct SpawnResponse {
    pub agent_id: String,
    pub name: String,
}

/// A file attachment reference (from a prior upload).
#[derive(Debug, Clone, Deserialize)]
pub struct AttachmentRef {
    pub file_id: String,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub content_type: String,
}

/// Legacy request to send a message to an agent.
#[derive(Debug, Deserialize)]
pub struct LegacyMessageRequest {
    pub message: String,
    /// Optional file attachments (uploaded via /upload endpoint).
    #[serde(default)]
    pub attachments: Vec<AttachmentRef>,
    /// Sender identity (e.g. WhatsApp phone number, Telegram user ID).
    #[serde(default)]
    pub sender_id: Option<String>,
    /// Sender display name.
    #[serde(default)]
    pub sender_name: Option<String>,
}

/// Legacy response from sending a message.
#[derive(Debug, Serialize)]
pub struct LegacyMessageResponse {
    pub response: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub iterations: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

/// Structured message input payload for v1 agent messaging.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct MessageInputPayload {
    pub items: Vec<MessageInputItem>,
}

/// One structured item inside a v1 message input payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct MessageInputItem {
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// V1 request to submit or stream an agent message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct MessageRequest {
    pub session_id: String,
    pub input: MessageInputPayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// V1 accepted response after submitting an agent message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct MessageResponse {
    pub accepted: bool,
    pub resource_id: String,
    pub status: String,
    pub session_id: String,
    pub message_id: String,
}

/// Provider resolution details returned by agent message dry-run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct MessageResolvedProvider {
    pub driver: String,
    pub model: String,
}

/// Session summary returned by agent message dry-run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct MessageResolvedSession {
    pub id: String,
    pub active: bool,
    pub message_count: u32,
}

/// Resolved execution plan returned by agent message dry-run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct MessageDryRunResolved {
    pub agent_id: String,
    pub session_id: String,
    pub provider: MessageResolvedProvider,
    pub model: String,
    pub tools: Vec<String>,
    pub session: MessageResolvedSession,
}

/// Estimated effects returned by agent message dry-run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct MessageDryRunEffects {
    pub message_submit: bool,
    pub estimated_tokens: u32,
    pub estimated_cost: f64,
}

/// Explanation payload returned by agent message dry-run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct MessageDryRunExplanation {
    pub skills: Vec<String>,
    pub capabilities: serde_json::Value,
    pub steps: Vec<String>,
}

/// V1 dry-run response for an agent message request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct MessageDryRunResponse {
    pub would_execute: bool,
    pub resolved: MessageDryRunResolved,
    pub effects: MessageDryRunEffects,
    pub explanation: MessageDryRunExplanation,
}

/// Canonical stream event payload emitted by v1 agent message SSE endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct StreamEvent {
    pub event: String,
    pub data: serde_json::Value,
}

/// Request to install a skill from the marketplace.
#[derive(Debug, Deserialize)]
pub struct SkillInstallRequest {
    pub name: String,
}

/// Request to uninstall a skill.
#[derive(Debug, Deserialize)]
pub struct SkillUninstallRequest {
    pub name: String,
}

/// Paginated skill list response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct SkillListResponse {
    pub items: Vec<SkillSummary>,
    pub next_cursor: Option<String>,
}

/// Full detail response for one skill resource.
pub type SkillResponse = SkillDetail;

/// Summary payload returned by task list endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskSummarySource {
    Workflow { run_id: String },
    Manual,
    Api,
}

/// Summary payload returned by task list endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct TaskSummaryResponse {
    pub id: TaskId,
    pub slug: String,
    pub title: String,
    pub status: TaskStatus,
    pub priority: Priority,
    pub position: i64,
    pub source: TaskSummarySource,
    pub updated_at: String,
}

/// Paginated task list response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct TaskListResponse {
    pub items: Vec<TaskSummaryResponse>,
    pub next_cursor: Option<String>,
}

/// Request payload for creating a durable task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CreateTaskRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<TaskId>,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub source: TaskSource,
    pub owner: OwnerRef,
    pub created_by: OwnerRef,
    #[serde(default = "default_task_status")]
    pub status: TaskStatus,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub complexity: Complexity,
    pub position: i64,
    #[serde(default)]
    pub repository_refs: Vec<RepositoryRef>,
    #[serde(default)]
    pub label_refs: Vec<LabelRef>,
    #[serde(default)]
    pub artifact_refs: Vec<openfang_types::task::ArtifactRef>,
    #[serde(default)]
    pub doc_refs: Vec<openfang_types::task::DocRef>,
    #[serde(default)]
    pub file_refs: Vec<FileRef>,
    #[serde(default = "default_empty_object")]
    pub metadata: serde_json::Value,
}

/// Request payload for updating a durable task.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct UpdateTaskRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<TaskSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<OwnerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<OwnerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity: Option<Complexity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_refs: Option<Vec<RepositoryRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_refs: Option<Vec<LabelRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_refs: Option<Vec<openfang_types::task::ArtifactRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_refs: Option<Vec<openfang_types::task::DocRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_refs: Option<Vec<FileRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Summary payload returned by subtask list endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct SubtaskSummaryResponse {
    pub id: SubtaskId,
    pub task_id: TaskId,
    pub title: String,
    pub status: SubtaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<AssigneeRef>,
    pub updated_at: String,
}

/// Paginated subtask list response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct SubtaskListResponse {
    pub items: Vec<SubtaskSummaryResponse>,
    pub next_cursor: Option<String>,
}

/// Request payload for creating a durable subtask beneath a task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CreateSubtaskRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<SubtaskId>,
    pub title: String,
    pub description: String,
    pub kind: SubtaskKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<SubtaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity: Option<Complexity>,
    pub position: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<AssigneeRef>,
    #[serde(default)]
    pub depends_on: Vec<SubtaskId>,
    #[serde(default)]
    pub parallelizable: bool,
    #[serde(default = "default_empty_object")]
    pub input: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default = "default_empty_object")]
    pub metadata: serde_json::Value,
}

/// Request payload for updating one durable subtask.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct UpdateSubtaskRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<SubtaskKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<SubtaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity: Option<Complexity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Option<AssigneeRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<Vec<SubtaskId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallelizable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Option<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Accepted response returned after applying a task replan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct TaskReplanAcceptedResponse {
    pub accepted: bool,
    pub resource_id: TaskId,
    pub status: String,
    pub effects: TaskReplanEffects,
}

/// Query parameters accepted by task list endpoints.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct TaskListQueryParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<TaskSortField>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<SortOrder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<openfang_types::task::TaskSourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, rename = "q", skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
}

/// Query parameters accepted by subtask list endpoints.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct SubtaskListQueryParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<SubtaskSortField>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<SortOrder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<SubtaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee_kind: Option<ActorKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<SubtaskKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<bool>,
}

fn default_empty_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

fn default_task_status() -> TaskStatus {
    TaskStatus::Planned
}

/// Request to update an agent's manifest.
#[derive(Debug, Deserialize)]
pub struct LegacyAgentUpdateRequest {
    pub manifest_toml: String,
}

/// Request payload for creating an agent definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CreateAgentRequest {
    pub definition: AgentDefinition,
}

/// Request payload for updating an agent definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UpdateAgentRequest {
    pub definition: AgentDefinition,
}

/// Definition origin metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentOriginKind {
    User,
    Pack,
}

/// Public definition origin payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AgentOrigin {
    pub kind: AgentOriginKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl AgentOrigin {
    #[must_use]
    pub fn user() -> Self {
        Self {
            kind: AgentOriginKind::User,
            pack_id: None,
            pack_version: None,
            source: None,
        }
    }
}

/// Upstream provenance for a forked definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AgentForkedFrom {
    pub kind: AgentOriginKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack_version: Option<String>,
    pub resource_type: String,
    pub resource_id: String,
}

/// Workflow definition origin uses the same payload shape as agent origins.
pub type WorkflowOriginKind = AgentOriginKind;
/// Workflow definition origin uses the same payload shape as agent origins.
pub type WorkflowOrigin = AgentOrigin;
/// Workflow fork provenance uses the same payload shape as agent provenance.
pub type WorkflowForkedFrom = AgentForkedFrom;
/// Schedule definition origin uses the typed scheduler metadata shape.
pub type ScheduleOrigin = CronDefinitionOrigin;
/// Schedule fork provenance uses the typed scheduler metadata shape.
pub type ScheduleForkedFrom = CronDefinitionForkedFrom;

fn default_schedule_enabled() -> bool {
    true
}

fn default_schedule_delivery() -> CronDelivery {
    CronDelivery::None
}

/// Normalized schedule definition payload accepted by create, update, and validate flows.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScheduleDefinition {
    pub agent: String,
    pub name: String,
    #[serde(default = "default_schedule_enabled")]
    pub enabled: bool,
    pub schedule: CronSchedule,
    pub action: CronAction,
    #[serde(default = "default_schedule_delivery")]
    pub delivery: CronDelivery,
}

/// Runtime summary embedded in full schedule resources.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ScheduleRuntimeStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    pub consecutive_errors: u32,
    pub one_shot: bool,
}

/// Full public schedule resource response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScheduleResponse {
    pub id: String,
    pub agent: String,
    pub name: String,
    pub enabled: bool,
    pub schedule: CronSchedule,
    pub action: CronAction,
    pub delivery: CronDelivery,
    pub origin: ScheduleOrigin,
    pub forked_from: Option<ScheduleForkedFrom>,
    pub created_at: String,
    pub updated_at: String,
    pub runtime_status: ScheduleRuntimeStatus,
}

/// Runtime summary embedded in schedule list items.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ScheduleListRuntimeStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
}

/// One schedule item returned by `/api/v1/schedules`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScheduleListItem {
    pub id: String,
    pub agent: String,
    pub name: String,
    pub enabled: bool,
    pub schedule: CronSchedule,
    pub action: CronAction,
    pub runtime_status: ScheduleListRuntimeStatus,
    pub updated_at: String,
}

/// Paginated schedule list response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScheduleListResponse {
    pub items: Vec<ScheduleListItem>,
    pub next_cursor: Option<String>,
}

/// Structured validation issue returned by schedule validation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ScheduleValidationIssue {
    pub severity: String,
    pub code: String,
    pub path: String,
    pub message: String,
}

/// Request payload for schedule validation.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScheduleValidateRequest {
    pub definition: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
}

/// Response payload for schedule validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScheduleValidateResponse {
    pub valid: bool,
    pub issues: Vec<ScheduleValidationIssue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized: Option<ScheduleDefinition>,
}

/// Full runtime response for one schedule definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ScheduleRuntimeResponse {
    pub schedule_id: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    pub consecutive_errors: u32,
    pub one_shot: bool,
}

/// Request payload for schedule forks.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScheduleForkRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

/// Request payload for schedule run-now operations.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScheduleRunNowRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Accepted response for schedule operational actions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ScheduleAcceptedActionResponse {
    pub accepted: bool,
    pub resource_id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

/// Resolved execution plan returned by schedule dry-run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScheduleDryRunResolved {
    pub schedule_id: String,
    pub action: CronAction,
}

/// Estimated effects returned by schedule dry-run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ScheduleDryRunEffects {
    pub schedule_fire: bool,
}

/// Explanation payload returned by schedule dry-run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScheduleDryRunExplanation {
    pub delivery: CronDelivery,
}

/// Dry-run response for schedule run-now simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScheduleDryRunResponse {
    pub would_execute: bool,
    pub resolved: ScheduleDryRunResolved,
    pub effects: ScheduleDryRunEffects,
    pub explanation: ScheduleDryRunExplanation,
}

/// Agent provider summary used in list responses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AgentProviderSummary {
    pub driver: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

/// Aggregated runtime status attached to a definition list item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AgentRuntimeStatus {
    pub loaded: bool,
    pub healthy: bool,
    pub active_sessions: u32,
    pub active_dispatches: u32,
}

/// Full public agent resource response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct AgentResponse {
    #[serde(flatten)]
    pub definition: AgentDefinition,
    pub origin: AgentOrigin,
    pub forked_from: Option<AgentForkedFrom>,
    pub created_at: String,
    pub updated_at: String,
}

/// Agent list item returned by `/api/v1/agents`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct AgentListItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub provider: AgentProviderSummary,
    pub origin: AgentOrigin,
    pub forked_from: Option<AgentForkedFrom>,
    pub runtime_status: AgentRuntimeStatus,
    pub updated_at: String,
}

/// Paginated agent definition list response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct AgentListResponse {
    pub items: Vec<AgentListItem>,
    pub next_cursor: Option<String>,
}

/// Aggregated workflow runtime status attached to list items.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowListRuntimeStatus {
    pub loaded: bool,
    pub active_runs: usize,
    pub waiting_runs: usize,
}

/// Full public workflow resource response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowResponse {
    #[serde(flatten)]
    pub definition: WorkflowV2Definition,
    pub origin: WorkflowOrigin,
    pub forked_from: Option<WorkflowForkedFrom>,
    pub created_at: String,
    pub updated_at: String,
}

/// Workflow list item returned by `/api/v1/workflows`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowListItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    pub steps: usize,
    pub origin: WorkflowOrigin,
    pub runtime_status: WorkflowListRuntimeStatus,
    pub updated_at: String,
}

/// Paginated workflow definition list response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowListResponse {
    pub items: Vec<WorkflowListItem>,
    pub next_cursor: Option<String>,
}

/// Full workflow runtime resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowRuntimeResponse {
    pub workflow_id: String,
    pub loaded: bool,
    pub healthy: bool,
    pub active_runs: usize,
    pub waiting_runs: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<String>,
}

/// Request payload for workflow forking.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowForkRequest {
    pub mode: String,
}

/// Request payload for workflow run creation and dry-run simulation.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowRunRequest {
    #[serde(default)]
    pub input: serde_json::Value,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default = "default_workflow_run_metadata")]
    pub metadata: serde_json::Value,
}

fn default_workflow_run_metadata() -> serde_json::Value {
    serde_json::json!({})
}

/// Request payload for agent-definition validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentValidateRequest {
    pub definition: AgentDefinition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
}

/// Response payload for agent-definition validation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct AgentValidateResponse {
    pub valid: bool,
    pub issues: Vec<ValidationIssue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized: Option<AgentDefinition>,
}

/// Request payload for agent-definition compilation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentCompileRequest {
    pub definition: AgentDefinition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
}

/// Compiled agent payload returned by compile endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentCompiledPayload {
    pub agent_manifest: AgentManifest,
    pub provider_binding: ProviderBinding,
    pub product_metadata: AgentProductMetadata,
}

/// Response payload for agent-definition compilation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentCompileResponse {
    pub definition_id: String,
    pub normalized: AgentDefinition,
    pub compiled: AgentCompiledPayload,
}

/// Response payload for fetching one compiled agent definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentCompiledResponse {
    pub definition_id: String,
    pub normalized: AgentDefinition,
    pub compiled: AgentCompiledPayload,
}

/// Public runtime resource payload for one agent definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AgentRuntimeResponse {
    pub agent_id: String,
    pub loaded: bool,
    pub state: openfang_types::agent::AgentState,
    pub mode: openfang_types::agent::AgentMode,
    pub healthy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_session_id: Option<String>,
    pub active_sessions: u32,
    pub active_dispatches: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<String>,
}

/// Request payload for updating an agent runtime mode.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct RuntimeModeRequest {
    pub mode: openfang_types::agent::AgentMode,
}

/// Session summary returned by `/api/v1/agents/{id}/sessions`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct SessionListItem {
    pub id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub active: bool,
    pub message_count: u32,
    pub dispatch_count: u32,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compacted_at: Option<String>,
}

/// Paginated session list payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct SessionListResponse {
    pub items: Vec<SessionListItem>,
    pub next_cursor: Option<String>,
}

/// Full session detail payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct SessionDetail {
    pub id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub active: bool,
    pub message_count: u32,
    pub dispatch_count: u32,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compacted_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<serde_json::Value>>,
}

/// Request payload for creating a new agent session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct CreateSessionRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Canonical accepted response for operational actions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AcceptedActionResponse {
    pub accepted: bool,
    pub resource_id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Request to change an agent's operational mode.
#[derive(Debug, Deserialize)]
pub struct SetModeRequest {
    pub mode: openfang_types::agent::AgentMode,
}

/// Request to run a migration.
#[derive(Debug, Deserialize)]
pub struct MigrateRequest {
    pub source: String,
    pub source_dir: String,
    pub target_dir: String,
    #[serde(default)]
    pub dry_run: bool,
}

/// Request to scan a directory for migration.
#[derive(Debug, Deserialize)]
pub struct MigrateScanRequest {
    pub path: String,
}

/// Request to install a skill from ClawHub.
#[derive(Debug, Deserialize)]
pub struct ClawHubInstallRequest {
    /// ClawHub skill slug (e.g., "github-helper").
    pub slug: String,
}

/// Request payload for workflow validation.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowValidateRequest {
    /// Workflow definition to validate.
    pub definition: serde_json::Value,
    /// Whether warnings should also mark the definition as invalid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
    /// Optional control-plane validation context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
}

/// Response payload for workflow validation.
#[derive(Debug, Clone, Serialize)]
pub struct WorkflowValidateResponse {
    /// Whether the definition passed validation under the selected strictness.
    pub valid: bool,
    /// Collected validation issues.
    pub issues: Vec<WorkflowValidationIssue>,
    /// Normalized workflow definition when validation can produce one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized: Option<NormalizedWorkflow>,
}

/// Request payload for workflow compilation.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowCompileRequest {
    /// Workflow definition to compile.
    pub definition: serde_json::Value,
    /// Optional control-plane compilation context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
}

/// Wrapper around the compiled workflow payload.
#[derive(Debug, Clone, Serialize)]
pub struct WorkflowCompiledPayload {
    /// Stable workflow IR returned by the compiler.
    pub workflow_ir: WorkflowIr,
}

/// Response payload for workflow compilation.
#[derive(Debug, Clone, Serialize)]
pub struct WorkflowCompileResponse {
    /// Stable workflow definition identifier.
    pub definition_id: String,
    /// Normalized workflow definition used during compilation.
    pub normalized: NormalizedWorkflow,
    /// Compiled workflow IR payload.
    pub compiled: WorkflowCompiledPayload,
}

/// Response payload for fetching a cached compiled workflow.
#[derive(Debug, Clone, Serialize)]
pub struct WorkflowCompiledResponse {
    /// Stable workflow definition identifier.
    pub definition_id: String,
    /// Normalized workflow definition used during compilation.
    pub normalized: NormalizedWorkflow,
    /// Cached compiled workflow IR payload.
    pub compiled: WorkflowCompiledPayload,
}

/// Request payload for durable workflow signal submission.
#[derive(Debug, Clone, Deserialize)]
pub struct RunSignalSubmitRequest {
    /// Signal name expected by the waiting workflow step.
    pub name: String,
    /// Optional arbitrary JSON payload carried by the signal.
    #[serde(default)]
    pub payload: serde_json::Value,
    /// Submission source, such as `api`, `trigger`, or `schedule`.
    pub source: String,
    /// Client-provided idempotency key scoped to the run.
    pub idempotency_key: String,
}
