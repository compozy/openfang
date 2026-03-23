//! Request/response types for the OpenFang API.

use openfang_types::workflow::{
    NormalizedWorkflow, ValidationIssue, WorkflowIr, WorkflowV2Definition,
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

/// Request to send a message to an agent.
#[derive(Debug, Deserialize)]
pub struct MessageRequest {
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

/// Response from sending a message.
#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub response: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub iterations: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
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
pub struct AgentUpdateRequest {
    pub manifest_toml: String,
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
    pub issues: Vec<ValidationIssue>,
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
