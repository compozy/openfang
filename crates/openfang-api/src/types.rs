//! Request/response types for the OpenFang API.

use openfang_agent_definition::{
    AgentDefinition, AgentProductMetadata, ProviderBinding, ValidationIssue,
};
use openfang_types::agent::AgentManifest;
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
    pub definition: WorkflowV2Definition,
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
    pub definition: WorkflowV2Definition,
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
