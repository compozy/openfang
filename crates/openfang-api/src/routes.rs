//! Route handlers for the OpenFang API.

use crate::agent_definitions::AgentDefinitionStore;
use crate::trigger_definitions::{canonicalize_trigger_definition, TriggerDefinitionStore};
use crate::types::*;
use crate::workflow_definitions::{canonicalize_workflow_definition, WorkflowDefinitionStore};
use axum::extract::{
    rejection::{JsonRejection, QueryRejection},
    Path, Query, State,
};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use dashmap::DashMap;
use openfang_agent_definition::{
    compile as compile_agent_ir, stage1_schema_validate, stage2_reference_validate,
    stage3_semantic_validate, stage4_normalize, AgentDefinition, CompileError as AgentCompileError,
    CompiledAgentDefinition, ValidationContext as AgentValidationContext,
    ValidationIssue as AgentValidationIssue,
};
use openfang_kernel::cron::JobMeta as ScheduleJobMeta;
use openfang_kernel::kernel::ScheduleExecutionResult;
use openfang_kernel::metering::MeteringEngine;
use openfang_kernel::trigger_v2::{
    compile_trigger_definition as compile_trigger_ir_definition, evaluate_compiled_trigger,
    normalize_trigger_definition, validate_trigger_definition, validate_trigger_value,
    TriggerCompileError, TriggerCompileRegistry, TriggerEngineError,
};
use openfang_kernel::triggers::{TriggerId, TriggerPattern};
use openfang_kernel::workflow::{
    ErrorMode, StepAgent, StepMode, Workflow, WorkflowId, WorkflowRunId, WorkflowStep,
};
use openfang_kernel::workflow_compiler::{
    compile_workflow_definition, normalize_workflow_definition, validate_normalized_workflow,
    validate_workflow_value, WorkflowCompileError,
};
use openfang_kernel::{AgentMessageDispatch, OpenFangKernel};
use openfang_memory::{
    now_timestamp, AgentRuntimeRecord, AgentSessionRecord, DispatchListQuery, DispatchRecord,
    DispatchRepository, DispatchStatus, HitlListQuery, HitlRecord, HitlRepository, HitlStatus,
    ScheduleRuntimeRecord, TaskStoreError,
};
use openfang_memory::{WorkflowRunListQuery, WorkflowRunStatus, WorkflowSignalRecord};
use openfang_runtime::kernel_handle::KernelHandle;
use openfang_runtime::tool_runner::builtin_tool_definitions;
use openfang_types::agent::{AgentId, AgentIdentity, AgentManifest, AgentState, SessionId};
use openfang_types::scheduler::{
    CronAction, CronDefinitionForkedFrom, CronDefinitionOrigin, CronDelivery, CronJob, CronJobId,
    CronSchedule, CronTextInputItem, CronTextInputPayload, CronWorkflowSignalSelector,
};
use openfang_types::task::{
    SortOrder, SubtaskId, SubtaskListQuery, SubtaskPatch, SubtaskRecord, SubtaskSortField,
    SubtaskStatus, TaskId, TaskListQuery, TaskRecord, TaskReplanRequest, TaskSortField, TaskSource,
};
use openfang_types::trigger::{NormalizedTrigger, TriggerRuntimeStatus, TriggerV2Definition};
use openfang_types::workflow::{WorkflowIr, WorkflowIrStepKind};
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Debug, Default, serde::Deserialize)]
pub struct RunListQueryParams {
    #[serde(default)]
    pub workflow_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub waiting_kind: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default, rename = "q")]
    pub search: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct RunSignalListQueryParams {
    #[serde(default)]
    pub consumed: Option<bool>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct AgentSessionDetailQuery {
    #[serde(default)]
    pub include_messages: Option<bool>,
    #[serde(default)]
    pub include_context: Option<bool>,
}

const DEFAULT_PAGE_LIMIT: usize = 50;
const MAX_PAGE_LIMIT: usize = 200;
static WORKFLOW_DEFINITION_WRITE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
static TRIGGER_DEFINITION_WRITE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
static SCHEDULE_DEFINITION_WRITE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[derive(Debug, Default, serde::Deserialize)]
pub struct WorkflowListQueryParams {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub order: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default, rename = "q")]
    pub search: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct TriggerListQueryParams {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub target_kind: Option<String>,
    #[serde(default, rename = "q")]
    pub search: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct WorkflowRunsListQueryParams {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub order: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct ScheduleListQueryParams {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub schedule_kind: Option<String>,
    #[serde(default)]
    pub action_kind: Option<String>,
    #[serde(default, rename = "q")]
    pub search: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct SkillListQueryParams {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default, rename = "q")]
    pub search: Option<String>,
}

#[derive(Debug, Clone)]
struct WorkflowRunPageQuery {
    limit: usize,
    offset: usize,
    sort: String,
    order: Ordering,
}

impl AgentSessionDetailQuery {
    fn wants_messages(&self) -> bool {
        self.include_messages.unwrap_or(false) || self.include_context.unwrap_or(false)
    }
}

fn parse_run_status_param(
    status: Option<&str>,
) -> Result<Option<WorkflowRunStatus>, (StatusCode, Json<serde_json::Value>)> {
    status
        .map(|value| value.parse::<WorkflowRunStatus>())
        .transpose()
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": {
                        "code": "invalid_status",
                        "message": error.to_string(),
                        "details": [],
                    }
                })),
            )
        })
}

fn parse_json_text_field(
    raw: &str,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    serde_json::from_str(raw).map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": {
                    "code": "invalid_durable_json",
                    "message": format!("Stored run payload was not valid JSON: {error}"),
                    "details": [],
                }
            })),
        )
    })
}

fn run_record_to_summary(record: &openfang_memory::WorkflowRunRecord) -> serde_json::Value {
    serde_json::json!({
        "id": record.run_id,
        "workflow_id": record.workflow_id,
        "status": record.status.as_str(),
        "current_step_id": record.current_step_id,
        "waiting_kind": record.waiting_kind,
        "started_at": record.started_at,
        "updated_at": record.updated_at,
    })
}

fn run_record_to_detail(
    record: &openfang_memory::WorkflowRunRecord,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    Ok(serde_json::json!({
        "id": record.run_id,
        "workflow_id": record.workflow_id,
        "workflow_version": record.workflow_version,
        "status": record.status.as_str(),
        "input": parse_json_text_field(&record.input_json)?,
        "vars": parse_json_text_field(&record.vars_json)?,
        "current_step_id": record.current_step_id,
        "waiting_kind": record.waiting_kind,
        "waiting_ref": record.waiting_ref,
        "active_dispatch_id": record.active_dispatch_id,
        "active_hitl_request_id": record.active_hitl_request_id,
        "labels": parse_json_text_field(&record.labels_json)?,
        "metadata": parse_json_text_field(&record.metadata_json)?,
        "error": record
            .error_json
            .as_deref()
            .map(parse_json_text_field)
            .transpose()?,
        "started_at": record.started_at,
        "updated_at": record.updated_at,
        "completed_at": record.completed_at,
    }))
}

fn checkpoint_record_to_json(
    record: &openfang_memory::WorkflowCheckpointRecord,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    Ok(serde_json::json!({
        "id": record.checkpoint_id,
        "run_id": record.run_id,
        "step_id": record.step_id,
        "kind": record.kind.as_str(),
        "data": parse_json_text_field(&record.data_json)?,
        "created_at": record.created_at,
    }))
}

fn signal_record_to_json(
    record: &WorkflowSignalRecord,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    Ok(serde_json::json!({
        "id": record.signal_id,
        "run_id": record.run_id,
        "name": record.name,
        "payload": parse_json_text_field(&record.payload_json)?,
        "source": record.source,
        "consumed": record.consumed,
        "created_at": record.created_at,
        "consumed_at": record.consumed_at,
    }))
}

fn run_not_found_response() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": {
                "code": "not_found",
                "message": "Run not found",
                "details": [],
            }
        })),
    )
}

fn run_internal_error_response(code: &str, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "error": {
                "code": code,
                "message": message,
                "details": [],
            }
        })),
    )
}

fn invalid_run_transition_response(
    action: &str,
    current_status: WorkflowRunStatus,
    allowed_statuses: &[WorkflowRunStatus],
) -> (StatusCode, Json<serde_json::Value>) {
    let action_phrase = match action {
        "pause" => "paused",
        "resume" => "resumed",
        "cancel" => "cancelled",
        _ => action,
    };
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "error": {
                "code": "invalid_run_transition",
                "message": format!(
                    "Run cannot be {action_phrase} from status '{}'",
                    current_status.as_str()
                ),
                "details": [{
                    "action": action,
                    "current_status": current_status.as_str(),
                    "allowed_statuses": allowed_statuses
                        .iter()
                        .map(|status| status.as_str())
                        .collect::<Vec<_>>(),
                }],
            }
        })),
    )
}

fn ensure_durable_run_exists(
    state: &AppState,
    run_id: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    match state.kernel.workflow_stores.workflow_run.find_by_id(run_id) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(run_not_found_response()),
        Err(error) => {
            tracing::warn!("Failed to load durable run {run_id}: {error}");
            Err(run_internal_error_response(
                "run_load_failed",
                "Failed to load run",
            ))
        }
    }
}

fn run_action_accepted_response(
    run_id: &str,
    status: WorkflowRunStatus,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "accepted": true,
            "resource_id": run_id,
            "run_id": run_id,
            "status": status.as_str(),
        })),
    )
}

fn agent_action_accepted_response(
    resource_id: &str,
    session_id: Option<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::OK,
        Json(serde_json::json!(AcceptedActionResponse {
            accepted: true,
            resource_id: resource_id.to_owned(),
            status: "accepted".to_owned(),
            session_id,
        })),
    )
}

fn operational_action_accepted_response(
    resource_id: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!(AcceptedActionResponse {
            accepted: true,
            resource_id: resource_id.to_owned(),
            status: "accepted".to_owned(),
            session_id: None,
        })),
    )
}

/// Shared application state.
///
/// The kernel is wrapped in Arc so it can serve as both the main kernel
/// and the KernelHandle for inter-agent tool access.
pub struct AppState {
    pub kernel: Arc<OpenFangKernel>,
    pub started_at: Instant,
    /// Optional peer registry for OFP mesh networking status.
    pub peer_registry: Option<Arc<openfang_wire::registry::PeerRegistry>>,
    /// Channel bridge manager — held behind a Mutex so it can be swapped on hot-reload.
    pub bridge_manager: tokio::sync::Mutex<Option<openfang_channels::bridge::BridgeManager>>,
    /// Live channel config — updated on every hot-reload so list_channels() reflects reality.
    pub channels_config: tokio::sync::RwLock<openfang_types::config::ChannelsConfig>,
    /// Notify handle to trigger graceful HTTP server shutdown from the API.
    pub shutdown_notify: Arc<tokio::sync::Notify>,
    /// ClawHub response cache — prevents 429 rate limiting on rapid dashboard refreshes.
    /// Maps cache key → (fetched_at, response_json) with 120s TTL.
    pub clawhub_cache: DashMap<String, (Instant, serde_json::Value)>,
    /// Probe cache for local provider health checks (ollama/vllm/lmstudio).
    /// Avoids blocking the `/api/providers` endpoint on TCP timeouts to
    /// unreachable local services. 60-second TTL.
    pub provider_probe_cache: openfang_runtime::provider_health::ProbeCache,
}

impl AppState {
    fn skill_registry(&self) -> &std::sync::RwLock<openfang_skills::registry::SkillRegistry> {
        &self.kernel.skill_registry
    }
}

fn agent_error_response(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
    details: Option<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "error": {
                "code": code,
                "message": message.into(),
                "details": details.unwrap_or_else(|| serde_json::json!([])),
            }
        })),
    )
}

fn agent_json_rejection(rejection: JsonRejection) -> (StatusCode, Json<serde_json::Value>) {
    match rejection {
        JsonRejection::MissingJsonContentType(_) => agent_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Missing `Content-Type: application/json` header",
            None,
        ),
        JsonRejection::JsonDataError(error) => agent_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("Invalid JSON body: {error}"),
            None,
        ),
        JsonRejection::JsonSyntaxError(error) => agent_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("Invalid JSON body: {error}"),
            None,
        ),
        JsonRejection::BytesRejection(error) => agent_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("Failed to read request body: {error}"),
            None,
        ),
        rejection => agent_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("Invalid request body: {rejection}"),
            None,
        ),
    }
}

fn task_query_rejection(rejection: QueryRejection) -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        format!("Invalid query string: {rejection}"),
        None,
    )
}

fn task_summary_source(source: &TaskSource) -> TaskSummarySource {
    match source {
        TaskSource::Workflow { run_id, .. } => TaskSummarySource::Workflow {
            run_id: run_id.clone(),
        },
        TaskSource::Manual => TaskSummarySource::Manual,
        TaskSource::Api => TaskSummarySource::Api,
    }
}

fn dispatch_record_to_summary_response(record: &DispatchRecord) -> DispatchSummaryResponse {
    DispatchSummaryResponse {
        id: record.dispatch_id.clone(),
        run_id: record.run_id.clone(),
        step_id: record.step_id.clone(),
        kind: record.kind,
        target_agent: record.target_agent.clone(),
        status: record.status,
        updated_at: record.updated_at.clone(),
    }
}

fn dispatch_record_to_detail_response(record: &DispatchRecord) -> DispatchDetailResponse {
    DispatchDetailResponse {
        id: record.dispatch_id.clone(),
        run_id: record.run_id.clone(),
        step_id: record.step_id.clone(),
        kind: record.kind,
        target_agent: record.target_agent.clone(),
        status: record.status,
        input: record.input_json.clone(),
        result: record.result_json.clone(),
        error: record.error_json.clone(),
        attempt: record.attempt,
        parent_dispatch_id: record.parent_dispatch_id.clone(),
        spawned_agent_id: record.spawned_agent_id.clone(),
        started_at: record.started_at.clone(),
        updated_at: record.updated_at.clone(),
        completed_at: record.completed_at.clone(),
    }
}

fn hitl_record_to_detail_response(record: &HitlRecord) -> HitlDetailResponse {
    HitlDetailResponse {
        id: record.hitl_request_id.clone(),
        run_id: record.run_id.clone(),
        step_id: record.step_id.clone(),
        dispatch_id: record.dispatch_id.clone(),
        kind: record.kind,
        status: record.status,
        question: record.question.clone(),
        context: record.context_json.clone(),
        response: record.response_json.clone(),
        sequence_no: record.sequence_no,
        created_at: record.created_at.to_rfc3339(),
        answered_at: record
            .answered_at
            .as_ref()
            .map(chrono::DateTime::to_rfc3339),
        timeout_at: record.timeout_at.as_ref().map(chrono::DateTime::to_rfc3339),
    }
}

fn dispatch_not_found_response() -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(
        StatusCode::NOT_FOUND,
        "not_found",
        "Dispatch not found",
        None,
    )
}

fn hitl_not_found_response() -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(
        StatusCode::NOT_FOUND,
        "not_found",
        "HITL request not found",
        None,
    )
}

fn dispatch_internal_error_response(
    code: &str,
    message: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(StatusCode::INTERNAL_SERVER_ERROR, code, message, None)
}

fn hitl_internal_error_response(
    code: &str,
    message: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(StatusCode::INTERNAL_SERVER_ERROR, code, message, None)
}

fn invalid_dispatch_transition_response(
    action: &str,
    current_status: DispatchStatus,
    allowed_statuses: &[DispatchStatus],
) -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(
        StatusCode::CONFLICT,
        "invalid_dispatch_transition",
        format!(
            "Dispatch cannot be {action} from status '{}'",
            current_status.as_str()
        ),
        Some(serde_json::json!([{
            "action": action,
            "current_status": current_status.as_str(),
            "allowed_statuses": allowed_statuses
                .iter()
                .map(|status| status.as_str())
                .collect::<Vec<_>>(),
        }])),
    )
}

fn invalid_hitl_transition_response(
    action: &str,
    current_status: HitlStatus,
    allowed_statuses: &[HitlStatus],
) -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(
        StatusCode::CONFLICT,
        "invalid_hitl_transition",
        format!(
            "HITL request cannot be {action} from status '{}'",
            current_status.as_str()
        ),
        Some(serde_json::json!([{
            "action": action,
            "current_status": current_status.as_str(),
            "allowed_statuses": allowed_statuses
                .iter()
                .map(|status| status.as_str())
                .collect::<Vec<_>>(),
        }])),
    )
}

fn task_summary_from_record(record: &TaskRecord) -> TaskSummaryResponse {
    TaskSummaryResponse {
        id: record.task_id.clone(),
        slug: record.slug.clone(),
        title: record.title.clone(),
        status: record.status,
        priority: record.priority,
        position: record.position,
        source: task_summary_source(&record.source),
        updated_at: record.updated_at.clone(),
    }
}

fn subtask_summary_from_record(record: &SubtaskRecord) -> SubtaskSummaryResponse {
    SubtaskSummaryResponse {
        id: record.subtask_id.clone(),
        task_id: record.task_id.clone(),
        title: record.title.clone(),
        status: record.status,
        assignee: record.assignee.clone(),
        updated_at: record.updated_at.clone(),
    }
}

fn task_list_query_from_params(
    params: TaskListQueryParams,
) -> Result<TaskListQuery, (StatusCode, Json<serde_json::Value>)> {
    Ok(TaskListQuery {
        limit: parse_pagination_limit(params.limit)?,
        cursor: params.cursor,
        sort: params.sort.unwrap_or(TaskSortField::Position),
        order: params.order.unwrap_or(SortOrder::Asc),
        status: params.status,
        priority: params.priority,
        created_by: params.created_by,
        source_kind: params.source_kind,
        label: params.label,
        repository: params.repository,
        search: params.search,
    })
}

fn subtask_list_query_from_params(
    params: SubtaskListQueryParams,
    scoped_task_id: Option<TaskId>,
) -> Result<SubtaskListQuery, (StatusCode, Json<serde_json::Value>)> {
    if let Some(path_task_id) = scoped_task_id.as_ref() {
        if let Some(query_task_id) = params.task_id.as_ref() {
            if query_task_id != path_task_id {
                return Err(workflow_v2_error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "`task_id` query parameter must match the path task ID",
                    Some(serde_json::json!([{
                        "path": "task_id",
                        "expected": path_task_id,
                        "actual": query_task_id,
                    }])),
                ));
            }
        }
    }

    Ok(SubtaskListQuery {
        limit: parse_pagination_limit(params.limit)?,
        cursor: params.cursor,
        sort: params.sort.unwrap_or(SubtaskSortField::Position),
        order: params.order.unwrap_or(SortOrder::Asc),
        task_id: scoped_task_id.or(params.task_id),
        status: params.status,
        assignee_kind: params.assignee_kind,
        assignee_ref: params.assignee_ref,
        kind: params.kind,
        ready: params.ready,
        blocked: params.blocked,
    })
}

fn apply_task_update(
    current: &TaskRecord,
    request: &UpdateTaskRequest,
    updated_at: &str,
) -> TaskRecord {
    let mut next = current.clone();
    if let Some(slug) = request.slug.as_ref() {
        next.slug = slug.clone();
    }
    if let Some(title) = request.title.as_ref() {
        next.title = title.clone();
    }
    if let Some(description) = request.description.as_ref() {
        next.description = description.clone();
    }
    if let Some(source) = request.source.as_ref() {
        next.source = source.clone();
    }
    if let Some(owner) = request.owner.as_ref() {
        next.owner = owner.clone();
    }
    if let Some(created_by) = request.created_by.as_ref() {
        next.created_by = created_by.clone();
    }
    if let Some(status) = request.status {
        next.status = status;
    }
    if let Some(priority) = request.priority {
        next.priority = priority;
    }
    if let Some(complexity) = request.complexity {
        next.complexity = complexity;
    }
    if let Some(position) = request.position {
        next.position = position;
    }
    if let Some(repository_refs) = request.repository_refs.as_ref() {
        next.repository_refs = repository_refs.clone();
    }
    if let Some(label_refs) = request.label_refs.as_ref() {
        next.label_refs = label_refs.clone();
    }
    if let Some(artifact_refs) = request.artifact_refs.as_ref() {
        next.artifact_refs = artifact_refs.clone();
    }
    if let Some(doc_refs) = request.doc_refs.as_ref() {
        next.doc_refs = doc_refs.clone();
    }
    if let Some(file_refs) = request.file_refs.as_ref() {
        next.file_refs = file_refs.clone();
    }
    if let Some(metadata) = request.metadata.as_ref() {
        next.metadata = metadata.clone();
    }
    next.updated_at = updated_at.to_string();
    next.completed_at = match (
        current.status.is_terminal(),
        next.status.is_terminal(),
        current.completed_at.as_ref(),
    ) {
        (false, true, _) => Some(updated_at.to_string()),
        (true, true, Some(existing_completed_at)) => Some(existing_completed_at.clone()),
        (true, true, None) => Some(updated_at.to_string()),
        (_, false, _) => None,
    };
    next
}

fn create_subtask_record(
    task_id: &TaskId,
    request: CreateSubtaskRequest,
    timestamp: &str,
) -> SubtaskRecord {
    let status = request.status.unwrap_or(SubtaskStatus::Planned);
    SubtaskRecord {
        subtask_id: request.id.unwrap_or_else(|| {
            SubtaskId::new(format!("subtask_{}", uuid::Uuid::new_v4().simple()))
        }),
        task_id: task_id.clone(),
        title: request.title,
        description: request.description,
        kind: request.kind,
        status,
        complexity: request.complexity.unwrap_or_default(),
        position: request.position,
        assignee: request.assignee,
        depends_on: request.depends_on,
        parallelizable: request.parallelizable,
        input: request.input,
        result: request.result,
        metadata: request.metadata,
        created_at: timestamp.to_string(),
        updated_at: timestamp.to_string(),
        completed_at: if status.is_terminal() {
            Some(timestamp.to_string())
        } else {
            None
        },
    }
}

fn apply_subtask_update(
    current: &SubtaskRecord,
    request: UpdateSubtaskRequest,
    updated_at: &str,
) -> SubtaskRecord {
    let patch = SubtaskPatch {
        id: current.subtask_id.clone(),
        title: request.title,
        description: request.description,
        kind: request.kind,
        status: request.status,
        complexity: request.complexity,
        position: request.position,
        assignee: request.assignee,
        depends_on: request.depends_on,
        parallelizable: request.parallelizable,
        input: request.input,
        result: request.result,
        metadata: request.metadata,
    };
    let mut next = current.clone();
    if let Some(title) = patch.title.as_ref() {
        next.title = title.clone();
    }
    if let Some(description) = patch.description.as_ref() {
        next.description = description.clone();
    }
    if let Some(kind) = patch.kind {
        next.kind = kind;
    }
    if let Some(status) = patch.status {
        next.status = status;
    }
    if let Some(complexity) = patch.complexity {
        next.complexity = complexity;
    }
    if let Some(position) = patch.position {
        next.position = position;
    }
    if let Some(assignee) = patch.assignee.as_ref() {
        next.assignee = assignee.clone();
    }
    if let Some(depends_on) = patch.depends_on.as_ref() {
        next.depends_on = depends_on.clone();
    }
    if let Some(parallelizable) = patch.parallelizable {
        next.parallelizable = parallelizable;
    }
    if let Some(input) = patch.input.as_ref() {
        next.input = input.clone();
    }
    if let Some(result) = patch.result.as_ref() {
        next.result = result.clone();
    }
    if let Some(metadata) = patch.metadata.as_ref() {
        next.metadata = metadata.clone();
    }
    next.updated_at = updated_at.to_string();
    next.completed_at = match (
        current.status.is_terminal(),
        next.status.is_terminal(),
        current.completed_at.as_ref(),
    ) {
        (false, true, _) => Some(updated_at.to_string()),
        (true, true, Some(existing_completed_at)) => Some(existing_completed_at.clone()),
        (true, true, None) => Some(updated_at.to_string()),
        (_, false, _) => None,
    };
    next
}

fn task_store_error_response(error: TaskStoreError) -> (StatusCode, Json<serde_json::Value>) {
    match error {
        TaskStoreError::TaskNotFound { task_id } => workflow_v2_error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "Task not found",
            Some(serde_json::json!([{ "task_id": task_id }])),
        ),
        TaskStoreError::SubtaskNotFound { subtask_id } => workflow_v2_error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "Subtask not found",
            Some(serde_json::json!([{ "subtask_id": subtask_id }])),
        ),
        TaskStoreError::SubtaskOutsideTask {
            subtask_id,
            task_id,
            actual_task_id,
        } => workflow_v2_error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_subtask_reference",
            "Subtask does not belong to the requested task",
            Some(serde_json::json!([{
                "subtask_id": subtask_id,
                "task_id": task_id,
                "actual_task_id": actual_task_id,
            }])),
        ),
        TaskStoreError::DuplicateSlug { slug } => workflow_v2_error_response(
            StatusCode::CONFLICT,
            "already_exists",
            "Task slug already exists",
            Some(serde_json::json!([{ "slug": slug }])),
        ),
        TaskStoreError::TaskAlreadyExists { task_id } => workflow_v2_error_response(
            StatusCode::CONFLICT,
            "already_exists",
            "Task already exists",
            Some(serde_json::json!([{ "task_id": task_id }])),
        ),
        TaskStoreError::SubtaskAlreadyExists { subtask_id } => workflow_v2_error_response(
            StatusCode::CONFLICT,
            "already_exists",
            "Subtask already exists",
            Some(serde_json::json!([{ "subtask_id": subtask_id }])),
        ),
        TaskStoreError::InvalidCursor { cursor } => workflow_v2_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Invalid cursor",
            Some(serde_json::json!([{ "path": "cursor", "value": cursor }])),
        ),
        TaskStoreError::SelfDependency { subtask_id } => workflow_v2_error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_dependency",
            "A subtask cannot depend on itself",
            Some(serde_json::json!([{ "subtask_id": subtask_id }])),
        ),
        TaskStoreError::DependencyNotFound {
            subtask_id,
            dependency_id,
        } => workflow_v2_error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_dependency",
            "A dependency subtask was not found",
            Some(serde_json::json!([{
                "subtask_id": subtask_id,
                "dependency_id": dependency_id,
            }])),
        ),
        TaskStoreError::DependencyOutsideTask {
            subtask_id,
            task_id,
            dependency_id,
            dependency_task_id,
        } => workflow_v2_error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_dependency",
            "A dependency subtask belongs to a different task",
            Some(serde_json::json!([{
                "subtask_id": subtask_id,
                "task_id": task_id,
                "dependency_id": dependency_id,
                "dependency_task_id": dependency_task_id,
            }])),
        ),
        TaskStoreError::InvalidMetadataShape { field } => workflow_v2_error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "Metadata fields must be JSON objects",
            Some(serde_json::json!([{ "field": field }])),
        ),
        TaskStoreError::ReservedMetadataKey { field, key } => workflow_v2_error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "Metadata contains a reserved key",
            Some(serde_json::json!([{ "field": field, "key": key }])),
        ),
        TaskStoreError::ConnectionLock(error) => workflow_v2_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "task_store_failed",
            "Task store operation failed",
            Some(serde_json::json!([{ "message": error }])),
        ),
        TaskStoreError::InvalidTaskStatus { status } => workflow_v2_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "task_store_failed",
            "Task store operation failed",
            Some(serde_json::json!([{ "message": status }])),
        ),
        TaskStoreError::InvalidSubtaskStatus { status } => workflow_v2_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "task_store_failed",
            "Task store operation failed",
            Some(serde_json::json!([{ "message": status }])),
        ),
        TaskStoreError::InvalidSubtaskKind { kind } => workflow_v2_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "task_store_failed",
            "Task store operation failed",
            Some(serde_json::json!([{ "message": kind }])),
        ),
        TaskStoreError::InvalidPriority { priority } => workflow_v2_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "task_store_failed",
            "Task store operation failed",
            Some(serde_json::json!([{ "message": priority }])),
        ),
        TaskStoreError::InvalidComplexity { complexity } => workflow_v2_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "task_store_failed",
            "Task store operation failed",
            Some(serde_json::json!([{ "message": complexity }])),
        ),
        TaskStoreError::InvalidActorKind { kind } => workflow_v2_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "task_store_failed",
            "Task store operation failed",
            Some(serde_json::json!([{ "message": kind }])),
        ),
        TaskStoreError::InvalidJsonField { message, .. } => workflow_v2_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "task_store_failed",
            "Task store operation failed",
            Some(serde_json::json!([{ "message": message }])),
        ),
        TaskStoreError::InvalidSourceEncoding { reason, .. } => workflow_v2_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "task_store_failed",
            "Task store operation failed",
            Some(serde_json::json!([{ "message": reason }])),
        ),
        TaskStoreError::Sqlite(error) => workflow_v2_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "task_store_failed",
            "Task store operation failed",
            Some(serde_json::json!([{ "message": error.to_string() }])),
        ),
        TaskStoreError::Json(error) => workflow_v2_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "task_store_failed",
            "Task store operation failed",
            Some(serde_json::json!([{ "message": error.to_string() }])),
        ),
    }
}

/// GET /api/v1/tasks — List durable tasks.
pub async fn list_tasks_v1(
    State(state): State<Arc<AppState>>,
    query: Result<Query<TaskListQueryParams>, QueryRejection>,
) -> impl IntoResponse {
    let Query(params) = match query {
        Ok(query) => query,
        Err(rejection) => return task_query_rejection(rejection),
    };
    let query = match task_list_query_from_params(params) {
        Ok(query) => query,
        Err(response) => return response,
    };

    match state.kernel.workflow_stores.task.list(&query) {
        Ok(page) => (
            StatusCode::OK,
            Json(serde_json::json!(TaskListResponse {
                items: page.items.iter().map(task_summary_from_record).collect(),
                next_cursor: page.next_cursor,
            })),
        ),
        Err(error) => task_store_error_response(error),
    }
}

/// POST /api/v1/tasks — Create a durable task.
pub async fn create_task_v1(
    State(state): State<Arc<AppState>>,
    payload: Result<Json<CreateTaskRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };
    let timestamp = now_timestamp();
    let task_id = request
        .id
        .unwrap_or_else(|| TaskId::new(format!("task_{}", uuid::Uuid::new_v4().simple())));
    let record = TaskRecord {
        task_id,
        slug: request.slug,
        title: request.title,
        description: request.description,
        status: request.status,
        priority: request.priority,
        complexity: request.complexity,
        position: request.position,
        source: request.source,
        owner: request.owner,
        created_by: request.created_by,
        repository_refs: request.repository_refs,
        label_refs: request.label_refs,
        artifact_refs: request.artifact_refs,
        doc_refs: request.doc_refs,
        file_refs: request.file_refs,
        metadata: request.metadata,
        created_at: timestamp.clone(),
        updated_at: timestamp.clone(),
        completed_at: if request.status.is_terminal() {
            Some(timestamp.clone())
        } else {
            None
        },
    };

    match state.kernel.workflow_stores.task.create(&record) {
        Ok(record) => (StatusCode::CREATED, Json(serde_json::json!(record))),
        Err(error) => task_store_error_response(error),
    }
}

/// GET /api/v1/tasks/{id} — Load one durable task.
pub async fn get_task_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state
        .kernel
        .workflow_stores
        .task
        .find_by_id(&TaskId::new(id.clone()))
    {
        Ok(Some(task)) => (StatusCode::OK, Json(serde_json::json!(task))),
        Ok(None) => task_store_error_response(TaskStoreError::TaskNotFound { task_id: id }),
        Err(error) => task_store_error_response(error),
    }
}

/// PUT /api/v1/tasks/{id} — Update one durable task.
pub async fn update_task_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<UpdateTaskRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };
    let task_id = TaskId::new(id.clone());
    let current = match state.kernel.workflow_stores.task.find_by_id(&task_id) {
        Ok(Some(task)) => task,
        Ok(None) => {
            return task_store_error_response(TaskStoreError::TaskNotFound { task_id: id });
        }
        Err(error) => return task_store_error_response(error),
    };
    let next = apply_task_update(&current, &request, &now_timestamp());

    match state.kernel.workflow_stores.task.update(&next) {
        Ok(task) => (StatusCode::OK, Json(serde_json::json!(task))),
        Err(error) => task_store_error_response(error),
    }
}

/// DELETE /api/v1/tasks/{id} — Delete one durable task.
pub async fn delete_task_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state
        .kernel
        .workflow_stores
        .task
        .delete(&TaskId::new(id.clone()))
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => task_store_error_response(error).into_response(),
    }
}

/// GET /api/v1/tasks/{id}/subtasks — List subtasks for one task.
pub async fn list_task_subtasks_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    query: Result<Query<SubtaskListQueryParams>, QueryRejection>,
) -> impl IntoResponse {
    let Query(params) = match query {
        Ok(query) => query,
        Err(rejection) => return task_query_rejection(rejection),
    };
    let task_id = TaskId::new(id);
    let query = match subtask_list_query_from_params(params, Some(task_id.clone())) {
        Ok(query) => query,
        Err(response) => return response,
    };

    match state
        .kernel
        .workflow_stores
        .subtask
        .list_for_task(&task_id, &query)
    {
        Ok(page) => (
            StatusCode::OK,
            Json(serde_json::json!(SubtaskListResponse {
                items: page.items.iter().map(subtask_summary_from_record).collect(),
                next_cursor: page.next_cursor,
            })),
        ),
        Err(error) => task_store_error_response(error),
    }
}

/// POST /api/v1/tasks/{id}/subtasks — Create a durable subtask under one task.
pub async fn create_task_subtask_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<CreateSubtaskRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };
    let task_id = TaskId::new(id);
    let timestamp = now_timestamp();
    let record = create_subtask_record(&task_id, request, &timestamp);

    match state.kernel.workflow_stores.subtask.create(&record) {
        Ok(record) => (StatusCode::CREATED, Json(serde_json::json!(record))),
        Err(error) => task_store_error_response(error),
    }
}

/// GET /api/v1/subtasks — List durable subtasks globally.
pub async fn list_subtasks_v1(
    State(state): State<Arc<AppState>>,
    query: Result<Query<SubtaskListQueryParams>, QueryRejection>,
) -> impl IntoResponse {
    let Query(params) = match query {
        Ok(query) => query,
        Err(rejection) => return task_query_rejection(rejection),
    };
    let query = match subtask_list_query_from_params(params, None) {
        Ok(query) => query,
        Err(response) => return response,
    };

    match state.kernel.workflow_stores.subtask.list(&query) {
        Ok(page) => (
            StatusCode::OK,
            Json(serde_json::json!(SubtaskListResponse {
                items: page.items.iter().map(subtask_summary_from_record).collect(),
                next_cursor: page.next_cursor,
            })),
        ),
        Err(error) => task_store_error_response(error),
    }
}

/// GET /api/v1/subtasks/{id} — Load one durable subtask.
pub async fn get_subtask_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state
        .kernel
        .workflow_stores
        .subtask
        .find_by_id(&SubtaskId::new(id.clone()))
    {
        Ok(Some(subtask)) => (StatusCode::OK, Json(serde_json::json!(subtask))),
        Ok(None) => task_store_error_response(TaskStoreError::SubtaskNotFound { subtask_id: id }),
        Err(error) => task_store_error_response(error),
    }
}

/// PUT /api/v1/subtasks/{id} — Update one durable subtask.
pub async fn update_subtask_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<UpdateSubtaskRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };
    let subtask_id = SubtaskId::new(id.clone());
    let current = match state.kernel.workflow_stores.subtask.find_by_id(&subtask_id) {
        Ok(Some(subtask)) => subtask,
        Ok(None) => {
            return task_store_error_response(TaskStoreError::SubtaskNotFound { subtask_id: id });
        }
        Err(error) => return task_store_error_response(error),
    };
    let next = apply_subtask_update(&current, request, &now_timestamp());

    match state.kernel.workflow_stores.subtask.update(&next) {
        Ok(subtask) => (StatusCode::OK, Json(serde_json::json!(subtask))),
        Err(error) => task_store_error_response(error),
    }
}

/// DELETE /api/v1/subtasks/{id} — Delete one durable subtask.
pub async fn delete_subtask_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state
        .kernel
        .workflow_stores
        .subtask
        .delete(&SubtaskId::new(id.clone()))
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => task_store_error_response(error).into_response(),
    }
}

/// POST /api/v1/tasks/{id}/replan — Atomically change one task's subtask plan.
pub async fn replan_task_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<TaskReplanRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };
    let task_id = TaskId::new(id.clone());

    match state.kernel.workflow_stores.task.replan(&task_id, &request) {
        Ok(effects) => (
            StatusCode::OK,
            Json(serde_json::json!(TaskReplanAcceptedResponse {
                accepted: true,
                resource_id: task_id,
                status: "accepted".to_string(),
                effects,
            })),
        ),
        Err(error) => task_store_error_response(error),
    }
}

/// GET /api/v1/tasks/{id}/artifacts — Project task-linked artifacts.
pub async fn get_task_artifacts_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state
        .kernel
        .workflow_stores
        .task
        .find_by_id(&TaskId::new(id.clone()))
    {
        Ok(Some(task)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "items": task.artifact_refs,
                "next_cursor": serde_json::Value::Null,
            })),
        ),
        Ok(None) => task_store_error_response(TaskStoreError::TaskNotFound { task_id: id }),
        Err(error) => task_store_error_response(error),
    }
}

/// GET /api/v1/tasks/{id}/docs — Project task-linked docs.
pub async fn get_task_docs_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state
        .kernel
        .workflow_stores
        .task
        .find_by_id(&TaskId::new(id.clone()))
    {
        Ok(Some(task)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "items": task.doc_refs,
                "next_cursor": serde_json::Value::Null,
            })),
        ),
        Ok(None) => task_store_error_response(TaskStoreError::TaskNotFound { task_id: id }),
        Err(error) => task_store_error_response(error),
    }
}

/// GET /api/v1/tasks/{id}/files — Project task-linked files.
pub async fn get_task_files_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state
        .kernel
        .workflow_stores
        .task
        .find_by_id(&TaskId::new(id.clone()))
    {
        Ok(Some(task)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "items": task.file_refs,
                "next_cursor": serde_json::Value::Null,
            })),
        ),
        Ok(None) => task_store_error_response(TaskStoreError::TaskNotFound { task_id: id }),
        Err(error) => task_store_error_response(error),
    }
}

fn agent_validation_error_response(
    issues: &[AgentValidationIssue],
) -> (StatusCode, Json<serde_json::Value>) {
    let details = serde_json::to_value(issues).unwrap_or_else(|_| serde_json::json!([]));
    agent_error_response(
        StatusCode::BAD_REQUEST,
        "validation_error",
        "agent definition is invalid",
        Some(details),
    )
}

fn agent_compile_error_response(
    error: &AgentCompileError,
) -> (StatusCode, Json<serde_json::Value>) {
    agent_error_response(
        StatusCode::BAD_REQUEST,
        "validation_error",
        "agent definition is invalid",
        Some(serde_json::json!([{
            "code": "compile_error",
            "message": error.to_string(),
        }])),
    )
}

fn agent_definition_store(state: &AppState) -> AgentDefinitionStore {
    AgentDefinitionStore::new(&state.kernel.config.home_dir)
}

fn agent_definition_id_is_safe(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn ensure_safe_agent_definition_id(id: &str) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if agent_definition_id_is_safe(id) {
        Ok(())
    } else {
        Err(agent_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Agent definition IDs may only contain ASCII letters, digits, `.`, `_`, or `-`",
            Some(serde_json::json!([{
                "path": "id",
                "value": id,
            }])),
        ))
    }
}

fn runtime_definition_id(entry: &openfang_types::agent::AgentEntry) -> Option<&str> {
    entry
        .manifest
        .metadata
        .get("compozy")
        .and_then(|value| value.get("definition_id"))
        .and_then(serde_json::Value::as_str)
}

fn agent_validation_context(
    state: &AppState,
    store: &AgentDefinitionStore,
) -> Result<AgentValidationContext, (StatusCode, Json<serde_json::Value>)> {
    let stored_definitions = store.list().map_err(|error| {
        agent_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "definition_load_failed",
            "Failed to load agent definitions",
            Some(serde_json::json!([{
                "message": error,
            }])),
        )
    })?;

    let mut known_agents = BTreeSet::new();
    for definition in stored_definitions {
        known_agents.insert(definition.definition.id);
        known_agents.insert(definition.definition.name);
    }
    for entry in state.kernel.registry.list() {
        known_agents.insert(entry.id.to_string());
        known_agents.insert(entry.name);
    }

    let known_skills = state
        .kernel
        .skill_registry
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .skill_names()
        .into_iter()
        .collect::<BTreeSet<_>>();

    Ok(AgentValidationContext {
        known_skills,
        known_agents,
        ..AgentValidationContext::default()
    })
}

fn collect_agent_validation_issues(
    definition: &AgentDefinition,
    context: &AgentValidationContext,
) -> Vec<AgentValidationIssue> {
    let mut issues = stage1_schema_validate(definition);
    issues.extend(stage2_reference_validate(definition, context));
    issues.extend(stage3_semantic_validate(definition));
    issues
}

fn agent_definition_is_valid(issues: &[AgentValidationIssue], strict: bool) -> bool {
    if strict {
        issues.is_empty()
    } else {
        !issues.iter().any(|issue| issue.severity.is_error())
    }
}

enum AgentPreparationFailure {
    Validation(Vec<AgentValidationIssue>),
    Compile(AgentCompileError),
}

fn prepare_agent_definition(
    definition: AgentDefinition,
    context: &AgentValidationContext,
) -> Result<(AgentDefinition, CompiledAgentDefinition), AgentPreparationFailure> {
    let issues = collect_agent_validation_issues(&definition, context);
    if issues.iter().any(|issue| issue.severity.is_error()) {
        return Err(AgentPreparationFailure::Validation(issues));
    }

    let normalized = stage4_normalize(definition);
    let compiled =
        compile_agent_ir(normalized.clone()).map_err(AgentPreparationFailure::Compile)?;
    Ok((normalized, compiled))
}

fn agent_compiled_payload(compiled: CompiledAgentDefinition) -> AgentCompiledPayload {
    AgentCompiledPayload {
        agent_manifest: compiled.agent_manifest,
        provider_binding: compiled.provider_binding,
        product_metadata: compiled.product_metadata,
    }
}

fn load_agent_definition_resource(
    state: &AppState,
    definition_id: &str,
) -> Result<Option<AgentResponse>, (StatusCode, Json<serde_json::Value>)> {
    agent_definition_store(state)
        .load(definition_id)
        .map_err(|error| {
            agent_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "definition_load_failed",
                "Failed to load agent definition",
                Some(serde_json::json!([{
                    "message": error,
                }])),
            )
        })
}

fn find_runtime_agent_for_definition(
    state: &AppState,
    definition_id: &str,
) -> Option<openfang_types::agent::AgentEntry> {
    state
        .kernel
        .registry
        .list()
        .into_iter()
        .filter(|entry| runtime_definition_id(entry) == Some(definition_id))
        .max_by_key(|entry| entry.created_at)
}

fn stable_runtime_agent_id(definition_id: &str) -> AgentId {
    AgentId::from_string(&format!("compozy-definition:{definition_id}"))
}

fn runtime_store_error(
    code: &str,
    message: &str,
    error: impl std::fmt::Display,
) -> (StatusCode, Json<serde_json::Value>) {
    agent_error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        code,
        message,
        Some(serde_json::json!([{
            "message": error.to_string(),
        }])),
    )
}

fn runtime_not_found_response() -> (StatusCode, Json<serde_json::Value>) {
    agent_error_response(
        StatusCode::NOT_FOUND,
        "not_found",
        "Agent runtime not found",
        None,
    )
}

fn parse_session_id_path(
    session_id: &str,
) -> Result<SessionId, (StatusCode, Json<serde_json::Value>)> {
    session_id
        .parse::<uuid::Uuid>()
        .map(SessionId)
        .map_err(|_| {
            agent_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Invalid session ID",
                Some(serde_json::json!([{
                    "path": "session_id",
                    "value": session_id,
                }])),
            )
        })
}

fn session_list_item(record: &AgentSessionRecord) -> SessionListItem {
    let session_id = record.session_id.to_string();
    SessionListItem {
        id: session_id.clone(),
        session_id,
        label: record.label.clone(),
        active: record.active,
        message_count: record.message_count,
        dispatch_count: record.dispatch_count,
        created_at: record.created_at.clone(),
        updated_at: record.updated_at.clone(),
        compacted_at: record.compacted_at.clone(),
    }
}

fn session_detail(
    record: &AgentSessionRecord,
    messages: Option<Vec<serde_json::Value>>,
) -> SessionDetail {
    let session_id = record.session_id.to_string();
    SessionDetail {
        id: session_id.clone(),
        session_id,
        label: record.label.clone(),
        active: record.active,
        message_count: record.message_count,
        dispatch_count: record.dispatch_count,
        created_at: record.created_at.clone(),
        updated_at: record.updated_at.clone(),
        compacted_at: record.compacted_at.clone(),
        messages,
    }
}

fn runtime_response(
    definition_id: &str,
    entry: Option<&openfang_types::agent::AgentEntry>,
    record: Option<&AgentRuntimeRecord>,
    active_sessions: u32,
) -> AgentRuntimeResponse {
    let loaded = record
        .map(|runtime| runtime.loaded)
        .unwrap_or(entry.is_some());
    let state = record
        .map(|runtime| runtime.state)
        .or_else(|| entry.map(|runtime_entry| runtime_entry.state))
        .unwrap_or(AgentState::Created);
    let mode = record
        .map(|runtime| runtime.mode)
        .or_else(|| entry.map(|runtime_entry| runtime_entry.mode))
        .unwrap_or_default();
    let healthy = record.map(|runtime| runtime.healthy).unwrap_or_else(|| {
        entry.is_some_and(|runtime_entry| {
            !matches!(
                runtime_entry.state,
                AgentState::Crashed | AgentState::Terminated
            )
        })
    });
    let active_session_id = record
        .and_then(|runtime| runtime.active_session_id.map(|value| value.to_string()))
        .or_else(|| entry.map(|runtime_entry| runtime_entry.session_id.to_string()));
    let active_dispatches = record.map(|runtime| runtime.active_dispatches).unwrap_or(0);
    let last_active_at = record
        .and_then(|runtime| runtime.last_active_at.clone())
        .or_else(|| entry.map(|runtime_entry| runtime_entry.last_active.to_rfc3339()));

    AgentRuntimeResponse {
        agent_id: definition_id.to_owned(),
        loaded,
        state,
        mode,
        healthy,
        active_session_id,
        active_sessions,
        active_dispatches,
        last_active_at,
    }
}

fn session_messages(
    state: &AppState,
    session_id: SessionId,
) -> Result<Vec<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let messages = state
        .kernel
        .runtime_stores
        .agent_message
        .list_messages_for_session(session_id)
        .map_err(|error| {
            runtime_store_error(
                "session_load_failed",
                "Failed to load agent session messages",
                error,
            )
        })?;

    let mut payloads = Vec::with_capacity(messages.len());
    for message in messages {
        payloads.push(serde_json::json!({
            "message_id": message.message_id,
            "direction": message.direction,
            "payload": serde_json::from_str::<serde_json::Value>(&message.payload_json).map_err(|error| {
                agent_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "session_load_failed",
                    "Failed to decode stored agent session message",
                    Some(serde_json::json!([{
                        "message": error.to_string(),
                        "session_id": session_id.to_string(),
                    }])),
                )
            })?,
            "status": message.status,
            "created_at": message.created_at,
            "completed_at": message.completed_at,
        }));
    }

    Ok(payloads)
}

fn agent_definition_not_found_response() -> (StatusCode, Json<serde_json::Value>) {
    agent_error_response(
        StatusCode::NOT_FOUND,
        "not_found",
        "Agent definition not found",
        None,
    )
}

fn agent_session_not_found_response() -> (StatusCode, Json<serde_json::Value>) {
    agent_error_response(
        StatusCode::NOT_FOUND,
        "not_found",
        "Agent session not found",
        None,
    )
}

fn runtime_not_started_response() -> (StatusCode, Json<serde_json::Value>) {
    agent_error_response(
        StatusCode::CONFLICT,
        "runtime_not_started",
        "Agent runtime is not started",
        None,
    )
}

fn resolve_message_text(
    request: &MessageRequest,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    if request.input.items.is_empty() {
        return Err(agent_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Message input must include at least one item",
            Some(serde_json::json!([{
                "path": "input.items",
            }])),
        ));
    }

    let mut parts = Vec::with_capacity(request.input.items.len());
    for (index, item) in request.input.items.iter().enumerate() {
        if item.item_type != "text" {
            return Err(agent_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Only text input items are currently supported",
                Some(serde_json::json!([{
                    "path": format!("input.items[{index}].type"),
                    "value": item.item_type,
                }])),
            ));
        }

        let Some(text) = item.text.as_deref() else {
            return Err(agent_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Text input items must include a text field",
                Some(serde_json::json!([{
                    "path": format!("input.items[{index}].text"),
                }])),
            ));
        };
        if !text.trim().is_empty() {
            parts.push(text.trim().to_owned());
        }
    }

    if parts.is_empty() {
        return Err(agent_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Message input must include non-empty text",
            Some(serde_json::json!([{
                "path": "input.items",
            }])),
        ));
    }

    Ok(parts.join("\n"))
}

fn new_message_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn load_session_record_for_agent(
    state: &AppState,
    agent_id: AgentId,
    session_id: SessionId,
) -> Result<AgentSessionRecord, (StatusCode, Json<serde_json::Value>)> {
    let record = state
        .kernel
        .runtime_stores
        .agent_session
        .get_agent_session(session_id)
        .map_err(|error| {
            runtime_store_error("session_load_failed", "Failed to load agent session", error)
        })?
        .ok_or_else(agent_session_not_found_response)?;

    if record.agent_id != agent_id {
        return Err(agent_session_not_found_response());
    }

    Ok(record)
}

fn require_running_runtime_for_definition(
    state: &AppState,
    definition_id: &str,
) -> Result<openfang_types::agent::AgentEntry, (StatusCode, Json<serde_json::Value>)> {
    let Some(entry) = find_runtime_agent_for_definition(state, definition_id) else {
        return match load_agent_definition_resource(state, definition_id)? {
            Some(_) => Err(runtime_not_started_response()),
            None => Err(agent_definition_not_found_response()),
        };
    };

    let runtime_record = state
        .kernel
        .runtime_stores
        .agent_runtime
        .get_agent_runtime(entry.id)
        .map_err(|error| {
            runtime_store_error(
                "runtime_status_failed",
                "Failed to load agent runtime status",
                error,
            )
        })?;

    let loaded = runtime_record
        .as_ref()
        .map(|record| record.loaded)
        .unwrap_or(true);
    let runtime_state = runtime_record
        .as_ref()
        .map(|record| record.state)
        .unwrap_or(entry.state);
    if !loaded || runtime_state != AgentState::Running {
        return Err(runtime_not_started_response());
    }

    Ok(entry)
}

fn compile_persisted_agent_definition(
    state: &AppState,
    definition_id: &str,
) -> Result<(AgentResponse, CompiledAgentDefinition), (StatusCode, Json<serde_json::Value>)> {
    let Some(resource) = load_agent_definition_resource(state, definition_id)? else {
        return Err(agent_definition_not_found_response());
    };
    let store = agent_definition_store(state);
    let context = agent_validation_context(state, &store)?;
    let (_, compiled) = match prepare_agent_definition(resource.definition.clone(), &context) {
        Ok(prepared) => prepared,
        Err(AgentPreparationFailure::Validation(issues)) => {
            return Err(agent_validation_error_response(&issues));
        }
        Err(AgentPreparationFailure::Compile(error)) => {
            return Err(agent_compile_error_response(&error));
        }
    };

    Ok((resource, compiled))
}

fn resolve_dry_run_tool_names(compiled: &CompiledAgentDefinition) -> Vec<String> {
    let declared_tools = &compiled.agent_manifest.capabilities.tools;
    let tools_unrestricted =
        declared_tools.is_empty() || declared_tools.iter().any(|tool| tool == "*");

    builtin_tool_definitions()
        .into_iter()
        .filter(|tool| tools_unrestricted || declared_tools.iter().any(|item| item == &tool.name))
        .map(|tool| tool.name)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn estimate_message_effects(
    state: &AppState,
    compiled: &CompiledAgentDefinition,
    session_id: SessionId,
    input_text: &str,
) -> MessageDryRunEffects {
    let mut messages = state
        .kernel
        .memory
        .get_session(session_id)
        .ok()
        .flatten()
        .map(|session| session.messages)
        .unwrap_or_default();
    messages.push(openfang_types::message::Message::user(input_text));

    let estimated_tokens = openfang_runtime::compactor::estimate_token_count(
        &messages,
        Some(&compiled.agent_manifest.model.system_prompt),
        None,
    );
    let estimated_tokens_u32 = u32::try_from(estimated_tokens).unwrap_or(u32::MAX);
    let estimated_cost = MeteringEngine::estimate_cost_with_catalog(
        &state
            .kernel
            .model_catalog
            .read()
            .unwrap_or_else(|error| error.into_inner()),
        &compiled.agent_manifest.model.model,
        u64::from(estimated_tokens_u32),
        u64::from(estimated_tokens_u32 / 2),
    );

    MessageDryRunEffects {
        message_submit: true,
        estimated_tokens: estimated_tokens_u32,
        estimated_cost,
    }
}

fn stream_event_to_sse(
    stream_event: &StreamEvent,
) -> Result<axum::response::sse::Event, std::convert::Infallible> {
    Ok(
        axum::response::sse::Event::default()
            .event(&stream_event.event)
            .json_data(&stream_event.data)
            .unwrap_or_else(|_| {
                axum::response::sse::Event::default()
                    .event("error")
                    .data(
                        "{\"error\":{\"code\":\"stream_encode_failed\",\"message\":\"Failed to encode SSE payload\",\"details\":[]}}",
                    )
            }),
    )
}

fn single_sse_event_response(
    _status: StatusCode,
    stream_event: StreamEvent,
) -> axum::response::Response {
    use axum::response::sse::Sse;
    use futures::stream;

    let stream = stream::once(async move { stream_event_to_sse(&stream_event) });
    Sse::new(stream).into_response()
}

fn ensure_runtime_agent_present(
    state: &AppState,
    definition_id: &str,
) -> Result<AgentId, (StatusCode, Json<serde_json::Value>)> {
    if let Some(entry) = find_runtime_agent_for_definition(state, definition_id) {
        return Ok(entry.id);
    }

    let Some(resource) = load_agent_definition_resource(state, definition_id)? else {
        return Err(runtime_not_found_response());
    };

    let store = agent_definition_store(state);
    let context = agent_validation_context(state, &store)?;
    let (_, compiled) = match prepare_agent_definition(resource.definition, &context) {
        Ok(prepared) => prepared,
        Err(AgentPreparationFailure::Validation(issues)) => {
            return Err(agent_validation_error_response(&issues));
        }
        Err(AgentPreparationFailure::Compile(error)) => {
            return Err(agent_compile_error_response(&error));
        }
    };

    let stable_id = stable_runtime_agent_id(definition_id);
    match state
        .kernel
        .spawn_agent_with_parent(compiled.agent_manifest, None, Some(stable_id))
    {
        Ok(agent_id) => Ok(agent_id),
        Err(error) => find_runtime_agent_for_definition(state, definition_id)
            .map(|entry| entry.id)
            .ok_or_else(|| {
                agent_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "runtime_start_failed",
                    "Failed to start agent runtime",
                    Some(serde_json::json!([{
                        "message": error.to_string(),
                    }])),
                )
            }),
    }
}

fn agent_runtime_status_for_definition(
    state: &AppState,
    definition_id: &str,
) -> Result<AgentRuntimeStatus, (StatusCode, Json<serde_json::Value>)> {
    let matching_agents = state
        .kernel
        .registry
        .list()
        .into_iter()
        .filter(|entry| runtime_definition_id(entry) == Some(definition_id))
        .collect::<Vec<_>>();

    if matching_agents.is_empty() {
        return Ok(AgentRuntimeStatus {
            loaded: false,
            healthy: false,
            active_sessions: 0,
            active_dispatches: 0,
        });
    }

    let runtime_records = state
        .kernel
        .runtime_stores
        .agent_runtime
        .list_agent_runtimes()
        .map_err(|error| {
            agent_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "runtime_status_failed",
                "Failed to load agent runtime status",
                Some(serde_json::json!([{
                    "message": error.to_string(),
                }])),
            )
        })?
        .into_iter()
        .map(|record| (record.agent_id, record))
        .collect::<HashMap<_, _>>();

    let mut loaded = false;
    let mut healthy = true;
    let mut active_sessions = 0u32;
    let mut active_dispatches = 0u32;

    for entry in matching_agents {
        let runtime = runtime_records.get(&entry.id);
        loaded |= runtime
            .map(|record| record.loaded)
            .unwrap_or(matches!(entry.state, AgentState::Running));
        healthy &= runtime.map(|record| record.healthy).unwrap_or(!matches!(
            entry.state,
            AgentState::Crashed | AgentState::Terminated
        ));

        let session_count = state
            .kernel
            .runtime_stores
            .agent_session
            .list_agent_sessions_for_agent(entry.id)
            .map_err(|error| {
                agent_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "runtime_status_failed",
                    "Failed to load agent runtime status",
                    Some(serde_json::json!([{
                        "message": error.to_string(),
                    }])),
                )
            })?
            .len();
        active_sessions =
            active_sessions.saturating_add(u32::try_from(session_count).unwrap_or(u32::MAX));
        active_dispatches = active_dispatches
            .saturating_add(runtime.map(|record| record.active_dispatches).unwrap_or(0));
    }

    Ok(AgentRuntimeStatus {
        loaded,
        healthy,
        active_sessions,
        active_dispatches,
    })
}

fn agent_list_item(resource: &AgentResponse, runtime_status: AgentRuntimeStatus) -> AgentListItem {
    AgentListItem {
        id: resource.definition.id.clone(),
        name: resource.definition.name.clone(),
        description: resource.definition.description.clone(),
        enabled: resource.definition.enabled.unwrap_or(true),
        group: resource.definition.group.clone(),
        tags: resource.definition.tags.clone(),
        provider: AgentProviderSummary {
            driver: resource.definition.provider.driver.clone(),
            model: resource.definition.provider.model.clone(),
            profile: resource.definition.provider.profile.clone(),
        },
        origin: resource.origin.clone(),
        forked_from: resource.forked_from.clone(),
        runtime_status,
        updated_at: resource.updated_at.clone(),
    }
}

/// GET /api/v1/agents — List file-backed agent definitions.
pub async fn list_agents(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let store = agent_definition_store(&state);
    let definitions = match store.list() {
        Ok(definitions) => definitions,
        Err(error) => {
            return agent_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "definition_load_failed",
                "Failed to load agent definitions",
                Some(serde_json::json!([{
                    "message": error,
                }])),
            );
        }
    };

    let mut items = Vec::with_capacity(definitions.len());
    for definition in &definitions {
        let runtime_status =
            match agent_runtime_status_for_definition(&state, &definition.definition.id) {
                Ok(runtime_status) => runtime_status,
                Err(response) => return response,
            };
        items.push(agent_list_item(definition, runtime_status));
    }

    (
        StatusCode::OK,
        Json(serde_json::json!(AgentListResponse {
            items,
            next_cursor: None,
        })),
    )
}

/// POST /api/v1/agents — Create and persist an agent definition.
pub async fn create_agent(
    State(state): State<Arc<AppState>>,
    payload: Result<Json<CreateAgentRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return agent_json_rejection(rejection),
    };
    let definition = request.definition;

    if let Err(response) = ensure_safe_agent_definition_id(&definition.id) {
        return response;
    }

    let store = agent_definition_store(&state);
    match store.load(&definition.id) {
        Ok(Some(_)) => {
            return agent_error_response(
                StatusCode::CONFLICT,
                "already_exists",
                "Agent definition already exists",
                Some(serde_json::json!([{
                    "path": "id",
                    "value": definition.id,
                }])),
            );
        }
        Ok(None) => {}
        Err(error) => {
            return agent_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "definition_load_failed",
                "Failed to load agent definitions",
                Some(serde_json::json!([{
                    "message": error,
                }])),
            );
        }
    }

    let context = match agent_validation_context(&state, &store) {
        Ok(context) => context,
        Err(response) => return response,
    };
    let (normalized, _compiled) = match prepare_agent_definition(definition, &context) {
        Ok(prepared) => prepared,
        Err(AgentPreparationFailure::Validation(issues)) => {
            return agent_validation_error_response(&issues);
        }
        Err(AgentPreparationFailure::Compile(error)) => {
            return agent_compile_error_response(&error);
        }
    };

    let timestamp = chrono::Utc::now().to_rfc3339();
    let resource = AgentResponse {
        definition: normalized,
        origin: AgentOrigin::user(),
        forked_from: None,
        created_at: timestamp.clone(),
        updated_at: timestamp,
    };

    if let Err(error) = store.persist(&resource) {
        return agent_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "definition_persist_failed",
            "Failed to persist agent definition",
            Some(serde_json::json!([{
                "message": error,
            }])),
        );
    }

    (StatusCode::CREATED, Json(serde_json::json!(resource)))
}

/// GET /api/v1/agents/{id} — Load one file-backed agent definition.
pub async fn get_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_agent_definition_id(&id) {
        return response;
    }

    let store = agent_definition_store(&state);
    match store.load(&id) {
        Ok(Some(definition)) => (StatusCode::OK, Json(serde_json::json!(definition))),
        Ok(None) => agent_error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "Agent definition not found",
            None,
        ),
        Err(error) => agent_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "definition_load_failed",
            "Failed to load agent definition",
            Some(serde_json::json!([{
                "message": error,
            }])),
        ),
    }
}

/// PUT /api/v1/agents/{id} — Replace one file-backed agent definition.
pub async fn update_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<UpdateAgentRequest>, JsonRejection>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_agent_definition_id(&id) {
        return response;
    }

    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return agent_json_rejection(rejection),
    };
    let definition = request.definition;

    if definition.id != id {
        return agent_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Path ID and body ID must match",
            Some(serde_json::json!([{
                "path": "id",
                "expected": id,
                "actual": definition.id,
            }])),
        );
    }

    let store = agent_definition_store(&state);
    let existing = match store.load(&id) {
        Ok(Some(definition)) => definition,
        Ok(None) => {
            return agent_error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                "Agent definition not found",
                None,
            );
        }
        Err(error) => {
            return agent_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "definition_load_failed",
                "Failed to load agent definition",
                Some(serde_json::json!([{
                    "message": error,
                }])),
            );
        }
    };

    let context = match agent_validation_context(&state, &store) {
        Ok(context) => context,
        Err(response) => return response,
    };
    let (normalized, _compiled) = match prepare_agent_definition(definition, &context) {
        Ok(prepared) => prepared,
        Err(AgentPreparationFailure::Validation(issues)) => {
            return agent_validation_error_response(&issues);
        }
        Err(AgentPreparationFailure::Compile(error)) => {
            return agent_compile_error_response(&error);
        }
    };

    let resource = AgentResponse {
        definition: normalized,
        origin: existing.origin,
        forked_from: existing.forked_from,
        created_at: existing.created_at,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };

    if let Err(error) = store.persist(&resource) {
        return agent_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "definition_persist_failed",
            "Failed to persist agent definition",
            Some(serde_json::json!([{
                "message": error,
            }])),
        );
    }

    (StatusCode::OK, Json(serde_json::json!(resource)))
}

/// DELETE /api/v1/agents/{id} — Delete one file-backed agent definition.
pub async fn delete_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_agent_definition_id(&id) {
        return response.into_response();
    }

    let store = agent_definition_store(&state);
    match store.delete(&id) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => agent_error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "Agent definition not found",
            None,
        )
        .into_response(),
        Err(error) => agent_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "definition_delete_failed",
            "Failed to delete agent definition",
            Some(serde_json::json!([{
                "message": error,
            }])),
        )
        .into_response(),
    }
}

/// POST /api/v1/agents/validate — Validate an agent definition.
pub async fn validate_agent_definition(
    State(state): State<Arc<AppState>>,
    payload: Result<Json<AgentValidateRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return agent_json_rejection(rejection),
    };
    let store = agent_definition_store(&state);
    let context = match agent_validation_context(&state, &store) {
        Ok(context) => context,
        Err(response) => return response,
    };

    let issues = collect_agent_validation_issues(&request.definition, &context);
    let normalized = if issues.iter().any(|issue| issue.severity.is_error()) {
        None
    } else {
        Some(stage4_normalize(request.definition))
    };
    let valid = agent_definition_is_valid(&issues, request.strict.unwrap_or(false));

    (
        StatusCode::OK,
        Json(serde_json::json!(AgentValidateResponse {
            valid,
            issues,
            normalized,
        })),
    )
}

/// POST /api/v1/agents/compile — Validate and compile an agent definition.
pub async fn compile_agent_definition(
    State(state): State<Arc<AppState>>,
    payload: Result<Json<AgentCompileRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return agent_json_rejection(rejection),
    };
    let store = agent_definition_store(&state);
    let context = match agent_validation_context(&state, &store) {
        Ok(context) => context,
        Err(response) => return response,
    };
    let (normalized, compiled) = match prepare_agent_definition(request.definition, &context) {
        Ok(prepared) => prepared,
        Err(AgentPreparationFailure::Validation(issues)) => {
            return agent_validation_error_response(&issues);
        }
        Err(AgentPreparationFailure::Compile(error)) => {
            return agent_compile_error_response(&error);
        }
    };

    (
        StatusCode::OK,
        Json(serde_json::json!(AgentCompileResponse {
            definition_id: normalized.id.clone(),
            normalized,
            compiled: agent_compiled_payload(compiled),
        })),
    )
}

/// GET /api/v1/agents/{id}/compiled — Compile one stored agent definition.
pub async fn get_agent_compiled(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_agent_definition_id(&id) {
        return response;
    }

    let store = agent_definition_store(&state);
    let definition = match store.load(&id) {
        Ok(Some(resource)) => resource.definition,
        Ok(None) => {
            return agent_error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                "Agent definition not found",
                None,
            );
        }
        Err(error) => {
            return agent_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "definition_load_failed",
                "Failed to load agent definition",
                Some(serde_json::json!([{
                    "message": error,
                }])),
            );
        }
    };
    let context = match agent_validation_context(&state, &store) {
        Ok(context) => context,
        Err(response) => return response,
    };
    let (normalized, compiled) = match prepare_agent_definition(definition, &context) {
        Ok(prepared) => prepared,
        Err(AgentPreparationFailure::Validation(issues)) => {
            return agent_validation_error_response(&issues);
        }
        Err(AgentPreparationFailure::Compile(error)) => {
            return agent_compile_error_response(&error);
        }
    };

    (
        StatusCode::OK,
        Json(serde_json::json!(AgentCompiledResponse {
            definition_id: normalized.id.clone(),
            normalized,
            compiled: agent_compiled_payload(compiled),
        })),
    )
}

/// GET /api/v1/agents/{id}/runtime — Load runtime state for one agent definition.
pub async fn get_agent_runtime(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_agent_definition_id(&id) {
        return response;
    }

    let runtime_entry = find_runtime_agent_for_definition(&state, &id);
    if runtime_entry.is_none() {
        match load_agent_definition_resource(&state, &id) {
            Ok(Some(_)) => {}
            Ok(None) => return runtime_not_found_response(),
            Err(response) => return response,
        }
    }

    let runtime_record = if let Some(entry) = runtime_entry.as_ref() {
        match state
            .kernel
            .runtime_stores
            .agent_runtime
            .get_agent_runtime(entry.id)
        {
            Ok(record) => record,
            Err(error) => {
                return runtime_store_error(
                    "runtime_status_failed",
                    "Failed to load agent runtime status",
                    error,
                )
            }
        }
    } else {
        None
    };

    let active_sessions = if let Some(entry) = runtime_entry.as_ref() {
        let count = match state
            .kernel
            .runtime_stores
            .agent_session
            .list_agent_sessions_for_agent(entry.id)
        {
            Ok(records) => records.len(),
            Err(error) => {
                return runtime_store_error(
                    "runtime_status_failed",
                    "Failed to load agent runtime status",
                    error,
                )
            }
        };
        u32::try_from(count).unwrap_or(u32::MAX)
    } else {
        0
    };

    (
        StatusCode::OK,
        Json(serde_json::json!(runtime_response(
            &id,
            runtime_entry.as_ref(),
            runtime_record.as_ref(),
            active_sessions,
        ))),
    )
}

/// POST /api/v1/agents/{id}/runtime/start — Ensure the runtime is loaded and running.
pub async fn start_agent_runtime(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_agent_definition_id(&id) {
        return response;
    }

    let agent_id = match ensure_runtime_agent_present(&state, &id) {
        Ok(agent_id) => agent_id,
        Err(response) => return response,
    };

    if let Err(error) = state.kernel.set_agent_state(agent_id, AgentState::Running) {
        return agent_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "runtime_start_failed",
            "Failed to start agent runtime",
            Some(serde_json::json!([{
                "message": error.to_string(),
            }])),
        );
    }

    agent_action_accepted_response(&id, None)
}

/// POST /api/v1/agents/{id}/runtime/stop — Suspend a loaded runtime.
pub async fn stop_agent_runtime(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_agent_definition_id(&id) {
        return response;
    }

    let Some(entry) = find_runtime_agent_for_definition(&state, &id) else {
        return match load_agent_definition_resource(&state, &id) {
            Ok(Some(_)) => agent_action_accepted_response(&id, None),
            Ok(None) => runtime_not_found_response(),
            Err(response) => response,
        };
    };

    if let Err(error) = state.kernel.stop_agent_run(entry.id) {
        return agent_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "runtime_stop_failed",
            "Failed to stop agent runtime",
            Some(serde_json::json!([{
                "message": error.to_string(),
            }])),
        );
    }

    if let Err(error) = state
        .kernel
        .set_agent_state(entry.id, AgentState::Suspended)
    {
        return agent_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "runtime_stop_failed",
            "Failed to stop agent runtime",
            Some(serde_json::json!([{
                "message": error.to_string(),
            }])),
        );
    }

    agent_action_accepted_response(&id, None)
}

/// POST /api/v1/agents/{id}/runtime/restart — Restart a runtime loop.
pub async fn restart_agent_runtime(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_agent_definition_id(&id) {
        return response;
    }

    let agent_id = match ensure_runtime_agent_present(&state, &id) {
        Ok(agent_id) => agent_id,
        Err(response) => return response,
    };

    if let Err(error) = state.kernel.stop_agent_run(agent_id) {
        return agent_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "runtime_restart_failed",
            "Failed to restart agent runtime",
            Some(serde_json::json!([{
                "message": error.to_string(),
            }])),
        );
    }

    if let Err(error) = state.kernel.set_agent_state(agent_id, AgentState::Running) {
        return agent_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "runtime_restart_failed",
            "Failed to restart agent runtime",
            Some(serde_json::json!([{
                "message": error.to_string(),
            }])),
        );
    }

    agent_action_accepted_response(&id, None)
}

/// PUT /api/v1/agents/{id}/runtime/mode — Update runtime mode without mutating the definition.
pub async fn set_agent_runtime_mode(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<RuntimeModeRequest>, JsonRejection>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_agent_definition_id(&id) {
        return response;
    }

    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return agent_json_rejection(rejection),
    };

    let agent_id = match ensure_runtime_agent_present(&state, &id) {
        Ok(agent_id) => agent_id,
        Err(response) => return response,
    };

    if let Err(error) = state.kernel.set_agent_mode(agent_id, request.mode) {
        return agent_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "runtime_mode_failed",
            "Failed to update agent runtime mode",
            Some(serde_json::json!([{
                "message": error.to_string(),
            }])),
        );
    }

    agent_action_accepted_response(&id, None)
}

/// GET /api/v1/agents/{id}/sessions — List session projections for one runtime.
pub async fn list_agent_sessions_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_agent_definition_id(&id) {
        return response;
    }

    let Some(entry) = find_runtime_agent_for_definition(&state, &id) else {
        return match load_agent_definition_resource(&state, &id) {
            Ok(Some(_)) => (
                StatusCode::OK,
                Json(serde_json::json!(SessionListResponse {
                    items: Vec::new(),
                    next_cursor: None,
                })),
            ),
            Ok(None) => runtime_not_found_response(),
            Err(response) => response,
        };
    };

    let records = match state
        .kernel
        .runtime_stores
        .agent_session
        .list_agent_sessions_for_agent(entry.id)
    {
        Ok(records) => records,
        Err(error) => {
            return runtime_store_error(
                "session_list_failed",
                "Failed to list agent sessions",
                error,
            )
        }
    };

    let items = records.iter().map(session_list_item).collect::<Vec<_>>();
    (
        StatusCode::OK,
        Json(serde_json::json!(SessionListResponse {
            items,
            next_cursor: None,
        })),
    )
}

/// POST /api/v1/agents/{id}/sessions — Create a new session and activate it.
pub async fn create_agent_session_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<CreateSessionRequest>, JsonRejection>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_agent_definition_id(&id) {
        return response;
    }

    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return agent_json_rejection(rejection),
    };

    if let Some(label) = request.label.as_deref() {
        if let Err(error) = openfang_types::agent::SessionLabel::new(label) {
            return agent_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Invalid session label",
                Some(serde_json::json!([{
                    "path": "label",
                    "message": error.to_string(),
                }])),
            );
        }
    }

    let agent_id = match ensure_runtime_agent_present(&state, &id) {
        Ok(agent_id) => agent_id,
        Err(response) => return response,
    };

    let created = match state
        .kernel
        .create_agent_session(agent_id, request.label.as_deref())
    {
        Ok(created) => created,
        Err(error) => {
            return agent_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "session_create_failed",
                "Failed to create agent session",
                Some(serde_json::json!([{
                    "message": error.to_string(),
                }])),
            )
        }
    };

    let Some(session_id_str) = created
        .get("session_id")
        .and_then(serde_json::Value::as_str)
    else {
        return agent_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "session_create_failed",
            "Failed to load the created agent session",
            None,
        );
    };
    let session_id = match parse_session_id_path(session_id_str) {
        Ok(session_id) => session_id,
        Err(response) => return response,
    };

    let session_record = match state
        .kernel
        .runtime_stores
        .agent_session
        .get_agent_session(session_id)
    {
        Ok(Some(record)) => record,
        Ok(None) => {
            return agent_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "session_create_failed",
                "Failed to load the created agent session",
                None,
            )
        }
        Err(error) => {
            return runtime_store_error(
                "session_create_failed",
                "Failed to load the created agent session",
                error,
            )
        }
    };

    (
        StatusCode::CREATED,
        Json(serde_json::json!(session_detail(&session_record, None))),
    )
}

/// GET /api/v1/agents/{id}/sessions/{session_id} — Load one session detail.
pub async fn get_agent_session_v1(
    State(state): State<Arc<AppState>>,
    Path((id, session_id_path)): Path<(String, String)>,
    Query(query): Query<AgentSessionDetailQuery>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_agent_definition_id(&id) {
        return response;
    }

    let session_id = match parse_session_id_path(&session_id_path) {
        Ok(session_id) => session_id,
        Err(response) => return response,
    };

    let runtime_entry = find_runtime_agent_for_definition(&state, &id);
    if runtime_entry.is_none() {
        match load_agent_definition_resource(&state, &id) {
            Ok(Some(_)) => {}
            Ok(None) => return runtime_not_found_response(),
            Err(response) => return response,
        }
    }

    let session_record = match state
        .kernel
        .runtime_stores
        .agent_session
        .get_agent_session(session_id)
    {
        Ok(Some(record)) => record,
        Ok(None) => {
            return agent_error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                "Agent session not found",
                None,
            )
        }
        Err(error) => {
            return runtime_store_error(
                "session_load_failed",
                "Failed to load agent session",
                error,
            )
        }
    };

    if runtime_entry
        .as_ref()
        .is_some_and(|entry| session_record.agent_id != entry.id)
    {
        return agent_error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "Agent session not found",
            None,
        );
    }

    let messages = if query.wants_messages() {
        match session_messages(&state, session_id) {
            Ok(messages) => Some(messages),
            Err(response) => return response,
        }
    } else {
        None
    };

    (
        StatusCode::OK,
        Json(serde_json::json!(session_detail(&session_record, messages))),
    )
}

/// POST /api/v1/agents/{id}/sessions/{session_id}/activate — Mark one session active.
pub async fn activate_agent_session(
    State(state): State<Arc<AppState>>,
    Path((id, session_id_path)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_agent_definition_id(&id) {
        return response;
    }

    let session_id = match parse_session_id_path(&session_id_path) {
        Ok(session_id) => session_id,
        Err(response) => return response,
    };
    let agent_id = match ensure_runtime_agent_present(&state, &id) {
        Ok(agent_id) => agent_id,
        Err(response) => return response,
    };

    let session_record = match state
        .kernel
        .runtime_stores
        .agent_session
        .get_agent_session(session_id)
    {
        Ok(Some(record)) => record,
        Ok(None) => {
            return agent_error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                "Agent session not found",
                None,
            )
        }
        Err(error) => {
            return runtime_store_error(
                "session_activate_failed",
                "Failed to activate agent session",
                error,
            )
        }
    };

    if session_record.agent_id != agent_id {
        return agent_error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "Agent session not found",
            None,
        );
    }

    if let Err(error) = state.kernel.switch_agent_session(agent_id, session_id) {
        return agent_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "session_activate_failed",
            "Failed to activate agent session",
            Some(serde_json::json!([{
                "message": error.to_string(),
            }])),
        );
    }

    agent_action_accepted_response(&id, Some(session_id.to_string()))
}

/// POST /api/v1/agents/{id}/sessions/{session_id}/reset — Reset one session.
pub async fn reset_agent_session(
    State(state): State<Arc<AppState>>,
    Path((id, session_id_path)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_agent_definition_id(&id) {
        return response;
    }

    let session_id = match parse_session_id_path(&session_id_path) {
        Ok(session_id) => session_id,
        Err(response) => return response,
    };
    let agent_id = match ensure_runtime_agent_present(&state, &id) {
        Ok(agent_id) => agent_id,
        Err(response) => return response,
    };

    let session_record = match state
        .kernel
        .runtime_stores
        .agent_session
        .get_agent_session(session_id)
    {
        Ok(Some(record)) => record,
        Ok(None) => {
            return agent_error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                "Agent session not found",
                None,
            )
        }
        Err(error) => {
            return runtime_store_error(
                "session_reset_failed",
                "Failed to reset agent session",
                error,
            )
        }
    };

    if session_record.agent_id != agent_id {
        return agent_error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "Agent session not found",
            None,
        );
    }

    if !session_record.active {
        if let Err(error) = state.kernel.switch_agent_session(agent_id, session_id) {
            return agent_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "session_reset_failed",
                "Failed to reset agent session",
                Some(serde_json::json!([{
                    "message": error.to_string(),
                }])),
            );
        }
    }

    if let Err(error) = state.kernel.reset_session(agent_id) {
        return agent_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "session_reset_failed",
            "Failed to reset agent session",
            Some(serde_json::json!([{
                "message": error.to_string(),
            }])),
        );
    }

    let next_session_id = match state
        .kernel
        .runtime_stores
        .agent_runtime
        .get_agent_runtime(agent_id)
    {
        Ok(Some(record)) => record.active_session_id.map(|value| value.to_string()),
        Ok(None) => None,
        Err(error) => {
            return runtime_store_error(
                "session_reset_failed",
                "Failed to load the reset agent session",
                error,
            )
        }
    };

    agent_action_accepted_response(&id, next_session_id)
}

/// POST /api/v1/agents/{id}/sessions/{session_id}/compact — Compact one session.
pub async fn compact_agent_session_v1(
    State(state): State<Arc<AppState>>,
    Path((id, session_id_path)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_agent_definition_id(&id) {
        return response;
    }

    let session_id = match parse_session_id_path(&session_id_path) {
        Ok(session_id) => session_id,
        Err(response) => return response,
    };
    let agent_id = match ensure_runtime_agent_present(&state, &id) {
        Ok(agent_id) => agent_id,
        Err(response) => return response,
    };

    let session_record = match state
        .kernel
        .runtime_stores
        .agent_session
        .get_agent_session(session_id)
    {
        Ok(Some(record)) => record,
        Ok(None) => {
            return agent_error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                "Agent session not found",
                None,
            )
        }
        Err(error) => {
            return runtime_store_error(
                "session_compact_failed",
                "Failed to compact agent session",
                error,
            )
        }
    };

    if session_record.agent_id != agent_id {
        return agent_error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "Agent session not found",
            None,
        );
    }

    if !session_record.active {
        if let Err(error) = state.kernel.switch_agent_session(agent_id, session_id) {
            return agent_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "session_compact_failed",
                "Failed to compact agent session",
                Some(serde_json::json!([{
                    "message": error.to_string(),
                }])),
            );
        }
    }

    if let Err(error) = state.kernel.compact_agent_session(agent_id).await {
        return agent_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "session_compact_failed",
            "Failed to compact agent session",
            Some(serde_json::json!([{
                "message": error.to_string(),
            }])),
        );
    }

    agent_action_accepted_response(&id, Some(session_id.to_string()))
}

/// POST /api/v1/agents/{id}/messages — Submit a message using an explicit session context.
pub async fn submit_agent_message(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<MessageRequest>, JsonRejection>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_agent_definition_id(&id) {
        return response;
    }

    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return agent_json_rejection(rejection),
    };
    let input_text = match resolve_message_text(&request) {
        Ok(text) => text,
        Err(response) => return response,
    };

    const MAX_MESSAGE_SIZE: usize = 64 * 1024;
    if input_text.len() > MAX_MESSAGE_SIZE {
        return agent_error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            "invalid_request",
            "Message too large (max 64KB)",
            Some(serde_json::json!([{
                "path": "input.items",
            }])),
        );
    }

    let session_id = match parse_session_id_path(&request.session_id) {
        Ok(session_id) => session_id,
        Err(response) => return response,
    };
    let runtime_entry = match require_running_runtime_for_definition(&state, &id) {
        Ok(entry) => entry,
        Err(response) => return response,
    };
    let session_record = match load_session_record_for_agent(&state, runtime_entry.id, session_id) {
        Ok(record) => record,
        Err(response) => return response,
    };
    if let Err(error) = state.kernel.scheduler.check_quota(runtime_entry.id) {
        return agent_error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "quota_exceeded",
            "Agent quota exceeded",
            Some(serde_json::json!([{
                "message": error.to_string(),
            }])),
        );
    }

    let message_id = new_message_id();
    let kernel = Arc::clone(&state.kernel);
    let message = input_text;
    let definition_id = id.clone();
    tokio::spawn(async move {
        let kernel_handle: Arc<dyn KernelHandle> = kernel.clone() as Arc<dyn KernelHandle>;
        if let Err(error) = kernel
            .send_message_with_handle_and_blocks_for_session(AgentMessageDispatch {
                agent_id: runtime_entry.id,
                session_id: Some(session_id),
                message: &message,
                kernel_handle: Some(kernel_handle),
                content_blocks: None,
                sender_id: None,
                sender_name: None,
            })
            .await
        {
            tracing::warn!(
                definition_id = %definition_id,
                agent_id = %runtime_entry.id,
                session_id = %session_id,
                error = %error,
                "Background agent message dispatch failed"
            );
        }
    });

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!(MessageResponse {
            accepted: true,
            resource_id: id,
            status: "accepted".to_owned(),
            session_id: session_record.session_id.to_string(),
            message_id,
        })),
    )
}

/// POST /api/v1/agents/{id}/messages/stream — Stream one agent turn via SSE.
pub async fn stream_agent_message(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<MessageRequest>, JsonRejection>,
) -> axum::response::Response {
    use axum::response::sse::{KeepAlive, Sse};
    use futures::stream;
    use openfang_runtime::llm_driver::StreamEvent as LlmStreamEvent;

    if let Err(response) = ensure_safe_agent_definition_id(&id) {
        return single_sse_event_response(
            response.0,
            StreamEvent {
                event: "error".to_owned(),
                data: serde_json::json!(response.1 .0),
            },
        );
    }

    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => {
            let response = agent_json_rejection(rejection);
            return single_sse_event_response(
                response.0,
                StreamEvent {
                    event: "error".to_owned(),
                    data: serde_json::json!(response.1 .0),
                },
            );
        }
    };
    let input_text = match resolve_message_text(&request) {
        Ok(text) => text,
        Err(response) => {
            return single_sse_event_response(
                response.0,
                StreamEvent {
                    event: "error".to_owned(),
                    data: serde_json::json!(response.1 .0),
                },
            );
        }
    };

    const MAX_MESSAGE_SIZE: usize = 64 * 1024;
    if input_text.len() > MAX_MESSAGE_SIZE {
        return single_sse_event_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            StreamEvent {
                event: "error".to_owned(),
                data: serde_json::json!({
                    "error": {
                        "code": "invalid_request",
                        "message": "Message too large (max 64KB)",
                        "details": [{
                            "path": "input.items",
                        }],
                    }
                }),
            },
        );
    }

    let session_id = match parse_session_id_path(&request.session_id) {
        Ok(session_id) => session_id,
        Err(response) => {
            return single_sse_event_response(
                response.0,
                StreamEvent {
                    event: "error".to_owned(),
                    data: serde_json::json!(response.1 .0),
                },
            );
        }
    };
    let runtime_entry = match require_running_runtime_for_definition(&state, &id) {
        Ok(entry) => entry,
        Err(response) => {
            return single_sse_event_response(
                response.0,
                StreamEvent {
                    event: "error".to_owned(),
                    data: serde_json::json!(response.1 .0),
                },
            );
        }
    };
    let session_record = match load_session_record_for_agent(&state, runtime_entry.id, session_id) {
        Ok(record) => record,
        Err(response) => {
            return single_sse_event_response(
                response.0,
                StreamEvent {
                    event: "error".to_owned(),
                    data: serde_json::json!(response.1 .0),
                },
            );
        }
    };
    let message_id = new_message_id();

    let kernel_handle: Arc<dyn KernelHandle> = state.kernel.clone() as Arc<dyn KernelHandle>;
    let (mut kernel_rx, handle) =
        match state
            .kernel
            .send_message_streaming_for_session(AgentMessageDispatch {
                agent_id: runtime_entry.id,
                session_id: Some(session_id),
                message: &input_text,
                kernel_handle: Some(kernel_handle),
                content_blocks: None,
                sender_id: None,
                sender_name: None,
            }) {
            Ok(pair) => pair,
            Err(error) => {
                return single_sse_event_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    StreamEvent {
                        event: "error".to_owned(),
                        data: serde_json::json!({
                            "error": {
                                "code": "message_stream_failed",
                                "message": "Failed to start agent message stream",
                                "details": [{
                                    "message": error.to_string(),
                                }],
                            }
                        }),
                    },
                );
            }
        };

    let (api_tx, api_rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);
    let initial_keepalive = StreamEvent {
        event: "keepalive".to_owned(),
        data: serde_json::json!({
            "session_id": session_record.session_id.to_string(),
            "message_id": message_id,
        }),
    };
    let _ = api_tx.try_send(initial_keepalive);

    tokio::spawn(async move {
        while let Some(event) = kernel_rx.recv().await {
            let next_event = match event {
                LlmStreamEvent::TextDelta { text } => Some(StreamEvent {
                    event: "message.delta".to_owned(),
                    data: serde_json::json!({
                        "session_id": session_id.to_string(),
                        "message_id": message_id,
                        "delta": text,
                    }),
                }),
                LlmStreamEvent::ToolUseEnd { id, name, input } => Some(StreamEvent {
                    event: "tool.started".to_owned(),
                    data: serde_json::json!({
                        "session_id": session_id.to_string(),
                        "message_id": message_id,
                        "tool_id": id,
                        "name": name,
                        "input": input,
                    }),
                }),
                LlmStreamEvent::ToolExecutionResult {
                    name,
                    result_preview,
                    is_error,
                } => Some(StreamEvent {
                    event: "tool.completed".to_owned(),
                    data: serde_json::json!({
                        "session_id": session_id.to_string(),
                        "message_id": message_id,
                        "name": name,
                        "result_preview": result_preview,
                        "is_error": is_error,
                    }),
                }),
                _ => None,
            };

            if let Some(next_event) = next_event {
                if api_tx.send(next_event).await.is_err() {
                    return;
                }
            }
        }

        let completion_event = match handle.await {
            Ok(Ok(result)) => StreamEvent {
                event: "message.completed".to_owned(),
                data: serde_json::json!({
                    "session_id": session_id.to_string(),
                    "message_id": message_id,
                    "content": crate::ws::strip_think_tags(&result.response),
                    "usage": {
                        "input_tokens": result.total_usage.input_tokens,
                        "output_tokens": result.total_usage.output_tokens,
                    },
                }),
            },
            Ok(Err(error)) => StreamEvent {
                event: "error".to_owned(),
                data: serde_json::json!({
                    "error": {
                        "code": "message_stream_failed",
                        "message": "Agent message stream failed",
                        "details": [{
                            "message": error.to_string(),
                        }],
                    }
                }),
            },
            Err(error) => StreamEvent {
                event: "error".to_owned(),
                data: serde_json::json!({
                    "error": {
                        "code": "message_stream_failed",
                        "message": "Agent message stream task failed",
                        "details": [{
                            "message": error.to_string(),
                        }],
                    }
                }),
            },
        };
        let _ = api_tx.send(completion_event).await;
    });

    let sse_stream = stream::unfold(api_rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|event| (stream_event_to_sse(&event), rx))
    });

    Sse::new(sse_stream)
        .keep_alive(
            KeepAlive::new().interval(Duration::from_secs(15)).event(
                axum::response::sse::Event::default()
                    .event("keepalive")
                    .data("{}"),
            ),
        )
        .into_response()
}

/// POST /api/v1/agents/{id}/messages/dry-run — Resolve one message request without dispatching.
pub async fn dry_run_agent_message(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<MessageRequest>, JsonRejection>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_agent_definition_id(&id) {
        return response;
    }

    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return agent_json_rejection(rejection),
    };
    let input_text = match resolve_message_text(&request) {
        Ok(text) => text,
        Err(response) => return response,
    };
    let session_id = match parse_session_id_path(&request.session_id) {
        Ok(session_id) => session_id,
        Err(response) => return response,
    };

    let (resource, compiled) = match compile_persisted_agent_definition(&state, &id) {
        Ok(result) => result,
        Err(response) => return response,
    };
    let runtime_entry = find_runtime_agent_for_definition(&state, &id);
    let session_record = match runtime_entry.as_ref() {
        Some(entry) => load_session_record_for_agent(&state, entry.id, session_id),
        None => match state
            .kernel
            .runtime_stores
            .agent_session
            .get_agent_session(session_id)
        {
            Ok(Some(record)) => {
                if record.agent_id != stable_runtime_agent_id(&id) {
                    Err(agent_session_not_found_response())
                } else {
                    Ok(record)
                }
            }
            Ok(None) => Err(agent_session_not_found_response()),
            Err(error) => Err(runtime_store_error(
                "session_load_failed",
                "Failed to load agent session",
                error,
            )),
        },
    };
    let session_record = match session_record {
        Ok(record) => record,
        Err(response) => return response,
    };

    let tools = resolve_dry_run_tool_names(&compiled);
    let effects = estimate_message_effects(&state, &compiled, session_id, &input_text);
    let capabilities = serde_json::json!({
        "network": resource.definition.capabilities.network,
        "workspace": resource.definition.capabilities.workspace,
    });

    (
        StatusCode::OK,
        Json(serde_json::json!(MessageDryRunResponse {
            would_execute: true,
            resolved: MessageDryRunResolved {
                agent_id: resource.definition.id.clone(),
                session_id: session_record.session_id.to_string(),
                provider: MessageResolvedProvider {
                    driver: resource.definition.provider.driver.clone(),
                    model: resource.definition.provider.model.clone(),
                },
                model: resource.definition.provider.model.clone(),
                tools,
                session: MessageResolvedSession {
                    id: session_record.session_id.to_string(),
                    active: runtime_entry
                        .as_ref()
                        .is_some_and(|entry| entry.session_id == session_record.session_id),
                    message_count: session_record.message_count,
                },
            },
            effects,
            explanation: MessageDryRunExplanation {
                skills: resource.definition.prompt.skills.clone(),
                capabilities,
                steps: vec![
                    "Validated the persisted agent definition".to_owned(),
                    "Compiled the definition into runtime metadata".to_owned(),
                    "Resolved the requested session context".to_owned(),
                    "Estimated the token and cost impact without dispatching".to_owned(),
                ],
            },
        })),
    )
}

/// POST /api/agents — Spawn a new agent.
pub async fn spawn_agent(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SpawnRequest>,
) -> impl IntoResponse {
    // Resolve template name → manifest_toml if template is provided and manifest_toml is empty
    let manifest_toml = if req.manifest_toml.trim().is_empty() {
        if let Some(ref tmpl_name) = req.template {
            // Sanitize template name to prevent path traversal
            let safe_name = tmpl_name
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .collect::<String>();
            if safe_name.is_empty() || safe_name != *tmpl_name {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Invalid template name"})),
                );
            }
            let tmpl_path = state
                .kernel
                .config
                .home_dir
                .join("agents")
                .join(&safe_name)
                .join("agent.toml");
            match std::fs::read_to_string(&tmpl_path) {
                Ok(content) => content,
                Err(_) => {
                    return (
                        StatusCode::NOT_FOUND,
                        Json(
                            serde_json::json!({"error": format!("Template '{}' not found", safe_name)}),
                        ),
                    );
                }
            }
        } else {
            return (
                StatusCode::BAD_REQUEST,
                Json(
                    serde_json::json!({"error": "Either 'manifest_toml' or 'template' is required"}),
                ),
            );
        }
    } else {
        req.manifest_toml.clone()
    };

    // SECURITY: Reject oversized manifests to prevent parser memory exhaustion.
    const MAX_MANIFEST_SIZE: usize = 1024 * 1024; // 1MB
    if manifest_toml.len() > MAX_MANIFEST_SIZE {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({"error": "Manifest too large (max 1MB)"})),
        );
    }

    // SECURITY: Verify Ed25519 signature when a signed manifest is provided
    if let Some(ref signed_json) = req.signed_manifest {
        match state.kernel.verify_signed_manifest(signed_json) {
            Ok(verified_toml) => {
                // Ensure the signed manifest matches the provided manifest_toml
                if verified_toml.trim() != manifest_toml.trim() {
                    tracing::warn!("Signed manifest content does not match manifest_toml");
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(
                            serde_json::json!({"error": "Signed manifest content does not match manifest_toml"}),
                        ),
                    );
                }
            }
            Err(e) => {
                tracing::warn!("Manifest signature verification failed: {e}");
                state.kernel.audit_log.record(
                    "system",
                    openfang_runtime::audit::AuditAction::AuthAttempt,
                    "manifest signature verification failed",
                    format!("error: {e}"),
                );
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"error": "Manifest signature verification failed"})),
                );
            }
        }
    }

    let manifest: AgentManifest = match toml::from_str(&manifest_toml) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("Invalid manifest TOML: {e}");
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid manifest format"})),
            );
        }
    };

    let name = manifest.name.clone();
    match state.kernel.spawn_agent(manifest) {
        Ok(id) => {
            // Register in channel router so binding resolution finds the new agent
            if let Some(ref mgr) = *state.bridge_manager.lock().await {
                mgr.router().register_agent(name.clone(), id);
            }
            (
                StatusCode::CREATED,
                Json(serde_json::json!(SpawnResponse {
                    agent_id: id.to_string(),
                    name,
                })),
            )
        }
        Err(e) => {
            tracing::warn!("Spawn failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Agent spawn failed"})),
            )
        }
    }
}

/// GET /api/agents — List all agents.
pub async fn list_agents_legacy(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Snapshot catalog once for enrichment
    let catalog = state.kernel.model_catalog.read().ok();
    let dm = &state.kernel.config.default_model;
    let runtime_projections: HashMap<AgentId, AgentRuntimeRecord> = state
        .kernel
        .runtime_stores
        .agent_runtime
        .list_agent_runtimes()
        .unwrap_or_default()
        .into_iter()
        .map(|record| (record.agent_id, record))
        .collect();

    let agents: Vec<serde_json::Value> = state
        .kernel
        .registry
        .list()
        .into_iter()
        .map(|e| {
            let runtime_projection = runtime_projections.get(&e.id);
            // Resolve "default" provider/model to actual kernel defaults
            let provider =
                if e.manifest.model.provider.is_empty() || e.manifest.model.provider == "default" {
                    dm.provider.as_str()
                } else {
                    e.manifest.model.provider.as_str()
                };
            let model = if e.manifest.model.model.is_empty() || e.manifest.model.model == "default"
            {
                dm.model.as_str()
            } else {
                e.manifest.model.model.as_str()
            };

            // Enrich from catalog
            let (tier, auth_status) = catalog
                .as_ref()
                .map(|cat| {
                    let tier = cat
                        .find_model(model)
                        .map(|m| format!("{:?}", m.tier).to_lowercase())
                        .unwrap_or_else(|| "unknown".to_string());
                    let auth = cat
                        .get_provider(provider)
                        .map(|p| format!("{:?}", p.auth_status).to_lowercase())
                        .unwrap_or_else(|| "unknown".to_string());
                    (tier, auth)
                })
                .unwrap_or(("unknown".to_string(), "unknown".to_string()));

            let state = runtime_projection
                .map(|record| record.state)
                .unwrap_or(e.state);
            let mode = runtime_projection
                .map(|record| record.mode)
                .unwrap_or(e.mode);
            let last_active = runtime_projection
                .and_then(|record| record.last_active_at.clone())
                .unwrap_or_else(|| e.last_active.to_rfc3339());
            let ready = matches!(state, openfang_types::agent::AgentState::Running)
                && runtime_projection
                    .map(|record| record.healthy)
                    .unwrap_or(true)
                && auth_status != "missing";

            serde_json::json!({
                "id": e.id.to_string(),
                "name": e.name,
                "state": format!("{:?}", state),
                "mode": mode,
                "created_at": e.created_at.to_rfc3339(),
                "last_active": last_active,
                "model_provider": provider,
                "model_name": model,
                "model_tier": tier,
                "auth_status": auth_status,
                "ready": ready,
                "profile": e.manifest.profile,
                "identity": {
                    "emoji": e.identity.emoji,
                    "avatar_url": e.identity.avatar_url,
                    "color": e.identity.color,
                },
            })
        })
        .collect();

    Json(agents)
}

/// Resolve uploaded file attachments into ContentBlock::Image blocks.
///
/// Reads each file from the upload directory, base64-encodes it, and
/// returns image content blocks ready to insert into a session message.
pub fn resolve_attachments(
    attachments: &[AttachmentRef],
) -> Vec<openfang_types::message::ContentBlock> {
    use base64::Engine;

    let upload_dir = std::env::temp_dir().join("openfang_uploads");
    let mut blocks = Vec::new();

    for att in attachments {
        // Look up metadata from the upload registry
        let meta = UPLOAD_REGISTRY.get(&att.file_id);
        let content_type = if let Some(ref m) = meta {
            m.content_type.clone()
        } else if !att.content_type.is_empty() {
            att.content_type.clone()
        } else {
            continue; // Skip unknown attachments
        };

        // Only process image types
        if !content_type.starts_with("image/") {
            continue;
        }

        // Validate file_id is a UUID to prevent path traversal
        if uuid::Uuid::parse_str(&att.file_id).is_err() {
            continue;
        }

        let file_path = upload_dir.join(&att.file_id);
        match std::fs::read(&file_path) {
            Ok(data) => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                blocks.push(openfang_types::message::ContentBlock::Image {
                    media_type: content_type,
                    data: b64,
                });
            }
            Err(e) => {
                tracing::warn!(file_id = %att.file_id, error = %e, "Failed to read upload for attachment");
            }
        }
    }

    blocks
}

/// Pre-insert image attachments into an agent's session so the LLM can see them.
///
/// This injects image content blocks into the session BEFORE the kernel
/// adds the text user message, so the LLM receives: [..., User(images), User(text)].
pub fn inject_attachments_into_session(
    kernel: &OpenFangKernel,
    agent_id: AgentId,
    image_blocks: Vec<openfang_types::message::ContentBlock>,
) {
    use openfang_types::message::{Message, MessageContent, Role};

    let entry = match kernel.registry.get(agent_id) {
        Some(e) => e,
        None => return,
    };

    let mut session = match kernel.memory.get_session(entry.session_id) {
        Ok(Some(s)) => s,
        _ => openfang_memory::session::Session {
            id: entry.session_id,
            agent_id,
            messages: Vec::new(),
            context_window_tokens: 0,
            label: None,
        },
    };

    session.messages.push(Message {
        role: Role::User,
        content: MessageContent::Blocks(image_blocks),
    });

    if let Err(e) = kernel.memory.save_session(&session) {
        tracing::warn!(error = %e, "Failed to save session with image attachments");
    } else if let Err(error) = kernel.refresh_agent_runtime_projection(agent_id) {
        tracing::warn!(agent_id = %agent_id, "Failed to refresh runtime session projection: {error}");
    }
}

/// POST /api/agents/:id/message — Send a message to an agent.
pub async fn send_message(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<LegacyMessageRequest>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            );
        }
    };

    // SECURITY: Reject oversized messages to prevent OOM / LLM token abuse.
    const MAX_MESSAGE_SIZE: usize = 64 * 1024; // 64KB
    if req.message.len() > MAX_MESSAGE_SIZE {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({"error": "Message too large (max 64KB)"})),
        );
    }

    // Check agent exists before processing
    if state.kernel.registry.get(agent_id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Agent not found"})),
        );
    }

    // Resolve file attachments into image content blocks.
    // Pass them as content_blocks so the LLM receives them in the current turn
    // (not as a separate session message which the LLM may not process).
    let content_blocks = if !req.attachments.is_empty() {
        let image_blocks = resolve_attachments(&req.attachments);
        if image_blocks.is_empty() {
            None
        } else {
            Some(image_blocks)
        }
    } else {
        None
    };

    let kernel_handle: Arc<dyn KernelHandle> = state.kernel.clone() as Arc<dyn KernelHandle>;
    match state
        .kernel
        .send_message_with_handle_and_blocks(
            agent_id,
            &req.message,
            Some(kernel_handle),
            content_blocks,
            req.sender_id,
            req.sender_name,
        )
        .await
    {
        Ok(result) => {
            // Strip <think>...</think> blocks from model output
            let cleaned = crate::ws::strip_think_tags(&result.response);

            // If the agent intentionally returned a silent/NO_REPLY response,
            // return an empty string — don't generate debug fallback text.
            let response = if result.silent {
                String::new()
            } else if cleaned.trim().is_empty() {
                format!(
                    "[The agent completed processing but returned no text response. ({} in / {} out | {} iter)]",
                    result.total_usage.input_tokens,
                    result.total_usage.output_tokens,
                    result.iterations,
                )
            } else {
                cleaned
            };
            (
                StatusCode::OK,
                Json(serde_json::json!(LegacyMessageResponse {
                    response,
                    input_tokens: result.total_usage.input_tokens,
                    output_tokens: result.total_usage.output_tokens,
                    iterations: result.iterations,
                    cost_usd: result.cost_usd,
                })),
            )
        }
        Err(e) => {
            tracing::warn!("send_message failed for agent {id}: {e}");
            let status = if format!("{e}").contains("Agent not found") {
                StatusCode::NOT_FOUND
            } else if format!("{e}").contains("quota") || format!("{e}").contains("Quota") {
                StatusCode::TOO_MANY_REQUESTS
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (
                status,
                Json(serde_json::json!({"error": format!("Message delivery failed: {e}")})),
            )
        }
    }
}

/// GET /api/agents/:id/session — Get agent session (conversation history).
pub async fn get_agent_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            );
        }
    };

    let entry = match state.kernel.registry.get(agent_id) {
        Some(e) => e,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent not found"})),
            );
        }
    };

    match state.kernel.memory.get_session(entry.session_id) {
        Ok(Some(session)) => {
            // Two-pass approach: ToolUse blocks live in Assistant messages while
            // ToolResult blocks arrive in subsequent User messages.  Pass 1
            // collects all tool_use entries keyed by id; pass 2 attaches results.

            // Pass 1: build messages and a lookup from tool_use_id → (msg_idx, tool_idx)
            use base64::Engine as _;
            let mut built_messages: Vec<serde_json::Value> = Vec::new();
            let mut tool_use_index: std::collections::HashMap<String, (usize, usize)> =
                std::collections::HashMap::new();

            for m in &session.messages {
                let mut tools: Vec<serde_json::Value> = Vec::new();
                let mut msg_images: Vec<serde_json::Value> = Vec::new();
                let content = match &m.content {
                    openfang_types::message::MessageContent::Text(t) => t.clone(),
                    openfang_types::message::MessageContent::Blocks(blocks) => {
                        let mut texts = Vec::new();
                        for b in blocks {
                            match b {
                                openfang_types::message::ContentBlock::Text { text, .. } => {
                                    texts.push(text.clone());
                                }
                                openfang_types::message::ContentBlock::Image {
                                    media_type,
                                    data,
                                } => {
                                    texts.push("[Image]".to_string());
                                    // Persist image to upload dir so it can be
                                    // served back when loading session history.
                                    let file_id = uuid::Uuid::new_v4().to_string();
                                    let upload_dir = std::env::temp_dir().join("openfang_uploads");
                                    let _ = std::fs::create_dir_all(&upload_dir);
                                    if let Ok(bytes) =
                                        base64::engine::general_purpose::STANDARD.decode(data)
                                    {
                                        let _ = std::fs::write(upload_dir.join(&file_id), &bytes);
                                        UPLOAD_REGISTRY.insert(
                                            file_id.clone(),
                                            UploadMeta {
                                                filename: format!(
                                                    "image.{}",
                                                    media_type.rsplit('/').next().unwrap_or("png")
                                                ),
                                                content_type: media_type.clone(),
                                            },
                                        );
                                        msg_images.push(serde_json::json!({
                                            "file_id": file_id,
                                            "filename": format!("image.{}", media_type.rsplit('/').next().unwrap_or("png")),
                                        }));
                                    }
                                }
                                openfang_types::message::ContentBlock::ToolUse {
                                    id,
                                    name,
                                    input,
                                    ..
                                } => {
                                    let tool_idx = tools.len();
                                    tools.push(serde_json::json!({
                                        "name": name,
                                        "input": input,
                                        "running": false,
                                        "expanded": false,
                                    }));
                                    // Will be filled after this loop when we know msg_idx
                                    tool_use_index.insert(id.clone(), (usize::MAX, tool_idx));
                                }
                                // ToolResult blocks are handled in pass 2
                                openfang_types::message::ContentBlock::ToolResult { .. } => {}
                                _ => {}
                            }
                        }
                        texts.join("\n")
                    }
                };
                // Skip messages that are purely tool results (User role with only ToolResult blocks)
                if content.is_empty() && tools.is_empty() {
                    continue;
                }
                let msg_idx = built_messages.len();
                // Fix up the msg_idx for tool_use entries registered with sentinel
                for (_, (mi, _)) in tool_use_index.iter_mut() {
                    if *mi == usize::MAX {
                        *mi = msg_idx;
                    }
                }
                let mut msg = serde_json::json!({
                    "role": format!("{:?}", m.role),
                    "content": content,
                });
                if !tools.is_empty() {
                    msg["tools"] = serde_json::Value::Array(tools);
                }
                if !msg_images.is_empty() {
                    msg["images"] = serde_json::Value::Array(msg_images);
                }
                built_messages.push(msg);
            }

            // Pass 2: walk messages again and attach ToolResult to the correct tool
            for m in &session.messages {
                if let openfang_types::message::MessageContent::Blocks(blocks) = &m.content {
                    for b in blocks {
                        if let openfang_types::message::ContentBlock::ToolResult {
                            tool_use_id,
                            content: result,
                            is_error,
                            ..
                        } = b
                        {
                            if let Some(&(msg_idx, tool_idx)) = tool_use_index.get(tool_use_id) {
                                if let Some(msg) = built_messages.get_mut(msg_idx) {
                                    if let Some(tools_arr) =
                                        msg.get_mut("tools").and_then(|v| v.as_array_mut())
                                    {
                                        if let Some(tool_obj) = tools_arr.get_mut(tool_idx) {
                                            let preview: String =
                                                result.chars().take(2000).collect();
                                            tool_obj["result"] = serde_json::Value::String(preview);
                                            tool_obj["is_error"] =
                                                serde_json::Value::Bool(*is_error);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            let messages = built_messages;
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "session_id": session.id.0.to_string(),
                    "agent_id": session.agent_id.0.to_string(),
                    "message_count": session.messages.len(),
                    "context_window_tokens": session.context_window_tokens,
                    "label": session.label,
                    "messages": messages,
                })),
            )
        }
        Ok(None) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "session_id": entry.session_id.0.to_string(),
                "agent_id": agent_id.to_string(),
                "message_count": 0,
                "context_window_tokens": 0,
                "messages": [],
            })),
        ),
        Err(e) => {
            tracing::warn!("Session load failed for agent {id}: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Session load failed"})),
            )
        }
    }
}

/// DELETE /api/agents/:id — Kill an agent.
pub async fn kill_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            );
        }
    };

    match state.kernel.kill_agent(agent_id) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "killed", "agent_id": id})),
        ),
        Err(e) => {
            tracing::warn!("kill_agent failed for {id}: {e}");
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent not found or already terminated"})),
            )
        }
    }
}

/// POST /api/agents/{id}/restart — Restart a crashed/stuck agent.
///
/// Cancels any active task, resets agent state to Running, and updates last_active.
/// Returns the agent's new state.
pub async fn restart_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            );
        }
    };

    // Check agent exists
    let entry = match state.kernel.registry.get(agent_id) {
        Some(e) => e,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent not found"})),
            );
        }
    };

    let agent_name = entry.name.clone();
    let previous_state = format!("{:?}", entry.state);
    drop(entry);

    // Cancel any running task
    let was_running = state.kernel.stop_agent_run(agent_id).unwrap_or(false);

    // Reset state to Running (also updates last_active)
    let _ = state
        .kernel
        .set_agent_state(agent_id, openfang_types::agent::AgentState::Running);

    tracing::info!(
        agent = %agent_name,
        previous_state = %previous_state,
        task_cancelled = was_running,
        "Agent restarted via API"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "restarted",
            "agent": agent_name,
            "agent_id": id,
            "previous_state": previous_state,
            "task_cancelled": was_running,
        })),
    )
}

/// GET /api/status — Kernel status.
pub async fn status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let runtime_projections: HashMap<AgentId, AgentRuntimeRecord> = state
        .kernel
        .runtime_stores
        .agent_runtime
        .list_agent_runtimes()
        .unwrap_or_default()
        .into_iter()
        .map(|record| (record.agent_id, record))
        .collect();
    let agents: Vec<serde_json::Value> = state
        .kernel
        .registry
        .list()
        .into_iter()
        .map(|e| {
            let runtime_projection = runtime_projections.get(&e.id);
            serde_json::json!({
                "id": e.id.to_string(),
                "name": e.name,
                "state": format!("{:?}", runtime_projection.map(|record| record.state).unwrap_or(e.state)),
                "mode": runtime_projection.map(|record| record.mode).unwrap_or(e.mode),
                "created_at": e.created_at.to_rfc3339(),
                "model_provider": e.manifest.model.provider,
                "model_name": e.manifest.model.model,
                "profile": e.manifest.profile,
            })
        })
        .collect();

    let uptime = state.started_at.elapsed().as_secs();
    let agent_count = agents.len();

    Json(serde_json::json!({
        "status": "running",
        "version": env!("CARGO_PKG_VERSION"),
        "agent_count": agent_count,
        "default_provider": state.kernel.config.default_model.provider,
        "default_model": state.kernel.config.default_model.model,
        "uptime_seconds": uptime,
        "api_listen": state.kernel.config.api_listen,
        "home_dir": state.kernel.config.home_dir.display().to_string(),
        "log_level": state.kernel.config.log_level,
        "network_enabled": state.kernel.config.network_enabled,
        "agents": agents,
    }))
}

/// POST /api/shutdown — Graceful shutdown.
pub async fn shutdown(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    tracing::info!("Shutdown requested via API");
    // SECURITY: Record shutdown in audit trail
    state.kernel.audit_log.record(
        "system",
        openfang_runtime::audit::AuditAction::ConfigChange,
        "shutdown requested via API",
        "ok",
    );
    state.kernel.shutdown();
    // Signal the HTTP server to initiate graceful shutdown so the process exits.
    state.shutdown_notify.notify_one();
    Json(serde_json::json!({"status": "shutting_down"}))
}

// ---------------------------------------------------------------------------
// Workflow routes
// ---------------------------------------------------------------------------

fn workflow_bad_request(message: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": message.into() })),
    )
}

fn workflow_internal_error(
    operation: &str,
    workflow_id: Option<WorkflowId>,
    error: &impl std::fmt::Display,
) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!(
        workflow_id = workflow_id.map(|id| id.to_string()),
        error = %error,
        "Failed to {operation} workflow definition"
    );

    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "error": format!("Failed to {operation} workflow definition")
        })),
    )
}

fn workflow_v2_error_response(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
    details: Option<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "error": {
                "code": code,
                "message": message.into(),
                "details": details.unwrap_or_else(|| serde_json::json!([])),
            }
        })),
    )
}

fn workflow_v2_json_rejection(rejection: JsonRejection) -> (StatusCode, Json<serde_json::Value>) {
    match rejection {
        JsonRejection::MissingJsonContentType(_) => workflow_v2_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Missing `Content-Type: application/json` header",
            None,
        ),
        JsonRejection::JsonDataError(error) => workflow_v2_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("Invalid JSON body: {error}"),
            None,
        ),
        JsonRejection::JsonSyntaxError(error) => workflow_v2_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("Invalid JSON body: {error}"),
            None,
        ),
        JsonRejection::BytesRejection(error) => workflow_v2_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("Failed to read request body: {error}"),
            None,
        ),
        rejection => workflow_v2_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("Invalid request body: {rejection}"),
            None,
        ),
    }
}

fn workflow_v2_compile_error_response(
    error: &WorkflowCompileError,
) -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(
        StatusCode::BAD_REQUEST,
        "validation_error",
        "workflow definition is invalid",
        Some(serde_json::to_value(error.issues()).unwrap_or(serde_json::Value::Null)),
    )
}

async fn workflow_v2_available_agent_refs(
    state: &AppState,
) -> Result<BTreeSet<String>, (StatusCode, Json<serde_json::Value>)> {
    let stored_definitions = agent_definition_store(state).list().map_err(|error| {
        workflow_store_load_error_response(
            "definition_load_failed",
            "Failed to load agent definitions",
            error,
        )
    })?;

    let mut agents = BTreeSet::new();
    for definition in stored_definitions {
        agents.insert(definition.definition.id);
        agents.insert(definition.definition.name);
    }
    for entry in state.kernel.registry.list() {
        agents.insert(entry.id.to_string());
        agents.insert(entry.name);
    }

    Ok(agents)
}

fn workflow_v2_is_valid(
    issues: &[openfang_types::workflow::ValidationIssue],
    strict: bool,
) -> bool {
    if strict {
        issues.is_empty()
    } else {
        !issues.iter().any(|issue| issue.severity.is_error())
    }
}

fn workflow_definition_store(state: &AppState) -> WorkflowDefinitionStore {
    WorkflowDefinitionStore::new(&state.kernel.config.home_dir)
}

fn workflow_definition_id_is_safe(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn ensure_safe_workflow_definition_id(
    id: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if workflow_definition_id_is_safe(id) {
        return Ok(());
    }

    Err(workflow_v2_error_response(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        "Workflow definition IDs may only contain ASCII letters, digits, `.`, `_`, or `-`",
        Some(serde_json::json!([{
            "path": "id",
            "value": id,
        }])),
    ))
}

fn workflow_definition_not_found_response() -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(
        StatusCode::NOT_FOUND,
        "not_found",
        "Workflow definition not found",
        None,
    )
}

fn workflow_pack_conflict_response(id: &str) -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(
        StatusCode::CONFLICT,
        "managed_definition_conflict",
        "Managed pack workflow definitions must be forked before modification",
        Some(serde_json::json!([{
            "path": "id",
            "value": id,
            "action": "fork",
        }])),
    )
}

fn workflow_store_load_error_response(
    code: &str,
    message: &str,
    error: impl Into<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        code,
        message,
        Some(serde_json::json!([{
            "message": error.into(),
        }])),
    )
}

fn trigger_compile_error_response(
    error: &TriggerCompileError,
) -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(
        StatusCode::BAD_REQUEST,
        "validation_error",
        "trigger definition is invalid",
        Some(serde_json::to_value(error.issues()).unwrap_or(serde_json::Value::Null)),
    )
}

fn trigger_definition_store(state: &AppState) -> TriggerDefinitionStore {
    TriggerDefinitionStore::new(&state.kernel.config.home_dir)
}

fn trigger_definition_id_is_safe(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn ensure_safe_trigger_definition_id(
    id: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if trigger_definition_id_is_safe(id) {
        return Ok(());
    }

    Err(workflow_v2_error_response(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        "Trigger definition IDs may only contain ASCII letters, digits, `.`, `_`, or `-`",
        Some(serde_json::json!([{
            "path": "id",
            "value": id,
        }])),
    ))
}

fn trigger_definition_not_found_response() -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(
        StatusCode::NOT_FOUND,
        "not_found",
        "Trigger definition not found",
        None,
    )
}

fn trigger_pack_conflict_response(id: &str) -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(
        StatusCode::CONFLICT,
        "managed_definition_conflict",
        "Managed pack trigger definitions must be forked before modification",
        Some(serde_json::json!([{
            "path": "id",
            "value": id,
            "action": "fork",
        }])),
    )
}

fn trigger_store_load_error_response(
    code: &str,
    message: &str,
    error: impl Into<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        code,
        message,
        Some(serde_json::json!([{
            "message": error.into(),
        }])),
    )
}

fn load_trigger_definition_resource(
    state: &AppState,
    definition_id: &str,
) -> Result<Option<TriggerResponse>, (StatusCode, Json<serde_json::Value>)> {
    trigger_definition_store(state)
        .load(definition_id)
        .map_err(|error| {
            trigger_store_load_error_response(
                "definition_load_failed",
                "Failed to load trigger definition",
                error,
            )
        })
}

fn load_all_trigger_definition_resources(
    state: &AppState,
) -> Result<Vec<TriggerResponse>, (StatusCode, Json<serde_json::Value>)> {
    trigger_definition_store(state).list().map_err(|error| {
        trigger_store_load_error_response(
            "definition_load_failed",
            "Failed to load trigger definitions",
            error,
        )
    })
}

fn trigger_v2_is_valid(
    issues: &[openfang_types::trigger::TriggerValidationIssue],
    strict: bool,
) -> bool {
    if strict {
        issues.is_empty()
    } else {
        !issues.iter().any(|issue| issue.severity.is_error())
    }
}

fn trigger_definition_from_normalized(normalized: NormalizedTrigger) -> TriggerV2Definition {
    TriggerV2Definition {
        id: normalized.id,
        name: normalized.name,
        description: normalized.description,
        enabled: normalized.enabled,
        max_fires: normalized.max_fires,
        cooldown_secs: normalized.cooldown_secs,
        trigger_match: normalized.trigger_match,
        target: normalized.target,
    }
}

fn trigger_runtime_status_or_default(
    state: &AppState,
    definition: &TriggerV2Definition,
) -> Result<TriggerRuntimeStatus, (StatusCode, Json<serde_json::Value>)> {
    state
        .kernel
        .runtime_stores
        .trigger_runtime
        .get_trigger_runtime(&definition.id)
        .map(|record| {
            record
                .map(trigger_runtime_status_from_record)
                .unwrap_or_else(|| TriggerRuntimeStatus {
                    trigger_id: definition.id.clone(),
                    enabled: definition.enabled,
                    fire_count: 0,
                    max_fires: definition.max_fires,
                    cooldown_secs: definition.cooldown_secs,
                    last_fired_at: None,
                })
        })
        .map_err(|error| {
            trigger_store_load_error_response(
                "runtime_status_failed",
                "Failed to load trigger runtime status",
                error.to_string(),
            )
        })
}

fn trigger_runtime_status_from_record(
    record: openfang_memory::TriggerRuntimeRecord,
) -> TriggerRuntimeStatus {
    TriggerRuntimeStatus {
        trigger_id: record.trigger_id,
        enabled: record.enabled,
        fire_count: record.fire_count,
        max_fires: record.max_fires,
        cooldown_secs: record.cooldown_secs,
        last_fired_at: record.last_fired_at,
    }
}

fn trigger_list_item(resource: TriggerResponse, runtime: TriggerRuntimeStatus) -> TriggerListItem {
    TriggerListItem {
        id: resource.definition.id,
        name: resource.definition.name,
        enabled: resource.definition.enabled,
        trigger_match: resource.definition.trigger_match,
        target: resource.definition.target,
        runtime_status: TriggerListRuntimeStatus {
            enabled: runtime.enabled,
            fire_count: runtime.fire_count,
            last_fired_at: runtime.last_fired_at,
        },
        updated_at: resource.updated_at,
    }
}

fn apply_trigger_engine_error(error: TriggerEngineError) -> (StatusCode, Json<serde_json::Value>) {
    match error {
        TriggerEngineError::Compile(error) => trigger_compile_error_response(&error),
        TriggerEngineError::Runtime(error) => trigger_store_load_error_response(
            "definition_reload_failed",
            "Failed to reload trigger definition into the runtime registry",
            error.to_string(),
        ),
    }
}

fn trigger_target_kind_name(target: &openfang_types::trigger::TriggerTarget) -> &'static str {
    target.kind()
}

fn trigger_runtime_response(runtime: TriggerRuntimeStatus) -> TriggerRuntimeStatus {
    runtime
}

async fn trigger_compile_registry(
    state: &AppState,
) -> Result<TriggerCompileRegistry, (StatusCode, Json<serde_json::Value>)> {
    let agents = agent_definition_store(state).list().map_err(|error| {
        trigger_store_load_error_response(
            "definition_load_failed",
            "Failed to load agent definitions",
            error,
        )
    })?;
    let workflows = load_all_workflow_definition_resources(state)?;
    let mut registry = TriggerCompileRegistry::new();
    for agent in agents {
        registry.insert_agent(agent.definition.id, Some(agent.definition.name));
    }
    for workflow in workflows {
        registry.insert_workflow(workflow.definition.id, Some(workflow.definition.name));
    }
    Ok(registry)
}

fn workflow_run_summary(record: &openfang_memory::WorkflowRunRecord) -> serde_json::Value {
    serde_json::json!({
        "id": record.run_id,
        "status": record.status.as_str(),
        "current_step_id": record.current_step_id,
        "started_at": record.started_at,
        "updated_at": record.updated_at,
    })
}

fn load_workflow_definition_resource(
    state: &AppState,
    definition_id: &str,
) -> Result<Option<WorkflowResponse>, (StatusCode, Json<serde_json::Value>)> {
    workflow_definition_store(state)
        .load(definition_id)
        .map_err(|error| {
            workflow_store_load_error_response(
                "definition_load_failed",
                "Failed to load workflow definition",
                error,
            )
        })
}

fn load_all_workflow_definition_resources(
    state: &AppState,
) -> Result<Vec<WorkflowResponse>, (StatusCode, Json<serde_json::Value>)> {
    workflow_definition_store(state).list().map_err(|error| {
        workflow_store_load_error_response(
            "definition_load_failed",
            "Failed to load workflow definitions",
            error,
        )
    })
}

async fn workflow_compile_registry(
    state: &AppState,
    additional_workflows: impl IntoIterator<Item = String>,
) -> Result<
    openfang_kernel::workflow_compiler::WorkflowCompileRegistry,
    (StatusCode, Json<serde_json::Value>),
> {
    let resources = load_all_workflow_definition_resources(state)?;
    let workflow_ids = resources
        .into_iter()
        .map(|resource| resource.definition.id)
        .chain(additional_workflows);
    let agent_refs = workflow_v2_available_agent_refs(state).await?;
    Ok(state
        .kernel
        .workflows
        .build_compile_registry(agent_refs, workflow_ids)
        .await)
}

async fn compile_workflow_resource(
    state: &AppState,
    resource: &WorkflowResponse,
) -> Result<WorkflowIr, (StatusCode, Json<serde_json::Value>)> {
    let registry =
        workflow_compile_registry(state, std::iter::once(resource.definition.id.clone())).await?;
    compile_workflow_definition(&resource.definition, &registry)
        .map_err(|error| workflow_v2_compile_error_response(&error))
}

fn workflow_runtime_counts(
    runs: &[openfang_memory::WorkflowRunRecord],
) -> (usize, usize, Option<String>) {
    let mut active_runs = 0usize;
    let mut waiting_runs = 0usize;
    let mut last_run_at = None;

    for run in runs {
        match run.status {
            WorkflowRunStatus::Running => active_runs += 1,
            WorkflowRunStatus::Pending
            | WorkflowRunStatus::WaitingSignal
            | WorkflowRunStatus::WaitingHitl => waiting_runs += 1,
            WorkflowRunStatus::Paused
            | WorkflowRunStatus::Completed
            | WorkflowRunStatus::Failed
            | WorkflowRunStatus::Cancelled => {}
        }

        if last_run_at
            .as_ref()
            .map(|current: &String| run.started_at > *current)
            .unwrap_or(true)
        {
            last_run_at = Some(run.started_at.clone());
        }
    }

    (active_runs, waiting_runs, last_run_at)
}

fn parse_pagination_limit(
    limit: Option<usize>,
) -> Result<usize, (StatusCode, Json<serde_json::Value>)> {
    match limit.unwrap_or(DEFAULT_PAGE_LIMIT) {
        0 => Err(workflow_v2_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "`limit` must be greater than zero",
            Some(serde_json::json!([{
                "path": "limit",
            }])),
        )),
        value => Ok(value.min(MAX_PAGE_LIMIT)),
    }
}

fn parse_cursor_offset(
    cursor: Option<&str>,
) -> Result<usize, (StatusCode, Json<serde_json::Value>)> {
    cursor
        .map(|value| value.parse::<usize>())
        .transpose()
        .map(|value| value.unwrap_or(0))
        .map_err(|_| {
            workflow_v2_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "`cursor` must be an unsigned integer offset",
                Some(serde_json::json!([{
                    "path": "cursor",
                    "value": cursor,
                }])),
            )
        })
}

fn dispatch_list_query_from_params(
    params: DispatchListQueryParams,
    scoped_run_id: Option<&str>,
    parent_dispatch_id: Option<&str>,
) -> Result<DispatchListQuery, (StatusCode, Json<serde_json::Value>)> {
    if let (Some(path_run_id), Some(query_run_id)) = (scoped_run_id, params.run_id.as_deref()) {
        if path_run_id != query_run_id {
            return Err(workflow_v2_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "`run_id` query parameter must match the path run ID",
                Some(serde_json::json!([{
                    "path": "run_id",
                    "expected": path_run_id,
                    "actual": query_run_id,
                }])),
            ));
        }
    }

    Ok(DispatchListQuery {
        limit: parse_pagination_limit(params.limit)?,
        offset: parse_cursor_offset(params.cursor.as_deref())?,
        run_id: scoped_run_id.map(ToOwned::to_owned).or(params.run_id),
        parent_dispatch_id: parent_dispatch_id.map(ToOwned::to_owned),
        status: params.status,
        target_agent: params.target_agent,
        step_id: params.step_id,
    })
}

fn hitl_list_query_from_params(
    params: HitlListQueryParams,
    scoped_run_id: Option<&str>,
) -> Result<HitlListQuery, (StatusCode, Json<serde_json::Value>)> {
    if let (Some(path_run_id), Some(query_run_id)) = (scoped_run_id, params.run_id.as_deref()) {
        if path_run_id != query_run_id {
            return Err(workflow_v2_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "`run_id` query parameter must match the path run ID",
                Some(serde_json::json!([{
                    "path": "run_id",
                    "expected": path_run_id,
                    "actual": query_run_id,
                }])),
            ));
        }
    }

    Ok(HitlListQuery {
        limit: parse_pagination_limit(params.limit)?,
        offset: parse_cursor_offset(params.cursor.as_deref())?,
        run_id: scoped_run_id.map(ToOwned::to_owned).or(params.run_id),
        dispatch_id: params.dispatch_id,
        status: params.status,
        kind: params.kind,
    })
}

fn parse_sort_order(
    order: Option<&str>,
) -> Result<Ordering, (StatusCode, Json<serde_json::Value>)> {
    match order.unwrap_or("asc") {
        "asc" => Ok(Ordering::Less),
        "desc" => Ok(Ordering::Greater),
        value => Err(workflow_v2_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "`order` must be either `asc` or `desc`",
            Some(serde_json::json!([{
                "path": "order",
                "value": value,
            }])),
        )),
    }
}

fn reverse_if_desc(ordering: Ordering, order: Ordering) -> Ordering {
    match order {
        Ordering::Less => ordering,
        Ordering::Greater => ordering.reverse(),
        Ordering::Equal => ordering,
    }
}

fn skill_detail_from_installed_skill(skill: &openfang_skills::InstalledSkill) -> SkillResponse {
    SkillResponse {
        id: skill.manifest.skill.name.clone(),
        name: skill.manifest.skill.name.clone(),
        description: skill.manifest.skill.description.clone(),
        source: skill.source_path.display().to_string(),
        created_at: skill.created_at.clone(),
        updated_at: skill.updated_at.clone(),
    }
}

fn skill_summary_from_detail(skill: &SkillResponse) -> SkillSummary {
    SkillSummary {
        id: skill.id.clone(),
        name: skill.name.clone(),
        description: skill.description.clone(),
        source: skill.source.clone(),
    }
}

fn skill_matches_search(skill: &SkillResponse, needle: &str) -> bool {
    let needle = needle.to_lowercase();
    skill.name.to_lowercase().contains(&needle)
        || skill.description.to_lowercase().contains(&needle)
}

fn paginate_skill_summaries(
    items: Vec<SkillSummary>,
    limit: usize,
    offset: usize,
) -> SkillListResponse {
    let next_cursor = if offset + limit < items.len() {
        Some((offset + limit).to_string())
    } else {
        None
    };
    let items = items.into_iter().skip(offset).take(limit).collect();

    SkillListResponse { items, next_cursor }
}

fn list_registered_skills_v1(state: &AppState) -> Vec<SkillResponse> {
    let skills_root = state.kernel.config.home_dir.join("skills");
    let registry = state
        .skill_registry()
        .read()
        .unwrap_or_else(|error| error.into_inner());
    let mut skills = registry
        .list()
        .into_iter()
        .filter(|skill| skill.source_path.starts_with(&skills_root))
        .map(skill_detail_from_installed_skill)
        .collect::<Vec<_>>();
    skills.sort_by(|left, right| left.id.cmp(&right.id));
    skills
}

fn find_registered_skill_v1(state: &AppState, skill_id: &str) -> Option<SkillResponse> {
    list_registered_skills_v1(state)
        .into_iter()
        .find(|skill| skill.id == skill_id)
}

fn skill_not_found_response(skill_id: &str) -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(
        StatusCode::NOT_FOUND,
        "not_found",
        format!("skill '{skill_id}' not found"),
        None,
    )
}

fn workflow_dry_run_initial_dispatches(workflow_ir: &WorkflowIr) -> usize {
    workflow_ir
        .steps
        .first()
        .map(|step| {
            usize::from(matches!(
                step.kind,
                WorkflowIrStepKind::Agent { .. }
                    | WorkflowIrStepKind::Primitive { .. }
                    | WorkflowIrStepKind::Workflow { .. }
                    | WorkflowIrStepKind::StartLooper { .. }
                    | WorkflowIrStepKind::EmitEvent { .. }
            ))
        })
        .unwrap_or(0)
}

fn workflow_run_list_query(
    id: &str,
    params: &WorkflowRunsListQueryParams,
) -> Result<WorkflowRunPageQuery, (StatusCode, Json<serde_json::Value>)> {
    let limit = parse_pagination_limit(params.limit)?;
    let offset = parse_cursor_offset(params.cursor.as_deref())?;
    let sort = params
        .sort
        .clone()
        .unwrap_or_else(|| "updated_at".to_string());
    if !matches!(sort.as_str(), "id" | "status" | "started_at" | "updated_at") {
        return Err(workflow_v2_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Unsupported workflow run sort field",
            Some(serde_json::json!([{
                "path": "sort",
                "value": sort,
                "workflow_id": id,
            }])),
        ));
    }
    let order = parse_sort_order(params.order.as_deref())?;
    Ok(WorkflowRunPageQuery {
        limit,
        offset,
        sort,
        order,
    })
}

fn sort_workflow_run_records(
    runs: &mut [openfang_memory::WorkflowRunRecord],
    sort: &str,
    order: Ordering,
) {
    runs.sort_by(|left, right| {
        let ordering = match sort {
            "id" => left.run_id.cmp(&right.run_id),
            "status" => left.status.as_str().cmp(right.status.as_str()),
            "started_at" => left.started_at.cmp(&right.started_at),
            "updated_at" => left.updated_at.cmp(&right.updated_at),
            _ => left.updated_at.cmp(&right.updated_at),
        };
        reverse_if_desc(ordering, order).then_with(|| left.run_id.cmp(&right.run_id))
    });
}

/// GET /api/v1/workflows — List file-backed workflow definitions.
pub async fn list_workflow_definitions_v1(
    State(state): State<Arc<AppState>>,
    Query(params): Query<WorkflowListQueryParams>,
) -> impl IntoResponse {
    let limit = match parse_pagination_limit(params.limit) {
        Ok(limit) => limit,
        Err(response) => return response,
    };
    let offset = match parse_cursor_offset(params.cursor.as_deref()) {
        Ok(offset) => offset,
        Err(response) => return response,
    };
    let sort = params.sort.unwrap_or_else(|| "id".to_string());
    if !matches!(sort.as_str(), "id" | "name" | "updated_at") {
        return workflow_v2_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Unsupported workflow sort field",
            Some(serde_json::json!([{
                "path": "sort",
                "value": sort,
            }])),
        );
    }
    let order = match parse_sort_order(params.order.as_deref()) {
        Ok(order) => order,
        Err(response) => return response,
    };

    let definitions = match load_all_workflow_definition_resources(&state) {
        Ok(definitions) => definitions,
        Err(response) => return response,
    };
    let runs = match state
        .kernel
        .workflow_stores
        .workflow_run
        .list_non_terminal()
    {
        Ok(runs) => runs,
        Err(error) => {
            return workflow_store_load_error_response(
                "runtime_status_failed",
                "Failed to load workflow runtime status",
                error.to_string(),
            )
        }
    };

    let mut runtime_by_workflow = HashMap::<String, (usize, usize)>::new();
    for run in runs {
        let entry = runtime_by_workflow
            .entry(run.workflow_id.clone())
            .or_insert((0usize, 0usize));
        match run.status {
            WorkflowRunStatus::Running => entry.0 += 1,
            WorkflowRunStatus::Pending
            | WorkflowRunStatus::WaitingSignal
            | WorkflowRunStatus::WaitingHitl => entry.1 += 1,
            WorkflowRunStatus::Paused
            | WorkflowRunStatus::Completed
            | WorkflowRunStatus::Failed
            | WorkflowRunStatus::Cancelled => {}
        }
    }

    let search = params.search.map(|value| value.to_lowercase());
    let tag = params.tag.map(|value| value.to_lowercase());
    let mut items = definitions
        .into_iter()
        .filter(|resource| {
            params
                .enabled
                .map(|enabled| resource.definition.enabled == enabled)
                .unwrap_or(true)
        })
        .filter(|resource| {
            tag.as_ref().is_none_or(|expected| {
                resource
                    .definition
                    .tags
                    .iter()
                    .any(|candidate| candidate.to_lowercase() == *expected)
            })
        })
        .filter(|resource| {
            search.as_ref().is_none_or(|needle| {
                resource.definition.id.to_lowercase().contains(needle)
                    || resource.definition.name.to_lowercase().contains(needle)
                    || resource
                        .definition
                        .description
                        .to_lowercase()
                        .contains(needle)
                    || resource
                        .definition
                        .tags
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(needle))
            })
        })
        .map(|resource| {
            let (active_runs, waiting_runs) = runtime_by_workflow
                .get(&resource.definition.id)
                .copied()
                .unwrap_or((0, 0));
            WorkflowListItem {
                id: resource.definition.id.clone(),
                name: resource.definition.name.clone(),
                description: resource.definition.description.clone(),
                enabled: resource.definition.enabled,
                tags: resource.definition.tags.clone(),
                steps: resource.definition.steps.len(),
                origin: resource.origin.clone(),
                runtime_status: WorkflowListRuntimeStatus {
                    loaded: true,
                    active_runs,
                    waiting_runs,
                },
                updated_at: resource.updated_at,
            }
        })
        .collect::<Vec<_>>();

    items.sort_by(|left, right| {
        let ordering = match sort.as_str() {
            "name" => left.name.cmp(&right.name),
            "updated_at" => left.updated_at.cmp(&right.updated_at),
            _ => left.id.cmp(&right.id),
        };
        reverse_if_desc(ordering, order).then_with(|| left.id.cmp(&right.id))
    });

    let next_cursor = if offset + limit < items.len() {
        Some((offset + limit).to_string())
    } else {
        None
    };
    let items = items
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();

    (
        StatusCode::OK,
        Json(serde_json::json!(WorkflowListResponse {
            items,
            next_cursor
        })),
    )
}

/// POST /api/v1/workflows — Create and persist a workflow definition.
pub async fn create_workflow_definition_v1(
    State(state): State<Arc<AppState>>,
    payload: Result<Json<serde_json::Value>, JsonRejection>,
) -> impl IntoResponse {
    let Json(body) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };
    let _write_guard = WORKFLOW_DEFINITION_WRITE_LOCK.lock().await;
    let requested_id = body
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_default();
    if !requested_id.is_empty() {
        if let Err(response) = ensure_safe_workflow_definition_id(&requested_id) {
            return response;
        }
    }

    let registry = match workflow_compile_registry(
        &state,
        std::iter::once(requested_id.clone()).filter(|id| !id.is_empty()),
    )
    .await
    {
        Ok(registry) => registry,
        Err(response) => return response,
    };
    let definition = match validate_workflow_value(&body, &registry) {
        Ok(definition) => definition,
        Err(error) => return workflow_v2_compile_error_response(&error),
    };
    if let Err(response) = ensure_safe_workflow_definition_id(&definition.id) {
        return response;
    }

    let store = workflow_definition_store(&state);
    match store.load(&definition.id) {
        Ok(Some(existing)) if existing.origin.kind == WorkflowOriginKind::Pack => {
            return workflow_pack_conflict_response(&definition.id)
        }
        Ok(Some(_)) => {
            return workflow_v2_error_response(
                StatusCode::CONFLICT,
                "already_exists",
                "Workflow definition already exists",
                Some(serde_json::json!([{
                    "path": "id",
                    "value": definition.id,
                }])),
            )
        }
        Ok(None) => {}
        Err(error) => {
            return workflow_store_load_error_response(
                "definition_load_failed",
                "Failed to load workflow definition",
                error,
            )
        }
    }

    let timestamp = chrono::Utc::now().to_rfc3339();
    let resource = WorkflowResponse {
        definition: canonicalize_workflow_definition(definition),
        origin: WorkflowOrigin::user(),
        forked_from: None,
        created_at: timestamp.clone(),
        updated_at: timestamp,
    };
    let compiled = match compile_workflow_resource(&state, &resource).await {
        Ok(compiled) => compiled,
        Err(response) => return response,
    };

    if let Err(error) = store.persist(&resource) {
        return workflow_store_load_error_response(
            "definition_persist_failed",
            "Failed to persist workflow definition",
            error,
        );
    }
    state
        .kernel
        .workflows
        .upsert_workflow_v2_definition(resource.definition.clone(), compiled)
        .await;

    (StatusCode::CREATED, Json(serde_json::json!(resource)))
}

/// GET /api/v1/workflows/{id} — Load one persisted workflow definition.
pub async fn get_workflow_definition_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_workflow_definition_id(&id) {
        return response;
    }

    match load_workflow_definition_resource(&state, &id) {
        Ok(Some(resource)) => (StatusCode::OK, Json(serde_json::json!(resource))),
        Ok(None) => workflow_definition_not_found_response(),
        Err(response) => response,
    }
}

/// PUT /api/v1/workflows/{id} — Replace one persisted workflow definition.
pub async fn update_workflow_definition_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<serde_json::Value>, JsonRejection>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_workflow_definition_id(&id) {
        return response;
    }

    let Json(body) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };
    let _write_guard = WORKFLOW_DEFINITION_WRITE_LOCK.lock().await;
    let registry = match workflow_compile_registry(&state, std::iter::once(id.clone())).await {
        Ok(registry) => registry,
        Err(response) => return response,
    };
    let definition = match validate_workflow_value(&body, &registry) {
        Ok(definition) => definition,
        Err(error) => return workflow_v2_compile_error_response(&error),
    };

    if definition.id != id {
        return workflow_v2_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Path ID and body ID must match",
            Some(serde_json::json!([{
                "path": "id",
                "expected": id,
                "actual": definition.id,
            }])),
        );
    }

    let store = workflow_definition_store(&state);
    let existing = match store.load(&id) {
        Ok(Some(resource)) => resource,
        Ok(None) => return workflow_definition_not_found_response(),
        Err(error) => {
            return workflow_store_load_error_response(
                "definition_load_failed",
                "Failed to load workflow definition",
                error,
            )
        }
    };
    if existing.origin.kind == WorkflowOriginKind::Pack {
        return workflow_pack_conflict_response(&id);
    }

    let resource = WorkflowResponse {
        definition: canonicalize_workflow_definition(definition),
        origin: existing.origin,
        forked_from: existing.forked_from,
        created_at: existing.created_at,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    let compiled = match compile_workflow_resource(&state, &resource).await {
        Ok(compiled) => compiled,
        Err(response) => return response,
    };

    if let Err(error) = store.persist(&resource) {
        return workflow_store_load_error_response(
            "definition_persist_failed",
            "Failed to persist workflow definition",
            error,
        );
    }
    state
        .kernel
        .workflows
        .upsert_workflow_v2_definition(resource.definition.clone(), compiled)
        .await;

    (StatusCode::OK, Json(serde_json::json!(resource)))
}

/// DELETE /api/v1/workflows/{id} — Delete one persisted workflow definition.
pub async fn delete_workflow_definition_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_workflow_definition_id(&id) {
        return response.into_response();
    }
    let _write_guard = WORKFLOW_DEFINITION_WRITE_LOCK.lock().await;

    let store = workflow_definition_store(&state);
    let existing = match store.load(&id) {
        Ok(Some(resource)) => resource,
        Ok(None) => return workflow_definition_not_found_response().into_response(),
        Err(error) => {
            return workflow_store_load_error_response(
                "definition_load_failed",
                "Failed to load workflow definition",
                error,
            )
            .into_response()
        }
    };
    if existing.origin.kind == WorkflowOriginKind::Pack {
        return workflow_pack_conflict_response(&id).into_response();
    }

    match store.delete(&id) {
        Ok(true) => {}
        Ok(false) => return workflow_definition_not_found_response().into_response(),
        Err(error) => {
            return workflow_store_load_error_response(
                "definition_delete_failed",
                "Failed to delete workflow definition",
                error,
            )
            .into_response()
        }
    }
    state
        .kernel
        .workflows
        .remove_workflow_v2_definition(&id)
        .await;

    StatusCode::NO_CONTENT.into_response()
}

/// POST /api/v1/workflows/{id}/fork — Shadow a managed pack workflow with a user-owned copy.
pub async fn fork_workflow_definition_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<WorkflowForkRequest>, JsonRejection>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_workflow_definition_id(&id) {
        return response;
    }

    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };
    let _write_guard = WORKFLOW_DEFINITION_WRITE_LOCK.lock().await;
    if request.mode != "shadow" {
        return workflow_v2_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Workflow forks currently support only `shadow` mode",
            Some(serde_json::json!([{
                "path": "mode",
                "value": request.mode,
            }])),
        );
    }

    let store = workflow_definition_store(&state);
    let existing = match store.load(&id) {
        Ok(Some(resource)) => resource,
        Ok(None) => return workflow_definition_not_found_response(),
        Err(error) => {
            return workflow_store_load_error_response(
                "definition_load_failed",
                "Failed to load workflow definition",
                error,
            )
        }
    };
    if existing.origin.kind != WorkflowOriginKind::Pack {
        return workflow_v2_error_response(
            StatusCode::CONFLICT,
            "invalid_fork_source",
            "Only managed pack workflow definitions can be forked",
            Some(serde_json::json!([{
                "workflow_id": id,
            }])),
        );
    }

    let timestamp = chrono::Utc::now().to_rfc3339();
    let resource = WorkflowResponse {
        definition: existing.definition.clone(),
        origin: WorkflowOrigin::user(),
        forked_from: Some(WorkflowForkedFrom {
            kind: existing.origin.kind,
            pack_id: existing.origin.pack_id.clone(),
            pack_version: existing.origin.pack_version.clone(),
            resource_type: "workflow".to_string(),
            resource_id: id.clone(),
        }),
        created_at: timestamp.clone(),
        updated_at: timestamp,
    };
    let compiled = match compile_workflow_resource(&state, &resource).await {
        Ok(compiled) => compiled,
        Err(response) => return response,
    };

    if let Err(error) = store.persist(&resource) {
        return workflow_store_load_error_response(
            "definition_persist_failed",
            "Failed to persist workflow definition",
            error,
        );
    }
    state
        .kernel
        .workflows
        .upsert_workflow_v2_definition(resource.definition.clone(), compiled)
        .await;

    (StatusCode::OK, Json(serde_json::json!(resource)))
}

fn workflow_from_request(
    workflow_id: WorkflowId,
    created_at: chrono::DateTime<chrono::Utc>,
    req: &serde_json::Value,
) -> Result<Workflow, (StatusCode, Json<serde_json::Value>)> {
    let name = req["name"].as_str().unwrap_or("unnamed").to_string();
    let description = req["description"].as_str().unwrap_or("").to_string();

    let steps_json = req["steps"]
        .as_array()
        .ok_or_else(|| workflow_bad_request("Missing 'steps' array"))?;

    let mut steps = Vec::with_capacity(steps_json.len());
    for step in steps_json {
        let step_name = step["name"].as_str().unwrap_or("step").to_string();
        let agent = if let Some(id) = step["agent_id"].as_str() {
            StepAgent::ById { id: id.to_string() }
        } else if let Some(name) = step["agent_name"].as_str() {
            StepAgent::ByName {
                name: name.to_string(),
            }
        } else {
            return Err(workflow_bad_request(format!(
                "Step '{step_name}' needs 'agent_id' or 'agent_name'"
            )));
        };

        let mode = match step["mode"].as_str().unwrap_or("sequential") {
            "fan_out" => StepMode::FanOut,
            "collect" => StepMode::Collect,
            "conditional" => StepMode::Conditional {
                condition: step["condition"].as_str().unwrap_or("").to_string(),
            },
            "loop" => StepMode::Loop {
                max_iterations: step["max_iterations"].as_u64().unwrap_or(5) as u32,
                until: step["until"].as_str().unwrap_or("").to_string(),
            },
            _ => StepMode::Sequential,
        };

        let error_mode = match step["error_mode"].as_str().unwrap_or("fail") {
            "skip" => ErrorMode::Skip,
            "retry" => ErrorMode::Retry {
                max_retries: step["max_retries"].as_u64().unwrap_or(3) as u32,
            },
            _ => ErrorMode::Fail,
        };

        steps.push(WorkflowStep {
            name: step_name,
            agent,
            prompt_template: step["prompt"].as_str().unwrap_or("{{input}}").to_string(),
            mode,
            timeout_secs: step["timeout_secs"].as_u64().unwrap_or(120),
            error_mode,
            output_var: step["output_var"].as_str().map(String::from),
        });
    }

    Ok(Workflow {
        id: workflow_id,
        name,
        description,
        steps,
        created_at,
    })
}

/// POST /api/workflows — Register a new workflow.
pub async fn create_workflow(
    State(state): State<Arc<AppState>>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let workflow_id = WorkflowId::new();
    let workflow = match workflow_from_request(workflow_id, chrono::Utc::now(), &req) {
        Ok(workflow) => workflow,
        Err(response) => return response,
    };

    match state.kernel.register_workflow(workflow).await {
        Ok(id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"workflow_id": id.to_string()})),
        ),
        Err(error) => workflow_internal_error("create", Some(workflow_id), &error),
    }
}

/// GET /api/workflows — List all workflows.
pub async fn list_workflows(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let workflows = state.kernel.workflows.list_workflows().await;
    let list: Vec<serde_json::Value> = workflows
        .iter()
        .map(|w| {
            serde_json::json!({
                "id": w.id.to_string(),
                "name": w.name,
                "description": w.description,
                "steps": w.steps.len(),
                "created_at": w.created_at.to_rfc3339(),
            })
        })
        .collect();
    Json(list)
}

/// POST /api/v1/workflows/validate — Validate a Workflow v2 definition.
pub async fn validate_workflow(
    State(state): State<Arc<AppState>>,
    payload: Result<Json<WorkflowValidateRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };

    let WorkflowValidateRequest {
        definition,
        strict,
        context: _context,
    } = request;
    let strict = strict.unwrap_or(false);
    let additional_workflow = definition
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    let registry = match workflow_compile_registry(&state, additional_workflow.into_iter()).await {
        Ok(registry) => registry,
        Err(response) => return response,
    };

    let (issues, normalized) = match validate_workflow_value(&definition, &registry) {
        Ok(definition) => {
            let candidate = normalize_workflow_definition(&definition);
            let issues = validate_normalized_workflow(&candidate);
            let normalized = if issues.iter().any(|issue| issue.severity.is_error()) {
                None
            } else {
                Some(candidate)
            };
            (issues, normalized)
        }
        Err(error) => (error.issues().to_vec(), None),
    };

    let valid = workflow_v2_is_valid(&issues, strict);

    (
        StatusCode::OK,
        Json(serde_json::json!(WorkflowValidateResponse {
            valid,
            issues,
            normalized,
        })),
    )
}

/// POST /api/v1/workflows/compile — Compile a Workflow v2 definition into IR.
pub async fn compile_workflow(
    State(state): State<Arc<AppState>>,
    payload: Result<Json<WorkflowCompileRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };

    let WorkflowCompileRequest {
        definition,
        context: _context,
    } = request;
    let additional_workflow = definition
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    let registry = match workflow_compile_registry(&state, additional_workflow.into_iter()).await {
        Ok(registry) => registry,
        Err(response) => return response,
    };
    let definition = match validate_workflow_value(&definition, &registry) {
        Ok(definition) => definition,
        Err(error) => return workflow_v2_compile_error_response(&error),
    };

    match compile_workflow_definition(&definition, &registry) {
        Ok(workflow_ir) => (
            StatusCode::OK,
            Json(serde_json::json!(WorkflowCompileResponse {
                definition_id: definition.id.clone(),
                normalized: normalize_workflow_definition(&definition),
                compiled: WorkflowCompiledPayload { workflow_ir },
            })),
        ),
        Err(error) => workflow_v2_compile_error_response(&error),
    }
}

/// GET /api/v1/workflows/{id}/compiled — Return the compiled IR for one persisted
/// workflow definition.
pub async fn get_workflow_compiled(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_workflow_definition_id(&id) {
        return response;
    }

    let resource = match load_workflow_definition_resource(&state, &id) {
        Ok(Some(resource)) => resource,
        Ok(None) => return workflow_definition_not_found_response(),
        Err(response) => return response,
    };
    match compile_workflow_resource(&state, &resource).await {
        Ok(workflow_ir) => (
            StatusCode::OK,
            Json(serde_json::json!(WorkflowCompiledResponse {
                definition_id: resource.definition.id.clone(),
                normalized: normalize_workflow_definition(&resource.definition),
                compiled: WorkflowCompiledPayload { workflow_ir },
            })),
        ),
        Err(response) => response,
    }
}

/// GET /api/v1/workflows/:id/runtime — Get workflow runtime status.
pub async fn get_workflow_runtime_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_workflow_definition_id(&id) {
        return response;
    }

    match load_workflow_definition_resource(&state, &id) {
        Ok(Some(_)) => {}
        Ok(None) => return workflow_definition_not_found_response(),
        Err(response) => return response,
    }

    let runs = match state
        .kernel
        .workflow_stores
        .workflow_run
        .list_for_workflow(&id)
    {
        Ok(runs) => runs,
        Err(error) => {
            return workflow_store_load_error_response(
                "runtime_status_failed",
                "Failed to load workflow runtime status",
                error.to_string(),
            )
        }
    };
    let (active_runs, waiting_runs, last_run_at) = workflow_runtime_counts(&runs);
    let loaded = true;

    (
        StatusCode::OK,
        Json(serde_json::json!(WorkflowRuntimeResponse {
            workflow_id: id,
            loaded,
            healthy: true,
            active_runs,
            waiting_runs,
            last_run_at,
        })),
    )
}

/// POST /api/workflows/:id/run — Execute a workflow.
pub async fn run_workflow(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let workflow_id = WorkflowId(match id.parse() {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid workflow ID"})),
            );
        }
    });

    let input = req["input"].as_str().unwrap_or("").to_string();

    match state.kernel.run_workflow(workflow_id, input).await {
        Ok((run_id, output)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "run_id": run_id.to_string(),
                "output": output,
                "status": "completed",
            })),
        ),
        Err(e) => {
            tracing::warn!("Workflow run failed for {id}: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Workflow execution failed"})),
            )
        }
    }
}

/// POST /api/v1/workflows/{id}/runs — Execute a workflow through the v1 durable
/// run surface.
pub async fn start_workflow_run_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<WorkflowRunRequest>, JsonRejection>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_workflow_definition_id(&id) {
        return response;
    }
    let Json(req) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };
    let resource = match load_workflow_definition_resource(&state, &id) {
        Ok(Some(resource)) => resource,
        Ok(None) => return workflow_definition_not_found_response(),
        Err(response) => return response,
    };
    let workflow_ir = match compile_workflow_resource(&state, &resource).await {
        Ok(workflow_ir) => workflow_ir,
        Err(response) => return response,
    };

    let input = match serde_json::to_string(&req.input) {
        Ok(input) => input,
        Err(error) => {
            return workflow_v2_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Workflow input must be valid JSON",
                Some(serde_json::json!([{
                    "path": "input",
                    "message": error.to_string(),
                }])),
            )
        }
    };

    match state
        .kernel
        .workflows
        .create_run_from_compiled_workflow(
            workflow_ir.workflow_id.clone(),
            resource.definition.name.clone(),
            workflow_ir.workflow_version.clone(),
            input,
            req.labels,
            req.metadata,
        )
        .await
    {
        Ok(run_id) => {
            let workflow_ir_for_task = workflow_ir.clone();
            let kernel = Arc::clone(&state.kernel);
            let definition_id = id.clone();
            tokio::spawn(async move {
                if let Err(error) = kernel
                    .execute_compiled_workflow_run(run_id, workflow_ir_for_task)
                    .await
                {
                    tracing::warn!(workflow_id = %definition_id, run_id = %run_id, "Workflow execution failed: {error}");
                }
            });

            (
                StatusCode::ACCEPTED,
                Json(serde_json::json!({
                    "accepted": true,
                    "resource_id": id,
                    "status": "accepted",
                    "run_id": run_id.to_string(),
                })),
            )
        }
        Err(error) => {
            tracing::warn!("Workflow run failed for {id}: {error}");
            workflow_v2_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "workflow_execution_failed",
                "Workflow execution failed",
                Some(serde_json::json!([{
                    "message": error.to_string(),
                }])),
            )
        }
    }
}

/// GET /api/workflows/:id/runs — List runs for a workflow.
pub async fn list_workflow_runs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state
        .kernel
        .workflow_stores
        .workflow_run
        .list_for_workflow(&id)
    {
        Ok(runs) => {
            let items = runs.iter().map(run_record_to_summary).collect::<Vec<_>>();
            (StatusCode::OK, Json(serde_json::json!(items)))
        }
        Err(error) => {
            tracing::warn!("Failed to list workflow runs for {id}: {error}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to list workflow runs"})),
            )
        }
    }
}

/// GET /api/v1/workflows/{id}/runs — List durable runs for one workflow.
pub async fn list_workflow_runs_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<WorkflowRunsListQueryParams>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_workflow_definition_id(&id) {
        return response;
    }
    match load_workflow_definition_resource(&state, &id) {
        Ok(Some(_)) => {}
        Ok(None) => return workflow_definition_not_found_response(),
        Err(response) => return response,
    }

    let query = match workflow_run_list_query(&id, &params) {
        Ok(query) => query,
        Err(response) => return response,
    };

    match state
        .kernel
        .workflow_stores
        .workflow_run
        .list_for_workflow(&id)
    {
        Ok(mut runs) => {
            sort_workflow_run_records(&mut runs, &query.sort, query.order);
            let next_cursor = if query.offset + query.limit < runs.len() {
                Some((query.offset + query.limit).to_string())
            } else {
                None
            };
            let items = runs
                .into_iter()
                .skip(query.offset)
                .take(query.limit)
                .map(|run| workflow_run_summary(&run))
                .collect::<Vec<_>>();

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "items": items,
                    "next_cursor": next_cursor,
                })),
            )
        }
        Err(error) => {
            tracing::warn!("Failed to list durable workflow runs for {id}: {error}");
            workflow_v2_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "run_list_failed",
                "Failed to list workflow runs",
                Some(serde_json::json!([{
                    "message": error.to_string(),
                }])),
            )
        }
    }
}

/// POST /api/v1/workflows/{id}/runs/dry-run — Simulate workflow run creation without executing it.
pub async fn dry_run_workflow_run_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<WorkflowRunRequest>, JsonRejection>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_workflow_definition_id(&id) {
        return response;
    }
    let Json(_request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };
    let resource = match load_workflow_definition_resource(&state, &id) {
        Ok(Some(resource)) => resource,
        Ok(None) => return workflow_definition_not_found_response(),
        Err(response) => return response,
    };
    let workflow_ir = match compile_workflow_resource(&state, &resource).await {
        Ok(workflow_ir) => workflow_ir,
        Err(response) => return response,
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "would_execute": true,
            "resolved": {
                "workflow_id": workflow_ir.workflow_id,
                "workflow_version": workflow_ir.workflow_version,
                "initial_step_id": workflow_ir.steps.first().map(|step| step.id.clone()),
            },
            "effects": {
                "run_create": true,
                "initial_dispatches": workflow_dry_run_initial_dispatches(&workflow_ir),
            },
            "explanation": {
                "input_contract": resource.definition.input,
                "output_contract": resource.definition.output,
            }
        })),
    )
}

/// GET /api/v1/runs — List durable runs.
pub async fn list_runs_v1(
    State(state): State<Arc<AppState>>,
    Query(params): Query<RunListQueryParams>,
) -> impl IntoResponse {
    let status = match parse_run_status_param(params.status.as_deref()) {
        Ok(status) => status,
        Err(response) => return response,
    };
    let query = WorkflowRunListQuery {
        workflow_id: params.workflow_id,
        status,
        waiting_kind: params.waiting_kind,
        label: params.label,
        search: params.search,
    };

    match state.kernel.workflow_stores.workflow_run.list_runs(&query) {
        Ok(runs) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "items": runs.iter().map(run_record_to_summary).collect::<Vec<_>>(),
                "next_cursor": serde_json::Value::Null,
            })),
        ),
        Err(error) => {
            tracing::warn!("Failed to list durable runs: {error}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "code": "run_list_failed",
                        "message": "Failed to list runs",
                        "details": [],
                    }
                })),
            )
        }
    }
}

/// GET /api/v1/runs/{id} — Load one durable run.
pub async fn get_run_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.kernel.workflow_stores.workflow_run.find_by_id(&id) {
        Ok(Some(run)) => match run_record_to_detail(&run) {
            Ok(body) => (StatusCode::OK, Json(body)),
            Err(response) => response,
        },
        Ok(None) => run_not_found_response(),
        Err(error) => {
            tracing::warn!("Failed to load durable run {id}: {error}");
            run_internal_error_response("run_load_failed", "Failed to load run")
        }
    }
}

/// GET /api/v1/runs/{id}/checkpoints — List durable checkpoints for one run.
pub async fn get_run_checkpoints_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.kernel.workflow_stores.workflow_run.find_by_id(&id) {
        Ok(Some(_)) => {}
        Ok(None) => return run_not_found_response(),
        Err(error) => {
            tracing::warn!("Failed to load durable run {id} before listing checkpoints: {error}");
            return run_internal_error_response("run_load_failed", "Failed to load run");
        }
    }

    match state
        .kernel
        .workflow_stores
        .workflow_run
        .find_checkpoints_for_run(&id)
    {
        Ok(records) => {
            let mut items = Vec::with_capacity(records.len());
            for record in &records {
                let item = match checkpoint_record_to_json(record) {
                    Ok(item) => item,
                    Err(response) => return response,
                };
                items.push(item);
            }

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "items": items,
                    "next_cursor": serde_json::Value::Null,
                })),
            )
        }
        Err(error) => {
            tracing::warn!("Failed to load checkpoints for run {id}: {error}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "code": "checkpoint_list_failed",
                        "message": "Failed to load run checkpoints",
                        "details": [],
                    }
                })),
            )
        }
    }
}

/// GET /api/v1/dispatches — List durable dispatches.
pub async fn get_dispatches_v1(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DispatchListQueryParams>,
) -> impl IntoResponse {
    let query = match dispatch_list_query_from_params(params, None, None) {
        Ok(query) => query,
        Err(response) => return response,
    };

    match state.kernel.workflow_stores.dispatch.list(&query).await {
        Ok(page) => (
            StatusCode::OK,
            Json(serde_json::json!(DispatchListResponse {
                items: page
                    .items
                    .iter()
                    .map(dispatch_record_to_summary_response)
                    .collect(),
                next_cursor: page.next_cursor,
            })),
        ),
        Err(error) => {
            tracing::warn!("Failed to list dispatches: {error}");
            dispatch_internal_error_response("dispatch_list_failed", "Failed to list dispatches")
        }
    }
}

/// GET /api/v1/dispatches/{id} — Load one durable dispatch.
pub async fn get_dispatch_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.kernel.workflow_stores.dispatch.find_by_id(&id).await {
        Ok(Some(record)) => (
            StatusCode::OK,
            Json(serde_json::json!(dispatch_record_to_detail_response(
                &record
            ))),
        ),
        Ok(None) => dispatch_not_found_response(),
        Err(error) => {
            tracing::warn!("Failed to load dispatch {id}: {error}");
            dispatch_internal_error_response("dispatch_load_failed", "Failed to load dispatch")
        }
    }
}

/// GET /api/v1/dispatches/{id}/children — List child dispatches for one parent.
pub async fn get_dispatch_children_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<DispatchListQueryParams>,
) -> impl IntoResponse {
    match state.kernel.workflow_stores.dispatch.find_by_id(&id).await {
        Ok(Some(_)) => {}
        Ok(None) => return dispatch_not_found_response(),
        Err(error) => {
            tracing::warn!("Failed to load dispatch {id} before listing children: {error}");
            return dispatch_internal_error_response(
                "dispatch_load_failed",
                "Failed to load dispatch",
            );
        }
    }

    let query = match dispatch_list_query_from_params(params, None, Some(&id)) {
        Ok(query) => query,
        Err(response) => return response,
    };

    match state.kernel.workflow_stores.dispatch.list(&query).await {
        Ok(page) => (
            StatusCode::OK,
            Json(serde_json::json!(DispatchListResponse {
                items: page
                    .items
                    .iter()
                    .map(dispatch_record_to_summary_response)
                    .collect(),
                next_cursor: page.next_cursor,
            })),
        ),
        Err(error) => {
            tracing::warn!("Failed to list child dispatches for {id}: {error}");
            dispatch_internal_error_response(
                "dispatch_children_failed",
                "Failed to list child dispatches",
            )
        }
    }
}

/// POST /api/v1/dispatches/{id}/retry — Retry one durable dispatch.
pub async fn post_dispatch_retry_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let current = match state.kernel.workflow_stores.dispatch.find_by_id(&id).await {
        Ok(Some(record)) => record,
        Ok(None) => return dispatch_not_found_response(),
        Err(error) => {
            tracing::warn!("Failed to load dispatch {id} before retry: {error}");
            return dispatch_internal_error_response(
                "dispatch_load_failed",
                "Failed to load dispatch",
            );
        }
    };

    if !matches!(
        current.status,
        DispatchStatus::Failed | DispatchStatus::Cancelled
    ) {
        return invalid_dispatch_transition_response(
            "retried",
            current.status,
            &[DispatchStatus::Failed, DispatchStatus::Cancelled],
        );
    }

    match state.kernel.retry_dispatch_control_plane(&id).await {
        Ok(_) => operational_action_accepted_response(&id),
        Err(error) => {
            tracing::warn!("Failed to retry dispatch {id}: {error}");
            match state.kernel.workflow_stores.dispatch.find_by_id(&id).await {
                Ok(Some(latest))
                    if !matches!(
                        latest.status,
                        DispatchStatus::Failed | DispatchStatus::Cancelled
                    ) =>
                {
                    invalid_dispatch_transition_response(
                        "retried",
                        latest.status,
                        &[DispatchStatus::Failed, DispatchStatus::Cancelled],
                    )
                }
                _ => dispatch_internal_error_response(
                    "dispatch_retry_failed",
                    "Failed to retry dispatch",
                ),
            }
        }
    }
}

/// POST /api/v1/dispatches/{id}/cancel — Cancel one durable dispatch.
pub async fn post_dispatch_cancel_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let current = match state.kernel.workflow_stores.dispatch.find_by_id(&id).await {
        Ok(Some(record)) => record,
        Ok(None) => return dispatch_not_found_response(),
        Err(error) => {
            tracing::warn!("Failed to load dispatch {id} before cancel: {error}");
            return dispatch_internal_error_response(
                "dispatch_load_failed",
                "Failed to load dispatch",
            );
        }
    };

    if !matches!(
        current.status,
        DispatchStatus::Pending | DispatchStatus::Running | DispatchStatus::WaitingHitl
    ) {
        return invalid_dispatch_transition_response(
            "cancelled",
            current.status,
            &[
                DispatchStatus::Pending,
                DispatchStatus::Running,
                DispatchStatus::WaitingHitl,
            ],
        );
    }

    match state
        .kernel
        .cancel_dispatch_control_plane(&id, "Dispatch cancelled via API")
        .await
    {
        Ok(_) => operational_action_accepted_response(&id),
        Err(error) => {
            tracing::warn!("Failed to cancel dispatch {id}: {error}");
            match state.kernel.workflow_stores.dispatch.find_by_id(&id).await {
                Ok(Some(latest))
                    if !matches!(
                        latest.status,
                        DispatchStatus::Pending
                            | DispatchStatus::Running
                            | DispatchStatus::WaitingHitl
                    ) =>
                {
                    invalid_dispatch_transition_response(
                        "cancelled",
                        latest.status,
                        &[
                            DispatchStatus::Pending,
                            DispatchStatus::Running,
                            DispatchStatus::WaitingHitl,
                        ],
                    )
                }
                _ => dispatch_internal_error_response(
                    "dispatch_cancel_failed",
                    "Failed to cancel dispatch",
                ),
            }
        }
    }
}

/// GET /api/v1/hitl-requests — List durable HITL requests.
pub async fn get_hitl_requests_v1(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HitlListQueryParams>,
) -> impl IntoResponse {
    let query = match hitl_list_query_from_params(params, None) {
        Ok(query) => query,
        Err(response) => return response,
    };

    match state.kernel.workflow_stores.hitl.list(&query).await {
        Ok(page) => (
            StatusCode::OK,
            Json(serde_json::json!(HitlListResponse {
                items: page
                    .items
                    .iter()
                    .map(hitl_record_to_detail_response)
                    .collect(),
                next_cursor: page.next_cursor,
            })),
        ),
        Err(error) => {
            tracing::warn!("Failed to list HITL requests: {error}");
            hitl_internal_error_response("hitl_list_failed", "Failed to list HITL requests")
        }
    }
}

/// GET /api/v1/hitl-requests/{id} — Load one durable HITL request.
pub async fn get_hitl_request_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.kernel.workflow_stores.hitl.find_by_id(&id).await {
        Ok(Some(record)) => (
            StatusCode::OK,
            Json(serde_json::json!(hitl_record_to_detail_response(&record))),
        ),
        Ok(None) => hitl_not_found_response(),
        Err(error) => {
            tracing::warn!("Failed to load HITL request {id}: {error}");
            hitl_internal_error_response("hitl_load_failed", "Failed to load HITL request")
        }
    }
}

/// POST /api/v1/hitl-requests/{id}/answer — Answer one pending HITL request.
pub async fn post_hitl_answer_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<HitlAnswerRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };

    let current = match state.kernel.workflow_stores.hitl.find_by_id(&id).await {
        Ok(Some(record)) => record,
        Ok(None) => return hitl_not_found_response(),
        Err(error) => {
            tracing::warn!("Failed to load HITL request {id} before answer: {error}");
            return hitl_internal_error_response("hitl_load_failed", "Failed to load HITL request");
        }
    };

    if current.status != HitlStatus::Pending {
        return invalid_hitl_transition_response(
            "answered",
            current.status,
            &[HitlStatus::Pending],
        );
    }

    match state
        .kernel
        .answer_hitl_request(&id, request.response, request.metadata)
        .await
    {
        Ok(_) => operational_action_accepted_response(&id),
        Err(error) => {
            tracing::warn!("Failed to answer HITL request {id}: {error}");
            match state.kernel.workflow_stores.hitl.find_by_id(&id).await {
                Ok(Some(latest)) if latest.status != HitlStatus::Pending => {
                    invalid_hitl_transition_response(
                        "answered",
                        latest.status,
                        &[HitlStatus::Pending],
                    )
                }
                _ => hitl_internal_error_response(
                    "hitl_answer_failed",
                    "Failed to answer HITL request",
                ),
            }
        }
    }
}

/// POST /api/v1/hitl-requests/{id}/cancel — Cancel one pending HITL request.
pub async fn post_hitl_cancel_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let current = match state.kernel.workflow_stores.hitl.find_by_id(&id).await {
        Ok(Some(record)) => record,
        Ok(None) => return hitl_not_found_response(),
        Err(error) => {
            tracing::warn!("Failed to load HITL request {id} before cancel: {error}");
            return hitl_internal_error_response("hitl_load_failed", "Failed to load HITL request");
        }
    };

    if current.status != HitlStatus::Pending {
        return invalid_hitl_transition_response(
            "cancelled",
            current.status,
            &[HitlStatus::Pending],
        );
    }

    match state.kernel.workflows.cancel_hitl_request(&id).await {
        Ok(_) => operational_action_accepted_response(&id),
        Err(error) => {
            tracing::warn!("Failed to cancel HITL request {id}: {error}");
            match state.kernel.workflow_stores.hitl.find_by_id(&id).await {
                Ok(Some(latest)) if latest.status != HitlStatus::Pending => {
                    invalid_hitl_transition_response(
                        "cancelled",
                        latest.status,
                        &[HitlStatus::Pending],
                    )
                }
                _ => hitl_internal_error_response(
                    "hitl_cancel_failed",
                    "Failed to cancel HITL request",
                ),
            }
        }
    }
}

/// GET /api/v1/runs/{id}/dispatches — List durable dispatches for one run.
pub async fn get_run_dispatches_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<DispatchListQueryParams>,
) -> impl IntoResponse {
    if let Err(response) = ensure_durable_run_exists(&state, &id) {
        return response;
    }

    let query = match dispatch_list_query_from_params(params, Some(&id), None) {
        Ok(query) => query,
        Err(response) => return response,
    };

    match state.kernel.workflow_stores.dispatch.list(&query).await {
        Ok(page) => (
            StatusCode::OK,
            Json(serde_json::json!(DispatchListResponse {
                items: page
                    .items
                    .iter()
                    .map(dispatch_record_to_summary_response)
                    .collect(),
                next_cursor: page.next_cursor,
            })),
        ),
        Err(error) => {
            tracing::warn!("Failed to list dispatches for run {id}: {error}");
            run_internal_error_response("dispatch_list_failed", "Failed to list run dispatches")
        }
    }
}

/// GET /api/v1/runs/{id}/hitl-requests — List durable HITL requests for one run.
pub async fn get_run_hitl_requests_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<HitlListQueryParams>,
) -> impl IntoResponse {
    if let Err(response) = ensure_durable_run_exists(&state, &id) {
        return response;
    }

    let query = match hitl_list_query_from_params(params, Some(&id)) {
        Ok(query) => query,
        Err(response) => return response,
    };

    match state.kernel.workflow_stores.hitl.list(&query).await {
        Ok(page) => (
            StatusCode::OK,
            Json(serde_json::json!(HitlListResponse {
                items: page
                    .items
                    .iter()
                    .map(hitl_record_to_detail_response)
                    .collect(),
                next_cursor: page.next_cursor,
            })),
        ),
        Err(error) => {
            tracing::warn!("Failed to list HITL requests for run {id}: {error}");
            run_internal_error_response("hitl_list_failed", "Failed to list run HITL requests")
        }
    }
}

/// GET /api/v1/dispatches/{id}/events — Stream a snapshot + keepalive heartbeat.
pub async fn stream_dispatch_events_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> axum::response::Response {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use futures::stream::{self, StreamExt};

    let record = match state.kernel.workflow_stores.dispatch.find_by_id(&id).await {
        Ok(Some(record)) => record,
        Ok(None) => return dispatch_not_found_response().into_response(),
        Err(error) => {
            tracing::warn!("Failed to load dispatch {id} before streaming events: {error}");
            return dispatch_internal_error_response(
                "dispatch_load_failed",
                "Failed to load dispatch",
            )
            .into_response();
        }
    };

    let snapshot = dispatch_record_to_detail_response(&record);
    let snapshot_json = serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string());
    let sse_stream = stream::once(async move {
        Ok::<Event, std::convert::Infallible>(
            Event::default()
                .event("stream.snapshot")
                .data(snapshot_json),
        )
    })
    .chain(stream::pending::<Result<Event, std::convert::Infallible>>());

    Sse::new(sse_stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(1))
                .event(Event::default().event("keepalive").data("{}")),
        )
        .into_response()
}

/// GET /api/v1/hitl-requests/stream — Stream a snapshot + keepalive heartbeat.
pub async fn stream_hitl_requests_v1(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HitlListQueryParams>,
) -> axum::response::Response {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use futures::stream::{self, StreamExt};

    let query = match hitl_list_query_from_params(params, None) {
        Ok(query) => query,
        Err(response) => return response.into_response(),
    };

    let page = match state.kernel.workflow_stores.hitl.list(&query).await {
        Ok(page) => page,
        Err(error) => {
            tracing::warn!("Failed to list HITL requests before streaming: {error}");
            return hitl_internal_error_response(
                "hitl_list_failed",
                "Failed to list HITL requests",
            )
            .into_response();
        }
    };

    let snapshot = HitlListResponse {
        items: page
            .items
            .iter()
            .map(hitl_record_to_detail_response)
            .collect(),
        next_cursor: page.next_cursor,
    };
    let snapshot_json = serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string());
    let sse_stream = stream::once(async move {
        Ok::<Event, std::convert::Infallible>(
            Event::default()
                .event("stream.snapshot")
                .data(snapshot_json),
        )
    })
    .chain(stream::pending::<Result<Event, std::convert::Infallible>>());

    Sse::new(sse_stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(1))
                .event(Event::default().event("keepalive").data("{}")),
        )
        .into_response()
}

/// GET /api/v1/runs/{id}/signals — List durable signals for one run.
pub async fn get_run_signals_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<RunSignalListQueryParams>,
) -> impl IntoResponse {
    match state.kernel.workflow_stores.workflow_run.find_by_id(&id) {
        Ok(Some(_)) => {}
        Ok(None) => return run_not_found_response(),
        Err(error) => {
            tracing::warn!("Failed to load durable run {id} before listing signals: {error}");
            return run_internal_error_response("run_load_failed", "Failed to load run");
        }
    }

    match state
        .kernel
        .workflow_stores
        .workflow_signal
        .list_for_run(&id, params.consumed)
    {
        Ok(records) => {
            let mut items = Vec::with_capacity(records.len());
            for record in &records {
                let item = match signal_record_to_json(record) {
                    Ok(item) => item,
                    Err(response) => return response,
                };
                items.push(item);
            }

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "items": items,
                    "next_cursor": serde_json::Value::Null,
                })),
            )
        }
        Err(error) => {
            tracing::warn!("Failed to list signals for run {id}: {error}");
            run_internal_error_response("signal_list_failed", "Failed to list run signals")
        }
    }
}

/// POST /api/v1/runs/{id}/pause — Pause a durable run.
pub async fn pause_run_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let current = match state.kernel.workflow_stores.workflow_run.find_by_id(&id) {
        Ok(Some(run)) => run,
        Ok(None) => return run_not_found_response(),
        Err(error) => {
            tracing::warn!("Failed to load durable run {id} before pause: {error}");
            return run_internal_error_response("run_load_failed", "Failed to load run");
        }
    };

    if !matches!(
        current.status,
        WorkflowRunStatus::Running | WorkflowRunStatus::WaitingSignal
    ) {
        return invalid_run_transition_response(
            "pause",
            current.status,
            &[WorkflowRunStatus::Running, WorkflowRunStatus::WaitingSignal],
        );
    }

    let run_id = match id.parse() {
        Ok(value) => WorkflowRunId(value),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": {
                        "code": "invalid_run_id",
                        "message": "Invalid run ID",
                        "details": [],
                    }
                })),
            );
        }
    };

    match state.kernel.workflows.pause_run(run_id, "api").await {
        Ok(()) => run_action_accepted_response(&id, WorkflowRunStatus::Paused),
        Err(error) => {
            tracing::warn!("Failed to pause run {id}: {error}");
            if let Ok(Some(latest)) = state.kernel.workflow_stores.workflow_run.find_by_id(&id) {
                return invalid_run_transition_response(
                    "pause",
                    latest.status,
                    &[WorkflowRunStatus::Running, WorkflowRunStatus::WaitingSignal],
                );
            }
            run_internal_error_response("run_pause_failed", "Failed to pause run")
        }
    }
}

/// POST /api/v1/runs/{id}/resume — Resume a durable run.
pub async fn resume_run_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let current = match state.kernel.workflow_stores.workflow_run.find_by_id(&id) {
        Ok(Some(run)) => run,
        Ok(None) => return run_not_found_response(),
        Err(error) => {
            tracing::warn!("Failed to load durable run {id} before resume: {error}");
            return run_internal_error_response("run_load_failed", "Failed to load run");
        }
    };

    if current.status != WorkflowRunStatus::Paused {
        return invalid_run_transition_response(
            "resume",
            current.status,
            &[WorkflowRunStatus::Paused],
        );
    }

    let run_id = match id.parse() {
        Ok(value) => WorkflowRunId(value),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": {
                        "code": "invalid_run_id",
                        "message": "Invalid run ID",
                        "details": [],
                    }
                })),
            );
        }
    };

    match state.kernel.workflows.resume_run(run_id, "api").await {
        Ok(()) => run_action_accepted_response(&id, WorkflowRunStatus::Running),
        Err(error) => {
            tracing::warn!("Failed to resume run {id}: {error}");
            if let Ok(Some(latest)) = state.kernel.workflow_stores.workflow_run.find_by_id(&id) {
                return invalid_run_transition_response(
                    "resume",
                    latest.status,
                    &[WorkflowRunStatus::Paused],
                );
            }
            run_internal_error_response("run_resume_failed", "Failed to resume run")
        }
    }
}

/// POST /api/v1/runs/{id}/cancel — Cancel a durable run.
pub async fn cancel_run_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let current = match state.kernel.workflow_stores.workflow_run.find_by_id(&id) {
        Ok(Some(run)) => run,
        Ok(None) => return run_not_found_response(),
        Err(error) => {
            tracing::warn!("Failed to load durable run {id} before cancel: {error}");
            return run_internal_error_response("run_load_failed", "Failed to load run");
        }
    };

    if current.status.is_terminal() {
        return invalid_run_transition_response(
            "cancel",
            current.status,
            &[
                WorkflowRunStatus::Pending,
                WorkflowRunStatus::Running,
                WorkflowRunStatus::WaitingSignal,
                WorkflowRunStatus::WaitingHitl,
                WorkflowRunStatus::Paused,
            ],
        );
    }

    let run_id = match id.parse() {
        Ok(value) => WorkflowRunId(value),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": {
                        "code": "invalid_run_id",
                        "message": "Invalid run ID",
                        "details": [],
                    }
                })),
            );
        }
    };

    match state.kernel.workflows.cancel_run(run_id, "api").await {
        Ok(()) => run_action_accepted_response(&id, WorkflowRunStatus::Cancelled),
        Err(error) => {
            tracing::warn!("Failed to cancel run {id}: {error}");
            if let Ok(Some(latest)) = state.kernel.workflow_stores.workflow_run.find_by_id(&id) {
                return invalid_run_transition_response(
                    "cancel",
                    latest.status,
                    &[
                        WorkflowRunStatus::Pending,
                        WorkflowRunStatus::Running,
                        WorkflowRunStatus::WaitingSignal,
                        WorkflowRunStatus::WaitingHitl,
                        WorkflowRunStatus::Paused,
                    ],
                );
            }
            run_internal_error_response("run_cancel_failed", "Failed to cancel run")
        }
    }
}

/// POST /api/v1/runs/{id}/signals — Submit a durable signal for one run.
pub async fn post_run_signal_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<RunSignalSubmitRequest>,
) -> impl IntoResponse {
    if req.name.trim().is_empty()
        || req.source.trim().is_empty()
        || req.idempotency_key.trim().is_empty()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": {
                    "code": "invalid_signal_request",
                    "message": "`name`, `source`, and `idempotency_key` are required",
                    "details": [],
                }
            })),
        );
    }

    let run_id = WorkflowRunId(match id.parse() {
        Ok(value) => value,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": {
                        "code": "invalid_run_id",
                        "message": "Invalid run ID",
                        "details": [],
                    }
                })),
            );
        }
    });

    match state.kernel.workflow_stores.workflow_run.find_by_id(&id) {
        Ok(Some(_)) => {}
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": {
                        "code": "not_found",
                        "message": "Run not found",
                        "details": [],
                    }
                })),
            );
        }
        Err(error) => {
            tracing::warn!("Failed to load durable run {id} before signal submit: {error}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "code": "run_load_failed",
                        "message": "Failed to load run",
                        "details": [],
                    }
                })),
            );
        }
    }

    match state
        .kernel
        .submit_run_signal(
            run_id,
            req.name,
            req.payload,
            req.source,
            req.idempotency_key,
        )
        .await
    {
        Ok(signal) => match signal_record_to_json(&signal) {
            Ok(body) => (StatusCode::OK, Json(body)),
            Err(response) => response,
        },
        Err(error) => {
            tracing::warn!("Failed to submit signal for run {id}: {error}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "code": "signal_submit_failed",
                        "message": "Failed to submit run signal",
                        "details": [],
                    }
                })),
            )
        }
    }
}

/// GET /api/workflows/:id — Get a single workflow by ID.
pub async fn get_workflow(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let workflow_id = WorkflowId(match id.parse() {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid workflow ID"})),
            );
        }
    });

    match state.kernel.workflows.get_workflow(workflow_id).await {
        Some(w) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "id": w.id.to_string(),
                "name": w.name,
                "description": w.description,
                "steps": w.steps,
                "created_at": w.created_at.to_rfc3339(),
            })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Workflow not found"})),
        ),
    }
}

/// PUT /api/workflows/:id — Update a workflow definition.
pub async fn update_workflow(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let workflow_id = WorkflowId(match id.parse() {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid workflow ID"})),
            );
        }
    });

    let updated = match workflow_from_request(workflow_id, chrono::Utc::now(), &req) {
        Ok(workflow) => workflow,
        Err(response) => return response,
    };

    match state
        .kernel
        .update_workflow_definition(workflow_id, updated)
        .await
    {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "updated", "workflow_id": id})),
        ),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Workflow not found"})),
        ),
        Err(error) => workflow_internal_error("update", Some(workflow_id), &error),
    }
}

/// DELETE /api/workflows/:id — Delete a workflow definition.
pub async fn delete_workflow(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let workflow_id = WorkflowId(match id.parse() {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid workflow ID"})),
            );
        }
    });

    match state.kernel.remove_workflow_definition(workflow_id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "removed", "workflow_id": id})),
        ),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Workflow not found"})),
        ),
        Err(error) => workflow_internal_error("delete", Some(workflow_id), &error),
    }
}

// ---------------------------------------------------------------------------
// Trigger v1 control-plane routes
// ---------------------------------------------------------------------------

/// GET /api/v1/triggers — List typed trigger definitions.
pub async fn list_trigger_definitions_v1(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TriggerListQueryParams>,
) -> impl IntoResponse {
    let limit = match parse_pagination_limit(params.limit) {
        Ok(limit) => limit,
        Err(response) => return response,
    };
    let offset = match parse_cursor_offset(params.cursor.as_deref()) {
        Ok(offset) => offset,
        Err(response) => return response,
    };
    let runtime_statuses = match state
        .kernel
        .runtime_stores
        .trigger_runtime
        .list_trigger_runtimes()
    {
        Ok(records) => records
            .into_iter()
            .map(|record| {
                let trigger_id = record.trigger_id.clone();
                (trigger_id, trigger_runtime_status_from_record(record))
            })
            .collect::<HashMap<_, _>>(),
        Err(error) => {
            return trigger_store_load_error_response(
                "runtime_status_failed",
                "Failed to load trigger runtime status",
                error.to_string(),
            )
        }
    };

    let search = params.search.map(|value| value.to_lowercase());
    let items = match load_all_trigger_definition_resources(&state) {
        Ok(resources) => resources,
        Err(response) => return response,
    }
    .into_iter()
    .filter(|resource| {
        params
            .enabled
            .map(|enabled| resource.definition.enabled == enabled)
            .unwrap_or(true)
    })
    .filter(|resource| {
        params
            .event
            .as_ref()
            .map(|event| resource.definition.trigger_match.event.as_deref() == Some(event.as_str()))
            .unwrap_or(true)
    })
    .filter(|resource| {
        params
            .target_kind
            .as_ref()
            .map(|kind| trigger_target_kind_name(&resource.definition.target) == kind)
            .unwrap_or(true)
    })
    .filter(|resource| {
        search.as_ref().is_none_or(|needle| {
            let haystack = format!(
                "{} {} {} {} {}",
                resource.definition.id,
                resource.definition.name,
                resource.definition.description,
                serde_json::to_string(&resource.definition.trigger_match).unwrap_or_default(),
                serde_json::to_string(&resource.definition.target).unwrap_or_default(),
            )
            .to_lowercase();
            haystack.contains(needle)
        })
    })
    .map(|resource| {
        let runtime = runtime_statuses
            .get(&resource.definition.id)
            .cloned()
            .unwrap_or_else(|| TriggerRuntimeStatus {
                trigger_id: resource.definition.id.clone(),
                enabled: resource.definition.enabled,
                fire_count: 0,
                max_fires: resource.definition.max_fires,
                cooldown_secs: resource.definition.cooldown_secs,
                last_fired_at: None,
            });
        trigger_list_item(resource, runtime)
    })
    .collect::<Vec<_>>();

    let mut items = items;
    items.sort_by(|left, right| {
        left.updated_at
            .cmp(&right.updated_at)
            .then(left.id.cmp(&right.id))
    });

    let next_cursor = if offset + limit < items.len() {
        Some((offset + limit).to_string())
    } else {
        None
    };
    let items = items
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();

    (
        StatusCode::OK,
        Json(serde_json::json!(TriggerListResponse {
            items,
            next_cursor
        })),
    )
}

/// POST /api/v1/triggers — Create and persist a typed trigger definition.
pub async fn create_trigger_definition_v1(
    State(state): State<Arc<AppState>>,
    payload: Result<Json<serde_json::Value>, JsonRejection>,
) -> impl IntoResponse {
    let Json(body) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };
    let _write_guard = TRIGGER_DEFINITION_WRITE_LOCK.lock().await;
    let registry = match trigger_compile_registry(&state).await {
        Ok(registry) => registry,
        Err(response) => return response,
    };
    let definition = match validate_trigger_value(&body, &registry) {
        Ok(definition) => definition,
        Err(error) => return trigger_compile_error_response(&error),
    };

    if let Err(response) = ensure_safe_trigger_definition_id(&definition.id) {
        return response;
    }

    let store = trigger_definition_store(&state);
    match store.load(&definition.id) {
        Ok(Some(_)) => {
            return workflow_v2_error_response(
                StatusCode::CONFLICT,
                "definition_exists",
                "Trigger definition already exists",
                Some(serde_json::json!([{
                    "path": "id",
                    "value": definition.id,
                }])),
            )
        }
        Ok(None) => {}
        Err(error) => {
            return trigger_store_load_error_response(
                "definition_load_failed",
                "Failed to load trigger definition",
                error,
            )
        }
    }

    let normalized = match normalize_trigger_definition(&definition, &registry) {
        Ok(normalized) => normalized,
        Err(error) => return trigger_compile_error_response(&error),
    };
    let definition =
        canonicalize_trigger_definition(trigger_definition_from_normalized(normalized));
    let timestamp = chrono::Utc::now().to_rfc3339();
    let resource = TriggerResponse {
        definition,
        origin: TriggerOrigin::user(),
        forked_from: None,
        created_at: timestamp.clone(),
        updated_at: timestamp,
    };

    if let Err(error) = store.persist(&resource) {
        return trigger_store_load_error_response(
            "definition_persist_failed",
            "Failed to persist trigger definition",
            error,
        );
    }
    if let Err(error) = state
        .kernel
        .trigger_v2
        .upsert_definition(resource.definition.clone(), &registry)
        .await
    {
        return apply_trigger_engine_error(error);
    }

    (StatusCode::CREATED, Json(serde_json::json!(resource)))
}

/// GET /api/v1/triggers/{id} — Load one persisted trigger definition.
pub async fn get_trigger_definition_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_trigger_definition_id(&id) {
        return response;
    }

    match load_trigger_definition_resource(&state, &id) {
        Ok(Some(resource)) => (StatusCode::OK, Json(serde_json::json!(resource))),
        Ok(None) => trigger_definition_not_found_response(),
        Err(response) => response,
    }
}

/// PUT /api/v1/triggers/{id} — Replace one persisted trigger definition.
pub async fn update_trigger_definition_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<serde_json::Value>, JsonRejection>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_trigger_definition_id(&id) {
        return response;
    }

    let Json(body) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };
    let _write_guard = TRIGGER_DEFINITION_WRITE_LOCK.lock().await;
    let registry = match trigger_compile_registry(&state).await {
        Ok(registry) => registry,
        Err(response) => return response,
    };
    let definition = match validate_trigger_value(&body, &registry) {
        Ok(definition) => definition,
        Err(error) => return trigger_compile_error_response(&error),
    };

    if definition.id != id {
        return workflow_v2_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Path ID and body ID must match",
            Some(serde_json::json!([{
                "path": "id",
                "expected": id,
                "actual": definition.id,
            }])),
        );
    }

    let store = trigger_definition_store(&state);
    let existing = match store.load(&id) {
        Ok(Some(resource)) => resource,
        Ok(None) => return trigger_definition_not_found_response(),
        Err(error) => {
            return trigger_store_load_error_response(
                "definition_load_failed",
                "Failed to load trigger definition",
                error,
            )
        }
    };
    if existing.origin.kind == TriggerOriginKind::Pack {
        return trigger_pack_conflict_response(&id);
    }

    let normalized = match normalize_trigger_definition(&definition, &registry) {
        Ok(normalized) => normalized,
        Err(error) => return trigger_compile_error_response(&error),
    };
    let resource = TriggerResponse {
        definition: canonicalize_trigger_definition(trigger_definition_from_normalized(normalized)),
        origin: existing.origin,
        forked_from: existing.forked_from,
        created_at: existing.created_at,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };

    if let Err(error) = store.persist(&resource) {
        return trigger_store_load_error_response(
            "definition_persist_failed",
            "Failed to persist trigger definition",
            error,
        );
    }
    if let Err(error) = state
        .kernel
        .trigger_v2
        .upsert_definition(resource.definition.clone(), &registry)
        .await
    {
        return apply_trigger_engine_error(error);
    }

    (StatusCode::OK, Json(serde_json::json!(resource)))
}

/// DELETE /api/v1/triggers/{id} — Delete one persisted trigger definition.
pub async fn delete_trigger_definition_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_trigger_definition_id(&id) {
        return response.into_response();
    }
    let _write_guard = TRIGGER_DEFINITION_WRITE_LOCK.lock().await;

    let store = trigger_definition_store(&state);
    let existing = match store.load(&id) {
        Ok(Some(resource)) => resource,
        Ok(None) => return trigger_definition_not_found_response().into_response(),
        Err(error) => {
            return trigger_store_load_error_response(
                "definition_load_failed",
                "Failed to load trigger definition",
                error,
            )
            .into_response()
        }
    };
    if existing.origin.kind == TriggerOriginKind::Pack {
        return trigger_pack_conflict_response(&id).into_response();
    }

    match store.delete(&id) {
        Ok(true) => {}
        Ok(false) => return trigger_definition_not_found_response().into_response(),
        Err(error) => {
            return trigger_store_load_error_response(
                "definition_delete_failed",
                "Failed to delete trigger definition",
                error,
            )
            .into_response()
        }
    }
    if let Err(error) = state.kernel.trigger_v2.remove_definition(&id).await {
        return trigger_store_load_error_response(
            "definition_reload_failed",
            "Failed to remove trigger definition from the runtime registry",
            error.to_string(),
        )
        .into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

/// POST /api/v1/triggers/validate — Validate a trigger definition.
pub async fn validate_trigger_definition_v1(
    State(state): State<Arc<AppState>>,
    payload: Result<Json<TriggerValidateRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };
    let registry = match trigger_compile_registry(&state).await {
        Ok(registry) => registry,
        Err(response) => return response,
    };
    let strict = request.strict.unwrap_or(false);

    let (issues, normalized) = match validate_trigger_value(&request.definition, &registry) {
        Ok(definition) => {
            let issues = validate_trigger_definition(&definition, &registry);
            let normalized = normalize_trigger_definition(&definition, &registry).ok();
            (issues, normalized)
        }
        Err(error) => (error.issues().to_vec(), None),
    };
    let valid = trigger_v2_is_valid(&issues, strict);

    (
        StatusCode::OK,
        Json(serde_json::json!(TriggerValidateResponse {
            valid,
            issues,
            normalized,
        })),
    )
}

/// POST /api/v1/triggers/compile — Compile a trigger definition into IR.
pub async fn compile_trigger_definition_v1(
    State(state): State<Arc<AppState>>,
    payload: Result<Json<TriggerCompileRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };
    let registry = match trigger_compile_registry(&state).await {
        Ok(registry) => registry,
        Err(response) => return response,
    };
    let definition = match validate_trigger_value(&request.definition, &registry) {
        Ok(definition) => definition,
        Err(error) => return trigger_compile_error_response(&error),
    };
    let normalized = match normalize_trigger_definition(&definition, &registry) {
        Ok(normalized) => normalized,
        Err(error) => return trigger_compile_error_response(&error),
    };

    match compile_trigger_ir_definition(&definition, &registry) {
        Ok(trigger_ir) => (
            StatusCode::OK,
            Json(serde_json::json!(TriggerCompileResponse {
                definition_id: definition.id,
                normalized,
                compiled: TriggerCompiledPayload { trigger_ir },
            })),
        ),
        Err(error) => trigger_compile_error_response(&error),
    }
}

/// GET /api/v1/triggers/{id}/compiled — Return the compiled IR for one persisted trigger definition.
pub async fn get_trigger_compiled_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_trigger_definition_id(&id) {
        return response;
    }

    let resource = match load_trigger_definition_resource(&state, &id) {
        Ok(Some(resource)) => resource,
        Ok(None) => return trigger_definition_not_found_response(),
        Err(response) => return response,
    };
    let registry = match trigger_compile_registry(&state).await {
        Ok(registry) => registry,
        Err(response) => return response,
    };
    let normalized = match normalize_trigger_definition(&resource.definition, &registry) {
        Ok(normalized) => normalized,
        Err(error) => return trigger_compile_error_response(&error),
    };
    match compile_trigger_ir_definition(&resource.definition, &registry) {
        Ok(trigger_ir) => (
            StatusCode::OK,
            Json(serde_json::json!(TriggerCompiledResponse {
                definition_id: resource.definition.id,
                normalized,
                compiled: TriggerCompiledPayload { trigger_ir },
            })),
        ),
        Err(error) => trigger_compile_error_response(&error),
    }
}

/// POST /api/v1/triggers/{id}/fork — Shadow a managed pack trigger with a user-owned copy.
pub async fn fork_trigger_definition_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<TriggerForkRequest>, JsonRejection>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_trigger_definition_id(&id) {
        return response;
    }

    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };
    let _write_guard = TRIGGER_DEFINITION_WRITE_LOCK.lock().await;
    if request.mode != "shadow" {
        return workflow_v2_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Trigger forks currently support only `shadow` mode",
            Some(serde_json::json!([{
                "path": "mode",
                "value": request.mode,
            }])),
        );
    }

    let store = trigger_definition_store(&state);
    let existing = match store.load(&id) {
        Ok(Some(resource)) => resource,
        Ok(None) => return trigger_definition_not_found_response(),
        Err(error) => {
            return trigger_store_load_error_response(
                "definition_load_failed",
                "Failed to load trigger definition",
                error,
            )
        }
    };
    if existing.origin.kind != TriggerOriginKind::Pack {
        return workflow_v2_error_response(
            StatusCode::CONFLICT,
            "invalid_fork_source",
            "Only managed pack trigger definitions can be forked",
            Some(serde_json::json!([{
                "trigger_id": id,
            }])),
        );
    }

    let timestamp = chrono::Utc::now().to_rfc3339();
    let resource = TriggerResponse {
        definition: existing.definition.clone(),
        origin: TriggerOrigin::user(),
        forked_from: Some(TriggerForkedFrom {
            kind: existing.origin.kind,
            pack_id: existing.origin.pack_id.clone(),
            pack_version: existing.origin.pack_version.clone(),
            resource_type: "trigger".to_string(),
            resource_id: id.clone(),
        }),
        created_at: timestamp.clone(),
        updated_at: timestamp,
    };

    if let Err(error) = store.persist(&resource) {
        return trigger_store_load_error_response(
            "definition_persist_failed",
            "Failed to persist trigger definition",
            error,
        );
    }
    let registry = match trigger_compile_registry(&state).await {
        Ok(registry) => registry,
        Err(response) => return response,
    };
    if let Err(error) = state
        .kernel
        .trigger_v2
        .upsert_definition(resource.definition.clone(), &registry)
        .await
    {
        return apply_trigger_engine_error(error);
    }

    (StatusCode::OK, Json(serde_json::json!(resource)))
}

/// GET /api/v1/triggers/{id}/runtime — Load runtime state for one trigger definition.
pub async fn get_trigger_runtime_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_trigger_definition_id(&id) {
        return response;
    }
    let resource = match load_trigger_definition_resource(&state, &id) {
        Ok(Some(resource)) => resource,
        Ok(None) => return trigger_definition_not_found_response(),
        Err(response) => return response,
    };
    let runtime = match trigger_runtime_status_or_default(&state, &resource.definition) {
        Ok(runtime) => runtime,
        Err(response) => return response,
    };

    (
        StatusCode::OK,
        Json(serde_json::json!(trigger_runtime_response(runtime))),
    )
}

/// POST /api/v1/triggers/{id}/enable — Enable one trigger definition.
pub async fn enable_trigger_definition_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    set_trigger_definition_enabled(state, id, true).await
}

/// POST /api/v1/triggers/{id}/disable — Disable one trigger definition.
pub async fn disable_trigger_definition_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    set_trigger_definition_enabled(state, id, false).await
}

async fn set_trigger_definition_enabled(
    state: Arc<AppState>,
    id: String,
    enabled: bool,
) -> axum::response::Response {
    if let Err(response) = ensure_safe_trigger_definition_id(&id) {
        return response.into_response();
    }
    let _write_guard = TRIGGER_DEFINITION_WRITE_LOCK.lock().await;
    let store = trigger_definition_store(&state);
    let mut resource = match store.load(&id) {
        Ok(Some(resource)) => resource,
        Ok(None) => return trigger_definition_not_found_response().into_response(),
        Err(error) => {
            return trigger_store_load_error_response(
                "definition_load_failed",
                "Failed to load trigger definition",
                error,
            )
            .into_response()
        }
    };
    if resource.origin.kind == TriggerOriginKind::Pack {
        return trigger_pack_conflict_response(&id).into_response();
    }

    resource.definition.enabled = enabled;
    resource.updated_at = chrono::Utc::now().to_rfc3339();
    if let Err(error) = store.persist(&resource) {
        return trigger_store_load_error_response(
            "definition_persist_failed",
            "Failed to persist trigger definition",
            error,
        )
        .into_response();
    }
    let registry = match trigger_compile_registry(&state).await {
        Ok(registry) => registry,
        Err(response) => return response.into_response(),
    };
    if let Err(error) = state
        .kernel
        .trigger_v2
        .upsert_definition(resource.definition.clone(), &registry)
        .await
    {
        return apply_trigger_engine_error(error).into_response();
    }

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!(AcceptedActionResponse {
            accepted: true,
            resource_id: id,
            status: "accepted".to_string(),
            session_id: None,
        })),
    )
        .into_response()
}

/// POST /api/v1/triggers/{id}/test — Evaluate a trigger against a synthetic event without dispatching.
pub async fn test_trigger_definition_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<TriggerTestRequest>, JsonRejection>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_trigger_definition_id(&id) {
        return response;
    }
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return workflow_v2_json_rejection(rejection),
    };
    let resource = match load_trigger_definition_resource(&state, &id) {
        Ok(Some(resource)) => resource,
        Ok(None) => return trigger_definition_not_found_response(),
        Err(response) => return response,
    };

    let evaluation = match state
        .kernel
        .trigger_v2
        .evaluate_trigger(&id, &request.event)
        .await
    {
        Ok(Some(evaluation)) => evaluation,
        Ok(None) => {
            let registry = match trigger_compile_registry(&state).await {
                Ok(registry) => registry,
                Err(response) => return response,
            };
            let compiled = match compile_trigger_ir_definition(&resource.definition, &registry) {
                Ok(compiled) => compiled,
                Err(error) => return trigger_compile_error_response(&error),
            };
            let runtime = match trigger_runtime_status_or_default(&state, &resource.definition) {
                Ok(runtime) => runtime,
                Err(response) => return response,
            };
            evaluate_compiled_trigger(&compiled, &runtime, &request.event, chrono::Utc::now())
        }
        Err(error) => {
            return trigger_store_load_error_response(
                "runtime_status_failed",
                "Failed to evaluate trigger",
                error.to_string(),
            )
        }
    };

    (
        StatusCode::OK,
        Json(serde_json::json!(TriggerTestResponse {
            matched: evaluation.matched,
            resolved_target: evaluation.resolved_target,
            would_dispatch: evaluation.would_dispatch,
            explanation: TriggerTestExplanation {
                r#match: evaluation.explanation.match_summary,
                target_kind: evaluation.explanation.target_kind,
                blocked_by: evaluation.explanation.blocked_by,
            },
        })),
    )
}

// ---------------------------------------------------------------------------
// Trigger routes
// ---------------------------------------------------------------------------

/// POST /api/triggers — Register a new event trigger.
pub async fn create_trigger(
    State(state): State<Arc<AppState>>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let agent_id_str = match req["agent_id"].as_str() {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing 'agent_id'"})),
            );
        }
    };

    let agent_id: AgentId = match agent_id_str.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent_id"})),
            );
        }
    };

    let pattern: TriggerPattern = match req.get("pattern") {
        Some(p) => match serde_json::from_value(p.clone()) {
            Ok(pat) => pat,
            Err(e) => {
                tracing::warn!("Invalid trigger pattern: {e}");
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Invalid trigger pattern"})),
                );
            }
        },
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing 'pattern'"})),
            );
        }
    };

    let prompt_template = req["prompt_template"]
        .as_str()
        .unwrap_or("Event: {{event}}")
        .to_string();
    let max_fires = req["max_fires"].as_u64().unwrap_or(0);

    match state
        .kernel
        .register_trigger(agent_id, pattern, prompt_template, max_fires)
    {
        Ok(trigger_id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "trigger_id": trigger_id.to_string(),
                "agent_id": agent_id.to_string(),
            })),
        ),
        Err(e) => {
            tracing::warn!("Trigger registration failed: {e}");
            (
                StatusCode::NOT_FOUND,
                Json(
                    serde_json::json!({"error": "Trigger registration failed (agent not found?)"}),
                ),
            )
        }
    }
}

/// GET /api/triggers — List all triggers (optionally filter by ?agent_id=...).
pub async fn list_triggers(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let agent_filter = params
        .get("agent_id")
        .and_then(|id| id.parse::<AgentId>().ok());

    let triggers = state.kernel.list_triggers(agent_filter);
    let list: Vec<serde_json::Value> = triggers
        .iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id.to_string(),
                "agent_id": t.agent_id.to_string(),
                "pattern": serde_json::to_value(&t.pattern).unwrap_or_default(),
                "prompt_template": t.prompt_template,
                "enabled": t.enabled,
                "fire_count": t.fire_count,
                "max_fires": t.max_fires,
                "created_at": t.created_at.to_rfc3339(),
            })
        })
        .collect();
    Json(list)
}

/// DELETE /api/triggers/:id — Remove a trigger.
pub async fn delete_trigger(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let trigger_id = TriggerId(match id.parse() {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid trigger ID"})),
            );
        }
    });

    if state.kernel.remove_trigger(trigger_id) {
        (
            StatusCode::OK,
            Json(serde_json::json!({"status": "removed", "trigger_id": id})),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Trigger not found"})),
        )
    }
}

// ---------------------------------------------------------------------------
// Profile + Mode endpoints
// ---------------------------------------------------------------------------

/// GET /api/profiles — List all tool profiles and their tool lists.
pub async fn list_profiles() -> impl IntoResponse {
    use openfang_types::agent::ToolProfile;

    let profiles = [
        ("minimal", ToolProfile::Minimal),
        ("coding", ToolProfile::Coding),
        ("research", ToolProfile::Research),
        ("messaging", ToolProfile::Messaging),
        ("automation", ToolProfile::Automation),
        ("full", ToolProfile::Full),
    ];

    let result: Vec<serde_json::Value> = profiles
        .iter()
        .map(|(name, profile)| {
            serde_json::json!({
                "name": name,
                "tools": profile.tools(),
            })
        })
        .collect();

    Json(result)
}

/// PUT /api/agents/:id/mode — Change an agent's operational mode.
pub async fn set_agent_mode(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<SetModeRequest>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            );
        }
    };

    match state.kernel.set_agent_mode(agent_id, body.mode) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "updated",
                "agent_id": id,
                "mode": body.mode,
            })),
        ),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Agent not found"})),
        ),
    }
}

// ---------------------------------------------------------------------------
// Version endpoint
// ---------------------------------------------------------------------------

/// GET /api/version — Build & version info.
pub async fn version() -> impl IntoResponse {
    Json(serde_json::json!({
        "name": "openfang",
        "version": env!("CARGO_PKG_VERSION"),
        "build_date": option_env!("BUILD_DATE").unwrap_or("dev"),
        "git_sha": option_env!("GIT_SHA").unwrap_or("unknown"),
        "rust_version": option_env!("RUSTC_VERSION").unwrap_or("unknown"),
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    }))
}

// ---------------------------------------------------------------------------
// Single agent detail + SSE streaming
// ---------------------------------------------------------------------------

/// GET /api/agents/:id — Get a single agent's detailed info.
pub async fn get_agent_legacy(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            );
        }
    };

    let entry = match state.kernel.registry.get(agent_id) {
        Some(e) => e,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent not found"})),
            );
        }
    };
    let runtime_projection = state
        .kernel
        .runtime_stores
        .agent_runtime
        .get_agent_runtime(agent_id)
        .ok()
        .flatten();
    let state_value = runtime_projection
        .as_ref()
        .map(|record| format!("{:?}", record.state))
        .unwrap_or_else(|| format!("{:?}", entry.state));
    let mode_value = runtime_projection
        .as_ref()
        .map(|record| record.mode)
        .unwrap_or(entry.mode);
    let session_id = runtime_projection
        .as_ref()
        .and_then(|record| record.active_session_id)
        .unwrap_or(entry.session_id);

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "id": entry.id.to_string(),
            "name": entry.name,
            "state": state_value,
            "mode": mode_value,
            "profile": entry.manifest.profile,
            "created_at": entry.created_at.to_rfc3339(),
            "session_id": session_id.0.to_string(),
            "model": {
                "provider": entry.manifest.model.provider,
                "model": entry.manifest.model.model,
            },
            "capabilities": {
                "tools": entry.manifest.capabilities.tools,
                "network": entry.manifest.capabilities.network,
            },
            "description": entry.manifest.description,
            "tags": entry.manifest.tags,
            "identity": {
                "emoji": entry.identity.emoji,
                "avatar_url": entry.identity.avatar_url,
                "color": entry.identity.color,
            },
            "skills": entry.manifest.skills,
            "skills_mode": if entry.manifest.skills.is_empty() { "all" } else { "allowlist" },
            "mcp_servers": entry.manifest.mcp_servers,
            "mcp_servers_mode": if entry.manifest.mcp_servers.is_empty() { "all" } else { "allowlist" },
            "fallback_models": entry.manifest.fallback_models,
        })),
    )
}

/// POST /api/agents/:id/message/stream — SSE streaming response.
pub async fn send_message_stream(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<LegacyMessageRequest>,
) -> axum::response::Response {
    use axum::response::sse::{Event, Sse};
    use futures::stream;
    use openfang_runtime::llm_driver::StreamEvent;

    // SECURITY: Reject oversized messages to prevent OOM / LLM token abuse.
    const MAX_MESSAGE_SIZE: usize = 64 * 1024; // 64KB
    if req.message.len() > MAX_MESSAGE_SIZE {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({"error": "Message too large (max 64KB)"})),
        )
            .into_response();
    }

    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            )
                .into_response();
        }
    };

    if state.kernel.registry.get(agent_id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Agent not found"})),
        )
            .into_response();
    }

    let kernel_handle: Arc<dyn KernelHandle> = state.kernel.clone() as Arc<dyn KernelHandle>;
    let (rx, _handle) = match state.kernel.send_message_streaming(
        agent_id,
        &req.message,
        Some(kernel_handle),
        req.sender_id,
        req.sender_name,
        None, // SSE streaming doesn't support image attachments yet
    ) {
        Ok(pair) => pair,
        Err(e) => {
            tracing::warn!("Streaming message failed for agent {id}: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Streaming message failed"})),
            )
                .into_response();
        }
    };

    let sse_stream = stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Some(event) => {
                let sse_event: Result<Event, std::convert::Infallible> = Ok(match event {
                    StreamEvent::TextDelta { text } => Event::default()
                        .event("chunk")
                        .json_data(serde_json::json!({"content": text, "done": false}))
                        .unwrap_or_else(|_| Event::default().data("error")),
                    StreamEvent::ToolUseStart { name, .. } => Event::default()
                        .event("tool_use")
                        .json_data(serde_json::json!({"tool": name}))
                        .unwrap_or_else(|_| Event::default().data("error")),
                    StreamEvent::ToolUseEnd { name, input, .. } => Event::default()
                        .event("tool_result")
                        .json_data(serde_json::json!({"tool": name, "input": input}))
                        .unwrap_or_else(|_| Event::default().data("error")),
                    StreamEvent::ContentComplete { usage, .. } => Event::default()
                        .event("done")
                        .json_data(serde_json::json!({
                            "done": true,
                            "usage": {
                                "input_tokens": usage.input_tokens,
                                "output_tokens": usage.output_tokens,
                            }
                        }))
                        .unwrap_or_else(|_| Event::default().data("error")),
                    StreamEvent::PhaseChange { phase, detail } => Event::default()
                        .event("phase")
                        .json_data(serde_json::json!({
                            "phase": phase,
                            "detail": detail,
                        }))
                        .unwrap_or_else(|_| Event::default().data("error")),
                    _ => Event::default().comment("skip"),
                });
                Some((sse_event, rx))
            }
            None => None,
        }
    });

    Sse::new(sse_stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

// ---------------------------------------------------------------------------
// Channel status endpoints — data-driven registry for all 40 adapters
// ---------------------------------------------------------------------------

/// Field type for the channel configuration form.
#[derive(Clone, Copy, PartialEq)]
enum FieldType {
    Secret,
    Text,
    Number,
    List,
}

impl FieldType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Secret => "secret",
            Self::Text => "text",
            Self::Number => "number",
            Self::List => "list",
        }
    }
}

/// A single configurable field for a channel adapter.
#[derive(Clone)]
struct ChannelField {
    key: &'static str,
    label: &'static str,
    field_type: FieldType,
    env_var: Option<&'static str>,
    required: bool,
    placeholder: &'static str,
    /// If true, this field is hidden under "Show Advanced" in the UI.
    advanced: bool,
}

/// Metadata for one channel adapter.
struct ChannelMeta {
    name: &'static str,
    display_name: &'static str,
    icon: &'static str,
    description: &'static str,
    category: &'static str,
    difficulty: &'static str,
    setup_time: &'static str,
    /// One-line quick setup hint shown in the simple form view.
    quick_setup: &'static str,
    /// Setup type: "form" (default), "qr" (QR code scan + form fallback).
    setup_type: &'static str,
    fields: &'static [ChannelField],
    setup_steps: &'static [&'static str],
    config_template: &'static str,
}

const CHANNEL_REGISTRY: &[ChannelMeta] = &[
    // ── Messaging (12) ──────────────────────────────────────────────
    ChannelMeta {
        name: "telegram", display_name: "Telegram", icon: "TG",
        description: "Telegram Bot API — long-polling adapter",
        category: "messaging", difficulty: "Easy", setup_time: "~2 min",
        quick_setup: "Paste your bot token from @BotFather",
        setup_type: "form",
        fields: &[
            ChannelField { key: "bot_token_env", label: "Bot Token", field_type: FieldType::Secret, env_var: Some("TELEGRAM_BOT_TOKEN"), required: true, placeholder: "123456:ABC-DEF...", advanced: false },
            ChannelField { key: "allowed_users", label: "Allowed User IDs", field_type: FieldType::List, env_var: None, required: false, placeholder: "12345, 67890", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
            ChannelField { key: "poll_interval_secs", label: "Poll Interval (sec)", field_type: FieldType::Number, env_var: None, required: false, placeholder: "1", advanced: true },
        ],
        setup_steps: &["Open @BotFather on Telegram", "Send /newbot and follow the prompts", "Paste the token below"],
        config_template: "[channels.telegram]\nbot_token_env = \"TELEGRAM_BOT_TOKEN\"",
    },
    ChannelMeta {
        name: "discord", display_name: "Discord", icon: "DC",
        description: "Discord Gateway bot adapter",
        category: "messaging", difficulty: "Easy", setup_time: "~3 min",
        quick_setup: "Paste your bot token from the Discord Developer Portal",
        setup_type: "form",
        fields: &[
            ChannelField { key: "bot_token_env", label: "Bot Token", field_type: FieldType::Secret, env_var: Some("DISCORD_BOT_TOKEN"), required: true, placeholder: "MTIz...", advanced: false },
            ChannelField { key: "allowed_guilds", label: "Allowed Guild IDs", field_type: FieldType::List, env_var: None, required: false, placeholder: "123456789, 987654321", advanced: true },
            ChannelField { key: "allowed_users", label: "Allowed User IDs", field_type: FieldType::List, env_var: None, required: false, placeholder: "123456789, 987654321", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
            ChannelField { key: "intents", label: "Intents Bitmask", field_type: FieldType::Number, env_var: None, required: false, placeholder: "37376", advanced: true },
        ],
        setup_steps: &["Go to discord.com/developers/applications", "Create a bot and copy the token", "Paste it below"],
        config_template: "[channels.discord]\nbot_token_env = \"DISCORD_BOT_TOKEN\"",
    },
    ChannelMeta {
        name: "slack", display_name: "Slack", icon: "SL",
        description: "Slack Socket Mode + Events API",
        category: "messaging", difficulty: "Medium", setup_time: "~5 min",
        quick_setup: "Paste your App Token and Bot Token from api.slack.com",
        setup_type: "form",
        fields: &[
            ChannelField { key: "app_token_env", label: "App Token (xapp-)", field_type: FieldType::Secret, env_var: Some("SLACK_APP_TOKEN"), required: true, placeholder: "xapp-1-...", advanced: false },
            ChannelField { key: "bot_token_env", label: "Bot Token (xoxb-)", field_type: FieldType::Secret, env_var: Some("SLACK_BOT_TOKEN"), required: true, placeholder: "xoxb-...", advanced: false },
            ChannelField { key: "allowed_channels", label: "Allowed Channel IDs", field_type: FieldType::List, env_var: None, required: false, placeholder: "C01234, C56789", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create app at api.slack.com/apps", "Enable Socket Mode and copy App Token", "Copy Bot Token from OAuth & Permissions"],
        config_template: "[channels.slack]\napp_token_env = \"SLACK_APP_TOKEN\"\nbot_token_env = \"SLACK_BOT_TOKEN\"",
    },
    ChannelMeta {
        name: "whatsapp", display_name: "WhatsApp", icon: "WA",
        description: "Connect your personal WhatsApp via QR scan",
        category: "messaging", difficulty: "Easy", setup_time: "~1 min",
        quick_setup: "Scan QR code with your phone — no developer account needed",
        setup_type: "qr",
        fields: &[
            // Business API fallback fields — all advanced (hidden behind "Use Business API" toggle)
            ChannelField { key: "access_token_env", label: "Access Token", field_type: FieldType::Secret, env_var: Some("WHATSAPP_ACCESS_TOKEN"), required: false, placeholder: "EAAx...", advanced: true },
            ChannelField { key: "phone_number_id", label: "Phone Number ID", field_type: FieldType::Text, env_var: None, required: false, placeholder: "1234567890", advanced: true },
            ChannelField { key: "verify_token_env", label: "Verify Token", field_type: FieldType::Secret, env_var: Some("WHATSAPP_VERIFY_TOKEN"), required: false, placeholder: "my-verify-token", advanced: true },
            ChannelField { key: "webhook_port", label: "Webhook Port", field_type: FieldType::Number, env_var: None, required: false, placeholder: "8443", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Open WhatsApp on your phone", "Go to Linked Devices", "Tap Link a Device and scan the QR code"],
        config_template: "[channels.whatsapp]\naccess_token_env = \"WHATSAPP_ACCESS_TOKEN\"\nphone_number_id = \"\"",
    },
    ChannelMeta {
        name: "signal", display_name: "Signal", icon: "SG",
        description: "Signal via signal-cli REST API",
        category: "messaging", difficulty: "Medium", setup_time: "~10 min",
        quick_setup: "Enter your signal-cli API URL",
        setup_type: "form",
        fields: &[
            ChannelField { key: "api_url", label: "signal-cli API URL", field_type: FieldType::Text, env_var: None, required: true, placeholder: "http://localhost:8080", advanced: false },
            ChannelField { key: "phone_number", label: "Phone Number", field_type: FieldType::Text, env_var: None, required: true, placeholder: "+1234567890", advanced: false },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Install signal-cli-rest-api", "Enter the API URL and your phone number"],
        config_template: "[channels.signal]\napi_url = \"http://localhost:8080\"\nphone_number = \"\"",
    },
    ChannelMeta {
        name: "matrix", display_name: "Matrix", icon: "MX",
        description: "Matrix/Element bot via homeserver",
        category: "messaging", difficulty: "Easy", setup_time: "~3 min",
        quick_setup: "Paste your access token and homeserver URL",
        setup_type: "form",
        fields: &[
            ChannelField { key: "access_token_env", label: "Access Token", field_type: FieldType::Secret, env_var: Some("MATRIX_ACCESS_TOKEN"), required: true, placeholder: "syt_...", advanced: false },
            ChannelField { key: "homeserver_url", label: "Homeserver URL", field_type: FieldType::Text, env_var: None, required: true, placeholder: "https://matrix.org", advanced: false },
            ChannelField { key: "user_id", label: "Bot User ID", field_type: FieldType::Text, env_var: None, required: false, placeholder: "@openfang:matrix.org", advanced: true },
            ChannelField { key: "allowed_rooms", label: "Allowed Room IDs", field_type: FieldType::List, env_var: None, required: false, placeholder: "!abc:matrix.org", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create a bot account on your homeserver", "Generate an access token", "Paste token and homeserver URL below"],
        config_template: "[channels.matrix]\naccess_token_env = \"MATRIX_ACCESS_TOKEN\"\nhomeserver_url = \"https://matrix.org\"",
    },
    ChannelMeta {
        name: "email", display_name: "Email", icon: "EM",
        description: "IMAP/SMTP email adapter",
        category: "messaging", difficulty: "Easy", setup_time: "~3 min",
        quick_setup: "Enter your email, password, and server hosts",
        setup_type: "form",
        fields: &[
            ChannelField { key: "username", label: "Email Address", field_type: FieldType::Text, env_var: None, required: true, placeholder: "bot@example.com", advanced: false },
            ChannelField { key: "password_env", label: "Password / App Password", field_type: FieldType::Secret, env_var: Some("EMAIL_PASSWORD"), required: true, placeholder: "app-password", advanced: false },
            ChannelField { key: "imap_host", label: "IMAP Host", field_type: FieldType::Text, env_var: None, required: true, placeholder: "imap.gmail.com", advanced: false },
            ChannelField { key: "smtp_host", label: "SMTP Host", field_type: FieldType::Text, env_var: None, required: true, placeholder: "smtp.gmail.com", advanced: false },
            ChannelField { key: "imap_port", label: "IMAP Port", field_type: FieldType::Number, env_var: None, required: false, placeholder: "993", advanced: true },
            ChannelField { key: "smtp_port", label: "SMTP Port", field_type: FieldType::Number, env_var: None, required: false, placeholder: "587", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Enable IMAP on your email account", "Generate an app password if using Gmail", "Fill in email, password, and hosts below"],
        config_template: "[channels.email]\nimap_host = \"imap.gmail.com\"\nsmtp_host = \"smtp.gmail.com\"\npassword_env = \"EMAIL_PASSWORD\"",
    },
    ChannelMeta {
        name: "line", display_name: "LINE", icon: "LN",
        description: "LINE Messaging API adapter",
        category: "messaging", difficulty: "Easy", setup_time: "~3 min",
        quick_setup: "Paste your Channel Secret and Access Token",
        setup_type: "form",
        fields: &[
            ChannelField { key: "channel_secret_env", label: "Channel Secret", field_type: FieldType::Secret, env_var: Some("LINE_CHANNEL_SECRET"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "access_token_env", label: "Channel Access Token", field_type: FieldType::Secret, env_var: Some("LINE_CHANNEL_ACCESS_TOKEN"), required: true, placeholder: "xyz789...", advanced: false },
            ChannelField { key: "webhook_port", label: "Webhook Port", field_type: FieldType::Number, env_var: None, required: false, placeholder: "8450", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create a Messaging API channel at LINE Developers", "Copy Channel Secret and Access Token", "Paste them below"],
        config_template: "[channels.line]\nchannel_secret_env = \"LINE_CHANNEL_SECRET\"\naccess_token_env = \"LINE_CHANNEL_ACCESS_TOKEN\"",
    },
    ChannelMeta {
        name: "viber", display_name: "Viber", icon: "VB",
        description: "Viber Bot API adapter",
        category: "messaging", difficulty: "Easy", setup_time: "~2 min",
        quick_setup: "Paste your auth token from partners.viber.com",
        setup_type: "form",
        fields: &[
            ChannelField { key: "auth_token_env", label: "Auth Token", field_type: FieldType::Secret, env_var: Some("VIBER_AUTH_TOKEN"), required: true, placeholder: "4dc...", advanced: false },
            ChannelField { key: "webhook_url", label: "Webhook URL", field_type: FieldType::Text, env_var: None, required: false, placeholder: "https://your-domain.com/viber", advanced: true },
            ChannelField { key: "webhook_port", label: "Webhook Port", field_type: FieldType::Number, env_var: None, required: false, placeholder: "8451", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create a bot at partners.viber.com", "Copy the auth token", "Paste it below"],
        config_template: "[channels.viber]\nauth_token_env = \"VIBER_AUTH_TOKEN\"",
    },
    ChannelMeta {
        name: "messenger", display_name: "Messenger", icon: "FB",
        description: "Facebook Messenger Platform adapter",
        category: "messaging", difficulty: "Medium", setup_time: "~10 min",
        quick_setup: "Paste your Page Access Token from developers.facebook.com",
        setup_type: "form",
        fields: &[
            ChannelField { key: "page_token_env", label: "Page Access Token", field_type: FieldType::Secret, env_var: Some("MESSENGER_PAGE_TOKEN"), required: true, placeholder: "EAAx...", advanced: false },
            ChannelField { key: "verify_token_env", label: "Verify Token", field_type: FieldType::Secret, env_var: Some("MESSENGER_VERIFY_TOKEN"), required: false, placeholder: "my-verify-token", advanced: true },
            ChannelField { key: "webhook_port", label: "Webhook Port", field_type: FieldType::Number, env_var: None, required: false, placeholder: "8452", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create a Facebook App and add Messenger", "Generate a Page Access Token", "Paste it below"],
        config_template: "[channels.messenger]\npage_token_env = \"MESSENGER_PAGE_TOKEN\"",
    },
    ChannelMeta {
        name: "threema", display_name: "Threema", icon: "3M",
        description: "Threema Gateway adapter",
        category: "messaging", difficulty: "Easy", setup_time: "~3 min",
        quick_setup: "Paste your Gateway ID and API secret",
        setup_type: "form",
        fields: &[
            ChannelField { key: "secret_env", label: "API Secret", field_type: FieldType::Secret, env_var: Some("THREEMA_SECRET"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "threema_id", label: "Gateway ID", field_type: FieldType::Text, env_var: None, required: true, placeholder: "*MYID01", advanced: false },
            ChannelField { key: "webhook_port", label: "Webhook Port", field_type: FieldType::Number, env_var: None, required: false, placeholder: "8454", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Register at gateway.threema.ch", "Copy your ID and API secret", "Paste them below"],
        config_template: "[channels.threema]\nthreema_id = \"\"\nsecret_env = \"THREEMA_SECRET\"",
    },
    ChannelMeta {
        name: "keybase", display_name: "Keybase", icon: "KB",
        description: "Keybase chat bot adapter",
        category: "messaging", difficulty: "Easy", setup_time: "~3 min",
        quick_setup: "Enter your username and paper key",
        setup_type: "form",
        fields: &[
            ChannelField { key: "username", label: "Username", field_type: FieldType::Text, env_var: None, required: true, placeholder: "openfang_bot", advanced: false },
            ChannelField { key: "paperkey_env", label: "Paper Key", field_type: FieldType::Secret, env_var: Some("KEYBASE_PAPERKEY"), required: true, placeholder: "word1 word2 word3...", advanced: false },
            ChannelField { key: "allowed_teams", label: "Allowed Teams", field_type: FieldType::List, env_var: None, required: false, placeholder: "team1, team2", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create a Keybase bot account", "Generate a paper key", "Enter username and paper key below"],
        config_template: "[channels.keybase]\nusername = \"\"\npaperkey_env = \"KEYBASE_PAPERKEY\"",
    },
    // ── Social (5) ──────────────────────────────────────────────────
    ChannelMeta {
        name: "reddit", display_name: "Reddit", icon: "RD",
        description: "Reddit API bot adapter",
        category: "social", difficulty: "Medium", setup_time: "~5 min",
        quick_setup: "Paste your Client ID, Secret, and bot credentials",
        setup_type: "form",
        fields: &[
            ChannelField { key: "client_id", label: "Client ID", field_type: FieldType::Text, env_var: None, required: true, placeholder: "abc123def", advanced: false },
            ChannelField { key: "client_secret_env", label: "Client Secret", field_type: FieldType::Secret, env_var: Some("REDDIT_CLIENT_SECRET"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "username", label: "Bot Username", field_type: FieldType::Text, env_var: None, required: true, placeholder: "openfang_bot", advanced: false },
            ChannelField { key: "password_env", label: "Bot Password", field_type: FieldType::Secret, env_var: Some("REDDIT_PASSWORD"), required: true, placeholder: "password", advanced: false },
            ChannelField { key: "subreddits", label: "Subreddits", field_type: FieldType::List, env_var: None, required: false, placeholder: "openfang, rust", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create a Reddit app at reddit.com/prefs/apps (script type)", "Copy Client ID and Secret", "Enter bot credentials below"],
        config_template: "[channels.reddit]\nclient_id = \"\"\nclient_secret_env = \"REDDIT_CLIENT_SECRET\"\nusername = \"\"\npassword_env = \"REDDIT_PASSWORD\"",
    },
    ChannelMeta {
        name: "mastodon", display_name: "Mastodon", icon: "MA",
        description: "Mastodon Streaming API adapter",
        category: "social", difficulty: "Easy", setup_time: "~2 min",
        quick_setup: "Paste your access token from Settings > Development",
        setup_type: "form",
        fields: &[
            ChannelField { key: "access_token_env", label: "Access Token", field_type: FieldType::Secret, env_var: Some("MASTODON_ACCESS_TOKEN"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "instance_url", label: "Instance URL", field_type: FieldType::Text, env_var: None, required: true, placeholder: "https://mastodon.social", advanced: false },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Go to Settings > Development on your instance", "Create an app and copy the token", "Paste it below"],
        config_template: "[channels.mastodon]\ninstance_url = \"https://mastodon.social\"\naccess_token_env = \"MASTODON_ACCESS_TOKEN\"",
    },
    ChannelMeta {
        name: "bluesky", display_name: "Bluesky", icon: "BS",
        description: "Bluesky/AT Protocol adapter",
        category: "social", difficulty: "Easy", setup_time: "~1 min",
        quick_setup: "Enter your handle and app password",
        setup_type: "form",
        fields: &[
            ChannelField { key: "identifier", label: "Handle", field_type: FieldType::Text, env_var: None, required: true, placeholder: "user.bsky.social", advanced: false },
            ChannelField { key: "app_password_env", label: "App Password", field_type: FieldType::Secret, env_var: Some("BLUESKY_APP_PASSWORD"), required: true, placeholder: "xxxx-xxxx-xxxx-xxxx", advanced: false },
            ChannelField { key: "service_url", label: "PDS URL", field_type: FieldType::Text, env_var: None, required: false, placeholder: "https://bsky.social", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Go to Settings > App Passwords in Bluesky", "Create an app password", "Enter handle and password below"],
        config_template: "[channels.bluesky]\nidentifier = \"\"\napp_password_env = \"BLUESKY_APP_PASSWORD\"",
    },
    ChannelMeta {
        name: "linkedin", display_name: "LinkedIn", icon: "LI",
        description: "LinkedIn Messaging API adapter",
        category: "social", difficulty: "Hard", setup_time: "~15 min",
        quick_setup: "Paste your OAuth2 access token and Organization ID",
        setup_type: "form",
        fields: &[
            ChannelField { key: "access_token_env", label: "Access Token", field_type: FieldType::Secret, env_var: Some("LINKEDIN_ACCESS_TOKEN"), required: true, placeholder: "AQV...", advanced: false },
            ChannelField { key: "organization_id", label: "Organization ID", field_type: FieldType::Text, env_var: None, required: true, placeholder: "12345678", advanced: false },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create a LinkedIn App at linkedin.com/developers", "Generate an OAuth2 token", "Enter token and org ID below"],
        config_template: "[channels.linkedin]\naccess_token_env = \"LINKEDIN_ACCESS_TOKEN\"\norganization_id = \"\"",
    },
    ChannelMeta {
        name: "nostr", display_name: "Nostr", icon: "NS",
        description: "Nostr relay protocol adapter",
        category: "social", difficulty: "Easy", setup_time: "~2 min",
        quick_setup: "Paste your private key (nsec or hex)",
        setup_type: "form",
        fields: &[
            ChannelField { key: "private_key_env", label: "Private Key", field_type: FieldType::Secret, env_var: Some("NOSTR_PRIVATE_KEY"), required: true, placeholder: "nsec1...", advanced: false },
            ChannelField { key: "relays", label: "Relay URLs", field_type: FieldType::List, env_var: None, required: false, placeholder: "wss://relay.damus.io", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Generate or use an existing Nostr keypair", "Paste your private key below"],
        config_template: "[channels.nostr]\nprivate_key_env = \"NOSTR_PRIVATE_KEY\"",
    },
    // ── Enterprise (10) ─────────────────────────────────────────────
    ChannelMeta {
        name: "teams", display_name: "Microsoft Teams", icon: "MS",
        description: "Teams Bot Framework adapter",
        category: "enterprise", difficulty: "Medium", setup_time: "~10 min",
        quick_setup: "Paste your Azure Bot App ID and Password",
        setup_type: "form",
        fields: &[
            ChannelField { key: "app_id", label: "App ID", field_type: FieldType::Text, env_var: None, required: true, placeholder: "00000000-0000-...", advanced: false },
            ChannelField { key: "app_password_env", label: "App Password", field_type: FieldType::Secret, env_var: Some("TEAMS_APP_PASSWORD"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "webhook_port", label: "Webhook Port", field_type: FieldType::Number, env_var: None, required: false, placeholder: "3978", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create an Azure Bot registration", "Copy App ID and generate a password", "Paste them below"],
        config_template: "[channels.teams]\napp_id = \"\"\napp_password_env = \"TEAMS_APP_PASSWORD\"",
    },
    ChannelMeta {
        name: "mattermost", display_name: "Mattermost", icon: "MM",
        description: "Mattermost WebSocket adapter",
        category: "enterprise", difficulty: "Easy", setup_time: "~2 min",
        quick_setup: "Paste your bot token and server URL",
        setup_type: "form",
        fields: &[
            ChannelField { key: "server_url", label: "Server URL", field_type: FieldType::Text, env_var: None, required: true, placeholder: "https://mattermost.example.com", advanced: false },
            ChannelField { key: "token_env", label: "Bot Token", field_type: FieldType::Secret, env_var: Some("MATTERMOST_TOKEN"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "allowed_channels", label: "Allowed Channels", field_type: FieldType::List, env_var: None, required: false, placeholder: "abc123, def456", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create a bot in System Console > Bot Accounts", "Copy the token", "Enter server URL and token below"],
        config_template: "[channels.mattermost]\nserver_url = \"\"\ntoken_env = \"MATTERMOST_TOKEN\"",
    },
    ChannelMeta {
        name: "google_chat", display_name: "Google Chat", icon: "GC",
        description: "Google Chat service account adapter",
        category: "enterprise", difficulty: "Hard", setup_time: "~15 min",
        quick_setup: "Enter path to your service account JSON key",
        setup_type: "form",
        fields: &[
            ChannelField { key: "service_account_env", label: "Service Account JSON", field_type: FieldType::Secret, env_var: Some("GOOGLE_CHAT_SERVICE_ACCOUNT"), required: true, placeholder: "/path/to/key.json", advanced: false },
            ChannelField { key: "space_ids", label: "Space IDs", field_type: FieldType::List, env_var: None, required: false, placeholder: "spaces/AAAA", advanced: true },
            ChannelField { key: "webhook_port", label: "Webhook Port", field_type: FieldType::Number, env_var: None, required: false, placeholder: "8444", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create a Google Cloud project with Chat API", "Download service account JSON key", "Enter the path below"],
        config_template: "[channels.google_chat]\nservice_account_env = \"GOOGLE_CHAT_SERVICE_ACCOUNT\"",
    },
    ChannelMeta {
        name: "webex", display_name: "Webex", icon: "WX",
        description: "Cisco Webex bot adapter",
        category: "enterprise", difficulty: "Easy", setup_time: "~2 min",
        quick_setup: "Paste your bot token from developer.webex.com",
        setup_type: "form",
        fields: &[
            ChannelField { key: "bot_token_env", label: "Bot Token", field_type: FieldType::Secret, env_var: Some("WEBEX_BOT_TOKEN"), required: true, placeholder: "NjI...", advanced: false },
            ChannelField { key: "allowed_rooms", label: "Allowed Rooms", field_type: FieldType::List, env_var: None, required: false, placeholder: "Y2lz...", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create a bot at developer.webex.com", "Copy the token", "Paste it below"],
        config_template: "[channels.webex]\nbot_token_env = \"WEBEX_BOT_TOKEN\"",
    },
    ChannelMeta {
        name: "feishu", display_name: "Feishu/Lark", icon: "FS",
        description: "Feishu/Lark Open Platform adapter (supports China & International)",
        category: "enterprise", difficulty: "Easy", setup_time: "~3 min",
        quick_setup: "Paste your App ID and App Secret",
        setup_type: "form",
        fields: &[
            ChannelField { key: "app_id", label: "App ID", field_type: FieldType::Text, env_var: None, required: true, placeholder: "cli_abc123", advanced: false },
            ChannelField { key: "app_secret_env", label: "App Secret", field_type: FieldType::Secret, env_var: Some("FEISHU_APP_SECRET"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "region", label: "Region", field_type: FieldType::Text, env_var: None, required: false, placeholder: "cn or intl", advanced: false },
            ChannelField { key: "webhook_port", label: "Webhook Port", field_type: FieldType::Number, env_var: None, required: false, placeholder: "8453", advanced: true },
            ChannelField { key: "webhook_path", label: "Webhook Path", field_type: FieldType::Text, env_var: None, required: false, placeholder: "/feishu/webhook", advanced: true },
            ChannelField { key: "verification_token", label: "Verification Token", field_type: FieldType::Text, env_var: None, required: false, placeholder: "verify-token", advanced: true },
            ChannelField { key: "encrypt_key_env", label: "Encrypt Key", field_type: FieldType::Secret, env_var: Some("FEISHU_ENCRYPT_KEY"), required: false, placeholder: "encrypt-key", advanced: true },
            ChannelField { key: "bot_names", label: "Bot Names", field_type: FieldType::List, env_var: None, required: false, placeholder: "MyBot, Assistant", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create an app at open.feishu.cn (CN) or open.larksuite.com (International)", "Copy App ID and Secret", "Set region: cn (Feishu) or intl (Lark)"],
        config_template: "[channels.feishu]\napp_id = \"\"\napp_secret_env = \"FEISHU_APP_SECRET\"\nregion = \"cn\"",
    },
    ChannelMeta {
        name: "dingtalk", display_name: "DingTalk", icon: "DT",
        description: "DingTalk Robot API adapter",
        category: "enterprise", difficulty: "Easy", setup_time: "~3 min",
        quick_setup: "Paste your webhook token and signing secret",
        setup_type: "form",
        fields: &[
            ChannelField { key: "access_token_env", label: "Access Token", field_type: FieldType::Secret, env_var: Some("DINGTALK_ACCESS_TOKEN"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "secret_env", label: "Signing Secret", field_type: FieldType::Secret, env_var: Some("DINGTALK_SECRET"), required: true, placeholder: "SEC...", advanced: false },
            ChannelField { key: "webhook_port", label: "Webhook Port", field_type: FieldType::Number, env_var: None, required: false, placeholder: "8457", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create a robot in your DingTalk group", "Copy the token and signing secret", "Paste them below"],
        config_template: "[channels.dingtalk]\naccess_token_env = \"DINGTALK_ACCESS_TOKEN\"\nsecret_env = \"DINGTALK_SECRET\"",
    },
    ChannelMeta {
        name: "dingtalk_stream", display_name: "DingTalk Stream", icon: "DS",
        description: "DingTalk Stream Mode (WebSocket long-connection)",
        category: "enterprise", difficulty: "Easy", setup_time: "~5 min",
        quick_setup: "Create an Enterprise Internal App with Stream Mode enabled",
        setup_type: "form",
        fields: &[
            ChannelField { key: "app_key_env", label: "App Key", field_type: FieldType::Secret, env_var: Some("DINGTALK_APP_KEY"), required: true, placeholder: "ding...", advanced: false },
            ChannelField { key: "app_secret_env", label: "App Secret", field_type: FieldType::Secret, env_var: Some("DINGTALK_APP_SECRET"), required: true, placeholder: "uAn4...", advanced: false },
            ChannelField { key: "robot_code_env", label: "Robot Code", field_type: FieldType::Text, env_var: Some("DINGTALK_ROBOT_CODE"), required: false, placeholder: "ding... (same as App Key)", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create an Enterprise Internal App in DingTalk Open Platform", "Enable Stream Mode in the app settings", "Add robot capability and configure permissions", "Copy App Key and App Secret below"],
        config_template: "[channels.dingtalk_stream]\napp_key_env = \"DINGTALK_APP_KEY\"\napp_secret_env = \"DINGTALK_APP_SECRET\"",
    },
    ChannelMeta {
        name: "pumble", display_name: "Pumble", icon: "PB",
        description: "Pumble bot adapter",
        category: "enterprise", difficulty: "Easy", setup_time: "~1 min",
        quick_setup: "Paste your bot token",
        setup_type: "form",
        fields: &[
            ChannelField { key: "bot_token_env", label: "Bot Token", field_type: FieldType::Secret, env_var: Some("PUMBLE_BOT_TOKEN"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "webhook_port", label: "Webhook Port", field_type: FieldType::Number, env_var: None, required: false, placeholder: "8455", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create a bot in Pumble Integrations", "Copy the token", "Paste it below"],
        config_template: "[channels.pumble]\nbot_token_env = \"PUMBLE_BOT_TOKEN\"",
    },
    ChannelMeta {
        name: "flock", display_name: "Flock", icon: "FL",
        description: "Flock bot adapter",
        category: "enterprise", difficulty: "Easy", setup_time: "~1 min",
        quick_setup: "Paste your bot token",
        setup_type: "form",
        fields: &[
            ChannelField { key: "bot_token_env", label: "Bot Token", field_type: FieldType::Secret, env_var: Some("FLOCK_BOT_TOKEN"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "webhook_port", label: "Webhook Port", field_type: FieldType::Number, env_var: None, required: false, placeholder: "8456", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Build an app in Flock App Store", "Copy the bot token", "Paste it below"],
        config_template: "[channels.flock]\nbot_token_env = \"FLOCK_BOT_TOKEN\"",
    },
    ChannelMeta {
        name: "twist", display_name: "Twist", icon: "TW",
        description: "Twist API v3 adapter",
        category: "enterprise", difficulty: "Easy", setup_time: "~2 min",
        quick_setup: "Paste your API token and workspace ID",
        setup_type: "form",
        fields: &[
            ChannelField { key: "token_env", label: "API Token", field_type: FieldType::Secret, env_var: Some("TWIST_TOKEN"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "workspace_id", label: "Workspace ID", field_type: FieldType::Text, env_var: None, required: true, placeholder: "12345", advanced: false },
            ChannelField { key: "allowed_channels", label: "Channel IDs", field_type: FieldType::List, env_var: None, required: false, placeholder: "123, 456", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create an integration in Twist Settings", "Copy the API token", "Enter token and workspace ID below"],
        config_template: "[channels.twist]\ntoken_env = \"TWIST_TOKEN\"\nworkspace_id = \"\"",
    },
    ChannelMeta {
        name: "zulip", display_name: "Zulip", icon: "ZL",
        description: "Zulip event queue adapter",
        category: "enterprise", difficulty: "Easy", setup_time: "~2 min",
        quick_setup: "Paste your API key, server URL, and bot email",
        setup_type: "form",
        fields: &[
            ChannelField { key: "server_url", label: "Server URL", field_type: FieldType::Text, env_var: None, required: true, placeholder: "https://chat.zulip.org", advanced: false },
            ChannelField { key: "bot_email", label: "Bot Email", field_type: FieldType::Text, env_var: None, required: true, placeholder: "bot@zulip.example.com", advanced: false },
            ChannelField { key: "api_key_env", label: "API Key", field_type: FieldType::Secret, env_var: Some("ZULIP_API_KEY"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "streams", label: "Streams", field_type: FieldType::List, env_var: None, required: false, placeholder: "general, dev", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create a bot in Zulip Settings > Your Bots", "Copy the API key", "Enter server URL, bot email, and key below"],
        config_template: "[channels.zulip]\nserver_url = \"\"\nbot_email = \"\"\napi_key_env = \"ZULIP_API_KEY\"",
    },
    // ── Developer (9) ───────────────────────────────────────────────
    ChannelMeta {
        name: "irc", display_name: "IRC", icon: "IR",
        description: "IRC raw TCP adapter",
        category: "developer", difficulty: "Easy", setup_time: "~2 min",
        quick_setup: "Enter server and nickname",
        setup_type: "form",
        fields: &[
            ChannelField { key: "server", label: "Server", field_type: FieldType::Text, env_var: None, required: true, placeholder: "irc.libera.chat", advanced: false },
            ChannelField { key: "nick", label: "Nickname", field_type: FieldType::Text, env_var: None, required: true, placeholder: "openfang", advanced: false },
            ChannelField { key: "channels", label: "Channels", field_type: FieldType::List, env_var: None, required: false, placeholder: "#openfang, #general", advanced: false },
            ChannelField { key: "port", label: "Port", field_type: FieldType::Number, env_var: None, required: false, placeholder: "6667", advanced: true },
            ChannelField { key: "use_tls", label: "Use TLS", field_type: FieldType::Text, env_var: None, required: false, placeholder: "false", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Choose an IRC server", "Enter server, nick, and channels below"],
        config_template: "[channels.irc]\nserver = \"irc.libera.chat\"\nnick = \"openfang\"",
    },
    ChannelMeta {
        name: "xmpp", display_name: "XMPP/Jabber", icon: "XM",
        description: "XMPP/Jabber protocol adapter",
        category: "developer", difficulty: "Easy", setup_time: "~3 min",
        quick_setup: "Enter your JID and password",
        setup_type: "form",
        fields: &[
            ChannelField { key: "jid", label: "JID", field_type: FieldType::Text, env_var: None, required: true, placeholder: "bot@jabber.org", advanced: false },
            ChannelField { key: "password_env", label: "Password", field_type: FieldType::Secret, env_var: Some("XMPP_PASSWORD"), required: true, placeholder: "password", advanced: false },
            ChannelField { key: "server", label: "Server", field_type: FieldType::Text, env_var: None, required: false, placeholder: "jabber.org", advanced: true },
            ChannelField { key: "port", label: "Port", field_type: FieldType::Number, env_var: None, required: false, placeholder: "5222", advanced: true },
            ChannelField { key: "rooms", label: "MUC Rooms", field_type: FieldType::List, env_var: None, required: false, placeholder: "room@conference.jabber.org", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create a bot account on your XMPP server", "Enter JID and password below"],
        config_template: "[channels.xmpp]\njid = \"\"\npassword_env = \"XMPP_PASSWORD\"",
    },
    ChannelMeta {
        name: "gitter", display_name: "Gitter", icon: "GT",
        description: "Gitter Streaming API adapter",
        category: "developer", difficulty: "Easy", setup_time: "~2 min",
        quick_setup: "Paste your auth token and room ID",
        setup_type: "form",
        fields: &[
            ChannelField { key: "token_env", label: "Auth Token", field_type: FieldType::Secret, env_var: Some("GITTER_TOKEN"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "room_id", label: "Room ID", field_type: FieldType::Text, env_var: None, required: true, placeholder: "abc123def456", advanced: false },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Get a token from developer.gitter.im", "Find your room ID", "Paste both below"],
        config_template: "[channels.gitter]\ntoken_env = \"GITTER_TOKEN\"\nroom_id = \"\"",
    },
    ChannelMeta {
        name: "discourse", display_name: "Discourse", icon: "DS",
        description: "Discourse forum API adapter",
        category: "developer", difficulty: "Easy", setup_time: "~2 min",
        quick_setup: "Paste your API key and forum URL",
        setup_type: "form",
        fields: &[
            ChannelField { key: "base_url", label: "Forum URL", field_type: FieldType::Text, env_var: None, required: true, placeholder: "https://forum.example.com", advanced: false },
            ChannelField { key: "api_key_env", label: "API Key", field_type: FieldType::Secret, env_var: Some("DISCOURSE_API_KEY"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "api_username", label: "API Username", field_type: FieldType::Text, env_var: None, required: false, placeholder: "system", advanced: true },
            ChannelField { key: "categories", label: "Categories", field_type: FieldType::List, env_var: None, required: false, placeholder: "general, support", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Go to Admin > API > Keys", "Generate an API key", "Enter forum URL and key below"],
        config_template: "[channels.discourse]\nbase_url = \"\"\napi_key_env = \"DISCOURSE_API_KEY\"",
    },
    ChannelMeta {
        name: "revolt", display_name: "Revolt", icon: "RV",
        description: "Revolt bot adapter",
        category: "developer", difficulty: "Easy", setup_time: "~1 min",
        quick_setup: "Paste your bot token",
        setup_type: "form",
        fields: &[
            ChannelField { key: "bot_token_env", label: "Bot Token", field_type: FieldType::Secret, env_var: Some("REVOLT_BOT_TOKEN"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "api_url", label: "API URL", field_type: FieldType::Text, env_var: None, required: false, placeholder: "https://api.revolt.chat", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Go to Settings > My Bots in Revolt", "Create a bot and copy the token", "Paste it below"],
        config_template: "[channels.revolt]\nbot_token_env = \"REVOLT_BOT_TOKEN\"",
    },
    ChannelMeta {
        name: "guilded", display_name: "Guilded", icon: "GD",
        description: "Guilded bot adapter",
        category: "developer", difficulty: "Easy", setup_time: "~1 min",
        quick_setup: "Paste your bot token",
        setup_type: "form",
        fields: &[
            ChannelField { key: "bot_token_env", label: "Bot Token", field_type: FieldType::Secret, env_var: Some("GUILDED_BOT_TOKEN"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "server_ids", label: "Server IDs", field_type: FieldType::List, env_var: None, required: false, placeholder: "abc123", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Go to Server Settings > Bots in Guilded", "Create a bot and copy the token", "Paste it below"],
        config_template: "[channels.guilded]\nbot_token_env = \"GUILDED_BOT_TOKEN\"",
    },
    ChannelMeta {
        name: "nextcloud", display_name: "Nextcloud Talk", icon: "NC",
        description: "Nextcloud Talk REST adapter",
        category: "developer", difficulty: "Easy", setup_time: "~2 min",
        quick_setup: "Paste your server URL and auth token",
        setup_type: "form",
        fields: &[
            ChannelField { key: "server_url", label: "Server URL", field_type: FieldType::Text, env_var: None, required: true, placeholder: "https://cloud.example.com", advanced: false },
            ChannelField { key: "token_env", label: "Auth Token", field_type: FieldType::Secret, env_var: Some("NEXTCLOUD_TOKEN"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "allowed_rooms", label: "Room Tokens", field_type: FieldType::List, env_var: None, required: false, placeholder: "abc123", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create a bot user in Nextcloud", "Generate an app password", "Enter URL and token below"],
        config_template: "[channels.nextcloud]\nserver_url = \"\"\ntoken_env = \"NEXTCLOUD_TOKEN\"",
    },
    ChannelMeta {
        name: "rocketchat", display_name: "Rocket.Chat", icon: "RC",
        description: "Rocket.Chat REST adapter",
        category: "developer", difficulty: "Easy", setup_time: "~2 min",
        quick_setup: "Paste your server URL, user ID, and token",
        setup_type: "form",
        fields: &[
            ChannelField { key: "server_url", label: "Server URL", field_type: FieldType::Text, env_var: None, required: true, placeholder: "https://rocket.example.com", advanced: false },
            ChannelField { key: "user_id", label: "Bot User ID", field_type: FieldType::Text, env_var: None, required: true, placeholder: "abc123", advanced: false },
            ChannelField { key: "token_env", label: "Auth Token", field_type: FieldType::Secret, env_var: Some("ROCKETCHAT_TOKEN"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "allowed_channels", label: "Channel IDs", field_type: FieldType::List, env_var: None, required: false, placeholder: "GENERAL", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create a bot in Admin > Users", "Generate a personal access token", "Enter URL, user ID, and token below"],
        config_template: "[channels.rocketchat]\nserver_url = \"\"\ntoken_env = \"ROCKETCHAT_TOKEN\"\nuser_id = \"\"",
    },
    ChannelMeta {
        name: "twitch", display_name: "Twitch", icon: "TV",
        description: "Twitch IRC gateway adapter",
        category: "developer", difficulty: "Easy", setup_time: "~2 min",
        quick_setup: "Paste your OAuth token and enter channel name",
        setup_type: "form",
        fields: &[
            ChannelField { key: "oauth_token_env", label: "OAuth Token", field_type: FieldType::Secret, env_var: Some("TWITCH_OAUTH_TOKEN"), required: true, placeholder: "oauth:abc123...", advanced: false },
            ChannelField { key: "nick", label: "Bot Nickname", field_type: FieldType::Text, env_var: None, required: true, placeholder: "openfang", advanced: false },
            ChannelField { key: "channels", label: "Channels (no #)", field_type: FieldType::List, env_var: None, required: true, placeholder: "mychannel", advanced: false },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Generate an OAuth token at twitchapps.com/tmi", "Enter token, nick, and channel below"],
        config_template: "[channels.twitch]\noauth_token_env = \"TWITCH_OAUTH_TOKEN\"\nnick = \"openfang\"",
    },
    // ── Notifications (4) ───────────────────────────────────────────
    ChannelMeta {
        name: "ntfy", display_name: "ntfy", icon: "NF",
        description: "ntfy.sh pub/sub notification adapter",
        category: "notifications", difficulty: "Easy", setup_time: "~1 min",
        quick_setup: "Just enter a topic name",
        setup_type: "form",
        fields: &[
            ChannelField { key: "topic", label: "Topic", field_type: FieldType::Text, env_var: None, required: true, placeholder: "openfang-alerts", advanced: false },
            ChannelField { key: "server_url", label: "Server URL", field_type: FieldType::Text, env_var: None, required: false, placeholder: "https://ntfy.sh", advanced: true },
            ChannelField { key: "token_env", label: "Auth Token", field_type: FieldType::Secret, env_var: Some("NTFY_TOKEN"), required: false, placeholder: "tk_abc123...", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Pick a topic name", "Enter it below — that's it!"],
        config_template: "[channels.ntfy]\ntopic = \"\"",
    },
    ChannelMeta {
        name: "gotify", display_name: "Gotify", icon: "GF",
        description: "Gotify WebSocket notification adapter",
        category: "notifications", difficulty: "Easy", setup_time: "~2 min",
        quick_setup: "Paste your server URL and tokens",
        setup_type: "form",
        fields: &[
            ChannelField { key: "server_url", label: "Server URL", field_type: FieldType::Text, env_var: None, required: true, placeholder: "https://gotify.example.com", advanced: false },
            ChannelField { key: "app_token_env", label: "App Token (send)", field_type: FieldType::Secret, env_var: Some("GOTIFY_APP_TOKEN"), required: true, placeholder: "abc123...", advanced: false },
            ChannelField { key: "client_token_env", label: "Client Token (receive)", field_type: FieldType::Secret, env_var: Some("GOTIFY_CLIENT_TOKEN"), required: true, placeholder: "def456...", advanced: false },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create an app and a client in Gotify", "Copy both tokens", "Enter URL and tokens below"],
        config_template: "[channels.gotify]\nserver_url = \"\"\napp_token_env = \"GOTIFY_APP_TOKEN\"\nclient_token_env = \"GOTIFY_CLIENT_TOKEN\"",
    },
    ChannelMeta {
        name: "webhook", display_name: "Webhook", icon: "WH",
        description: "Generic HMAC-signed webhook adapter",
        category: "notifications", difficulty: "Easy", setup_time: "~1 min",
        quick_setup: "Optionally set an HMAC secret",
        setup_type: "form",
        fields: &[
            ChannelField { key: "secret_env", label: "HMAC Secret", field_type: FieldType::Secret, env_var: Some("WEBHOOK_SECRET"), required: false, placeholder: "my-secret", advanced: false },
            ChannelField { key: "listen_port", label: "Listen Port", field_type: FieldType::Number, env_var: None, required: false, placeholder: "8460", advanced: true },
            ChannelField { key: "callback_url", label: "Callback URL", field_type: FieldType::Text, env_var: None, required: false, placeholder: "https://example.com/webhook", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Enter an HMAC secret (or leave blank)", "Click Save — that's it!"],
        config_template: "[channels.webhook]\nsecret_env = \"WEBHOOK_SECRET\"",
    },
    ChannelMeta {
        name: "mumble", display_name: "Mumble", icon: "MB",
        description: "Mumble text chat adapter",
        category: "notifications", difficulty: "Easy", setup_time: "~2 min",
        quick_setup: "Enter server host and username",
        setup_type: "form",
        fields: &[
            ChannelField { key: "host", label: "Host", field_type: FieldType::Text, env_var: None, required: true, placeholder: "mumble.example.com", advanced: false },
            ChannelField { key: "username", label: "Username", field_type: FieldType::Text, env_var: None, required: true, placeholder: "openfang", advanced: false },
            ChannelField { key: "password_env", label: "Server Password", field_type: FieldType::Secret, env_var: Some("MUMBLE_PASSWORD"), required: false, placeholder: "password", advanced: true },
            ChannelField { key: "port", label: "Port", field_type: FieldType::Number, env_var: None, required: false, placeholder: "64738", advanced: true },
            ChannelField { key: "channel", label: "Channel", field_type: FieldType::Text, env_var: None, required: false, placeholder: "Root", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Enter host and username below", "Optionally add a password"],
        config_template: "[channels.mumble]\nhost = \"\"\nusername = \"openfang\"",
    },
    ChannelMeta {
        name: "wecom", display_name: "WeCom", icon: "WC",
        description: "WeCom (WeChat Work) adapter",
        category: "messaging", difficulty: "Easy", setup_time: "~3 min",
        quick_setup: "Enter your Corp ID, Agent ID, and Secret",
        setup_type: "form",
        fields: &[
            ChannelField { key: "corp_id", label: "Corp ID", field_type: FieldType::Text, env_var: None, required: true, placeholder: "wwxxxxx", advanced: false },
            ChannelField { key: "agent_id", label: "Agent ID", field_type: FieldType::Text, env_var: None, required: true, placeholder: "wwxxxxx", advanced: false },
            ChannelField { key: "secret_env", label: "Secret", field_type: FieldType::Secret, env_var: Some("WECOM_SECRET"), required: true, placeholder: "secret", advanced: false },
            ChannelField { key: "token", label: "Callback Token", field_type: FieldType::Text, env_var: None, required: false, placeholder: "callback_token", advanced: true },
            ChannelField { key: "encoding_aes_key", label: "Encoding AES Key", field_type: FieldType::Text, env_var: None, required: false, placeholder: "encoding_aes_key", advanced: true },
            ChannelField { key: "webhook_port", label: "Webhook Port", field_type: FieldType::Number, env_var: None, required: false, placeholder: "8454", advanced: true },
            ChannelField { key: "default_agent", label: "Default Agent", field_type: FieldType::Text, env_var: None, required: false, placeholder: "assistant", advanced: true },
        ],
        setup_steps: &["Create a WeCom application at work.weixin.qq.com", "Get Corp ID, Agent ID, and Secret", "Configure callback URL to your webhook endpoint"],
        config_template: "[channels.wecom]\ncorp_id = \"\"\nagent_id = \"\"\nsecret_env = \"WECOM_SECRET\"",
    },
];

/// Check if a channel is configured (has a `[channels.xxx]` section in config).
fn is_channel_configured(config: &openfang_types::config::ChannelsConfig, name: &str) -> bool {
    match name {
        "telegram" => config.telegram.is_some(),
        "discord" => config.discord.is_some(),
        "slack" => config.slack.is_some(),
        "whatsapp" => config.whatsapp.is_some(),
        "signal" => config.signal.is_some(),
        "matrix" => config.matrix.is_some(),
        "email" => config.email.is_some(),
        "line" => config.line.is_some(),
        "viber" => config.viber.is_some(),
        "messenger" => config.messenger.is_some(),
        "threema" => config.threema.is_some(),
        "keybase" => config.keybase.is_some(),
        "reddit" => config.reddit.is_some(),
        "mastodon" => config.mastodon.is_some(),
        "bluesky" => config.bluesky.is_some(),
        "linkedin" => config.linkedin.is_some(),
        "nostr" => config.nostr.is_some(),
        "teams" => config.teams.is_some(),
        "mattermost" => config.mattermost.is_some(),
        "google_chat" => config.google_chat.is_some(),
        "webex" => config.webex.is_some(),
        "feishu" => config.feishu.is_some(),
        "dingtalk" => config.dingtalk.is_some(),
        "dingtalk_stream" => config.dingtalk_stream.is_some(),
        "pumble" => config.pumble.is_some(),
        "flock" => config.flock.is_some(),
        "twist" => config.twist.is_some(),
        "zulip" => config.zulip.is_some(),
        "irc" => config.irc.is_some(),
        "xmpp" => config.xmpp.is_some(),
        "gitter" => config.gitter.is_some(),
        "discourse" => config.discourse.is_some(),
        "revolt" => config.revolt.is_some(),
        "guilded" => config.guilded.is_some(),
        "nextcloud" => config.nextcloud.is_some(),
        "rocketchat" => config.rocketchat.is_some(),
        "twitch" => config.twitch.is_some(),
        "ntfy" => config.ntfy.is_some(),
        "gotify" => config.gotify.is_some(),
        "webhook" => config.webhook.is_some(),
        "mumble" => config.mumble.is_some(),
        "wecom" => config.wecom.is_some(),
        _ => false,
    }
}

/// Build a JSON field descriptor, checking env var presence but never exposing secrets.
/// For non-secret fields, includes the actual config value from `config_values` if available.
fn build_field_json(
    f: &ChannelField,
    config_values: Option<&serde_json::Value>,
) -> serde_json::Value {
    let has_value = f
        .env_var
        .map(|ev| std::env::var(ev).map(|v| !v.is_empty()).unwrap_or(false))
        .unwrap_or(false);
    let mut field = serde_json::json!({
        "key": f.key,
        "label": f.label,
        "type": f.field_type.as_str(),
        "env_var": f.env_var,
        "required": f.required,
        "has_value": has_value,
        "placeholder": f.placeholder,
        "advanced": f.advanced,
    });
    // For non-secret fields, include the actual saved config value so the
    // dashboard can pre-populate forms when editing existing configs.
    if f.env_var.is_none() {
        if let Some(obj) = config_values.and_then(|v| v.as_object()) {
            if let Some(val) = obj.get(f.key) {
                // Convert arrays to comma-separated string for list fields
                let display_val = if f.field_type == FieldType::List {
                    if let Some(arr) = val.as_array() {
                        serde_json::Value::String(
                            arr.iter()
                                .filter_map(|v| {
                                    v.as_str()
                                        .map(|s| s.to_string())
                                        .or_else(|| Some(v.to_string()))
                                })
                                .collect::<Vec<_>>()
                                .join(", "),
                        )
                    } else {
                        val.clone()
                    }
                } else {
                    val.clone()
                };
                field["value"] = display_val;
                if !val.is_null() && val.as_str().map(|s| !s.is_empty()).unwrap_or(true) {
                    field["has_value"] = serde_json::Value::Bool(true);
                }
            }
        }
    }
    field
}

/// Find a channel definition by name.
fn find_channel_meta(name: &str) -> Option<&'static ChannelMeta> {
    CHANNEL_REGISTRY.iter().find(|c| c.name == name)
}

/// Serialize a channel's config to a JSON Value for pre-populating dashboard forms.
fn channel_config_values(
    config: &openfang_types::config::ChannelsConfig,
    name: &str,
) -> Option<serde_json::Value> {
    match name {
        "telegram" => config
            .telegram
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "discord" => config
            .discord
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "slack" => config
            .slack
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "whatsapp" => config
            .whatsapp
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "signal" => config
            .signal
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "matrix" => config
            .matrix
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "email" => config
            .email
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "teams" => config
            .teams
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "mattermost" => config
            .mattermost
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "irc" => config
            .irc
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "google_chat" => config
            .google_chat
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "twitch" => config
            .twitch
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "rocketchat" => config
            .rocketchat
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "zulip" => config
            .zulip
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "xmpp" => config
            .xmpp
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "line" => config
            .line
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "viber" => config
            .viber
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "messenger" => config
            .messenger
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "reddit" => config
            .reddit
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "mastodon" => config
            .mastodon
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "bluesky" => config
            .bluesky
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "feishu" => config
            .feishu
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "revolt" => config
            .revolt
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "nextcloud" => config
            .nextcloud
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "guilded" => config
            .guilded
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "keybase" => config
            .keybase
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "threema" => config
            .threema
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "nostr" => config
            .nostr
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "webex" => config
            .webex
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "pumble" => config
            .pumble
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "flock" => config
            .flock
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "twist" => config
            .twist
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "mumble" => config
            .mumble
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "dingtalk" => config
            .dingtalk
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "dingtalk_stream" => config
            .dingtalk_stream
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "discourse" => config
            .discourse
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "gitter" => config
            .gitter
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "ntfy" => config
            .ntfy
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "gotify" => config
            .gotify
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "webhook" => config
            .webhook
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "linkedin" => config
            .linkedin
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        "wecom" => config
            .wecom
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        _ => None,
    }
}

/// GET /api/channels — List all 40 channel adapters with status and field metadata.
pub async fn list_channels(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Read the live channels config (updated on every hot-reload) instead of the
    // stale boot-time kernel.config, so newly configured channels show correctly.
    let live_channels = state.channels_config.read().await;
    let mut channels = Vec::new();
    let mut configured_count = 0u32;

    for meta in CHANNEL_REGISTRY {
        let configured = is_channel_configured(&live_channels, meta.name);
        if configured {
            configured_count += 1;
        }

        // Check if all required secret env vars are set
        let has_token = meta
            .fields
            .iter()
            .filter(|f| f.required && f.env_var.is_some())
            .all(|f| {
                f.env_var
                    .map(|ev| std::env::var(ev).map(|v| !v.is_empty()).unwrap_or(false))
                    .unwrap_or(true)
            });

        let config_vals = channel_config_values(&live_channels, meta.name);
        let fields: Vec<serde_json::Value> = meta
            .fields
            .iter()
            .map(|f| build_field_json(f, config_vals.as_ref()))
            .collect();

        channels.push(serde_json::json!({
            "name": meta.name,
            "display_name": meta.display_name,
            "icon": meta.icon,
            "description": meta.description,
            "category": meta.category,
            "difficulty": meta.difficulty,
            "setup_time": meta.setup_time,
            "quick_setup": meta.quick_setup,
            "setup_type": meta.setup_type,
            "configured": configured,
            "has_token": has_token,
            "fields": fields,
            "setup_steps": meta.setup_steps,
            "config_template": meta.config_template,
        }));
    }

    Json(serde_json::json!({
        "channels": channels,
        "total": channels.len(),
        "configured_count": configured_count,
    }))
}

/// POST /api/channels/{name}/configure — Save channel secrets + config fields.
pub async fn configure_channel(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let meta = match find_channel_meta(&name) {
        Some(m) => m,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Unknown channel"})),
            )
        }
    };

    let fields = match body.get("fields").and_then(|v| v.as_object()) {
        Some(f) => f,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing 'fields' object"})),
            )
        }
    };

    let home = openfang_kernel::config::openfang_home();
    let secrets_path = home.join("secrets.env");
    let config_path = home.join("config.toml");
    let mut config_fields: HashMap<String, (String, FieldType)> = HashMap::new();

    for field_def in meta.fields {
        let value = fields
            .get(field_def.key)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if value.is_empty() {
            continue;
        }

        if let Some(env_var) = field_def.env_var {
            // Secret field — write to secrets.env and set in process
            if let Err(e) = write_secret_env(&secrets_path, env_var, value) {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("Failed to write secret: {e}")})),
                );
            }
            // SAFETY: We are the only writer; this is a single-threaded config operation
            unsafe {
                std::env::set_var(env_var, value);
            }
            // Also write the env var NAME to config.toml so the channel section
            // is not empty and the kernel knows which env var to read.
            config_fields.insert(
                field_def.key.to_string(),
                (env_var.to_string(), FieldType::Text),
            );
        } else {
            // Config field — collect for TOML write with type info
            config_fields.insert(
                field_def.key.to_string(),
                (value.to_string(), field_def.field_type),
            );
        }
    }

    // Write config.toml section
    if let Err(e) = upsert_channel_config(&config_path, &name, &config_fields) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to write config: {e}")})),
        );
    }

    // Hot-reload: activate the channel immediately
    match crate::channel_bridge::reload_channels_from_disk(&state).await {
        Ok(started) => {
            let activated = started.iter().any(|s| s.eq_ignore_ascii_case(&name));
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "configured",
                    "channel": name,
                    "activated": activated,
                    "started_channels": started,
                    "note": if activated {
                        format!("{} activated successfully.", name)
                    } else {
                        "Channel configured but could not start (check credentials).".to_string()
                    }
                })),
            )
        }
        Err(e) => {
            tracing::warn!(error = %e, "Channel hot-reload failed after configure");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "configured",
                    "channel": name,
                    "activated": false,
                    "note": format!("Configured, but hot-reload failed: {e}. Restart daemon to activate.")
                })),
            )
        }
    }
}

/// DELETE /api/channels/{name}/configure — Remove channel secrets + config section.
pub async fn remove_channel(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let meta = match find_channel_meta(&name) {
        Some(m) => m,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Unknown channel"})),
            )
        }
    };

    let home = openfang_kernel::config::openfang_home();
    let secrets_path = home.join("secrets.env");
    let config_path = home.join("config.toml");

    // Remove all secret env vars for this channel
    for field_def in meta.fields {
        if let Some(env_var) = field_def.env_var {
            let _ = remove_secret_env(&secrets_path, env_var);
            // SAFETY: Single-threaded config operation
            unsafe {
                std::env::remove_var(env_var);
            }
        }
    }

    // Remove config section
    if let Err(e) = remove_channel_config(&config_path, &name) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to remove config: {e}")})),
        );
    }

    // Hot-reload: deactivate the channel immediately
    match crate::channel_bridge::reload_channels_from_disk(&state).await {
        Ok(started) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "removed",
                "channel": name,
                "remaining_channels": started,
                "note": format!("{} deactivated.", name)
            })),
        ),
        Err(e) => {
            tracing::warn!(error = %e, "Channel hot-reload failed after remove");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "removed",
                    "channel": name,
                    "note": format!("Removed, but hot-reload failed: {e}. Restart daemon to fully deactivate.")
                })),
            )
        }
    }
}

/// POST /api/channels/{name}/test — Connectivity check + optional live test message.
///
/// Accepts an optional JSON body with `channel_id` (for Discord/Slack) or `chat_id`
/// (for Telegram). When provided, sends a real test message to verify the bot can
/// post to that channel.
pub async fn test_channel(
    Path(name): Path<String>,
    raw_body: axum::body::Bytes,
) -> impl IntoResponse {
    let meta = match find_channel_meta(&name) {
        Some(m) => m,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"status": "error", "message": "Unknown channel"})),
            )
        }
    };

    // Check all required env vars are set
    let mut missing = Vec::new();
    for field_def in meta.fields {
        if field_def.required {
            if let Some(env_var) = field_def.env_var {
                if std::env::var(env_var).map(|v| v.is_empty()).unwrap_or(true) {
                    missing.push(env_var);
                }
            }
        }
    }

    if !missing.is_empty() {
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("Missing required env vars: {}", missing.join(", "))
            })),
        );
    }

    // If a target channel/chat ID is provided, send a real test message
    let body: serde_json::Value = if raw_body.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&raw_body).unwrap_or(serde_json::Value::Null)
    };
    let target = body
        .get("channel_id")
        .or_else(|| body.get("chat_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Some(target_id) = target {
        match send_channel_test_message(&name, &target_id).await {
            Ok(()) => {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "status": "ok",
                        "message": format!("Test message sent to {} channel {}.", meta.display_name, target_id)
                    })),
                );
            }
            Err(e) => {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "status": "error",
                        "message": format!("Credentials valid but failed to send test message: {e}")
                    })),
                );
            }
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "message": format!("All required credentials for {} are set. Provide channel_id or chat_id to send a test message.", meta.display_name)
        })),
    )
}

/// Send a real test message to a specific channel/chat on the given platform.
async fn send_channel_test_message(channel_name: &str, target_id: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let test_msg = "OpenFang test message — your channel is connected!";

    match channel_name {
        "discord" => {
            let token = std::env::var("DISCORD_BOT_TOKEN")
                .map_err(|_| "DISCORD_BOT_TOKEN not set".to_string())?;
            let url = format!("https://discord.com/api/v10/channels/{target_id}/messages");
            let resp = client
                .post(&url)
                .header("Authorization", format!("Bot {token}"))
                .json(&serde_json::json!({ "content": test_msg }))
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("Discord API error: {body}"));
            }
        }
        "telegram" => {
            let token = std::env::var("TELEGRAM_BOT_TOKEN")
                .map_err(|_| "TELEGRAM_BOT_TOKEN not set".to_string())?;
            let url = format!("https://api.telegram.org/bot{token}/sendMessage");
            let resp = client
                .post(&url)
                .json(&serde_json::json!({ "chat_id": target_id, "text": test_msg }))
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("Telegram API error: {body}"));
            }
        }
        "slack" => {
            let token = std::env::var("SLACK_BOT_TOKEN")
                .map_err(|_| "SLACK_BOT_TOKEN not set".to_string())?;
            let url = "https://slack.com/api/chat.postMessage";
            let resp = client
                .post(url)
                .header("Authorization", format!("Bearer {token}"))
                .json(&serde_json::json!({ "channel": target_id, "text": test_msg }))
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("Slack API error: {body}"));
            }
        }
        _ => {
            return Err(format!(
                "Live test messaging not supported for {channel_name}. Credentials are valid."
            ));
        }
    }
    Ok(())
}

/// POST /api/channels/reload — Manually trigger a channel hot-reload from disk config.
pub async fn reload_channels(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match crate::channel_bridge::reload_channels_from_disk(&state).await {
        Ok(started) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ok",
                "started": started,
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "error": e,
            })),
        ),
    }
}

// ---------------------------------------------------------------------------
// WhatsApp QR login flow (OpenClaw-style)
// ---------------------------------------------------------------------------

/// POST /api/channels/whatsapp/qr/start — Start a WhatsApp Web QR login session.
///
/// If a WhatsApp Web gateway is available (e.g. a Baileys-based bridge process),
/// this proxies the request and returns a base64 QR code data URL. If no gateway
/// is running, it returns instructions to set one up.
pub async fn whatsapp_qr_start() -> impl IntoResponse {
    // Check for WhatsApp Web gateway URL in config or env
    let gateway_url = std::env::var("WHATSAPP_WEB_GATEWAY_URL").unwrap_or_default();

    if gateway_url.is_empty() {
        return Json(serde_json::json!({
            "available": false,
            "message": "WhatsApp Web gateway not running. Start the gateway or use Business API mode.",
            "help": "The WhatsApp Web gateway auto-starts with the daemon when configured. Ensure Node.js >= 18 is installed and WhatsApp is configured in config.toml. Set WHATSAPP_WEB_GATEWAY_URL to use an external gateway."
        }));
    }

    // Try to reach the gateway and start a QR session.
    // Uses a raw HTTP request via tokio TcpStream to avoid adding reqwest as a runtime dep.
    let start_url = format!("{}/login/start", gateway_url.trim_end_matches('/'));
    match gateway_http_post(&start_url).await {
        Ok(body) => {
            let qr_url = body
                .get("qr_data_url")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let sid = body
                .get("session_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let msg = body
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Scan this QR code with WhatsApp → Linked Devices");
            let connected = body
                .get("connected")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            Json(serde_json::json!({
                "available": true,
                "qr_data_url": qr_url,
                "session_id": sid,
                "message": msg,
                "connected": connected,
            }))
        }
        Err(e) => Json(serde_json::json!({
            "available": false,
            "message": format!("Could not reach WhatsApp Web gateway: {e}"),
            "help": "Make sure the gateway is running at the configured URL"
        })),
    }
}

/// GET /api/channels/whatsapp/qr/status — Poll for QR scan completion.
///
/// After calling `/qr/start`, the frontend polls this to check if the user
/// has scanned the QR code and the WhatsApp Web session is connected.
pub async fn whatsapp_qr_status(
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let gateway_url = std::env::var("WHATSAPP_WEB_GATEWAY_URL").unwrap_or_default();

    if gateway_url.is_empty() {
        return Json(serde_json::json!({
            "connected": false,
            "message": "Gateway not available"
        }));
    }

    let session_id = params.get("session_id").cloned().unwrap_or_default();
    let status_url = format!(
        "{}/login/status?session_id={}",
        gateway_url.trim_end_matches('/'),
        session_id
    );

    match gateway_http_get(&status_url).await {
        Ok(body) => {
            let connected = body
                .get("connected")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let msg = body
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Waiting for scan...");
            let expired = body
                .get("expired")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            Json(serde_json::json!({
                "connected": connected,
                "message": msg,
                "expired": expired,
            }))
        }
        Err(_) => Json(serde_json::json!({ "connected": false, "message": "Gateway unreachable" })),
    }
}

/// Lightweight HTTP POST to a gateway URL. Returns parsed JSON body.
async fn gateway_http_post(url_with_path: &str) -> Result<serde_json::Value, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // Split into base URL + path from the full URL like "http://127.0.0.1:3009/login/start"
    let without_scheme = url_with_path
        .strip_prefix("http://")
        .or_else(|| url_with_path.strip_prefix("https://"))
        .unwrap_or(url_with_path);
    let (host_port, path) = if let Some(idx) = without_scheme.find('/') {
        (&without_scheme[..idx], &without_scheme[idx..])
    } else {
        (without_scheme, "/")
    };
    let (host, port) = if let Some((h, p)) = host_port.rsplit_once(':') {
        (h, p.parse().unwrap_or(3009u16))
    } else {
        (host_port, 3009u16)
    };

    let mut stream = tokio::net::TcpStream::connect(format!("{host}:{port}"))
        .await
        .map_err(|e| format!("Connect failed: {e}"))?;

    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
    );
    stream
        .write_all(req.as_bytes())
        .await
        .map_err(|e| format!("Write failed: {e}"))?;

    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .await
        .map_err(|e| format!("Read failed: {e}"))?;
    let response = String::from_utf8_lossy(&buf);

    // Find the JSON body after the blank line separating headers from body
    if let Some(idx) = response.find("\r\n\r\n") {
        let body_str = &response[idx + 4..];
        serde_json::from_str(body_str.trim()).map_err(|e| format!("Parse failed: {e}"))
    } else {
        Err("No HTTP body in response".to_string())
    }
}

/// Lightweight HTTP GET to a gateway URL. Returns parsed JSON body.
async fn gateway_http_get(url_with_path: &str) -> Result<serde_json::Value, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let without_scheme = url_with_path
        .strip_prefix("http://")
        .or_else(|| url_with_path.strip_prefix("https://"))
        .unwrap_or(url_with_path);
    let (host_port, path_and_query) = if let Some(idx) = without_scheme.find('/') {
        (&without_scheme[..idx], &without_scheme[idx..])
    } else {
        (without_scheme, "/")
    };
    let (host, port) = if let Some((h, p)) = host_port.rsplit_once(':') {
        (h, p.parse().unwrap_or(3009u16))
    } else {
        (host_port, 3009u16)
    };

    let mut stream = tokio::net::TcpStream::connect(format!("{host}:{port}"))
        .await
        .map_err(|e| format!("Connect failed: {e}"))?;

    let req = format!(
        "GET {path_and_query} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(req.as_bytes())
        .await
        .map_err(|e| format!("Write failed: {e}"))?;

    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .await
        .map_err(|e| format!("Read failed: {e}"))?;
    let response = String::from_utf8_lossy(&buf);

    if let Some(idx) = response.find("\r\n\r\n") {
        let body_str = &response[idx + 4..];
        serde_json::from_str(body_str.trim()).map_err(|e| format!("Parse failed: {e}"))
    } else {
        Err("No HTTP body in response".to_string())
    }
}

// ---------------------------------------------------------------------------
// Template endpoints
// ---------------------------------------------------------------------------

/// GET /api/templates — List available agent templates.
pub async fn list_templates() -> impl IntoResponse {
    let agents_dir = openfang_kernel::config::openfang_home().join("agents");
    let mut templates = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&agents_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let manifest_path = path.join("agent.toml");
                if manifest_path.exists() {
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();

                    let description = std::fs::read_to_string(&manifest_path)
                        .ok()
                        .and_then(|content| toml::from_str::<AgentManifest>(&content).ok())
                        .map(|m| m.description)
                        .unwrap_or_default();

                    templates.push(serde_json::json!({
                        "name": name,
                        "description": description,
                    }));
                }
            }
        }
    }

    Json(serde_json::json!({
        "templates": templates,
        "total": templates.len(),
    }))
}

/// GET /api/templates/:name — Get template details.
pub async fn get_template(Path(name): Path<String>) -> impl IntoResponse {
    let agents_dir = openfang_kernel::config::openfang_home().join("agents");
    let manifest_path = agents_dir.join(&name).join("agent.toml");

    if !manifest_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Template not found"})),
        );
    }

    match std::fs::read_to_string(&manifest_path) {
        Ok(content) => match toml::from_str::<AgentManifest>(&content) {
            Ok(manifest) => (
                StatusCode::OK,
                Json(serde_json::json!({
                    "name": name,
                    "manifest": {
                        "name": manifest.name,
                        "description": manifest.description,
                        "module": manifest.module,
                        "tags": manifest.tags,
                        "model": {
                            "provider": manifest.model.provider,
                            "model": manifest.model.model,
                        },
                        "capabilities": {
                            "tools": manifest.capabilities.tools,
                            "network": manifest.capabilities.network,
                        },
                    },
                    "manifest_toml": content,
                })),
            ),
            Err(e) => {
                tracing::warn!("Invalid template manifest for '{name}': {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "Invalid template manifest"})),
                )
            }
        },
        Err(e) => {
            tracing::warn!("Failed to read template '{name}': {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to read template"})),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Memory endpoints
// ---------------------------------------------------------------------------

/// GET /api/memory/agents/:id/kv — List KV pairs for an agent.
///
/// Note: memory_store tool writes to a shared namespace, so we read from that
/// same namespace regardless of which agent ID is in the URL.
pub async fn get_agent_kv(
    State(state): State<Arc<AppState>>,
    Path(_id): Path<String>,
) -> impl IntoResponse {
    let agent_id = openfang_kernel::kernel::shared_memory_agent_id();

    match state.kernel.memory.list_kv(agent_id) {
        Ok(pairs) => {
            let kv: Vec<serde_json::Value> = pairs
                .into_iter()
                .map(|(k, v)| serde_json::json!({"key": k, "value": v}))
                .collect();
            (StatusCode::OK, Json(serde_json::json!({"kv_pairs": kv})))
        }
        Err(e) => {
            tracing::warn!("Memory list_kv failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Memory operation failed"})),
            )
        }
    }
}

/// GET /api/memory/agents/:id/kv/:key — Get a specific KV value.
pub async fn get_agent_kv_key(
    State(state): State<Arc<AppState>>,
    Path((_id, key)): Path<(String, String)>,
) -> impl IntoResponse {
    let agent_id = openfang_kernel::kernel::shared_memory_agent_id();

    match state.kernel.memory.structured_get(agent_id, &key) {
        Ok(Some(val)) => (
            StatusCode::OK,
            Json(serde_json::json!({"key": key, "value": val})),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Key not found"})),
        ),
        Err(e) => {
            tracing::warn!("Memory get failed for key '{key}': {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Memory operation failed"})),
            )
        }
    }
}

/// PUT /api/memory/agents/:id/kv/:key — Set a KV value.
pub async fn set_agent_kv_key(
    State(state): State<Arc<AppState>>,
    Path((_id, key)): Path<(String, String)>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let agent_id = openfang_kernel::kernel::shared_memory_agent_id();

    let value = body.get("value").cloned().unwrap_or(body);

    match state.kernel.memory.structured_set(agent_id, &key, value) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "stored", "key": key})),
        ),
        Err(e) => {
            tracing::warn!("Memory set failed for key '{key}': {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Memory operation failed"})),
            )
        }
    }
}

/// DELETE /api/memory/agents/:id/kv/:key — Delete a KV value.
pub async fn delete_agent_kv_key(
    State(state): State<Arc<AppState>>,
    Path((_id, key)): Path<(String, String)>,
) -> impl IntoResponse {
    let agent_id = openfang_kernel::kernel::shared_memory_agent_id();

    match state.kernel.memory.structured_delete(agent_id, &key) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "deleted", "key": key})),
        ),
        Err(e) => {
            tracing::warn!("Memory delete failed for key '{key}': {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Memory operation failed"})),
            )
        }
    }
}

/// GET /api/health — Minimal liveness probe (public, no auth required).
/// Returns only status and version to prevent information leakage.
/// Use GET /api/health/detail for full diagnostics (requires auth).
pub async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Run the database checks on a blocking thread so we never hold the
    // std::sync::Mutex<Connection> on a tokio worker thread.
    let kernel = Arc::clone(&state.kernel);
    let (db_health, runtime_projection_ok) = tokio::task::spawn_blocking(move || {
        let projection_ok = kernel
            .runtime_stores
            .agent_runtime
            .list_agent_runtimes()
            .is_ok();
        (kernel.db_health(), projection_ok)
    })
    .await
    .unwrap_or_default();

    let status = if db_health.is_healthy() && runtime_projection_ok {
        "ok"
    } else {
        "degraded"
    };

    Json(serde_json::json!({
        "status": status,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// GET /api/health/detail — Full health diagnostics (requires auth).
pub async fn health_detail(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let health = state.kernel.supervisor.health();

    let kernel = Arc::clone(&state.kernel);
    let (db_health, runtime_agent_count) = tokio::task::spawn_blocking(move || {
        let runtime_agent_count = kernel
            .runtime_stores
            .agent_runtime
            .list_agent_runtimes()
            .map(|records| records.len())
            .ok();
        (kernel.db_health(), runtime_agent_count)
    })
    .await
    .unwrap_or_default();

    let config_warnings = state.kernel.config.validate();
    let status = if db_health.is_healthy() && runtime_agent_count.is_some() {
        "ok"
    } else {
        "degraded"
    };

    Json(serde_json::json!({
        "status": status,
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_seconds": state.started_at.elapsed().as_secs(),
        "panic_count": health.panic_count,
        "restart_count": health.restart_count,
        "agent_count": runtime_agent_count.unwrap_or_else(|| state.kernel.registry.count()),
        "database": if db_health.is_healthy() { "connected" } else { "error" },
        "runtime_projection": if runtime_agent_count.is_some() { "connected" } else { "error" },
        "config_warnings": config_warnings,
    }))
}

// ---------------------------------------------------------------------------
// Prometheus metrics endpoint
// ---------------------------------------------------------------------------

/// GET /api/metrics — Prometheus text-format metrics.
///
/// Returns counters and gauges for monitoring OpenFang in production:
/// - `openfang_agents_active` — number of active agents
/// - `openfang_uptime_seconds` — seconds since daemon started
/// - `openfang_tokens_total` — total tokens consumed (per agent)
/// - `openfang_tool_calls_total` — total tool calls (per agent)
/// - `openfang_panics_total` — supervisor panic count
/// - `openfang_restarts_total` — supervisor restart count
pub async fn prometheus_metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut out = String::with_capacity(2048);

    // Uptime
    let uptime = state.started_at.elapsed().as_secs();
    out.push_str("# HELP openfang_uptime_seconds Time since daemon started.\n");
    out.push_str("# TYPE openfang_uptime_seconds gauge\n");
    out.push_str(&format!("openfang_uptime_seconds {uptime}\n\n"));

    // Active agents
    let agents = state.kernel.registry.list();
    let active = agents
        .iter()
        .filter(|a| matches!(a.state, openfang_types::agent::AgentState::Running))
        .count();
    out.push_str("# HELP openfang_agents_active Number of active agents.\n");
    out.push_str("# TYPE openfang_agents_active gauge\n");
    out.push_str(&format!("openfang_agents_active {active}\n"));
    out.push_str("# HELP openfang_agents_total Total number of registered agents.\n");
    out.push_str("# TYPE openfang_agents_total gauge\n");
    out.push_str(&format!("openfang_agents_total {}\n\n", agents.len()));

    // Per-agent token and tool usage
    out.push_str("# HELP openfang_tokens_total Total tokens consumed (rolling hourly window).\n");
    out.push_str("# TYPE openfang_tokens_total gauge\n");
    out.push_str("# HELP openfang_tool_calls_total Total tool calls (rolling hourly window).\n");
    out.push_str("# TYPE openfang_tool_calls_total gauge\n");
    for agent in &agents {
        let name = &agent.name;
        let provider = &agent.manifest.model.provider;
        let model = &agent.manifest.model.model;
        if let Some((tokens, tools)) = state.kernel.scheduler.get_usage(agent.id) {
            out.push_str(&format!(
                "openfang_tokens_total{{agent=\"{name}\",provider=\"{provider}\",model=\"{model}\"}} {tokens}\n"
            ));
            out.push_str(&format!(
                "openfang_tool_calls_total{{agent=\"{name}\"}} {tools}\n"
            ));
        }
    }
    out.push('\n');

    // Supervisor health
    let health = state.kernel.supervisor.health();
    out.push_str("# HELP openfang_panics_total Total supervisor panics since start.\n");
    out.push_str("# TYPE openfang_panics_total counter\n");
    out.push_str(&format!("openfang_panics_total {}\n", health.panic_count));
    out.push_str("# HELP openfang_restarts_total Total supervisor restarts since start.\n");
    out.push_str("# TYPE openfang_restarts_total counter\n");
    out.push_str(&format!(
        "openfang_restarts_total {}\n\n",
        health.restart_count
    ));

    // Version info
    out.push_str("# HELP openfang_info OpenFang version and build info.\n");
    out.push_str("# TYPE openfang_info gauge\n");
    out.push_str(&format!(
        "openfang_info{{version=\"{}\"}} 1\n",
        env!("CARGO_PKG_VERSION")
    ));

    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        out,
    )
}

// ---------------------------------------------------------------------------
// Skills endpoints
// ---------------------------------------------------------------------------

/// GET /api/v1/skills — List file-backed skills from the boot-loaded registry.
pub async fn list_skills_v1(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SkillListQueryParams>,
) -> impl IntoResponse {
    let limit = match parse_pagination_limit(params.limit) {
        Ok(limit) => limit,
        Err(response) => return response,
    };
    let offset = match parse_cursor_offset(params.cursor.as_deref()) {
        Ok(offset) => offset,
        Err(response) => return response,
    };

    let mut items = list_registered_skills_v1(&state);
    if let Some(search) = params.search.as_deref() {
        items.retain(|skill| skill_matches_search(skill, search));
    }
    let items = items
        .into_iter()
        .map(|skill| skill_summary_from_detail(&skill))
        .collect::<Vec<_>>();

    (
        StatusCode::OK,
        Json(serde_json::json!(paginate_skill_summaries(
            items, limit, offset
        ))),
    )
}

/// GET /api/v1/skills/{id} — Load one file-backed skill from the boot-loaded registry.
pub async fn get_skill_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match find_registered_skill_v1(&state, &id) {
        Some(skill) => (StatusCode::OK, Json(serde_json::json!(skill))).into_response(),
        None => skill_not_found_response(&id).into_response(),
    }
}

/// GET /api/skills — List installed skills.
pub async fn list_skills(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let skills_dir = state.kernel.config.home_dir.join("skills");
    let mut registry = openfang_skills::registry::SkillRegistry::new(skills_dir);
    let _ = registry.load_all();

    let skills: Vec<serde_json::Value> = registry
        .list()
        .iter()
        .map(|s| {
            let source = match &s.manifest.source {
                Some(openfang_skills::SkillSource::ClawHub { slug, version }) => {
                    serde_json::json!({"type": "clawhub", "slug": slug, "version": version})
                }
                Some(openfang_skills::SkillSource::OpenClaw) => {
                    serde_json::json!({"type": "openclaw"})
                }
                Some(openfang_skills::SkillSource::Bundled) => {
                    serde_json::json!({"type": "bundled"})
                }
                Some(openfang_skills::SkillSource::Native) | None => {
                    serde_json::json!({"type": "local"})
                }
            };
            serde_json::json!({
                "name": s.manifest.skill.name,
                "description": s.manifest.skill.description,
                "version": s.manifest.skill.version,
                "author": s.manifest.skill.author,
                "runtime": format!("{:?}", s.manifest.runtime.runtime_type),
                "tools_count": s.manifest.tools.provided.len(),
                "tags": s.manifest.skill.tags,
                "enabled": s.enabled,
                "source": source,
                "has_prompt_context": s.manifest.prompt_context.is_some(),
            })
        })
        .collect();

    Json(serde_json::json!({ "skills": skills, "total": skills.len() }))
}

/// POST /api/skills/install — Install a skill from FangHub (GitHub).
pub async fn install_skill(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SkillInstallRequest>,
) -> impl IntoResponse {
    let skills_dir = state.kernel.config.home_dir.join("skills");
    let config = openfang_skills::marketplace::MarketplaceConfig::default();
    let client = openfang_skills::marketplace::MarketplaceClient::new(config);

    match client.install(&req.name, &skills_dir).await {
        Ok(version) => {
            // Hot-reload so agents see the new skill immediately
            state.kernel.reload_skills();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "installed",
                    "name": req.name,
                    "version": version,
                })),
            )
        }
        Err(e) => {
            tracing::warn!("Skill install failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Install failed: {e}")})),
            )
        }
    }
}

/// POST /api/skills/uninstall — Uninstall a skill.
pub async fn uninstall_skill(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SkillUninstallRequest>,
) -> impl IntoResponse {
    let skills_dir = state.kernel.config.home_dir.join("skills");
    let mut registry = openfang_skills::registry::SkillRegistry::new(skills_dir);
    let _ = registry.load_all();

    match registry.remove(&req.name) {
        Ok(()) => {
            // Hot-reload so agents stop seeing the removed skill
            state.kernel.reload_skills();
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "uninstalled", "name": req.name})),
            )
        }
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

/// GET /api/marketplace/search — Search the FangHub marketplace.
pub async fn marketplace_search(
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let query = params.get("q").cloned().unwrap_or_default();
    if query.is_empty() {
        return Json(serde_json::json!({"results": [], "total": 0}));
    }

    let config = openfang_skills::marketplace::MarketplaceConfig::default();
    let client = openfang_skills::marketplace::MarketplaceClient::new(config);

    match client.search(&query).await {
        Ok(results) => {
            let items: Vec<serde_json::Value> = results
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "name": r.name,
                        "description": r.description,
                        "stars": r.stars,
                        "url": r.url,
                    })
                })
                .collect();
            Json(serde_json::json!({"results": items, "total": items.len()}))
        }
        Err(e) => {
            tracing::warn!("Marketplace search failed: {e}");
            Json(serde_json::json!({"results": [], "total": 0, "error": format!("{e}")}))
        }
    }
}

// ---------------------------------------------------------------------------
// ClawHub (OpenClaw ecosystem) endpoints
// ---------------------------------------------------------------------------

/// GET /api/clawhub/search — Search ClawHub skills using vector/semantic search.
///
/// Query parameters:
/// - `q` — search query (required)
/// - `limit` — max results (default: 20, max: 50)
pub async fn clawhub_search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let query = params.get("q").cloned().unwrap_or_default();
    if query.is_empty() {
        return (
            StatusCode::OK,
            Json(serde_json::json!({"items": [], "next_cursor": null})),
        );
    }

    let limit: u32 = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);

    // Check cache (120s TTL)
    let cache_key = format!("search:{}:{}", query, limit);
    if let Some(entry) = state.clawhub_cache.get(&cache_key) {
        if entry.0.elapsed().as_secs() < 120 {
            return (StatusCode::OK, Json(entry.1.clone()));
        }
    }

    let cache_dir = state.kernel.config.home_dir.join(".cache").join("clawhub");
    let client = openfang_skills::clawhub::ClawHubClient::new(cache_dir);

    let skills_dir = state.kernel.config.home_dir.join("skills");
    match client.search(&query, limit).await {
        Ok(results) => {
            let items: Vec<serde_json::Value> = results
                .results
                .iter()
                .map(|e| {
                    let installed = skills_dir.join(&e.slug).exists();
                    serde_json::json!({
                        "slug": e.slug,
                        "name": e.display_name,
                        "description": e.summary,
                        "version": e.version,
                        "score": e.score,
                        "updated_at": e.updated_at,
                        "installed": installed,
                    })
                })
                .collect();
            let resp = serde_json::json!({
                "items": items,
                "next_cursor": null,
            });
            state
                .clawhub_cache
                .insert(cache_key, (Instant::now(), resp.clone()));
            (StatusCode::OK, Json(resp))
        }
        Err(e) => {
            let msg = format!("{e}");
            tracing::warn!("ClawHub search failed: {msg}");
            let status = if is_clawhub_rate_limit(&e) {
                StatusCode::TOO_MANY_REQUESTS
            } else {
                StatusCode::OK
            };
            (
                status,
                Json(serde_json::json!({"items": [], "next_cursor": null, "error": msg})),
            )
        }
    }
}

/// GET /api/clawhub/browse — Browse ClawHub skills by sort order.
///
/// Query parameters:
/// - `sort` — sort order: "trending", "downloads", "stars", "updated", "rating" (default: "trending")
/// - `limit` — max results (default: 20, max: 50)
/// - `cursor` — pagination cursor from previous response
pub async fn clawhub_browse(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let sort = match params.get("sort").map(|s| s.as_str()) {
        Some("downloads") => openfang_skills::clawhub::ClawHubSort::Downloads,
        Some("stars") => openfang_skills::clawhub::ClawHubSort::Stars,
        Some("updated") => openfang_skills::clawhub::ClawHubSort::Updated,
        Some("rating") => openfang_skills::clawhub::ClawHubSort::Rating,
        _ => openfang_skills::clawhub::ClawHubSort::Trending,
    };

    let limit: u32 = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);

    let cursor = params.get("cursor").map(|s| s.as_str());

    // Check cache (120s TTL)
    let cache_key = format!("browse:{:?}:{}:{}", sort, limit, cursor.unwrap_or(""));
    if let Some(entry) = state.clawhub_cache.get(&cache_key) {
        if entry.0.elapsed().as_secs() < 120 {
            return (StatusCode::OK, Json(entry.1.clone()));
        }
    }

    let cache_dir = state.kernel.config.home_dir.join(".cache").join("clawhub");
    let client = openfang_skills::clawhub::ClawHubClient::new(cache_dir);

    let skills_dir = state.kernel.config.home_dir.join("skills");
    match client.browse(sort, limit, cursor).await {
        Ok(results) => {
            let items: Vec<serde_json::Value> = results
                .items
                .iter()
                .map(|entry| {
                    let mut json = clawhub_browse_entry_to_json(entry);
                    let installed = skills_dir.join(&entry.slug).exists();
                    json["installed"] = serde_json::json!(installed);
                    json
                })
                .collect();
            let resp = serde_json::json!({
                "items": items,
                "next_cursor": results.next_cursor,
            });
            state
                .clawhub_cache
                .insert(cache_key, (Instant::now(), resp.clone()));
            (StatusCode::OK, Json(resp))
        }
        Err(e) => {
            let msg = format!("{e}");
            tracing::warn!("ClawHub browse failed: {msg}");
            let status = if is_clawhub_rate_limit(&e) {
                StatusCode::TOO_MANY_REQUESTS
            } else {
                StatusCode::OK
            };
            (
                status,
                Json(serde_json::json!({"items": [], "next_cursor": null, "error": msg})),
            )
        }
    }
}

/// GET /api/clawhub/skill/{slug} — Get detailed info about a ClawHub skill.
pub async fn clawhub_skill_detail(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    let cache_dir = state.kernel.config.home_dir.join(".cache").join("clawhub");
    let client = openfang_skills::clawhub::ClawHubClient::new(cache_dir);

    let skills_dir = state.kernel.config.home_dir.join("skills");
    let is_installed = client.is_installed(&slug, &skills_dir);

    match client.get_skill(&slug).await {
        Ok(detail) => {
            let version = detail
                .latest_version
                .as_ref()
                .map(|v| v.version.as_str())
                .unwrap_or("");
            let author = detail
                .owner
                .as_ref()
                .map(|o| o.handle.as_str())
                .unwrap_or("");
            let author_name = detail
                .owner
                .as_ref()
                .map(|o| o.display_name.as_str())
                .unwrap_or("");
            let author_image = detail
                .owner
                .as_ref()
                .map(|o| o.image.as_str())
                .unwrap_or("");

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "slug": detail.skill.slug,
                    "name": detail.skill.display_name,
                    "description": detail.skill.summary,
                    "version": version,
                    "downloads": detail.skill.stats.downloads,
                    "stars": detail.skill.stats.stars,
                    "author": author,
                    "author_name": author_name,
                    "author_image": author_image,
                    "tags": detail.skill.tags,
                    "updated_at": detail.skill.updated_at,
                    "created_at": detail.skill.created_at,
                    "installed": is_installed,
                })),
            )
        }
        Err(e) => {
            let status = if is_clawhub_rate_limit(&e) {
                StatusCode::TOO_MANY_REQUESTS
            } else {
                StatusCode::NOT_FOUND
            };
            (status, Json(serde_json::json!({"error": format!("{e}")})))
        }
    }
}

/// GET /api/clawhub/skill/{slug}/code — Fetch the source code (SKILL.md) of a ClawHub skill.
pub async fn clawhub_skill_code(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    let cache_dir = state.kernel.config.home_dir.join(".cache").join("clawhub");
    let client = openfang_skills::clawhub::ClawHubClient::new(cache_dir);

    // Try to fetch SKILL.md first, then fallback to package.json
    let mut code = String::new();
    let mut filename = String::new();

    if let Ok(content) = client.get_file(&slug, "SKILL.md").await {
        code = content;
        filename = "SKILL.md".to_string();
    } else if let Ok(content) = client.get_file(&slug, "package.json").await {
        code = content;
        filename = "package.json".to_string();
    } else if let Ok(content) = client.get_file(&slug, "skill.toml").await {
        code = content;
        filename = "skill.toml".to_string();
    }

    if code.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "No source code found for this skill"})),
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "slug": slug,
            "filename": filename,
            "code": code,
        })),
    )
}

/// POST /api/clawhub/install — Install a skill from ClawHub.
///
/// Runs the full security pipeline: SHA256 verification, format detection,
/// manifest security scan, prompt injection scan, and binary dependency check.
pub async fn clawhub_install(
    State(state): State<Arc<AppState>>,
    Json(req): Json<crate::types::ClawHubInstallRequest>,
) -> impl IntoResponse {
    let skills_dir = state.kernel.config.home_dir.join("skills");
    let cache_dir = state.kernel.config.home_dir.join(".cache").join("clawhub");
    let client = openfang_skills::clawhub::ClawHubClient::new(cache_dir);

    // Check if already installed
    if client.is_installed(&req.slug, &skills_dir) {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!("Skill '{}' is already installed", req.slug),
                "status": "already_installed",
            })),
        );
    }

    match client.install(&req.slug, &skills_dir).await {
        Ok(result) => {
            let warnings: Vec<serde_json::Value> = result
                .warnings
                .iter()
                .map(|w| {
                    serde_json::json!({
                        "severity": format!("{:?}", w.severity),
                        "message": w.message,
                    })
                })
                .collect();

            let translations: Vec<serde_json::Value> = result
                .tool_translations
                .iter()
                .map(|(from, to)| serde_json::json!({"from": from, "to": to}))
                .collect();

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "installed",
                    "name": result.skill_name,
                    "version": result.version,
                    "slug": result.slug,
                    "is_prompt_only": result.is_prompt_only,
                    "warnings": warnings,
                    "tool_translations": translations,
                })),
            )
        }
        Err(e) => {
            let msg = format!("{e}");
            let status = if matches!(e, openfang_skills::SkillError::SecurityBlocked(_)) {
                StatusCode::FORBIDDEN
            } else if is_clawhub_rate_limit(&e) {
                StatusCode::TOO_MANY_REQUESTS
            } else if matches!(e, openfang_skills::SkillError::Network(_)) {
                StatusCode::BAD_GATEWAY
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            tracing::warn!("ClawHub install failed: {msg}");
            (status, Json(serde_json::json!({"error": msg})))
        }
    }
}

/// Check whether a SkillError represents a ClawHub rate-limit (429).
fn is_clawhub_rate_limit(err: &openfang_skills::SkillError) -> bool {
    matches!(err, openfang_skills::SkillError::RateLimited(_))
}

/// Convert a browse entry (nested stats/tags) to a flat JSON object for the frontend.
fn clawhub_browse_entry_to_json(
    entry: &openfang_skills::clawhub::ClawHubBrowseEntry,
) -> serde_json::Value {
    let version = openfang_skills::clawhub::ClawHubClient::entry_version(entry);
    serde_json::json!({
        "slug": entry.slug,
        "name": entry.display_name,
        "description": entry.summary,
        "version": version,
        "downloads": entry.stats.downloads,
        "stars": entry.stats.stars,
        "updated_at": entry.updated_at,
    })
}

// ---------------------------------------------------------------------------
// Hands endpoints
// ---------------------------------------------------------------------------

/// Detect the server platform for install command selection.
fn server_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

/// GET /api/hands — List all hand definitions (marketplace).
pub async fn list_hands(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let defs = state.kernel.hand_registry.list_definitions();
    let hands: Vec<serde_json::Value> = defs
        .iter()
        .map(|d| {
            let reqs = state
                .kernel
                .hand_registry
                .check_requirements(&d.id)
                .unwrap_or_default();
            let readiness = state.kernel.hand_registry.readiness(&d.id);
            let requirements_met = readiness
                .as_ref()
                .map(|r| r.requirements_met)
                .unwrap_or(false);
            let active = readiness.as_ref().map(|r| r.active).unwrap_or(false);
            let degraded = readiness.as_ref().map(|r| r.degraded).unwrap_or(false);
            serde_json::json!({
                "id": d.id,
                "name": d.name,
                "description": d.description,
                "category": d.category,
                "icon": d.icon,
                "tools": d.tools,
                "requirements_met": requirements_met,
                "active": active,
                "degraded": degraded,
                "requirements": reqs.iter().map(|(r, ok)| serde_json::json!({
                    "key": r.key,
                    "label": r.label,
                    "satisfied": ok,
                    "optional": r.optional,
                })).collect::<Vec<_>>(),
                "dashboard_metrics": d.dashboard.metrics.len(),
                "has_settings": !d.settings.is_empty(),
                "settings_count": d.settings.len(),
            })
        })
        .collect();

    Json(serde_json::json!({ "hands": hands, "total": hands.len() }))
}

/// GET /api/hands/active — List active hand instances.
pub async fn list_active_hands(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let instances = state.kernel.hand_registry.list_instances();
    let items: Vec<serde_json::Value> = instances
        .iter()
        .map(|i| {
            serde_json::json!({
                "instance_id": i.instance_id,
                "hand_id": i.hand_id,
                "status": format!("{}", i.status),
                "agent_id": i.agent_id.map(|a| a.to_string()),
                "agent_name": i.agent_name,
                "activated_at": i.activated_at.to_rfc3339(),
                "updated_at": i.updated_at.to_rfc3339(),
            })
        })
        .collect();

    Json(serde_json::json!({ "instances": items, "total": items.len() }))
}

/// GET /api/hands/{hand_id} — Get a single hand definition with requirements check.
pub async fn get_hand(
    State(state): State<Arc<AppState>>,
    Path(hand_id): Path<String>,
) -> impl IntoResponse {
    match state.kernel.hand_registry.get_definition(&hand_id) {
        Some(def) => {
            let reqs = state
                .kernel
                .hand_registry
                .check_requirements(&hand_id)
                .unwrap_or_default();
            let readiness = state.kernel.hand_registry.readiness(&hand_id);
            let requirements_met = readiness
                .as_ref()
                .map(|r| r.requirements_met)
                .unwrap_or(false);
            let active = readiness.as_ref().map(|r| r.active).unwrap_or(false);
            let degraded = readiness.as_ref().map(|r| r.degraded).unwrap_or(false);
            let settings_status = state
                .kernel
                .hand_registry
                .check_settings_availability(&hand_id)
                .unwrap_or_default();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "id": def.id,
                    "name": def.name,
                    "description": def.description,
                    "category": def.category,
                    "icon": def.icon,
                    "tools": def.tools,
                    "requirements_met": requirements_met,
                    "active": active,
                    "degraded": degraded,
                    "requirements": reqs.iter().map(|(r, ok)| {
                        let mut req_json = serde_json::json!({
                            "key": r.key,
                            "label": r.label,
                            "type": format!("{:?}", r.requirement_type),
                            "check_value": r.check_value,
                            "satisfied": ok,
                            "optional": r.optional,
                        });
                        if let Some(ref desc) = r.description {
                            req_json["description"] = serde_json::json!(desc);
                        }
                        if let Some(ref install) = r.install {
                            req_json["install"] = serde_json::to_value(install).unwrap_or_default();
                        }
                        req_json
                    }).collect::<Vec<_>>(),
                    "server_platform": server_platform(),
                    "agent": {
                        "name": def.agent.name,
                        "description": def.agent.description,
                        "provider": if def.agent.provider == "default" {
                            &state.kernel.config.default_model.provider
                        } else { &def.agent.provider },
                        "model": if def.agent.model == "default" {
                            &state.kernel.config.default_model.model
                        } else { &def.agent.model },
                    },
                    "dashboard": def.dashboard.metrics.iter().map(|m| serde_json::json!({
                        "label": m.label,
                        "memory_key": m.memory_key,
                        "format": m.format,
                    })).collect::<Vec<_>>(),
                    "settings": settings_status,
                })),
            )
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("Hand not found: {hand_id}")})),
        ),
    }
}

/// POST /api/hands/{hand_id}/check-deps — Re-check dependency status for a hand.
pub async fn check_hand_deps(
    State(state): State<Arc<AppState>>,
    Path(hand_id): Path<String>,
) -> impl IntoResponse {
    match state.kernel.hand_registry.get_definition(&hand_id) {
        Some(def) => {
            let reqs = state
                .kernel
                .hand_registry
                .check_requirements(&hand_id)
                .unwrap_or_default();
            let readiness = state.kernel.hand_registry.readiness(&hand_id);
            let requirements_met = readiness
                .as_ref()
                .map(|r| r.requirements_met)
                .unwrap_or(false);
            let active = readiness.as_ref().map(|r| r.active).unwrap_or(false);
            let degraded = readiness.as_ref().map(|r| r.degraded).unwrap_or(false);
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "hand_id": def.id,
                    "requirements_met": requirements_met,
                    "active": active,
                    "degraded": degraded,
                    "server_platform": server_platform(),
                    "requirements": reqs.iter().map(|(r, ok)| {
                        let mut req_json = serde_json::json!({
                            "key": r.key,
                            "label": r.label,
                            "type": format!("{:?}", r.requirement_type),
                            "check_value": r.check_value,
                            "satisfied": ok,
                            "optional": r.optional,
                        });
                        if let Some(ref desc) = r.description {
                            req_json["description"] = serde_json::json!(desc);
                        }
                        if let Some(ref install) = r.install {
                            req_json["install"] = serde_json::to_value(install).unwrap_or_default();
                        }
                        req_json
                    }).collect::<Vec<_>>(),
                })),
            )
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("Hand not found: {hand_id}")})),
        ),
    }
}

/// POST /api/hands/{hand_id}/install-deps — Auto-install missing dependencies for a hand.
pub async fn install_hand_deps(
    State(state): State<Arc<AppState>>,
    Path(hand_id): Path<String>,
) -> impl IntoResponse {
    let def = match state.kernel.hand_registry.get_definition(&hand_id) {
        Some(d) => d.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": format!("Hand not found: {hand_id}")})),
            );
        }
    };

    let reqs = state
        .kernel
        .hand_registry
        .check_requirements(&hand_id)
        .unwrap_or_default();

    let platform = server_platform();
    let mut results = Vec::new();

    for (req, already_satisfied) in &reqs {
        if *already_satisfied {
            results.push(serde_json::json!({
                "key": req.key,
                "status": "already_installed",
                "message": format!("{} is already available", req.label),
            }));
            continue;
        }

        let install = match &req.install {
            Some(i) => i,
            None => {
                results.push(serde_json::json!({
                    "key": req.key,
                    "status": "skipped",
                    "message": "No install instructions available",
                }));
                continue;
            }
        };

        // Pick the best install command for this platform
        let cmd = match platform {
            "windows" => install.windows.as_deref().or(install.pip.as_deref()),
            "macos" => install.macos.as_deref().or(install.pip.as_deref()),
            _ => install
                .linux_apt
                .as_deref()
                .or(install.linux_dnf.as_deref())
                .or(install.linux_pacman.as_deref())
                .or(install.pip.as_deref()),
        };

        let cmd = match cmd {
            Some(c) => c,
            None => {
                results.push(serde_json::json!({
                    "key": req.key,
                    "status": "no_command",
                    "message": format!("No install command for platform: {platform}"),
                }));
                continue;
            }
        };

        // Execute the install command
        let (shell, flag) = if cfg!(windows) {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };

        // For winget on Windows, add --accept flags to avoid interactive prompts
        let final_cmd = if cfg!(windows) && cmd.starts_with("winget ") {
            format!("{cmd} --accept-source-agreements --accept-package-agreements")
        } else {
            cmd.to_string()
        };

        tracing::info!(hand = %hand_id, dep = %req.key, cmd = %final_cmd, "Auto-installing dependency");

        let output = match tokio::time::timeout(
            std::time::Duration::from_secs(300),
            tokio::process::Command::new(shell)
                .arg(flag)
                .arg(&final_cmd)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .stdin(std::process::Stdio::null())
                .output(),
        )
        .await
        {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => {
                results.push(serde_json::json!({
                    "key": req.key,
                    "status": "error",
                    "command": final_cmd,
                    "message": format!("Failed to execute: {e}"),
                }));
                continue;
            }
            Err(_) => {
                results.push(serde_json::json!({
                    "key": req.key,
                    "status": "timeout",
                    "command": final_cmd,
                    "message": "Installation timed out after 5 minutes",
                }));
                continue;
            }
        };

        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if exit_code == 0 {
            results.push(serde_json::json!({
                "key": req.key,
                "status": "installed",
                "command": final_cmd,
                "message": format!("{} installed successfully", req.label),
            }));
        } else {
            // On Windows, winget may return non-zero even on success (e.g., already installed)
            let combined = format!("{stdout}{stderr}");
            let likely_ok = combined.contains("already installed")
                || combined.contains("No applicable update")
                || combined.contains("No available upgrade")
                || combined.contains("already an App at")
                || combined.contains("is already installed");
            results.push(serde_json::json!({
                "key": req.key,
                "status": if likely_ok { "installed" } else { "error" },
                "command": final_cmd,
                "exit_code": exit_code,
                "message": if likely_ok {
                    format!("{} is already installed", req.label)
                } else {
                    let msg = stderr.chars().take(500).collect::<String>();
                    format!("Install failed (exit {}): {}", exit_code, msg.trim())
                },
            }));
        }
    }

    // On Windows, refresh PATH to pick up newly installed binaries from winget/pip
    #[cfg(windows)]
    {
        let home = std::env::var("USERPROFILE").unwrap_or_default();
        if !home.is_empty() {
            let winget_pkgs =
                std::path::Path::new(&home).join("AppData\\Local\\Microsoft\\WinGet\\Packages");
            if winget_pkgs.is_dir() {
                let mut extra_paths = Vec::new();
                if let Ok(entries) = std::fs::read_dir(&winget_pkgs) {
                    for entry in entries.flatten() {
                        let pkg_dir = entry.path();
                        // Look for bin/ subdirectory (ffmpeg style)
                        if let Ok(sub_entries) = std::fs::read_dir(&pkg_dir) {
                            for sub in sub_entries.flatten() {
                                let bin_dir = sub.path().join("bin");
                                if bin_dir.is_dir() {
                                    extra_paths.push(bin_dir.to_string_lossy().to_string());
                                }
                            }
                        }
                        // Direct exe in package dir (yt-dlp style)
                        if std::fs::read_dir(&pkg_dir)
                            .map(|rd| {
                                rd.flatten().any(|e| {
                                    e.path().extension().map(|x| x == "exe").unwrap_or(false)
                                })
                            })
                            .unwrap_or(false)
                        {
                            extra_paths.push(pkg_dir.to_string_lossy().to_string());
                        }
                    }
                }
                // Also add pip Scripts dir
                let pip_scripts =
                    std::path::Path::new(&home).join("AppData\\Local\\Programs\\Python");
                if pip_scripts.is_dir() {
                    if let Ok(entries) = std::fs::read_dir(&pip_scripts) {
                        for entry in entries.flatten() {
                            let scripts = entry.path().join("Scripts");
                            if scripts.is_dir() {
                                extra_paths.push(scripts.to_string_lossy().to_string());
                            }
                        }
                    }
                }
                if !extra_paths.is_empty() {
                    let current_path = std::env::var("PATH").unwrap_or_default();
                    let new_path = format!("{};{}", extra_paths.join(";"), current_path);
                    std::env::set_var("PATH", &new_path);
                    tracing::info!(
                        added = extra_paths.len(),
                        "Refreshed PATH with winget/pip directories"
                    );
                }
            }
        }
    }

    // Re-check requirements after installation
    let reqs_after = state
        .kernel
        .hand_registry
        .check_requirements(&hand_id)
        .unwrap_or_default();
    let all_satisfied = reqs_after.iter().all(|(_, ok)| *ok);

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "hand_id": def.id,
            "results": results,
            "requirements_met": all_satisfied,
            "requirements": reqs_after.iter().map(|(r, ok)| {
                serde_json::json!({
                    "key": r.key,
                    "label": r.label,
                    "satisfied": ok,
                })
            }).collect::<Vec<_>>(),
        })),
    )
}

/// POST /api/hands/install — Install a hand from TOML content.
pub async fn install_hand(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let toml_content = body["toml_content"].as_str().unwrap_or("");
    let skill_content = body["skill_content"].as_str().unwrap_or("");

    if toml_content.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Missing toml_content field"})),
        );
    }

    match state
        .kernel
        .hand_registry
        .install_from_content(toml_content, skill_content)
    {
        Ok(def) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "id": def.id,
                "name": def.name,
                "description": def.description,
                "category": format!("{:?}", def.category),
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

/// POST /api/hands/upsert — Install or update a hand definition.
///
/// Like `install_hand` but overwrites an existing definition with the same ID.
/// Active instances are NOT automatically restarted — deactivate + reactivate
/// to pick up the new definition.
pub async fn upsert_hand(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let toml_content = body["toml_content"].as_str().unwrap_or("");
    let skill_content = body["skill_content"].as_str().unwrap_or("");

    if toml_content.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Missing toml_content field"})),
        );
    }

    match state
        .kernel
        .hand_registry
        .upsert_from_content(toml_content, skill_content)
    {
        Ok(def) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "id": def.id,
                "name": def.name,
                "description": def.description,
                "category": format!("{:?}", def.category),
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

/// POST /api/hands/{hand_id}/activate — Activate a hand (spawns agent).
pub async fn activate_hand(
    State(state): State<Arc<AppState>>,
    Path(hand_id): Path<String>,
    body: Option<Json<openfang_hands::ActivateHandRequest>>,
) -> impl IntoResponse {
    let config = body.map(|b| b.0.config).unwrap_or_default();

    match state.kernel.activate_hand(&hand_id, config) {
        Ok(instance) => {
            // If the hand agent has a non-reactive schedule (autonomous hands),
            // start its background loop so it begins running immediately.
            if let Some(agent_id) = instance.agent_id {
                let entry = state
                    .kernel
                    .registry
                    .list()
                    .into_iter()
                    .find(|e| e.id == agent_id);
                if let Some(entry) = entry {
                    if !matches!(
                        entry.manifest.schedule,
                        openfang_types::agent::ScheduleMode::Reactive
                    ) {
                        state.kernel.start_background_for_agent(
                            agent_id,
                            &entry.name,
                            &entry.manifest.schedule,
                        );
                    }
                }
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "instance_id": instance.instance_id,
                    "hand_id": instance.hand_id,
                    "status": format!("{}", instance.status),
                    "agent_id": instance.agent_id.map(|a| a.to_string()),
                    "agent_name": instance.agent_name,
                    "activated_at": instance.activated_at.to_rfc3339(),
                })),
            )
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

/// POST /api/hands/instances/{id}/pause — Pause a hand instance.
pub async fn pause_hand(
    State(state): State<Arc<AppState>>,
    Path(id): Path<uuid::Uuid>,
) -> impl IntoResponse {
    match state.kernel.pause_hand(id) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "paused", "instance_id": id})),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

/// POST /api/hands/instances/{id}/resume — Resume a paused hand instance.
pub async fn resume_hand(
    State(state): State<Arc<AppState>>,
    Path(id): Path<uuid::Uuid>,
) -> impl IntoResponse {
    match state.kernel.resume_hand(id) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "resumed", "instance_id": id})),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

/// DELETE /api/hands/instances/{id} — Deactivate a hand (kills agent).
pub async fn deactivate_hand(
    State(state): State<Arc<AppState>>,
    Path(id): Path<uuid::Uuid>,
) -> impl IntoResponse {
    match state.kernel.deactivate_hand(id) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "deactivated", "instance_id": id})),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

/// GET /api/hands/{hand_id}/settings — Get settings schema and current values for a hand.
pub async fn get_hand_settings(
    State(state): State<Arc<AppState>>,
    Path(hand_id): Path<String>,
) -> impl IntoResponse {
    let settings_status = match state
        .kernel
        .hand_registry
        .check_settings_availability(&hand_id)
    {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": format!("Hand not found: {hand_id}")})),
            );
        }
    };

    // Find active instance config values (if any)
    let instance_config: std::collections::HashMap<String, serde_json::Value> = state
        .kernel
        .hand_registry
        .list_instances()
        .iter()
        .find(|i| i.hand_id == hand_id)
        .map(|i| i.config.clone())
        .unwrap_or_default();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "hand_id": hand_id,
            "settings": settings_status,
            "current_values": instance_config,
        })),
    )
}

/// PUT /api/hands/{hand_id}/settings — Update settings for a hand instance.
pub async fn update_hand_settings(
    State(state): State<Arc<AppState>>,
    Path(hand_id): Path<String>,
    Json(config): Json<std::collections::HashMap<String, serde_json::Value>>,
) -> impl IntoResponse {
    // Find active instance for this hand
    let instance_id = state
        .kernel
        .hand_registry
        .list_instances()
        .iter()
        .find(|i| i.hand_id == hand_id)
        .map(|i| i.instance_id);

    match instance_id {
        Some(id) => match state.kernel.hand_registry.update_config(id, config.clone()) {
            Ok(()) => (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "ok",
                    "hand_id": hand_id,
                    "instance_id": id,
                    "config": config,
                })),
            ),
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("{e}")})),
            ),
        },
        None => (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({"error": format!("No active instance for hand: {hand_id}. Activate the hand first.")}),
            ),
        ),
    }
}

/// GET /api/hands/instances/{id}/stats — Get dashboard stats for a hand instance.
pub async fn hand_stats(
    State(state): State<Arc<AppState>>,
    Path(id): Path<uuid::Uuid>,
) -> impl IntoResponse {
    let instance = match state.kernel.hand_registry.get_instance(id) {
        Some(i) => i,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Instance not found"})),
            );
        }
    };

    let def = match state.kernel.hand_registry.get_definition(&instance.hand_id) {
        Some(d) => d,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Hand definition not found"})),
            );
        }
    };

    let agent_id = match instance.agent_id {
        Some(aid) => aid,
        None => {
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "instance_id": id,
                    "hand_id": instance.hand_id,
                    "metrics": {},
                })),
            );
        }
    };

    // Read dashboard metrics from shared structured memory (memory_store uses shared namespace)
    let shared_id = openfang_kernel::kernel::shared_memory_agent_id();
    let mut metrics = serde_json::Map::new();
    for metric in &def.dashboard.metrics {
        // Try shared memory first (where memory_store tool writes), fall back to agent-specific
        let value = state
            .kernel
            .memory
            .structured_get(shared_id, &metric.memory_key)
            .ok()
            .flatten()
            .or_else(|| {
                state
                    .kernel
                    .memory
                    .structured_get(agent_id, &metric.memory_key)
                    .ok()
                    .flatten()
            })
            .unwrap_or(serde_json::Value::Null);
        metrics.insert(
            metric.label.clone(),
            serde_json::json!({
                "value": value,
                "format": metric.format,
            }),
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "instance_id": id,
            "hand_id": instance.hand_id,
            "status": format!("{}", instance.status),
            "agent_id": agent_id.to_string(),
            "metrics": metrics,
        })),
    )
}

/// GET /api/hands/instances/{id}/browser — Get live browser state for a hand instance.
pub async fn hand_instance_browser(
    State(state): State<Arc<AppState>>,
    Path(id): Path<uuid::Uuid>,
) -> impl IntoResponse {
    // 1. Look up instance
    let instance = match state.kernel.hand_registry.get_instance(id) {
        Some(i) => i,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Instance not found"})),
            );
        }
    };

    // 2. Get agent_id
    let agent_id = match instance.agent_id {
        Some(aid) => aid,
        None => {
            return (StatusCode::OK, Json(serde_json::json!({"active": false})));
        }
    };

    let agent_id_str = agent_id.to_string();

    // 3. Check if a browser session exists (without creating one)
    if !state.kernel.browser_ctx.has_session(&agent_id_str) {
        return (StatusCode::OK, Json(serde_json::json!({"active": false})));
    }

    // 4. Send ReadPage command to get page info
    let mut url = String::new();
    let mut title = String::new();
    let mut content = String::new();

    match state
        .kernel
        .browser_ctx
        .send_command(
            &agent_id_str,
            openfang_runtime::browser::BrowserCommand::ReadPage,
        )
        .await
    {
        Ok(resp) if resp.success => {
            if let Some(data) = &resp.data {
                url = data["url"].as_str().unwrap_or("").to_string();
                title = data["title"].as_str().unwrap_or("").to_string();
                content = data["content"].as_str().unwrap_or("").to_string();
                // Truncate content to avoid huge payloads (UTF-8 safe)
                if content.len() > 2000 {
                    content = format!(
                        "{}... (truncated)",
                        openfang_types::truncate_str(&content, 2000)
                    );
                }
            }
        }
        Ok(_) => {}  // Non-success: leave defaults
        Err(_) => {} // Error: leave defaults
    }

    // 5. Send Screenshot command to get visual state
    let mut screenshot_base64 = String::new();

    match state
        .kernel
        .browser_ctx
        .send_command(
            &agent_id_str,
            openfang_runtime::browser::BrowserCommand::Screenshot,
        )
        .await
    {
        Ok(resp) if resp.success => {
            if let Some(data) = &resp.data {
                screenshot_base64 = data["image_base64"].as_str().unwrap_or("").to_string();
            }
        }
        Ok(_) => {}
        Err(_) => {}
    }

    // 6. Return combined state
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "active": true,
            "url": url,
            "title": title,
            "content": content,
            "screenshot_base64": screenshot_base64,
        })),
    )
}

// ---------------------------------------------------------------------------
// MCP server endpoints
// ---------------------------------------------------------------------------

/// GET /api/mcp/servers — List configured MCP servers and their tools.
pub async fn list_mcp_servers(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Get configured servers from config
    let config_servers: Vec<serde_json::Value> = state
        .kernel
        .config
        .mcp_servers
        .iter()
        .map(|s| {
            let transport = match &s.transport {
                openfang_types::config::McpTransportEntry::Stdio { command, args } => {
                    serde_json::json!({
                        "type": "stdio",
                        "command": command,
                        "args": args,
                    })
                }
                openfang_types::config::McpTransportEntry::Sse { url } => {
                    serde_json::json!({
                        "type": "sse",
                        "url": url,
                    })
                }
            };
            serde_json::json!({
                "name": s.name,
                "transport": transport,
                "timeout_secs": s.timeout_secs,
                "env": s.env,
            })
        })
        .collect();

    // Get connected servers and their tools from the live MCP connections
    let connections = state.kernel.mcp_connections.lock().await;
    let connected: Vec<serde_json::Value> = connections
        .iter()
        .map(|conn| {
            let tools: Vec<serde_json::Value> = conn
                .tools()
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name,
                        "description": t.description,
                    })
                })
                .collect();
            serde_json::json!({
                "name": conn.name(),
                "tools_count": tools.len(),
                "tools": tools,
                "connected": true,
            })
        })
        .collect();

    Json(serde_json::json!({
        "configured": config_servers,
        "connected": connected,
        "total_configured": config_servers.len(),
        "total_connected": connected.len(),
    }))
}

// ---------------------------------------------------------------------------
// Audit endpoints
// ---------------------------------------------------------------------------

/// GET /api/audit/recent — Get recent audit log entries.
pub async fn audit_recent(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let n: usize = params
        .get("n")
        .and_then(|v| v.parse().ok())
        .unwrap_or(50)
        .min(1000); // Cap at 1000

    let entries = state.kernel.audit_log.recent(n);
    let tip = state.kernel.audit_log.tip_hash();

    let items: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "seq": e.seq,
                "timestamp": e.timestamp,
                "agent_id": e.agent_id,
                "action": format!("{:?}", e.action),
                "detail": e.detail,
                "outcome": e.outcome,
                "hash": e.hash,
            })
        })
        .collect();

    Json(serde_json::json!({
        "entries": items,
        "total": state.kernel.audit_log.len(),
        "tip_hash": tip,
    }))
}

/// GET /api/audit/verify — Verify the audit chain integrity.
pub async fn audit_verify(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let entry_count = state.kernel.audit_log.len();
    match state.kernel.audit_log.verify_integrity() {
        Ok(()) => {
            if entry_count == 0 {
                // SECURITY: Warn that an empty audit log has no forensic value
                Json(serde_json::json!({
                    "valid": true,
                    "entries": 0,
                    "warning": "Audit log is empty — no events have been recorded yet",
                    "tip_hash": state.kernel.audit_log.tip_hash(),
                }))
            } else {
                Json(serde_json::json!({
                    "valid": true,
                    "entries": entry_count,
                    "tip_hash": state.kernel.audit_log.tip_hash(),
                }))
            }
        }
        Err(msg) => Json(serde_json::json!({
            "valid": false,
            "error": msg,
            "entries": entry_count,
        })),
    }
}

/// GET /api/logs/stream — SSE endpoint for real-time audit log streaming.
///
/// Streams new audit entries as Server-Sent Events. Accepts optional query
/// parameters for filtering:
///   - `level`  — filter by classified level (info, warn, error)
///   - `filter` — text substring filter across action/detail/agent_id
///   - `token`  — auth token (for EventSource clients that cannot set headers)
///
/// A heartbeat ping is sent every 15 seconds to keep the connection alive.
/// The endpoint polls the audit log every second and sends only new entries
/// (tracked by sequence number). On first connect, existing entries are sent
/// as a backfill so the client has immediate context.
pub async fn logs_stream(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> axum::response::Response {
    use axum::response::sse::{Event, KeepAlive, Sse};

    let level_filter = params.get("level").cloned().unwrap_or_default();
    let text_filter = params
        .get("filter")
        .cloned()
        .unwrap_or_default()
        .to_lowercase();

    let (tx, rx) = tokio::sync::mpsc::channel::<
        Result<axum::response::sse::Event, std::convert::Infallible>,
    >(256);

    tokio::spawn(async move {
        let mut last_seq: u64 = 0;
        let mut first_poll = true;

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;

            let entries = state.kernel.audit_log.recent(200);

            for entry in &entries {
                // On first poll, send all existing entries as backfill.
                // After that, only send entries newer than last_seq.
                if !first_poll && entry.seq <= last_seq {
                    continue;
                }

                let action_str = format!("{:?}", entry.action);

                // Apply level filter
                if !level_filter.is_empty() {
                    let classified = classify_audit_level(&action_str);
                    if classified != level_filter {
                        continue;
                    }
                }

                // Apply text filter
                if !text_filter.is_empty() {
                    let haystack = format!("{} {} {}", action_str, entry.detail, entry.agent_id)
                        .to_lowercase();
                    if !haystack.contains(&text_filter) {
                        continue;
                    }
                }

                let json = serde_json::json!({
                    "seq": entry.seq,
                    "timestamp": entry.timestamp,
                    "agent_id": entry.agent_id,
                    "action": action_str,
                    "detail": entry.detail,
                    "outcome": entry.outcome,
                    "hash": entry.hash,
                });
                let data = serde_json::to_string(&json).unwrap_or_default();
                if tx.send(Ok(Event::default().data(data))).await.is_err() {
                    return; // Client disconnected
                }
            }

            // Update tracking state
            if let Some(last) = entries.last() {
                last_seq = last.seq;
            }
            first_poll = false;
        }
    });

    let rx_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Sse::new(rx_stream)
        .keep_alive(
            KeepAlive::new()
                .interval(std::time::Duration::from_secs(15))
                .text("ping"),
        )
        .into_response()
}

/// Classify an audit action string into a level (info, warn, error).
fn classify_audit_level(action: &str) -> &'static str {
    let a = action.to_lowercase();
    if a.contains("error") || a.contains("fail") || a.contains("crash") || a.contains("denied") {
        "error"
    } else if a.contains("warn") || a.contains("block") || a.contains("kill") {
        "warn"
    } else {
        "info"
    }
}

// ---------------------------------------------------------------------------
// Peer endpoints
// ---------------------------------------------------------------------------

/// GET /api/peers — List known OFP peers.
pub async fn list_peers(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Peers are tracked in the wire module's PeerRegistry.
    // The kernel doesn't directly hold a PeerRegistry, so we return an empty list
    // unless one is available. The API server can be extended to inject a registry.
    if let Some(ref peer_registry) = state.peer_registry {
        let peers: Vec<serde_json::Value> = peer_registry
            .all_peers()
            .iter()
            .map(|p| {
                serde_json::json!({
                    "node_id": p.node_id,
                    "node_name": p.node_name,
                    "address": p.address.to_string(),
                    "state": format!("{:?}", p.state),
                    "agents": p.agents.iter().map(|a| serde_json::json!({
                        "id": a.id,
                        "name": a.name,
                    })).collect::<Vec<_>>(),
                    "connected_at": p.connected_at.to_rfc3339(),
                    "protocol_version": p.protocol_version,
                })
            })
            .collect();
        Json(serde_json::json!({"peers": peers, "total": peers.len()}))
    } else {
        Json(serde_json::json!({"peers": [], "total": 0}))
    }
}

/// GET /api/network/status — OFP network status summary.
pub async fn network_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let enabled = state.kernel.config.network_enabled
        && !state.kernel.config.network.shared_secret.is_empty();

    let (node_id, listen_address, connected_peers, total_peers) =
        if let Some(peer_node) = state.kernel.peer_node.get() {
            let registry = peer_node.registry();
            (
                peer_node.node_id().to_string(),
                peer_node.local_addr().to_string(),
                registry.connected_count(),
                registry.total_count(),
            )
        } else {
            (String::new(), String::new(), 0, 0)
        };

    Json(serde_json::json!({
        "enabled": enabled,
        "node_id": node_id,
        "listen_address": listen_address,
        "connected_peers": connected_peers,
        "total_peers": total_peers,
    }))
}

// ---------------------------------------------------------------------------
// Tools endpoint
// ---------------------------------------------------------------------------

/// GET /api/tools — List all tool definitions (built-in + MCP).
pub async fn list_tools(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut tools: Vec<serde_json::Value> = builtin_tool_definitions()
        .iter()
        .map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.input_schema,
            })
        })
        .collect();

    // Include MCP tools so they're visible in Settings -> Tools
    if let Ok(mcp_tools) = state.kernel.mcp_tools.lock() {
        for t in mcp_tools.iter() {
            tools.push(serde_json::json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.input_schema,
                "source": "mcp",
            }));
        }
    }

    Json(serde_json::json!({"tools": tools, "total": tools.len()}))
}

// ---------------------------------------------------------------------------
// Config endpoint
// ---------------------------------------------------------------------------

/// GET /api/config — Get kernel configuration (secrets redacted).
pub async fn get_config(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Return a redacted view of the kernel config
    let config = &state.kernel.config;
    Json(serde_json::json!({
        "home_dir": config.home_dir.to_string_lossy(),
        "data_dir": config.data_dir.to_string_lossy(),
        "persistence": {
            "runtime_db": config
                .persistence
                .resolve_runtime_db(&config.data_dir)
                .to_string_lossy(),
            "compozy_db": config
                .persistence
                .resolve_compozy_db(&config.data_dir)
                .to_string_lossy(),
        },
        "api_key": if config.api_key.is_empty() { "not set" } else { "***" },
        "default_model": {
            "provider": config.default_model.provider,
            "model": config.default_model.model,
            "api_key_env": config.default_model.api_key_env,
        },
        "memory": {
            "decay_rate": config.memory.decay_rate,
        },
    }))
}

// ---------------------------------------------------------------------------
// Usage endpoint
// ---------------------------------------------------------------------------

/// GET /api/usage — Get per-agent usage statistics.
pub async fn usage_stats(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let agents: Vec<serde_json::Value> = state
        .kernel
        .registry
        .list()
        .iter()
        .map(|e| {
            let (tokens, tool_calls) = state.kernel.scheduler.get_usage(e.id).unwrap_or((0, 0));
            serde_json::json!({
                "agent_id": e.id.to_string(),
                "name": e.name,
                "total_tokens": tokens,
                "tool_calls": tool_calls,
            })
        })
        .collect();

    Json(serde_json::json!({"agents": agents}))
}

// ---------------------------------------------------------------------------
// Usage summary endpoints
// ---------------------------------------------------------------------------

/// GET /api/usage/summary — Get overall usage summary from UsageStore.
pub async fn usage_summary(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.kernel.memory.usage().query_summary(None) {
        Ok(s) => Json(serde_json::json!({
            "total_input_tokens": s.total_input_tokens,
            "total_output_tokens": s.total_output_tokens,
            "total_cost_usd": s.total_cost_usd,
            "call_count": s.call_count,
            "total_tool_calls": s.total_tool_calls,
        })),
        Err(_) => Json(serde_json::json!({
            "total_input_tokens": 0,
            "total_output_tokens": 0,
            "total_cost_usd": 0.0,
            "call_count": 0,
            "total_tool_calls": 0,
        })),
    }
}

/// GET /api/usage/by-model — Get usage grouped by model.
pub async fn usage_by_model(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.kernel.memory.usage().query_by_model() {
        Ok(models) => {
            let list: Vec<serde_json::Value> = models
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "model": m.model,
                        "total_cost_usd": m.total_cost_usd,
                        "total_input_tokens": m.total_input_tokens,
                        "total_output_tokens": m.total_output_tokens,
                        "call_count": m.call_count,
                    })
                })
                .collect();
            Json(serde_json::json!({"models": list}))
        }
        Err(_) => Json(serde_json::json!({"models": []})),
    }
}

/// GET /api/usage/daily — Get daily usage breakdown for the last 7 days.
pub async fn usage_daily(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let days = state.kernel.memory.usage().query_daily_breakdown(7);
    let today_cost = state.kernel.memory.usage().query_today_cost();
    let first_event = state.kernel.memory.usage().query_first_event_date();

    let days_list = match days {
        Ok(d) => d
            .iter()
            .map(|day| {
                serde_json::json!({
                    "date": day.date,
                    "cost_usd": day.cost_usd,
                    "tokens": day.tokens,
                    "calls": day.calls,
                })
            })
            .collect::<Vec<_>>(),
        Err(_) => vec![],
    };

    Json(serde_json::json!({
        "days": days_list,
        "today_cost_usd": today_cost.unwrap_or(0.0),
        "first_event_date": first_event.unwrap_or(None),
    }))
}

// ---------------------------------------------------------------------------
// Budget endpoints
// ---------------------------------------------------------------------------

/// GET /api/budget — Current budget status (limits, spend, % used).
pub async fn budget_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let status = state
        .kernel
        .metering
        .budget_status(&state.kernel.config.budget);
    Json(serde_json::to_value(&status).unwrap_or_default())
}

/// PUT /api/budget — Update global budget limits (in-memory only, not persisted to config.toml).
pub async fn update_budget(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    // SAFETY: Budget config is updated in-place. Since KernelConfig is behind
    // an Arc and we only have &self, we use ptr mutation (same pattern as OFP).
    let config_ptr = &state.kernel.config as *const openfang_types::config::KernelConfig
        as *mut openfang_types::config::KernelConfig;

    // Apply updates
    unsafe {
        if let Some(v) = body["max_hourly_usd"].as_f64() {
            (*config_ptr).budget.max_hourly_usd = v;
        }
        if let Some(v) = body["max_daily_usd"].as_f64() {
            (*config_ptr).budget.max_daily_usd = v;
        }
        if let Some(v) = body["max_monthly_usd"].as_f64() {
            (*config_ptr).budget.max_monthly_usd = v;
        }
        if let Some(v) = body["alert_threshold"].as_f64() {
            (*config_ptr).budget.alert_threshold = v.clamp(0.0, 1.0);
        }
        if let Some(v) = body["default_max_llm_tokens_per_hour"].as_u64() {
            (*config_ptr).budget.default_max_llm_tokens_per_hour = v;
        }
    }

    let status = state
        .kernel
        .metering
        .budget_status(&state.kernel.config.budget);
    Json(serde_json::to_value(&status).unwrap_or_default())
}

/// GET /api/budget/agents/{id} — Per-agent budget/quota status.
pub async fn agent_budget_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            )
        }
    };

    let entry = match state.kernel.registry.get(agent_id) {
        Some(e) => e,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent not found"})),
            )
        }
    };

    let quota = &entry.manifest.resources;
    let usage_store = openfang_memory::usage::UsageStore::new(state.kernel.memory.usage_conn());
    let hourly = usage_store.query_hourly(agent_id).unwrap_or(0.0);
    let daily = usage_store.query_daily(agent_id).unwrap_or(0.0);
    let monthly = usage_store.query_monthly(agent_id).unwrap_or(0.0);

    // Token usage from scheduler
    let token_usage = state.kernel.scheduler.get_usage(agent_id);
    let tokens_used = token_usage.map(|(t, _)| t).unwrap_or(0);

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "agent_id": agent_id.to_string(),
            "agent_name": entry.name,
            "hourly": {
                "spend": hourly,
                "limit": quota.max_cost_per_hour_usd,
                "pct": if quota.max_cost_per_hour_usd > 0.0 { hourly / quota.max_cost_per_hour_usd } else { 0.0 },
            },
            "daily": {
                "spend": daily,
                "limit": quota.max_cost_per_day_usd,
                "pct": if quota.max_cost_per_day_usd > 0.0 { daily / quota.max_cost_per_day_usd } else { 0.0 },
            },
            "monthly": {
                "spend": monthly,
                "limit": quota.max_cost_per_month_usd,
                "pct": if quota.max_cost_per_month_usd > 0.0 { monthly / quota.max_cost_per_month_usd } else { 0.0 },
            },
            "tokens": {
                "used": tokens_used,
                "limit": quota.max_llm_tokens_per_hour,
                "pct": if quota.max_llm_tokens_per_hour > 0 { tokens_used as f64 / quota.max_llm_tokens_per_hour as f64 } else { 0.0 },
            },
        })),
    )
}

/// GET /api/budget/agents — Per-agent cost ranking (top spenders).
pub async fn agent_budget_ranking(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let usage_store = openfang_memory::usage::UsageStore::new(state.kernel.memory.usage_conn());
    let agents: Vec<serde_json::Value> = state
        .kernel
        .registry
        .list()
        .iter()
        .filter_map(|entry| {
            let daily = usage_store.query_daily(entry.id).unwrap_or(0.0);
            if daily > 0.0 {
                Some(serde_json::json!({
                    "agent_id": entry.id.to_string(),
                    "name": entry.name,
                    "daily_cost_usd": daily,
                    "hourly_limit": entry.manifest.resources.max_cost_per_hour_usd,
                    "daily_limit": entry.manifest.resources.max_cost_per_day_usd,
                    "monthly_limit": entry.manifest.resources.max_cost_per_month_usd,
                    "max_llm_tokens_per_hour": entry.manifest.resources.max_llm_tokens_per_hour,
                }))
            } else {
                None
            }
        })
        .collect();

    Json(serde_json::json!({"agents": agents, "total": agents.len()}))
}

/// PUT /api/budget/agents/{id} — Update per-agent budget limits at runtime.
pub async fn update_agent_budget(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            )
        }
    };

    let hourly = body["max_cost_per_hour_usd"].as_f64();
    let daily = body["max_cost_per_day_usd"].as_f64();
    let monthly = body["max_cost_per_month_usd"].as_f64();
    let tokens = body["max_llm_tokens_per_hour"].as_u64();

    if hourly.is_none() && daily.is_none() && monthly.is_none() && tokens.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({"error": "Provide at least one of: max_cost_per_hour_usd, max_cost_per_day_usd, max_cost_per_month_usd, max_llm_tokens_per_hour"}),
            ),
        );
    }

    match state
        .kernel
        .registry
        .update_resources(agent_id, hourly, daily, monthly, tokens)
    {
        Ok(()) => {
            // Persist updated entry
            if let Some(entry) = state.kernel.registry.get(agent_id) {
                let _ = state.kernel.memory.save_agent(&entry);
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "ok", "message": "Agent budget updated"})),
            )
        }
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

// ---------------------------------------------------------------------------
// Session listing endpoints
// ---------------------------------------------------------------------------

/// GET /api/sessions — List all sessions with metadata.
pub async fn list_sessions(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.kernel.memory.list_sessions() {
        Ok(sessions) => Json(serde_json::json!({"sessions": sessions})),
        Err(_) => Json(serde_json::json!({"sessions": []})),
    }
}

/// DELETE /api/sessions/:id — Delete a session.
pub async fn delete_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let session_id = match id.parse::<uuid::Uuid>() {
        Ok(u) => openfang_types::agent::SessionId(u),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid session ID"})),
            );
        }
    };

    let existing_session = state.kernel.memory.get_session(session_id).ok().flatten();
    match state.kernel.memory.delete_session(session_id) {
        Ok(()) => {
            if let Some(session) = existing_session {
                if let Some(entry) = state.kernel.registry.get(session.agent_id) {
                    if entry.session_id == session_id {
                        let _ = state.kernel.create_agent_session(session.agent_id, None);
                    } else {
                        let _ = state
                            .kernel
                            .refresh_agent_runtime_projection(session.agent_id);
                    }
                }
            }

            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "deleted", "session_id": id})),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// PUT /api/sessions/:id/label — Set a session label.
pub async fn set_session_label(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let session_id = match id.parse::<uuid::Uuid>() {
        Ok(u) => openfang_types::agent::SessionId(u),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid session ID"})),
            );
        }
    };

    let label = req.get("label").and_then(|v| v.as_str());

    // Validate label if present
    if let Some(lbl) = label {
        if let Err(e) = openfang_types::agent::SessionLabel::new(lbl) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e.to_string()})),
            );
        }
    }

    let agent_id = state
        .kernel
        .memory
        .get_session(session_id)
        .ok()
        .flatten()
        .map(|session| session.agent_id);

    match state.kernel.memory.set_session_label(session_id, label) {
        Ok(()) => {
            if let Some(agent_id) = agent_id {
                let _ = state.kernel.refresh_agent_runtime_projection(agent_id);
            }

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "updated",
                    "session_id": id,
                    "label": label,
                })),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// GET /api/sessions/by-label/:label — Find session by label (scoped to agent).
pub async fn find_session_by_label(
    State(state): State<Arc<AppState>>,
    Path((agent_id_str, label)): Path<(String, String)>,
) -> impl IntoResponse {
    let agent_id = match agent_id_str.parse::<uuid::Uuid>() {
        Ok(u) => openfang_types::agent::AgentId(u),
        Err(_) => {
            // Try name lookup
            match state.kernel.registry.find_by_name(&agent_id_str) {
                Some(entry) => entry.id,
                None => {
                    return (
                        StatusCode::NOT_FOUND,
                        Json(serde_json::json!({"error": "Agent not found"})),
                    );
                }
            }
        }
    };

    match state.kernel.memory.find_session_by_label(agent_id, &label) {
        Ok(Some(session)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "session_id": session.id.0.to_string(),
                "agent_id": session.agent_id.0.to_string(),
                "label": session.label,
                "message_count": session.messages.len(),
            })),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "No session found with that label"})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

// ---------------------------------------------------------------------------
// Trigger update endpoint
// ---------------------------------------------------------------------------

/// PUT /api/triggers/:id — Update a trigger (enable/disable toggle).
pub async fn update_trigger(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let trigger_id = TriggerId(match id.parse() {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid trigger ID"})),
            );
        }
    });

    if let Some(enabled) = req.get("enabled").and_then(|v| v.as_bool()) {
        if state.kernel.set_trigger_enabled(trigger_id, enabled) {
            (
                StatusCode::OK,
                Json(
                    serde_json::json!({"status": "updated", "trigger_id": id, "enabled": enabled}),
                ),
            )
        } else {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Trigger not found"})),
            )
        }
    } else {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Missing 'enabled' field"})),
        )
    }
}

// ---------------------------------------------------------------------------
// Agent update endpoint
// ---------------------------------------------------------------------------

/// PUT /api/agents/:id — Update an agent (currently: re-set manifest fields).
pub async fn update_agent_legacy(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<LegacyAgentUpdateRequest>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            );
        }
    };

    if state.kernel.registry.get(agent_id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Agent not found"})),
        );
    }

    // Parse the new manifest
    let _manifest: AgentManifest = match toml::from_str(&req.manifest_toml) {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Invalid manifest: {e}")})),
            );
        }
    };

    // Note: Full manifest update requires kill + respawn. For now, acknowledge receipt.
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "acknowledged",
            "agent_id": id,
            "note": "Full manifest update requires agent restart. Use DELETE + POST to apply.",
        })),
    )
}

/// PATCH /api/agents/{id} — Partial update of agent fields (name, description, model, system_prompt).
pub async fn patch_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            );
        }
    };

    if state.kernel.registry.get(agent_id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Agent not found"})),
        );
    }

    // Apply partial updates using dedicated registry methods
    if let Some(name) = body.get("name").and_then(|v| v.as_str()) {
        if let Err(e) = state
            .kernel
            .registry
            .update_name(agent_id, name.to_string())
        {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("{e}")})),
            );
        }
    }
    if let Some(desc) = body.get("description").and_then(|v| v.as_str()) {
        if let Err(e) = state
            .kernel
            .registry
            .update_description(agent_id, desc.to_string())
        {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("{e}")})),
            );
        }
    }
    if let Some(model) = body.get("model").and_then(|v| v.as_str()) {
        let explicit_provider = body.get("provider").and_then(|v| v.as_str());
        if let Err(e) = state
            .kernel
            .set_agent_model(agent_id, model, explicit_provider)
        {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("{e}")})),
            );
        }
    }
    if let Some(system_prompt) = body.get("system_prompt").and_then(|v| v.as_str()) {
        if let Err(e) = state
            .kernel
            .registry
            .update_system_prompt(agent_id, system_prompt.to_string())
        {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("{e}")})),
            );
        }
    }

    // Persist updated entry to SQLite
    if let Some(entry) = state.kernel.registry.get(agent_id) {
        let _ = state.kernel.memory.save_agent(&entry);
        (
            StatusCode::OK,
            Json(
                serde_json::json!({"status": "ok", "agent_id": entry.id.to_string(), "name": entry.name}),
            ),
        )
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Agent vanished during update"})),
        )
    }
}

// ---------------------------------------------------------------------------
// Migration endpoint
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Security dashboard endpoint
// ---------------------------------------------------------------------------

/// GET /api/security — Security feature status for the dashboard.
pub async fn security_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let auth_mode = if state.kernel.config.api_key.is_empty() {
        "localhost_only"
    } else {
        "bearer_token"
    };

    let audit_count = state.kernel.audit_log.len();

    Json(serde_json::json!({
        "core_protections": {
            "path_traversal": true,
            "ssrf_protection": true,
            "capability_system": true,
            "privilege_escalation_prevention": true,
            "subprocess_isolation": true,
            "security_headers": true,
            "wire_hmac_auth": true,
            "request_id_tracking": true
        },
        "configurable": {
            "rate_limiter": {
                "enabled": true,
                "tokens_per_minute": 500,
                "algorithm": "GCRA"
            },
            "websocket_limits": {
                "max_per_ip": 5,
                "idle_timeout_secs": 1800,
                "max_message_size": 65536,
                "max_messages_per_minute": 10
            },
            "wasm_sandbox": {
                "fuel_metering": true,
                "epoch_interruption": true,
                "default_timeout_secs": 30,
                "default_fuel_limit": 1_000_000u64
            },
            "auth": {
                "mode": auth_mode,
                "api_key_set": !state.kernel.config.api_key.is_empty()
            }
        },
        "monitoring": {
            "audit_trail": {
                "enabled": true,
                "algorithm": "SHA-256 Merkle Chain",
                "entry_count": audit_count
            },
            "taint_tracking": {
                "enabled": true,
                "tracked_labels": [
                    "ExternalNetwork",
                    "UserInput",
                    "PII",
                    "Secret",
                    "UntrustedAgent"
                ]
            },
            "manifest_signing": {
                "algorithm": "Ed25519",
                "available": true
            }
        },
        "secret_zeroization": true,
        "total_features": 15
    }))
}

/// GET /api/migrate/detect — Auto-detect OpenClaw installation.
pub async fn migrate_detect() -> impl IntoResponse {
    match openfang_migrate::openclaw::detect_openclaw_home() {
        Some(path) => {
            let scan = openfang_migrate::openclaw::scan_openclaw_workspace(&path);
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "detected": true,
                    "path": path.display().to_string(),
                    "scan": scan,
                })),
            )
        }
        None => (
            StatusCode::OK,
            Json(serde_json::json!({
                "detected": false,
                "path": null,
                "scan": null,
            })),
        ),
    }
}

/// POST /api/migrate/scan — Scan a specific directory for OpenClaw workspace.
pub async fn migrate_scan(Json(req): Json<MigrateScanRequest>) -> impl IntoResponse {
    let path = std::path::PathBuf::from(&req.path);
    if !path.exists() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Directory not found"})),
        );
    }
    let scan = openfang_migrate::openclaw::scan_openclaw_workspace(&path);
    (StatusCode::OK, Json(serde_json::json!(scan)))
}

/// POST /api/migrate — Run migration from another agent framework.
pub async fn run_migrate(Json(req): Json<MigrateRequest>) -> impl IntoResponse {
    let source = match req.source.as_str() {
        "openclaw" => openfang_migrate::MigrateSource::OpenClaw,
        "langchain" => openfang_migrate::MigrateSource::LangChain,
        "autogpt" => openfang_migrate::MigrateSource::AutoGpt,
        other => {
            return (
                StatusCode::BAD_REQUEST,
                Json(
                    serde_json::json!({"error": format!("Unknown source: {other}. Use 'openclaw', 'langchain', or 'autogpt'")}),
                ),
            );
        }
    };

    let options = openfang_migrate::MigrateOptions {
        source,
        source_dir: std::path::PathBuf::from(&req.source_dir),
        target_dir: std::path::PathBuf::from(&req.target_dir),
        dry_run: req.dry_run,
    };

    match openfang_migrate::run_migration(&options) {
        Ok(report) => {
            let imported: Vec<serde_json::Value> = report
                .imported
                .iter()
                .map(|i| {
                    serde_json::json!({
                        "kind": format!("{}", i.kind),
                        "name": i.name,
                        "destination": i.destination,
                    })
                })
                .collect();

            let skipped: Vec<serde_json::Value> = report
                .skipped
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "kind": format!("{}", s.kind),
                        "name": s.name,
                        "reason": s.reason,
                    })
                })
                .collect();

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "completed",
                    "dry_run": req.dry_run,
                    "imported": imported,
                    "imported_count": imported.len(),
                    "skipped": skipped,
                    "skipped_count": skipped.len(),
                    "warnings": report.warnings,
                    "report_markdown": report.to_markdown(),
                })),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Migration failed: {e}")})),
        ),
    }
}

// ── Model Catalog Endpoints ─────────────────────────────────────────

/// GET /api/models — List all models in the catalog.
///
/// Query parameters:
/// - `provider` — filter by provider (e.g. `?provider=anthropic`)
/// - `tier` — filter by tier (e.g. `?tier=smart`)
/// - `available` — only show models from configured providers (`?available=true`)
pub async fn list_models(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let catalog = state
        .kernel
        .model_catalog
        .read()
        .unwrap_or_else(|e| e.into_inner());
    let provider_filter = params.get("provider").map(|s| s.to_lowercase());
    let tier_filter = params.get("tier").map(|s| s.to_lowercase());
    let available_only = params
        .get("available")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    let models: Vec<serde_json::Value> = catalog
        .list_models()
        .iter()
        .filter(|m| {
            if let Some(ref p) = provider_filter {
                if m.provider.to_lowercase() != *p {
                    return false;
                }
            }
            if let Some(ref t) = tier_filter {
                if m.tier.to_string() != *t {
                    return false;
                }
            }
            if available_only {
                let provider = catalog.get_provider(&m.provider);
                if let Some(p) = provider {
                    if p.auth_status == openfang_types::model_catalog::AuthStatus::Missing {
                        return false;
                    }
                }
            }
            true
        })
        .map(|m| {
            // Custom models from unknown providers are assumed available
            let available = catalog
                .get_provider(&m.provider)
                .map(|p| p.auth_status != openfang_types::model_catalog::AuthStatus::Missing)
                .unwrap_or(m.tier == openfang_types::model_catalog::ModelTier::Custom);
            serde_json::json!({
                "id": m.id,
                "display_name": m.display_name,
                "provider": m.provider,
                "tier": m.tier,
                "context_window": m.context_window,
                "max_output_tokens": m.max_output_tokens,
                "input_cost_per_m": m.input_cost_per_m,
                "output_cost_per_m": m.output_cost_per_m,
                "supports_tools": m.supports_tools,
                "supports_vision": m.supports_vision,
                "supports_streaming": m.supports_streaming,
                "available": available,
            })
        })
        .collect();

    let total = catalog.list_models().len();
    let available_count = catalog.available_models().len();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "models": models,
            "total": total,
            "available": available_count,
        })),
    )
}

/// GET /api/models/aliases — List all alias-to-model mappings.
pub async fn list_aliases(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let aliases = state
        .kernel
        .model_catalog
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .list_aliases()
        .clone();
    let entries: Vec<serde_json::Value> = aliases
        .iter()
        .map(|(alias, model_id)| {
            serde_json::json!({
                "alias": alias,
                "model_id": model_id,
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "aliases": entries,
            "total": entries.len(),
        })),
    )
}

/// GET /api/models/{id} — Get a single model by ID or alias.
pub async fn get_model(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let catalog = state
        .kernel
        .model_catalog
        .read()
        .unwrap_or_else(|e| e.into_inner());
    match catalog.find_model(&id) {
        Some(m) => {
            let available = catalog
                .get_provider(&m.provider)
                .map(|p| p.auth_status != openfang_types::model_catalog::AuthStatus::Missing)
                .unwrap_or(m.tier == openfang_types::model_catalog::ModelTier::Custom);
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "id": m.id,
                    "display_name": m.display_name,
                    "provider": m.provider,
                    "tier": m.tier,
                    "context_window": m.context_window,
                    "max_output_tokens": m.max_output_tokens,
                    "input_cost_per_m": m.input_cost_per_m,
                    "output_cost_per_m": m.output_cost_per_m,
                    "supports_tools": m.supports_tools,
                    "supports_vision": m.supports_vision,
                    "supports_streaming": m.supports_streaming,
                    "aliases": m.aliases,
                    "available": available,
                })),
            )
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("Model '{}' not found", id)})),
        ),
    }
}

/// GET /api/providers — List all providers with auth status.
///
/// For local providers (ollama, vllm, lmstudio), also probes reachability and
/// discovers available models via their health endpoints.
///
/// Probes run **concurrently** and results are **cached for 60 seconds** so the
/// endpoint responds instantly on repeated dashboard loads even when local
/// providers are unreachable (fixes #474).
pub async fn list_providers(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let provider_list: Vec<openfang_types::model_catalog::ProviderInfo> = {
        let catalog = state
            .kernel
            .model_catalog
            .read()
            .unwrap_or_else(|e| e.into_inner());
        catalog.list_providers().to_vec()
    };

    // Collect local providers that need probing
    let local_providers: Vec<(usize, String, String)> = provider_list
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.key_required && !p.base_url.is_empty())
        .map(|(i, p)| (i, p.id.clone(), p.base_url.clone()))
        .collect();

    // Fire all probes concurrently (cached results return instantly)
    let cache = &state.provider_probe_cache;
    let probe_futures: Vec<_> = local_providers
        .iter()
        .map(|(_, id, url)| {
            openfang_runtime::provider_health::probe_provider_cached(id, url, cache)
        })
        .collect();
    let probe_results = futures::future::join_all(probe_futures).await;

    // Index probe results by provider list position for O(1) lookup
    let mut probe_map: HashMap<usize, openfang_runtime::provider_health::ProbeResult> =
        HashMap::with_capacity(local_providers.len());
    for ((idx, _, _), result) in local_providers.iter().zip(probe_results.into_iter()) {
        probe_map.insert(*idx, result);
    }

    let mut providers: Vec<serde_json::Value> = Vec::with_capacity(provider_list.len());

    for (i, p) in provider_list.iter().enumerate() {
        let mut entry = serde_json::json!({
            "id": p.id,
            "display_name": p.display_name,
            "auth_status": p.auth_status,
            "model_count": p.model_count,
            "key_required": p.key_required,
            "api_key_env": p.api_key_env,
            "base_url": p.base_url,
        });

        // For local providers, attach the probe result
        if let Some(probe) = probe_map.remove(&i) {
            entry["is_local"] = serde_json::json!(true);
            entry["reachable"] = serde_json::json!(probe.reachable);
            entry["latency_ms"] = serde_json::json!(probe.latency_ms);
            if !probe.discovered_models.is_empty() {
                entry["discovered_models"] = serde_json::json!(probe.discovered_models);
                // Merge discovered models into the catalog so agents can use them
                if let Ok(mut catalog) = state.kernel.model_catalog.write() {
                    catalog.merge_discovered_models(&p.id, &probe.discovered_models);
                }
            }
            if let Some(err) = &probe.error {
                entry["error"] = serde_json::json!(err);
            }
        } else if !p.key_required {
            // Local provider with empty base_url (e.g. claude-code) — skip probing
            entry["is_local"] = serde_json::json!(true);
        }

        providers.push(entry);
    }

    let total = providers.len();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "providers": providers,
            "total": total,
        })),
    )
}

/// POST /api/models/custom — Add a custom model to the catalog.
///
/// Persists to `~/.openfang/custom_models.json` and makes the model immediately
/// available for agent assignment.
pub async fn add_custom_model(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let provider = body
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or("openrouter")
        .to_string();
    let context_window = body
        .get("context_window")
        .and_then(|v| v.as_u64())
        .unwrap_or(128_000);
    let max_output = body
        .get("max_output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(8_192);

    if id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Missing required field: id"})),
        );
    }

    let display = body
        .get("display_name")
        .and_then(|v| v.as_str())
        .unwrap_or(&id)
        .to_string();

    let entry = openfang_types::model_catalog::ModelCatalogEntry {
        id: id.clone(),
        display_name: display,
        provider: provider.clone(),
        tier: openfang_types::model_catalog::ModelTier::Custom,
        context_window,
        max_output_tokens: max_output,
        input_cost_per_m: body
            .get("input_cost_per_m")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        output_cost_per_m: body
            .get("output_cost_per_m")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        supports_tools: body
            .get("supports_tools")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        supports_vision: body
            .get("supports_vision")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        supports_streaming: body
            .get("supports_streaming")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        aliases: vec![],
    };

    let mut catalog = state
        .kernel
        .model_catalog
        .write()
        .unwrap_or_else(|e| e.into_inner());

    if !catalog.add_custom_model(entry) {
        return (
            StatusCode::CONFLICT,
            Json(
                serde_json::json!({"error": format!("Model '{}' already exists for provider '{}'", id, provider)}),
            ),
        );
    }

    // Persist to disk
    let custom_path = state.kernel.config.home_dir.join("custom_models.json");
    if let Err(e) = catalog.save_custom_models(&custom_path) {
        tracing::warn!("Failed to persist custom models: {e}");
    }

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": id,
            "provider": provider,
            "status": "added"
        })),
    )
}

/// DELETE /api/models/custom/{id} — Remove a custom model.
pub async fn remove_custom_model(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(model_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let mut catalog = state
        .kernel
        .model_catalog
        .write()
        .unwrap_or_else(|e| e.into_inner());

    if !catalog.remove_custom_model(&model_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("Custom model '{}' not found", model_id)})),
        );
    }

    let custom_path = state.kernel.config.home_dir.join("custom_models.json");
    if let Err(e) = catalog.save_custom_models(&custom_path) {
        tracing::warn!("Failed to persist custom models: {e}");
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "removed"})),
    )
}

// ── A2A (Agent-to-Agent) Protocol Endpoints ─────────────────────────

/// GET /.well-known/agent.json — A2A Agent Card for the default agent.
pub async fn a2a_agent_card(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let agents = state.kernel.registry.list();
    let base_url = format!("http://{}", state.kernel.config.api_listen);

    if let Some(first) = agents.first() {
        let card = openfang_runtime::a2a::build_agent_card(&first.manifest, &base_url);
        (
            StatusCode::OK,
            Json(serde_json::to_value(&card).unwrap_or_default()),
        )
    } else {
        let card = serde_json::json!({
            "name": "openfang",
            "description": "OpenFang Agent OS — no agents spawned yet",
            "url": format!("{base_url}/a2a"),
            "version": "0.1.0",
            "capabilities": { "streaming": true },
            "skills": [],
            "defaultInputModes": ["text"],
            "defaultOutputModes": ["text"],
        });
        (StatusCode::OK, Json(card))
    }
}

/// GET /a2a/agents — List all A2A agent cards.
pub async fn a2a_list_agents(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let agents = state.kernel.registry.list();
    let base_url = format!("http://{}", state.kernel.config.api_listen);

    let cards: Vec<serde_json::Value> = agents
        .iter()
        .map(|entry| {
            let card = openfang_runtime::a2a::build_agent_card(&entry.manifest, &base_url);
            serde_json::to_value(&card).unwrap_or_default()
        })
        .collect();

    let total = cards.len();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "agents": cards,
            "total": total,
        })),
    )
}

/// POST /a2a/tasks/send — Submit a task to an agent via A2A.
pub async fn a2a_send_task(
    State(state): State<Arc<AppState>>,
    Json(request): Json<serde_json::Value>,
) -> impl IntoResponse {
    // Extract message text from A2A format
    let message_text = request["params"]["message"]["parts"]
        .as_array()
        .and_then(|parts| {
            parts.iter().find_map(|p| {
                if p["type"].as_str() == Some("text") {
                    p["text"].as_str().map(String::from)
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| "No message provided".to_string());

    // Find target agent (use first available or specified)
    let agents = state.kernel.registry.list();
    if agents.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "No agents available"})),
        );
    }

    let agent = &agents[0];
    let task_id = uuid::Uuid::new_v4().to_string();
    let session_id = request["params"]["sessionId"].as_str().map(String::from);

    // Create the task in the store as Working
    let task = openfang_runtime::a2a::A2aTask {
        id: task_id.clone(),
        session_id: session_id.clone(),
        status: openfang_runtime::a2a::A2aTaskStatus::Working.into(),
        messages: vec![openfang_runtime::a2a::A2aMessage {
            role: "user".to_string(),
            parts: vec![openfang_runtime::a2a::A2aPart::Text {
                text: message_text.clone(),
            }],
        }],
        artifacts: vec![],
    };
    state.kernel.a2a_task_store.insert(task);

    // Send message to agent
    match state.kernel.send_message(agent.id, &message_text).await {
        Ok(result) => {
            let response_msg = openfang_runtime::a2a::A2aMessage {
                role: "agent".to_string(),
                parts: vec![openfang_runtime::a2a::A2aPart::Text {
                    text: result.response,
                }],
            };
            state
                .kernel
                .a2a_task_store
                .complete(&task_id, response_msg, vec![]);
            match state.kernel.a2a_task_store.get(&task_id) {
                Some(completed_task) => (
                    StatusCode::OK,
                    Json(serde_json::to_value(&completed_task).unwrap_or_default()),
                ),
                None => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "Task disappeared after completion"})),
                ),
            }
        }
        Err(e) => {
            let error_msg = openfang_runtime::a2a::A2aMessage {
                role: "agent".to_string(),
                parts: vec![openfang_runtime::a2a::A2aPart::Text {
                    text: format!("Error: {e}"),
                }],
            };
            state.kernel.a2a_task_store.fail(&task_id, error_msg);
            match state.kernel.a2a_task_store.get(&task_id) {
                Some(failed_task) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::to_value(&failed_task).unwrap_or_default()),
                ),
                None => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("Agent error: {e}")})),
                ),
            }
        }
    }
}

/// GET /a2a/tasks/{id} — Get task status from the task store.
pub async fn a2a_get_task(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    match state.kernel.a2a_task_store.get(&task_id) {
        Some(task) => (
            StatusCode::OK,
            Json(serde_json::to_value(&task).unwrap_or_default()),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("Task '{}' not found", task_id)})),
        ),
    }
}

/// POST /a2a/tasks/{id}/cancel — Cancel a tracked task.
pub async fn a2a_cancel_task(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    if state.kernel.a2a_task_store.cancel(&task_id) {
        match state.kernel.a2a_task_store.get(&task_id) {
            Some(task) => (
                StatusCode::OK,
                Json(serde_json::to_value(&task).unwrap_or_default()),
            ),
            None => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Task disappeared after cancellation"})),
            ),
        }
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("Task '{}' not found", task_id)})),
        )
    }
}

// ── A2A Management Endpoints (outbound) ─────────────────────────────────

/// GET /api/a2a/agents — List discovered external A2A agents.
pub async fn a2a_list_external_agents(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let agents = state
        .kernel
        .a2a_external_agents
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let items: Vec<serde_json::Value> = agents
        .iter()
        .map(|(_, card)| {
            serde_json::json!({
                "name": card.name,
                "url": card.url,
                "description": card.description,
                "skills": card.skills,
                "version": card.version,
            })
        })
        .collect();
    Json(serde_json::json!({"agents": items, "total": items.len()}))
}

/// POST /api/a2a/discover — Discover a new external A2A agent by URL.
pub async fn a2a_discover_external(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let url = match body["url"].as_str() {
        Some(u) => u.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing 'url' field"})),
            )
        }
    };

    let client = openfang_runtime::a2a::A2aClient::new();
    match client.discover(&url).await {
        Ok(card) => {
            let card_json = serde_json::to_value(&card).unwrap_or_default();
            // Store in kernel's external agents list
            {
                let mut agents = state
                    .kernel
                    .a2a_external_agents
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                // Update or add
                if let Some(existing) = agents.iter_mut().find(|(u, _)| u == &url) {
                    existing.1 = card;
                } else {
                    agents.push((url.clone(), card));
                }
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "url": url,
                    "agent": card_json,
                })),
            )
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": e})),
        ),
    }
}

/// POST /api/a2a/send — Send a task to an external A2A agent.
pub async fn a2a_send_external(
    State(_state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let url = match body["url"].as_str() {
        Some(u) => u.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing 'url' field"})),
            )
        }
    };
    let message = match body["message"].as_str() {
        Some(m) => m.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing 'message' field"})),
            )
        }
    };
    let session_id = body["session_id"].as_str();

    let client = openfang_runtime::a2a::A2aClient::new();
    match client.send_task(&url, &message, session_id).await {
        Ok(task) => (
            StatusCode::OK,
            Json(serde_json::to_value(&task).unwrap_or_default()),
        ),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": e})),
        ),
    }
}

/// GET /api/a2a/tasks/{id}/status — Get task status from an external A2A agent.
pub async fn a2a_external_task_status(
    State(_state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let url = match params.get("url") {
        Some(u) => u.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing 'url' query parameter"})),
            )
        }
    };

    let client = openfang_runtime::a2a::A2aClient::new();
    match client.get_task(&url, &task_id).await {
        Ok(task) => (
            StatusCode::OK,
            Json(serde_json::to_value(&task).unwrap_or_default()),
        ),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": e})),
        ),
    }
}

// ── MCP HTTP Endpoint ───────────────────────────────────────────────────

/// POST /mcp — Handle MCP JSON-RPC requests over HTTP.
///
/// Exposes the same MCP protocol normally served via stdio, allowing
/// external MCP clients to connect over HTTP instead.
pub async fn mcp_http(
    State(state): State<Arc<AppState>>,
    Json(request): Json<serde_json::Value>,
) -> impl IntoResponse {
    // Gather all available tools (builtin + skills + MCP)
    let mut tools = builtin_tool_definitions();
    {
        let registry = state
            .kernel
            .skill_registry
            .read()
            .unwrap_or_else(|e| e.into_inner());
        for skill_tool in registry.all_tool_definitions() {
            tools.push(openfang_types::tool::ToolDefinition {
                name: skill_tool.name.clone(),
                description: skill_tool.description.clone(),
                input_schema: skill_tool.input_schema.clone(),
            });
        }
    }
    if let Ok(mcp_tools) = state.kernel.mcp_tools.lock() {
        tools.extend(mcp_tools.iter().cloned());
    }

    // Check if this is a tools/call that needs real execution
    let method = request["method"].as_str().unwrap_or("");
    if method == "tools/call" {
        let tool_name = request["params"]["name"].as_str().unwrap_or("");
        let arguments = request["params"]
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::json!({}));

        // Verify the tool exists
        if !tools.iter().any(|t| t.name == tool_name) {
            return Json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": request.get("id").cloned(),
                "error": {"code": -32602, "message": format!("Unknown tool: {tool_name}")}
            }));
        }

        // Snapshot skill registry before async call (RwLockReadGuard is !Send)
        let skill_snapshot = state
            .kernel
            .skill_registry
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .snapshot();

        // Execute the tool via the kernel's tool runner
        let kernel_handle: Arc<dyn openfang_runtime::kernel_handle::KernelHandle> =
            state.kernel.clone() as Arc<dyn openfang_runtime::kernel_handle::KernelHandle>;
        let result = openfang_runtime::tool_runner::execute_tool(
            "mcp-http",
            tool_name,
            &arguments,
            Some(&kernel_handle),
            None,
            None,
            Some(&skill_snapshot),
            Some(&state.kernel.mcp_connections),
            Some(&state.kernel.web_ctx),
            Some(&state.kernel.browser_ctx),
            None,
            None,
            Some(&state.kernel.media_engine),
            None, // exec_policy
            if state.kernel.config.tts.enabled {
                Some(&state.kernel.tts_engine)
            } else {
                None
            },
            if state.kernel.config.docker.enabled {
                Some(&state.kernel.config.docker)
            } else {
                None
            },
            Some(&*state.kernel.process_manager),
        )
        .await;

        return Json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": request.get("id").cloned(),
            "result": {
                "content": [{"type": "text", "text": result.content}],
                "isError": result.is_error,
            }
        }));
    }

    // For non-tools/call methods (initialize, tools/list, etc.), delegate to the handler
    let response = openfang_runtime::mcp_server::handle_mcp_request(&request, &tools).await;
    Json(response)
}

// ── Multi-Session Endpoints ─────────────────────────────────────────────

/// GET /api/agents/{id}/sessions — List all sessions for an agent.
pub async fn list_agent_sessions(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            )
        }
    };
    match state.kernel.list_agent_sessions(agent_id) {
        Ok(sessions) => (
            StatusCode::OK,
            Json(serde_json::json!({"sessions": sessions})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

/// POST /api/agents/{id}/sessions — Create a new session for an agent.
pub async fn create_agent_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            )
        }
    };
    let label = req.get("label").and_then(|v| v.as_str());
    match state.kernel.create_agent_session(agent_id, label) {
        Ok(session) => (StatusCode::OK, Json(session)),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

/// POST /api/agents/{id}/sessions/{session_id}/switch — Switch to an existing session.
pub async fn switch_agent_session(
    State(state): State<Arc<AppState>>,
    Path((id, session_id_str)): Path<(String, String)>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            )
        }
    };
    let session_id = match session_id_str.parse::<uuid::Uuid>() {
        Ok(uuid) => openfang_types::agent::SessionId(uuid),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid session ID"})),
            )
        }
    };
    match state.kernel.switch_agent_session(agent_id, session_id) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "ok", "message": "Session switched"})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

// ── Extended Chat Command API Endpoints ─────────────────────────────────

/// POST /api/agents/{id}/session/reset — Reset an agent's session.
pub async fn reset_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            )
        }
    };
    match state.kernel.reset_session(agent_id) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "ok", "message": "Session reset"})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

/// DELETE /api/agents/{id}/history — Clear ALL conversation history for an agent.
pub async fn clear_agent_history(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            )
        }
    };
    if state.kernel.registry.get(agent_id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Agent not found"})),
        );
    }
    match state.kernel.clear_agent_history(agent_id) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "ok", "message": "All history cleared"})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

/// POST /api/agents/{id}/session/compact — Trigger LLM session compaction.
pub async fn compact_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            )
        }
    };
    match state.kernel.compact_agent_session(agent_id).await {
        Ok(msg) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "ok", "message": msg})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

/// POST /api/agents/{id}/stop — Cancel an agent's current LLM run.
pub async fn stop_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            )
        }
    };
    match state.kernel.stop_agent_run(agent_id) {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "ok", "message": "Run cancelled"})),
        ),
        Ok(false) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "ok", "message": "No active run"})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

/// PUT /api/agents/{id}/model — Switch an agent's model.
pub async fn set_model(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            )
        }
    };
    let model = match body["model"].as_str() {
        Some(m) if !m.is_empty() => m,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing 'model' field"})),
            )
        }
    };
    let explicit_provider = body["provider"].as_str();
    match state
        .kernel
        .set_agent_model(agent_id, model, explicit_provider)
    {
        Ok(()) => {
            // Return the resolved model+provider so frontend stays in sync.
            // The model name may have been normalized (provider prefix stripped),
            // so we read it back from the registry instead of echoing the raw input.
            let (resolved_model, resolved_provider) = state
                .kernel
                .registry
                .get(agent_id)
                .map(|e| {
                    (
                        e.manifest.model.model.clone(),
                        e.manifest.model.provider.clone(),
                    )
                })
                .unwrap_or_else(|| (model.to_string(), String::new()));
            (
                StatusCode::OK,
                Json(
                    serde_json::json!({"status": "ok", "model": resolved_model, "provider": resolved_provider}),
                ),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

/// GET /api/agents/{id}/tools — Get an agent's tool allowlist/blocklist.
pub async fn get_agent_tools(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            )
        }
    };
    let entry = match state.kernel.registry.get(agent_id) {
        Some(e) => e,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent not found"})),
            )
        }
    };
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "tool_allowlist": entry.manifest.tool_allowlist,
            "tool_blocklist": entry.manifest.tool_blocklist,
        })),
    )
}

/// PUT /api/agents/{id}/tools — Update an agent's tool allowlist/blocklist.
pub async fn set_agent_tools(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            )
        }
    };
    let allowlist = body
        .get("tool_allowlist")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        });
    let blocklist = body
        .get("tool_blocklist")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        });

    if allowlist.is_none() && blocklist.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Provide 'tool_allowlist' and/or 'tool_blocklist'"})),
        );
    }

    match state
        .kernel
        .set_agent_tool_filters(agent_id, allowlist, blocklist)
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"status": "ok"}))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

// ── Per-Agent Skill & MCP Endpoints ────────────────────────────────────

/// GET /api/agents/{id}/skills — Get an agent's skill assignment info.
pub async fn get_agent_skills(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            )
        }
    };
    let entry = match state.kernel.registry.get(agent_id) {
        Some(e) => e,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent not found"})),
            )
        }
    };
    let available = state
        .kernel
        .skill_registry
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .skill_names();
    let mode = if entry.manifest.skills.is_empty() {
        "all"
    } else {
        "allowlist"
    };
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "assigned": entry.manifest.skills,
            "available": available,
            "mode": mode,
        })),
    )
}

/// PUT /api/agents/{id}/skills — Update an agent's skill allowlist.
pub async fn set_agent_skills(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            )
        }
    };
    let skills: Vec<String> = body["skills"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    match state.kernel.set_agent_skills(agent_id, skills.clone()) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "ok", "skills": skills})),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

/// GET /api/agents/{id}/mcp_servers — Get an agent's MCP server assignment info.
pub async fn get_agent_mcp_servers(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            )
        }
    };
    let entry = match state.kernel.registry.get(agent_id) {
        Some(e) => e,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent not found"})),
            )
        }
    };
    // Collect known MCP server names from connected tools
    let mut available: Vec<String> = Vec::new();
    if let Ok(mcp_tools) = state.kernel.mcp_tools.lock() {
        let mut seen = std::collections::HashSet::new();
        for tool in mcp_tools.iter() {
            if let Some(server) = openfang_runtime::mcp::extract_mcp_server(&tool.name) {
                if seen.insert(server.to_string()) {
                    available.push(server.to_string());
                }
            }
        }
    }
    let mode = if entry.manifest.mcp_servers.is_empty() {
        "all"
    } else {
        "allowlist"
    };
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "assigned": entry.manifest.mcp_servers,
            "available": available,
            "mode": mode,
        })),
    )
}

/// PUT /api/agents/{id}/mcp_servers — Update an agent's MCP server allowlist.
pub async fn set_agent_mcp_servers(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            )
        }
    };
    let servers: Vec<String> = body["mcp_servers"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    match state
        .kernel
        .set_agent_mcp_servers(agent_id, servers.clone())
    {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "ok", "mcp_servers": servers})),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

// ── Provider Key Management Endpoints ──────────────────────────────────

/// POST /api/providers/{name}/key — Save an API key for a provider.
///
/// SECURITY: Writes to `~/.openfang/secrets.env`, sets env var in process,
/// and refreshes auth detection. Key is zeroized after use.
pub async fn set_provider_key(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let key = match body["key"].as_str() {
        Some(k) if !k.trim().is_empty() => k.trim().to_string(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing or empty 'key' field"})),
            );
        }
    };

    // Look up env var from catalog; for unknown/custom providers derive one.
    let env_var = {
        let catalog = state
            .kernel
            .model_catalog
            .read()
            .unwrap_or_else(|e| e.into_inner());
        catalog
            .get_provider(&name)
            .map(|p| p.api_key_env.clone())
            .unwrap_or_else(|| {
                // Custom provider — derive env var: MY_PROVIDER → MY_PROVIDER_API_KEY
                format!("{}_API_KEY", name.to_uppercase().replace('-', "_"))
            })
    };

    // Store in vault (best-effort — no-op if vault not initialized)
    state.kernel.store_credential(&env_var, &key);

    // Write to secrets.env file (dual-write for backward compat / vault corruption recovery)
    let secrets_path = state.kernel.config.home_dir.join("secrets.env");
    if let Err(e) = write_secret_env(&secrets_path, &env_var, &key) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to write secrets.env: {e}")})),
        );
    }

    // Set env var in current process so detect_auth picks it up
    std::env::set_var(&env_var, &key);

    // Refresh auth detection
    state
        .kernel
        .model_catalog
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .detect_auth();

    // Auto-switch default provider if current default has no working key.
    // This fixes the common case where a user adds e.g. a Gemini key via dashboard
    // but their agent still tries to use the previous provider (which has no key).
    //
    // Read the effective default from the hot-reload override (if set) rather than
    // the stale boot-time config — a previous set_provider_key call may have already
    // switched the default.
    let (current_provider, current_key_env) = {
        let guard = state
            .kernel
            .default_model_override
            .read()
            .unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(dm) => (dm.provider.clone(), dm.api_key_env.clone()),
            None => (
                state.kernel.config.default_model.provider.clone(),
                state.kernel.config.default_model.api_key_env.clone(),
            ),
        }
    };
    let current_has_key = if current_key_env.is_empty() {
        false
    } else {
        std::env::var(&current_key_env)
            .ok()
            .filter(|v| !v.is_empty())
            .is_some()
    };
    let switched = if !current_has_key && current_provider != name {
        // Find a default model for the newly-keyed provider
        let default_model = {
            let catalog = state
                .kernel
                .model_catalog
                .read()
                .unwrap_or_else(|e| e.into_inner());
            catalog.default_model_for_provider(&name)
        };
        if let Some(model_id) = default_model {
            // Update config.toml to persist the switch
            let config_path = state.kernel.config.home_dir.join("config.toml");
            let update_toml = format!(
                "\n[default_model]\nprovider = \"{}\"\nmodel = \"{}\"\napi_key_env = \"{}\"\n",
                name, model_id, env_var
            );
            backup_config(&config_path);
            if let Ok(existing) = std::fs::read_to_string(&config_path) {
                let cleaned = remove_toml_section(&existing, "default_model");
                let _ =
                    std::fs::write(&config_path, format!("{}\n{}", cleaned.trim(), update_toml));
            } else {
                let _ = std::fs::write(&config_path, update_toml);
            }

            // Hot-update the in-memory default model override so resolve_driver()
            // immediately creates drivers for the new provider — no restart needed.
            {
                let new_dm = openfang_types::config::DefaultModelConfig {
                    provider: name.clone(),
                    model: model_id,
                    api_key_env: env_var.clone(),
                    base_url: None,
                };
                let mut guard = state
                    .kernel
                    .default_model_override
                    .write()
                    .unwrap_or_else(|e| e.into_inner());
                *guard = Some(new_dm);
            }
            true
        } else {
            false
        }
    } else if current_provider == name {
        // User is saving a key for the CURRENT default provider. The env var is
        // already set (set_var above), but we must ensure default_model_override
        // has the correct api_key_env so resolve_driver reads the right variable.
        let needs_update = {
            let guard = state
                .kernel
                .default_model_override
                .read()
                .unwrap_or_else(|e| e.into_inner());
            match guard.as_ref() {
                Some(dm) => dm.api_key_env != env_var,
                None => state.kernel.config.default_model.api_key_env != env_var,
            }
        };
        if needs_update {
            let mut guard = state
                .kernel
                .default_model_override
                .write()
                .unwrap_or_else(|e| e.into_inner());
            let base = guard
                .clone()
                .unwrap_or_else(|| state.kernel.config.default_model.clone());
            *guard = Some(openfang_types::config::DefaultModelConfig {
                api_key_env: env_var.clone(),
                ..base
            });
        }
        false
    } else {
        false
    };

    let mut resp = serde_json::json!({"status": "saved", "provider": name});
    if switched {
        resp["switched_default"] = serde_json::json!(true);
        resp["message"] = serde_json::json!(format!(
            "API key saved and default provider switched to '{}'.",
            name
        ));
    }

    (StatusCode::OK, Json(resp))
}

/// DELETE /api/providers/{name}/key — Remove an API key for a provider.
pub async fn delete_provider_key(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let env_var = {
        let catalog = state
            .kernel
            .model_catalog
            .read()
            .unwrap_or_else(|e| e.into_inner());
        catalog
            .get_provider(&name)
            .map(|p| p.api_key_env.clone())
            .unwrap_or_else(|| {
                // Custom/unknown provider — derive env var from convention
                format!("{}_API_KEY", name.to_uppercase().replace('-', "_"))
            })
    };

    if env_var.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Provider does not require an API key"})),
        );
    }

    // Remove from vault (best-effort)
    state.kernel.remove_credential(&env_var);

    // Remove from secrets.env
    let secrets_path = state.kernel.config.home_dir.join("secrets.env");
    if let Err(e) = remove_secret_env(&secrets_path, &env_var) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to update secrets.env: {e}")})),
        );
    }

    // Remove from process environment
    std::env::remove_var(&env_var);

    // Refresh auth detection
    state
        .kernel
        .model_catalog
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .detect_auth();

    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "removed", "provider": name})),
    )
}

/// POST /api/providers/{name}/test — Test a provider's connectivity.
pub async fn test_provider(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let (env_var, base_url, key_required, default_model) = {
        let catalog = state
            .kernel
            .model_catalog
            .read()
            .unwrap_or_else(|e| e.into_inner());
        match catalog.get_provider(&name) {
            Some(p) => {
                // Find a default model for this provider to use in the test request
                let model_id = catalog
                    .default_model_for_provider(&name)
                    .unwrap_or_default();
                (
                    p.api_key_env.clone(),
                    p.base_url.clone(),
                    p.key_required,
                    model_id,
                )
            }
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": format!("Unknown provider '{}'", name)})),
                );
            }
        }
    };

    let api_key = std::env::var(&env_var).ok();
    // Only require API key for providers that need one (skip local providers like ollama/vllm/lmstudio)
    if key_required && api_key.is_none() && !env_var.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Provider API key not configured"})),
        );
    }

    // Attempt a lightweight connectivity test
    let start = std::time::Instant::now();
    let driver_config = openfang_runtime::llm_driver::DriverConfig {
        provider: name.clone(),
        api_key,
        base_url: if base_url.is_empty() {
            None
        } else {
            Some(base_url)
        },
        skip_permissions: true,
    };

    match openfang_runtime::drivers::create_driver(&driver_config) {
        Ok(driver) => {
            // Send a minimal completion request to test connectivity
            let test_req = openfang_runtime::llm_driver::CompletionRequest {
                model: default_model.clone(),
                messages: vec![openfang_types::message::Message::user("Hi")],
                tools: vec![],
                max_tokens: 1,
                temperature: 0.0,
                system: None,
                thinking: None,
                session: None,
            };
            match driver.complete(test_req).await {
                Ok(_) => {
                    let latency_ms = start.elapsed().as_millis();
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": "ok",
                            "provider": name,
                            "latency_ms": latency_ms,
                        })),
                    )
                }
                Err(e) => (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "status": "error",
                        "provider": name,
                        "error": format!("{e}"),
                    })),
                ),
            }
        }
        Err(e) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "error",
                "provider": name,
                "error": format!("Failed to create driver: {e}"),
            })),
        ),
    }
}

/// PUT /api/providers/{name}/url — Set a custom base URL for a provider.
pub async fn set_provider_url(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    // Accept any provider name — custom providers are supported via OpenAI-compatible format.
    let base_url = match body["base_url"].as_str() {
        Some(u) if !u.trim().is_empty() => u.trim().to_string(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing or empty 'base_url' field"})),
            );
        }
    };

    // Validate URL scheme
    if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "base_url must start with http:// or https://"})),
        );
    }

    // Update catalog in memory
    {
        let mut catalog = state
            .kernel
            .model_catalog
            .write()
            .unwrap_or_else(|e| e.into_inner());
        catalog.set_provider_url(&name, &base_url);
    }

    // Persist to config.toml [provider_urls] section
    let config_path = state.kernel.config.home_dir.join("config.toml");
    if let Err(e) = upsert_provider_url(&config_path, &name, &base_url) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to save config: {e}")})),
        );
    }

    // Probe reachability at the new URL
    let probe = openfang_runtime::provider_health::probe_provider(&name, &base_url).await;

    // Merge discovered models into catalog
    if !probe.discovered_models.is_empty() {
        if let Ok(mut catalog) = state.kernel.model_catalog.write() {
            catalog.merge_discovered_models(&name, &probe.discovered_models);
        }
    }

    let mut resp = serde_json::json!({
        "status": "saved",
        "provider": name,
        "base_url": base_url,
        "reachable": probe.reachable,
        "latency_ms": probe.latency_ms,
    });
    if !probe.discovered_models.is_empty() {
        resp["discovered_models"] = serde_json::json!(probe.discovered_models);
    }

    (StatusCode::OK, Json(resp))
}

/// Upsert a provider URL in the `[provider_urls]` section of config.toml.
fn upsert_provider_url(
    config_path: &std::path::Path,
    provider: &str,
    url: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = if config_path.exists() {
        std::fs::read_to_string(config_path)?
    } else {
        String::new()
    };

    let mut doc: toml::Value = if content.trim().is_empty() {
        toml::Value::Table(toml::map::Map::new())
    } else {
        toml::from_str(&content)?
    };

    let root = doc.as_table_mut().ok_or("Config is not a TOML table")?;

    if !root.contains_key("provider_urls") {
        root.insert(
            "provider_urls".to_string(),
            toml::Value::Table(toml::map::Map::new()),
        );
    }
    let urls_table = root
        .get_mut("provider_urls")
        .and_then(|v| v.as_table_mut())
        .ok_or("provider_urls is not a table")?;

    urls_table.insert(provider.to_string(), toml::Value::String(url.to_string()));

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(config_path, toml::to_string_pretty(&doc)?)?;
    Ok(())
}

/// POST /api/skills/create — Create a local prompt-only skill.
pub async fn create_skill(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let name = match body["name"].as_str() {
        Some(n) if !n.trim().is_empty() => n.trim().to_string(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing or empty 'name' field"})),
            );
        }
    };

    // Validate name (alphanumeric + hyphens only)
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({"error": "Skill name must contain only letters, numbers, hyphens, and underscores"}),
            ),
        );
    }

    let description = body["description"].as_str().unwrap_or("").to_string();
    let runtime = body["runtime"].as_str().unwrap_or("prompt_only");
    let prompt_context = body["prompt_context"].as_str().unwrap_or("").to_string();

    // Only allow prompt_only skills from the web UI for safety
    if runtime != "prompt_only" {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({"error": "Only prompt_only skills can be created from the web UI"}),
            ),
        );
    }

    // Write skill.toml to ~/.openfang/skills/{name}/
    let skill_dir = state.kernel.config.home_dir.join("skills").join(&name);
    if skill_dir.exists() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": format!("Skill '{}' already exists", name)})),
        );
    }

    if let Err(e) = std::fs::create_dir_all(&skill_dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to create skill directory: {e}")})),
        );
    }

    let toml_content = format!(
        "[skill]\nname = \"{}\"\ndescription = \"{}\"\nruntime = \"prompt_only\"\n\n[prompt]\ncontext = \"\"\"\n{}\n\"\"\"\n",
        name,
        description.replace('"', "\\\""),
        prompt_context
    );

    let toml_path = skill_dir.join("skill.toml");
    if let Err(e) = std::fs::write(&toml_path, &toml_content) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to write skill.toml: {e}")})),
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "created",
            "name": name,
            "note": "Restart the daemon to load the new skill, or it will be available on next boot."
        })),
    )
}

// ── Helper functions for secrets.env management ────────────────────────

/// Write or update a key in the secrets.env file.
/// File format: one `KEY=value` per line. Existing keys are overwritten.
fn write_secret_env(path: &std::path::Path, key: &str, value: &str) -> Result<(), std::io::Error> {
    let mut lines: Vec<String> = if path.exists() {
        std::fs::read_to_string(path)?
            .lines()
            .map(|l| l.to_string())
            .collect()
    } else {
        Vec::new()
    };

    // Remove existing line for this key
    lines.retain(|l| !l.starts_with(&format!("{key}=")));

    // Add new line
    lines.push(format!("{key}={value}"));

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(path, lines.join("\n") + "\n")?;

    // SECURITY: Restrict file permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }

    Ok(())
}

/// Remove a key from the secrets.env file.
fn remove_secret_env(path: &std::path::Path, key: &str) -> Result<(), std::io::Error> {
    if !path.exists() {
        return Ok(());
    }

    let lines: Vec<String> = std::fs::read_to_string(path)?
        .lines()
        .filter(|l| !l.starts_with(&format!("{key}=")))
        .map(|l| l.to_string())
        .collect();

    std::fs::write(path, lines.join("\n") + "\n")?;

    Ok(())
}

// ── Config.toml channel management helpers ──────────────────────────

/// Upsert a `[channels.<name>]` section in config.toml with the given non-secret fields.
fn upsert_channel_config(
    config_path: &std::path::Path,
    channel_name: &str,
    fields: &HashMap<String, (String, FieldType)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = if config_path.exists() {
        std::fs::read_to_string(config_path)?
    } else {
        String::new()
    };

    let mut doc: toml::Value = if content.trim().is_empty() {
        toml::Value::Table(toml::map::Map::new())
    } else {
        toml::from_str(&content)?
    };

    let root = doc.as_table_mut().ok_or("Config is not a TOML table")?;

    // Ensure [channels] table exists
    if !root.contains_key("channels") {
        root.insert(
            "channels".to_string(),
            toml::Value::Table(toml::map::Map::new()),
        );
    }
    let channels_table = root
        .get_mut("channels")
        .and_then(|v| v.as_table_mut())
        .ok_or("channels is not a table")?;

    // Build channel sub-table with correct TOML types
    let mut ch_table = toml::map::Map::new();
    for (k, (v, ft)) in fields {
        let toml_val = match ft {
            FieldType::Number => {
                if let Ok(n) = v.parse::<i64>() {
                    toml::Value::Integer(n)
                } else {
                    toml::Value::String(v.clone())
                }
            }
            FieldType::List => {
                // Always store list items as strings so that numeric IDs
                // (e.g. Discord guild snowflakes, Telegram user IDs) are
                // deserialized correctly into Vec<String> config fields.
                let items: Vec<toml::Value> = v
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| toml::Value::String(s.to_string()))
                    .collect();
                toml::Value::Array(items)
            }
            _ => toml::Value::String(v.clone()),
        };
        ch_table.insert(k.clone(), toml_val);
    }
    channels_table.insert(channel_name.to_string(), toml::Value::Table(ch_table));

    // Ensure parent directory exists
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(config_path, toml::to_string_pretty(&doc)?)?;
    Ok(())
}

/// Remove a `[channels.<name>]` section from config.toml.
fn remove_channel_config(
    config_path: &std::path::Path,
    channel_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if !config_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(config_path)?;
    if content.trim().is_empty() {
        return Ok(());
    }

    let mut doc: toml::Value = toml::from_str(&content)?;

    if let Some(channels) = doc
        .as_table_mut()
        .and_then(|r| r.get_mut("channels"))
        .and_then(|c| c.as_table_mut())
    {
        channels.remove(channel_name);
    }

    std::fs::write(config_path, toml::to_string_pretty(&doc)?)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Integration management endpoints
// ---------------------------------------------------------------------------

/// GET /api/integrations — List installed integrations with status.
pub async fn list_integrations(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let registry = state
        .kernel
        .extension_registry
        .read()
        .unwrap_or_else(|e| e.into_inner());
    let health = &state.kernel.extension_health;

    let mut entries = Vec::new();
    for info in registry.list_all_info() {
        let h = health.get_health(&info.template.id);
        let status = match &info.installed {
            Some(inst) if !inst.enabled => "disabled",
            Some(_) => match h.as_ref().map(|h| &h.status) {
                Some(openfang_extensions::IntegrationStatus::Ready) => "ready",
                Some(openfang_extensions::IntegrationStatus::Error(_)) => "error",
                _ => "installed",
            },
            None => continue, // Only show installed
        };
        entries.push(serde_json::json!({
            "id": info.template.id,
            "name": info.template.name,
            "icon": info.template.icon,
            "category": info.template.category.to_string(),
            "status": status,
            "tool_count": h.as_ref().map(|h| h.tool_count).unwrap_or(0),
            "installed_at": info.installed.as_ref().map(|i| i.installed_at.to_rfc3339()),
        }));
    }

    Json(serde_json::json!({
        "installed": entries,
        "count": entries.len(),
    }))
}

/// GET /api/integrations/available — List all available templates.
pub async fn list_available_integrations(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let registry = state
        .kernel
        .extension_registry
        .read()
        .unwrap_or_else(|e| e.into_inner());
    let templates: Vec<serde_json::Value> = registry
        .list_templates()
        .iter()
        .map(|t| {
            let installed = registry.is_installed(&t.id);
            serde_json::json!({
                "id": t.id,
                "name": t.name,
                "description": t.description,
                "icon": t.icon,
                "category": t.category.to_string(),
                "installed": installed,
                "tags": t.tags,
                "required_env": t.required_env.iter().map(|e| serde_json::json!({
                    "name": e.name,
                    "label": e.label,
                    "help": e.help,
                    "is_secret": e.is_secret,
                    "get_url": e.get_url,
                })).collect::<Vec<_>>(),
                "has_oauth": t.oauth.is_some(),
                "setup_instructions": t.setup_instructions,
            })
        })
        .collect();

    Json(serde_json::json!({
        "integrations": templates,
        "count": templates.len(),
    }))
}

/// POST /api/integrations/add — Install an integration.
pub async fn add_integration(
    State(state): State<Arc<AppState>>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let id = match req.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing 'id' field"})),
            );
        }
    };

    // Scope the write lock so it's dropped before any .await
    let install_err = {
        let mut registry = state
            .kernel
            .extension_registry
            .write()
            .unwrap_or_else(|e| e.into_inner());

        if registry.is_installed(&id) {
            Some((
                StatusCode::CONFLICT,
                format!("Integration '{}' already installed", id),
            ))
        } else if registry.get_template(&id).is_none() {
            Some((
                StatusCode::NOT_FOUND,
                format!("Unknown integration: '{}'", id),
            ))
        } else {
            let entry = openfang_extensions::InstalledIntegration {
                id: id.clone(),
                installed_at: chrono::Utc::now(),
                enabled: true,
                oauth_provider: None,
                config: std::collections::HashMap::new(),
            };
            match registry.install(entry) {
                Ok(_) => None,
                Err(e) => Some((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
            }
        }
    }; // write lock dropped here

    if let Some((status, error)) = install_err {
        return (status, Json(serde_json::json!({"error": error})));
    }

    state.kernel.extension_health.register(&id);

    // Hot-connect the new MCP server
    let connected = state.kernel.reload_extension_mcps().await.unwrap_or(0);

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": id,
            "status": "installed",
            "connected": connected > 0,
            "message": format!("Integration '{}' installed", id),
        })),
    )
}

/// DELETE /api/integrations/:id — Remove an integration.
pub async fn remove_integration(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Scope the write lock
    let uninstall_err = {
        let mut registry = state
            .kernel
            .extension_registry
            .write()
            .unwrap_or_else(|e| e.into_inner());
        registry.uninstall(&id).err()
    };

    if let Some(e) = uninstall_err {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e.to_string()})),
        );
    }

    state.kernel.extension_health.unregister(&id);

    // Hot-disconnect the removed MCP server
    let _ = state.kernel.reload_extension_mcps().await;

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "id": id,
            "status": "removed",
        })),
    )
}

/// POST /api/integrations/:id/reconnect — Reconnect an MCP server.
pub async fn reconnect_integration(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let is_installed = {
        let registry = state
            .kernel
            .extension_registry
            .read()
            .unwrap_or_else(|e| e.into_inner());
        registry.is_installed(&id)
    };

    if !is_installed {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("Integration '{}' not installed", id)})),
        );
    }

    match state.kernel.reconnect_extension_mcp(&id).await {
        Ok(tool_count) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "id": id,
                "status": "connected",
                "tool_count": tool_count,
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "id": id,
                "status": "error",
                "error": e,
            })),
        ),
    }
}

/// GET /api/integrations/health — Health status for all integrations.
pub async fn integrations_health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let health_entries = state.kernel.extension_health.all_health();
    let entries: Vec<serde_json::Value> = health_entries
        .iter()
        .map(|h| {
            serde_json::json!({
                "id": h.id,
                "status": h.status.to_string(),
                "tool_count": h.tool_count,
                "last_ok": h.last_ok.map(|t| t.to_rfc3339()),
                "last_error": h.last_error,
                "consecutive_failures": h.consecutive_failures,
                "reconnecting": h.reconnecting,
                "reconnect_attempts": h.reconnect_attempts,
                "connected_since": h.connected_since.map(|t| t.to_rfc3339()),
            })
        })
        .collect();

    Json(serde_json::json!({
        "health": entries,
        "count": entries.len(),
    }))
}

/// POST /api/integrations/reload — Hot-reload integration configs and reconnect MCP.
pub async fn reload_integrations(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.kernel.reload_extension_mcps().await {
        Ok(connected) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "reloaded",
                "new_connections": connected,
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        ),
    }
}

// ---------------------------------------------------------------------------
// Schedule v1 routes
// ---------------------------------------------------------------------------

fn schedule_error_response(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
    details: Option<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "error": {
                "code": code,
                "message": message.into(),
                "details": details.unwrap_or_else(|| serde_json::json!([])),
            }
        })),
    )
}

fn schedule_json_rejection(rejection: JsonRejection) -> (StatusCode, Json<serde_json::Value>) {
    match rejection {
        JsonRejection::MissingJsonContentType(_) => schedule_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Missing `Content-Type: application/json` header",
            None,
        ),
        JsonRejection::JsonDataError(error) => schedule_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("Invalid JSON body: {error}"),
            None,
        ),
        JsonRejection::JsonSyntaxError(error) => schedule_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("Invalid JSON body: {error}"),
            None,
        ),
        JsonRejection::BytesRejection(error) => schedule_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("Failed to read request body: {error}"),
            None,
        ),
        rejection => schedule_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("Invalid request body: {rejection}"),
            None,
        ),
    }
}

fn schedule_definition_id_is_safe(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn ensure_safe_schedule_definition_id(
    id: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if schedule_definition_id_is_safe(id) {
        return Ok(());
    }

    Err(schedule_error_response(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        "Schedule IDs may only contain ASCII letters, digits, `.`, `_`, or `-`",
        Some(serde_json::json!([{
            "path": "id",
            "value": id,
        }])),
    ))
}

fn schedule_not_found_response() -> (StatusCode, Json<serde_json::Value>) {
    schedule_error_response(
        StatusCode::NOT_FOUND,
        "not_found",
        "Schedule definition not found",
        None,
    )
}

fn schedule_validation_issue(
    severity: &str,
    code: &str,
    path: &str,
    message: impl Into<String>,
) -> ScheduleValidationIssue {
    ScheduleValidationIssue {
        severity: severity.to_string(),
        code: code.to_string(),
        path: path.to_string(),
        message: message.into(),
    }
}

fn schedule_validation_error_response(
    issues: &[ScheduleValidationIssue],
) -> (StatusCode, Json<serde_json::Value>) {
    schedule_error_response(
        StatusCode::BAD_REQUEST,
        "validation_error",
        "schedule definition is invalid",
        Some(serde_json::to_value(issues).unwrap_or_else(|_| serde_json::json!([]))),
    )
}

fn schedule_definition_is_valid(issues: &[ScheduleValidationIssue], strict: bool) -> bool {
    if strict {
        issues.is_empty()
    } else {
        !issues.iter().any(|issue| issue.severity == "error")
    }
}

fn generate_schedule_definition_id() -> String {
    format!("sched_{}", uuid::Uuid::new_v4().simple())
}

fn schedule_kind_name(schedule: &CronSchedule) -> &'static str {
    match schedule {
        CronSchedule::At { .. } => "at",
        CronSchedule::Every { .. } => "every",
        CronSchedule::Cron { .. } => "cron",
    }
}

fn schedule_action_kind_name(action: &CronAction) -> &'static str {
    match action {
        CronAction::SystemEvent { .. } => "system_event",
        CronAction::AgentTurn { .. } => "agent_turn",
        CronAction::WorkflowRun { .. } => "workflow_run",
        CronAction::WorkflowSignal { .. } => "workflow_signal",
    }
}

fn schedule_runtime_snapshot(meta: &ScheduleJobMeta) -> ScheduleRuntimeRecord {
    ScheduleRuntimeRecord {
        schedule_id: meta.definition_id.clone(),
        enabled: meta.job.enabled,
        last_run: meta.job.last_run.map(|value| value.to_rfc3339()),
        next_run: meta.job.next_run.map(|value| value.to_rfc3339()),
        last_status: meta.last_status.clone(),
        consecutive_errors: meta.consecutive_errors,
        one_shot: meta.one_shot,
        updated_at: meta.updated_at.clone(),
    }
}

fn schedule_runtime_status_from_record(record: &ScheduleRuntimeRecord) -> ScheduleRuntimeStatus {
    ScheduleRuntimeStatus {
        last_run: record.last_run.clone(),
        next_run: record.next_run.clone(),
        last_status: record.last_status.clone(),
        consecutive_errors: record.consecutive_errors,
        one_shot: record.one_shot,
    }
}

fn schedule_runtime_response_from_record(
    record: &ScheduleRuntimeRecord,
) -> ScheduleRuntimeResponse {
    ScheduleRuntimeResponse {
        schedule_id: record.schedule_id.clone(),
        enabled: record.enabled,
        last_run: record.last_run.clone(),
        next_run: record.next_run.clone(),
        last_status: record.last_status.clone(),
        consecutive_errors: record.consecutive_errors,
        one_shot: record.one_shot,
    }
}

fn schedule_list_item(meta: &ScheduleJobMeta, runtime: &ScheduleRuntimeRecord) -> ScheduleListItem {
    ScheduleListItem {
        id: meta.definition_id.clone(),
        agent: meta.agent_ref.clone(),
        name: meta.job.name.clone(),
        enabled: meta.job.enabled,
        schedule: meta.job.schedule.clone(),
        action: meta.job.action.clone(),
        runtime_status: ScheduleListRuntimeStatus {
            next_run: runtime.next_run.clone(),
            last_status: runtime.last_status.clone(),
        },
        updated_at: meta.updated_at.clone(),
    }
}

fn schedule_response(meta: &ScheduleJobMeta, runtime: &ScheduleRuntimeRecord) -> ScheduleResponse {
    ScheduleResponse {
        id: meta.definition_id.clone(),
        agent: meta.agent_ref.clone(),
        name: meta.job.name.clone(),
        enabled: meta.job.enabled,
        schedule: meta.job.schedule.clone(),
        action: meta.job.action.clone(),
        delivery: meta.job.delivery.clone(),
        origin: meta.origin.clone(),
        forked_from: meta.forked_from.clone(),
        created_at: meta.job.created_at.to_rfc3339(),
        updated_at: meta.updated_at.clone(),
        runtime_status: schedule_runtime_status_from_record(runtime),
    }
}

fn schedule_runtime_record_for_meta(
    state: &AppState,
    meta: &ScheduleJobMeta,
) -> Result<ScheduleRuntimeRecord, (StatusCode, Json<serde_json::Value>)> {
    state
        .kernel
        .runtime_stores
        .schedule_runtime
        .get_schedule_runtime(&meta.definition_id)
        .map_err(|error| {
            schedule_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "runtime_status_failed",
                "Failed to load schedule runtime status",
                Some(serde_json::json!([{
                    "message": error.to_string(),
                }])),
            )
        })
        .map(|record| record.unwrap_or_else(|| schedule_runtime_snapshot(meta)))
}

#[derive(Debug, Clone)]
struct ResolvedScheduleAgent {
    public_ref: String,
    runtime_id: AgentId,
}

#[derive(Debug, Clone)]
struct ValidatedScheduleDefinition {
    definition: ScheduleDefinition,
    runtime_agent_id: AgentId,
}

fn resolve_schedule_agent_reference(
    state: &AppState,
    value: &str,
) -> Option<ResolvedScheduleAgent> {
    if let Ok(Some(_)) = load_agent_definition_resource(state, value) {
        return Some(ResolvedScheduleAgent {
            public_ref: value.to_string(),
            runtime_id: stable_runtime_agent_id(value),
        });
    }

    if let Ok(definitions) = agent_definition_store(state).list() {
        if let Some(resource) = definitions
            .into_iter()
            .find(|resource| resource.definition.name == value)
        {
            return Some(ResolvedScheduleAgent {
                public_ref: resource.definition.id.clone(),
                runtime_id: stable_runtime_agent_id(&resource.definition.id),
            });
        }
    }

    if let Ok(agent_id) = value.parse::<AgentId>() {
        if let Some(entry) = state.kernel.registry.get(agent_id) {
            return Some(ResolvedScheduleAgent {
                public_ref: runtime_definition_id(&entry)
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| entry.name.clone()),
                runtime_id: agent_id,
            });
        }
    }

    state
        .kernel
        .registry
        .find_by_name(value)
        .map(|entry| ResolvedScheduleAgent {
            public_ref: runtime_definition_id(&entry)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| entry.name.clone()),
            runtime_id: entry.id,
        })
}

fn resolve_schedule_workflow_reference(state: &AppState, value: &str) -> Option<String> {
    match load_workflow_definition_resource(state, value) {
        Ok(Some(_)) => Some(value.to_string()),
        Ok(None) => load_all_workflow_definition_resources(state)
            .ok()
            .and_then(|resources| {
                resources
                    .into_iter()
                    .find(|resource| resource.definition.name == value)
                    .map(|resource| resource.definition.id)
            }),
        Err(_) => None,
    }
}

fn parse_schedule_timeout_secs(
    action: &serde_json::Map<String, serde_json::Value>,
    issues: &mut Vec<ScheduleValidationIssue>,
) -> Option<u64> {
    let value = action.get("timeout_secs")?;
    let Some(timeout_secs) = value.as_u64() else {
        issues.push(schedule_validation_issue(
            "error",
            "invalid_type",
            "action.timeout_secs",
            "`action.timeout_secs` must be an unsigned integer",
        ));
        return None;
    };
    Some(timeout_secs)
}

fn parse_schedule_block(
    value: Option<&serde_json::Value>,
    issues: &mut Vec<ScheduleValidationIssue>,
) -> Option<CronSchedule> {
    let Some(serde_json::Value::Object(object)) = value else {
        issues.push(schedule_validation_issue(
            "error",
            "missing_field",
            "schedule",
            "`schedule` must be an object",
        ));
        return None;
    };
    let Some(kind) = object.get("kind").and_then(serde_json::Value::as_str) else {
        issues.push(schedule_validation_issue(
            "error",
            "missing_field",
            "schedule.kind",
            "`schedule.kind` is required",
        ));
        return None;
    };

    match kind {
        "cron" => {
            let Some(expr) = object.get("expr").and_then(serde_json::Value::as_str) else {
                issues.push(schedule_validation_issue(
                    "error",
                    "missing_field",
                    "schedule.expr",
                    "`schedule.expr` is required",
                ));
                return None;
            };
            let normalized_expr = expr.split_whitespace().collect::<Vec<_>>().join(" ");
            let fields = normalized_expr.split_whitespace().collect::<Vec<_>>();
            if fields.len() != 5 {
                issues.push(schedule_validation_issue(
                    "error",
                    "invalid_cron",
                    "schedule.expr",
                    "cron expression must have exactly 5 fields",
                ));
                return None;
            }
            let seven_field = format!("0 {normalized_expr} *");
            if let Err(error) = seven_field.parse::<cron::Schedule>() {
                issues.push(schedule_validation_issue(
                    "error",
                    "invalid_cron",
                    "schedule.expr",
                    format!("invalid cron expression: {error}"),
                ));
                return None;
            }
            let timezone = object
                .get("tz")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
            if let Some(timezone) = timezone.as_deref() {
                if timezone.parse::<chrono_tz::Tz>().is_err() {
                    issues.push(schedule_validation_issue(
                        "error",
                        "invalid_timezone",
                        "schedule.tz",
                        format!("invalid timezone: {timezone}"),
                    ));
                    return None;
                }
            }

            Some(CronSchedule::Cron {
                expr: normalized_expr,
                tz: timezone,
            })
        }
        "every" => object
            .get("every_secs")
            .and_then(serde_json::Value::as_u64)
            .map(|every_secs| CronSchedule::Every { every_secs })
            .or_else(|| {
                issues.push(schedule_validation_issue(
                    "error",
                    "missing_field",
                    "schedule.every_secs",
                    "`schedule.every_secs` is required",
                ));
                None
            }),
        "at" => {
            let Some(at) = object.get("at").and_then(serde_json::Value::as_str) else {
                issues.push(schedule_validation_issue(
                    "error",
                    "missing_field",
                    "schedule.at",
                    "`schedule.at` is required",
                ));
                return None;
            };
            match chrono::DateTime::parse_from_rfc3339(at) {
                Ok(timestamp) => Some(CronSchedule::At {
                    at: timestamp.with_timezone(&chrono::Utc),
                }),
                Err(error) => {
                    issues.push(schedule_validation_issue(
                        "error",
                        "invalid_datetime",
                        "schedule.at",
                        format!("invalid RFC 3339 timestamp: {error}"),
                    ));
                    None
                }
            }
        }
        other => {
            issues.push(schedule_validation_issue(
                "error",
                "unsupported_schedule_kind",
                "schedule.kind",
                format!("unsupported schedule kind `{other}`"),
            ));
            None
        }
    }
}

fn parse_schedule_action_block(
    state: &AppState,
    value: Option<&serde_json::Value>,
    issues: &mut Vec<ScheduleValidationIssue>,
) -> Option<CronAction> {
    let Some(serde_json::Value::Object(object)) = value else {
        issues.push(schedule_validation_issue(
            "error",
            "missing_field",
            "action",
            "`action` must be an object",
        ));
        return None;
    };
    let Some(kind) = object.get("kind").and_then(serde_json::Value::as_str) else {
        issues.push(schedule_validation_issue(
            "error",
            "missing_field",
            "action.kind",
            "`action.kind` is required",
        ));
        return None;
    };

    match kind {
        "system_event" => {
            let Some(event) = object
                .get("event")
                .or_else(|| object.get("text"))
                .and_then(serde_json::Value::as_str)
            else {
                issues.push(schedule_validation_issue(
                    "error",
                    "missing_field",
                    "action.event",
                    "`action.event` is required",
                ));
                return None;
            };

            Some(CronAction::SystemEvent {
                event: event.to_string(),
                payload: object
                    .get("payload")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            })
        }
        "agent_turn" => {
            let model_override = object
                .get("model_override")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned);
            let timeout_secs = parse_schedule_timeout_secs(object, issues);
            let parsed_input = object
                .get("input")
                .cloned()
                .map(serde_json::from_value::<CronTextInputPayload>)
                .transpose();
            let input = match parsed_input {
                Ok(Some(input)) => input,
                Ok(None) => CronTextInputPayload::default(),
                Err(error) => {
                    issues.push(schedule_validation_issue(
                        "error",
                        "invalid_input",
                        "action.input",
                        format!("invalid `action.input`: {error}"),
                    ));
                    CronTextInputPayload::default()
                }
            };
            let legacy_message = object
                .get("message")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned);
            let normalized_input = if !input.items.is_empty() {
                input
            } else if let Some(message) = legacy_message.as_deref() {
                CronTextInputPayload {
                    items: vec![CronTextInputItem {
                        item_type: "text".to_string(),
                        text: Some(message.to_string()),
                    }],
                }
            } else {
                issues.push(schedule_validation_issue(
                    "error",
                    "missing_field",
                    "action.input",
                    "`action.input` is required for `agent_turn`",
                ));
                CronTextInputPayload::default()
            };

            Some(CronAction::AgentTurn {
                message: None,
                input: normalized_input,
                model_override,
                timeout_secs,
            })
        }
        "workflow_run" => {
            let Some(workflow_id) = object
                .get("workflow_id")
                .and_then(serde_json::Value::as_str)
            else {
                issues.push(schedule_validation_issue(
                    "error",
                    "missing_field",
                    "action.workflow_id",
                    "`action.workflow_id` is required",
                ));
                return None;
            };
            let Some(workflow_id) = resolve_schedule_workflow_reference(state, workflow_id) else {
                issues.push(schedule_validation_issue(
                    "error",
                    "not_found",
                    "action.workflow_id",
                    format!("workflow definition not found: {workflow_id}"),
                ));
                return None;
            };

            Some(CronAction::WorkflowRun {
                workflow_id,
                input: object.get("input").cloned(),
                timeout_secs: parse_schedule_timeout_secs(object, issues),
            })
        }
        "workflow_signal" => {
            let Some(signal) = object.get("signal").and_then(serde_json::Value::as_str) else {
                issues.push(schedule_validation_issue(
                    "error",
                    "missing_field",
                    "action.signal",
                    "`action.signal` is required",
                ));
                return None;
            };
            let Some(selector) = object
                .get("selector")
                .and_then(serde_json::Value::as_object)
            else {
                issues.push(schedule_validation_issue(
                    "error",
                    "missing_field",
                    "action.selector.workflow_id",
                    "`action.selector.workflow_id` is required",
                ));
                return None;
            };
            let Some(workflow_id) = selector
                .get("workflow_id")
                .and_then(serde_json::Value::as_str)
            else {
                issues.push(schedule_validation_issue(
                    "error",
                    "missing_field",
                    "action.selector.workflow_id",
                    "`action.selector.workflow_id` is required",
                ));
                return None;
            };
            let Some(workflow_id) = resolve_schedule_workflow_reference(state, workflow_id) else {
                issues.push(schedule_validation_issue(
                    "error",
                    "not_found",
                    "action.selector.workflow_id",
                    format!("workflow definition not found: {workflow_id}"),
                ));
                return None;
            };

            Some(CronAction::WorkflowSignal {
                signal: signal.to_string(),
                selector: CronWorkflowSignalSelector { workflow_id },
                payload: object
                    .get("payload")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            })
        }
        other => {
            issues.push(schedule_validation_issue(
                "error",
                "unsupported_action_kind",
                "action.kind",
                format!("unsupported action kind `{other}`"),
            ));
            None
        }
    }
}

fn parse_schedule_delivery_block(
    value: Option<&serde_json::Value>,
    issues: &mut Vec<ScheduleValidationIssue>,
) -> Option<CronDelivery> {
    let Some(value) = value else {
        return Some(CronDelivery::None);
    };
    let Some(object) = value.as_object() else {
        issues.push(schedule_validation_issue(
            "error",
            "invalid_type",
            "delivery",
            "`delivery` must be an object",
        ));
        return None;
    };
    let Some(kind) = object.get("kind").and_then(serde_json::Value::as_str) else {
        issues.push(schedule_validation_issue(
            "error",
            "missing_field",
            "delivery.kind",
            "`delivery.kind` is required",
        ));
        return None;
    };

    match kind {
        "none" => Some(CronDelivery::None),
        "last_channel" => Some(CronDelivery::LastChannel),
        "channel" => {
            let Some(channel) = object.get("channel").and_then(serde_json::Value::as_str) else {
                issues.push(schedule_validation_issue(
                    "error",
                    "missing_field",
                    "delivery.channel",
                    "`delivery.channel` is required",
                ));
                return None;
            };
            let Some(to) = object.get("to").and_then(serde_json::Value::as_str) else {
                issues.push(schedule_validation_issue(
                    "error",
                    "missing_field",
                    "delivery.to",
                    "`delivery.to` is required",
                ));
                return None;
            };
            Some(CronDelivery::Channel {
                channel: channel.to_string(),
                to: to.to_string(),
            })
        }
        "webhook" => {
            let Some(url) = object.get("url").and_then(serde_json::Value::as_str) else {
                issues.push(schedule_validation_issue(
                    "error",
                    "missing_field",
                    "delivery.url",
                    "`delivery.url` is required",
                ));
                return None;
            };
            Some(CronDelivery::Webhook {
                url: url.to_string(),
            })
        }
        other => {
            issues.push(schedule_validation_issue(
                "error",
                "unsupported_delivery_kind",
                "delivery.kind",
                format!("unsupported delivery kind `{other}`"),
            ));
            None
        }
    }
}

fn schedule_validation_issue_from_error(error: &str) -> ScheduleValidationIssue {
    let path = if error.contains("cron expression") {
        "schedule.expr"
    } else if error.contains("timezone") {
        "schedule.tz"
    } else if error.contains("every_secs") {
        "schedule.every_secs"
    } else if error.contains("scheduled time") {
        "schedule.at"
    } else if error.contains("workflow signal selector.workflow_id") {
        "action.selector.workflow_id"
    } else if error.contains("workflow signal name") {
        "action.signal"
    } else if error.contains("workflow_id") {
        "action.workflow_id"
    } else if error.contains("timeout_secs") {
        "action.timeout_secs"
    } else if error.contains("agent turn input") {
        "action.input"
    } else if error.contains("system event") {
        "action.event"
    } else if error.contains("webhook URL") {
        "delivery.url"
    } else if error.contains("delivery channel") {
        "delivery.channel"
    } else if error.contains("recipient") {
        "delivery.to"
    } else if error.contains("name") {
        "name"
    } else {
        "definition"
    };
    schedule_validation_issue("error", "invalid_definition", path, error)
}

fn validate_schedule_definition_value(
    state: &AppState,
    body: &serde_json::Value,
) -> (
    Vec<ScheduleValidationIssue>,
    Option<ValidatedScheduleDefinition>,
) {
    let mut issues = Vec::new();
    let Some(object) = body.as_object() else {
        issues.push(schedule_validation_issue(
            "error",
            "invalid_request",
            "definition",
            "schedule definition body must be an object",
        ));
        return (issues, None);
    };

    let agent_value = object.get("agent").and_then(serde_json::Value::as_str);
    let resolved_agent = match agent_value {
        Some(value) if !value.trim().is_empty() => resolve_schedule_agent_reference(state, value),
        _ => {
            issues.push(schedule_validation_issue(
                "error",
                "missing_field",
                "agent",
                "`agent` is required",
            ));
            None
        }
    };
    if agent_value.is_some() && resolved_agent.is_none() {
        issues.push(schedule_validation_issue(
            "error",
            "not_found",
            "agent",
            format!(
                "agent definition or runtime not found: {}",
                agent_value.unwrap_or_default()
            ),
        ));
    }

    let name = object
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            issues.push(schedule_validation_issue(
                "error",
                "missing_field",
                "name",
                "`name` is required",
            ));
            String::new()
        });

    let enabled = match object.get("enabled") {
        Some(serde_json::Value::Bool(value)) => *value,
        Some(_) => {
            issues.push(schedule_validation_issue(
                "error",
                "invalid_type",
                "enabled",
                "`enabled` must be a boolean",
            ));
            true
        }
        None => true,
    };

    let schedule = parse_schedule_block(object.get("schedule"), &mut issues);
    let action = parse_schedule_action_block(state, object.get("action"), &mut issues);
    let delivery = parse_schedule_delivery_block(object.get("delivery"), &mut issues);

    let (Some(resolved_agent), Some(schedule), Some(action), Some(delivery)) =
        (resolved_agent, schedule, action, delivery)
    else {
        return (issues, None);
    };

    let normalized = ScheduleDefinition {
        agent: resolved_agent.public_ref.clone(),
        name,
        enabled,
        schedule,
        action,
        delivery,
    };

    let candidate = CronJob {
        id: CronJobId::new(),
        agent_id: resolved_agent.runtime_id,
        name: normalized.name.clone(),
        enabled: normalized.enabled,
        schedule: normalized.schedule.clone(),
        action: normalized.action.clone(),
        delivery: normalized.delivery.clone(),
        created_at: chrono::Utc::now(),
        last_run: None,
        next_run: None,
    };
    if let Err(error) = candidate.validate(0) {
        issues.push(schedule_validation_issue_from_error(&error));
        return (issues, None);
    }

    (
        issues,
        Some(ValidatedScheduleDefinition {
            definition: normalized,
            runtime_agent_id: resolved_agent.runtime_id,
        }),
    )
}

fn schedule_definition_to_meta(
    definition_id: String,
    validated: ValidatedScheduleDefinition,
    existing: Option<&ScheduleJobMeta>,
) -> ScheduleJobMeta {
    let timestamp = chrono::Utc::now();
    let one_shot = matches!(validated.definition.schedule, CronSchedule::At { .. });
    let (created_at, last_run, last_status, consecutive_errors, origin, forked_from) = existing
        .map(|meta| {
            (
                meta.job.created_at,
                meta.job.last_run,
                meta.last_status.clone(),
                meta.consecutive_errors,
                meta.origin.clone(),
                meta.forked_from.clone(),
            )
        })
        .unwrap_or_else(|| {
            (
                timestamp,
                None,
                None,
                0,
                CronDefinitionOrigin::user(),
                None::<CronDefinitionForkedFrom>,
            )
        });

    ScheduleJobMeta {
        job: CronJob {
            id: existing.map(|meta| meta.job.id).unwrap_or_default(),
            agent_id: validated.runtime_agent_id,
            name: validated.definition.name.clone(),
            enabled: validated.definition.enabled,
            schedule: validated.definition.schedule.clone(),
            action: validated.definition.action.clone(),
            delivery: validated.definition.delivery.clone(),
            created_at,
            last_run,
            next_run: None,
        },
        definition_id,
        agent_ref: validated.definition.agent,
        origin,
        forked_from,
        updated_at: timestamp.to_rfc3339(),
        one_shot,
        last_status,
        consecutive_errors,
    }
}

/// GET /api/v1/schedules — List typed schedule definitions.
pub async fn list_schedule_definitions_v1(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ScheduleListQueryParams>,
) -> impl IntoResponse {
    let limit = match parse_pagination_limit(params.limit) {
        Ok(limit) => limit,
        Err(response) => return response,
    };
    let offset = match parse_cursor_offset(params.cursor.as_deref()) {
        Ok(offset) => offset,
        Err(response) => return response,
    };

    let search = params.search.map(|value| value.to_lowercase());
    let mut items = state
        .kernel
        .cron_scheduler
        .list_all_metas()
        .into_iter()
        .filter(|meta| {
            params
                .agent
                .as_ref()
                .map(|agent| meta.agent_ref == *agent)
                .unwrap_or(true)
        })
        .filter(|meta| {
            params
                .enabled
                .map(|enabled| meta.job.enabled == enabled)
                .unwrap_or(true)
        })
        .filter(|meta| {
            params
                .schedule_kind
                .as_ref()
                .map(|kind| schedule_kind_name(&meta.job.schedule) == kind)
                .unwrap_or(true)
        })
        .filter(|meta| {
            params
                .action_kind
                .as_ref()
                .map(|kind| schedule_action_kind_name(&meta.job.action) == kind)
                .unwrap_or(true)
        })
        .filter(|meta| {
            search.as_ref().is_none_or(|needle| {
                let haystack = format!(
                    "{} {} {} {} {}",
                    meta.definition_id,
                    meta.agent_ref,
                    meta.job.name,
                    serde_json::to_string(&meta.job.schedule).unwrap_or_default(),
                    serde_json::to_string(&meta.job.action).unwrap_or_default(),
                )
                .to_lowercase();
                haystack.contains(needle)
            })
        })
        .map(|meta| {
            let runtime = state
                .kernel
                .runtime_stores
                .schedule_runtime
                .get_schedule_runtime(&meta.definition_id)
                .ok()
                .flatten()
                .unwrap_or_else(|| schedule_runtime_snapshot(&meta));
            schedule_list_item(&meta, &runtime)
        })
        .collect::<Vec<_>>();

    items.sort_by(|left, right| {
        left.updated_at
            .cmp(&right.updated_at)
            .then(left.id.cmp(&right.id))
    });
    let next_cursor = if offset + limit < items.len() {
        Some((offset + limit).to_string())
    } else {
        None
    };
    let items = items
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();

    (
        StatusCode::OK,
        Json(serde_json::json!(ScheduleListResponse {
            items,
            next_cursor
        })),
    )
}

/// POST /api/v1/schedules — Create and persist a typed schedule definition.
pub async fn create_schedule_definition_v1(
    State(state): State<Arc<AppState>>,
    payload: Result<Json<serde_json::Value>, JsonRejection>,
) -> impl IntoResponse {
    let Json(body) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return schedule_json_rejection(rejection),
    };
    let _write_guard = SCHEDULE_DEFINITION_WRITE_LOCK.lock().await;

    if body.get("id").is_some() {
        return schedule_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Schedule IDs are assigned by the server",
            Some(serde_json::json!([{
                "path": "id",
            }])),
        );
    }

    let (issues, validated) = validate_schedule_definition_value(&state, &body);
    if validated.is_none() {
        return schedule_validation_error_response(&issues);
    }
    let validated = validated.expect("validated schedule definition should exist");
    let definition_id = generate_schedule_definition_id();
    let meta = schedule_definition_to_meta(definition_id.clone(), validated, None);
    match state.kernel.cron_scheduler.add_job_meta(meta.clone()) {
        Ok(_) => {}
        Err(error) => {
            return schedule_error_response(
                StatusCode::BAD_REQUEST,
                "validation_error",
                "schedule definition is invalid",
                Some(serde_json::json!([schedule_validation_issue_from_error(
                    &error.to_string(),
                )])),
            )
        }
    }
    if let Err(error) = state.kernel.cron_scheduler.persist() {
        return schedule_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "definition_persist_failed",
            "Failed to persist schedule definition",
            Some(serde_json::json!([{
                "message": error.to_string(),
            }])),
        );
    }
    let runtime = match schedule_runtime_record_for_meta(&state, &meta) {
        Ok(runtime) => runtime,
        Err(response) => return response,
    };

    (
        StatusCode::CREATED,
        Json(serde_json::json!(schedule_response(&meta, &runtime))),
    )
}

/// GET /api/v1/schedules/{id} — Load one schedule definition.
pub async fn get_schedule_definition_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_schedule_definition_id(&id) {
        return response;
    }
    let Some(meta) = state.kernel.cron_scheduler.get_meta_by_definition_id(&id) else {
        return schedule_not_found_response();
    };
    let runtime = match schedule_runtime_record_for_meta(&state, &meta) {
        Ok(runtime) => runtime,
        Err(response) => return response,
    };
    (
        StatusCode::OK,
        Json(serde_json::json!(schedule_response(&meta, &runtime))),
    )
}

/// PUT /api/v1/schedules/{id} — Replace one persisted schedule definition.
pub async fn update_schedule_definition_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<serde_json::Value>, JsonRejection>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_schedule_definition_id(&id) {
        return response;
    }
    let Json(body) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return schedule_json_rejection(rejection),
    };
    let _write_guard = SCHEDULE_DEFINITION_WRITE_LOCK.lock().await;
    let Some(existing) = state.kernel.cron_scheduler.get_meta_by_definition_id(&id) else {
        return schedule_not_found_response();
    };

    if let Some(body_id) = body.get("id").and_then(serde_json::Value::as_str) {
        if body_id != id {
            return schedule_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Path ID and body ID must match",
                Some(serde_json::json!([{
                    "path": "id",
                    "expected": id,
                    "actual": body_id,
                }])),
            );
        }
    }

    let (issues, validated) = validate_schedule_definition_value(&state, &body);
    if validated.is_none() {
        return schedule_validation_error_response(&issues);
    }
    let meta =
        schedule_definition_to_meta(id.clone(), validated.expect("validated"), Some(&existing));
    let meta = match state
        .kernel
        .cron_scheduler
        .replace_job_meta_by_definition_id(&id, meta)
    {
        Ok(meta) => meta,
        Err(error) => {
            return schedule_error_response(
                StatusCode::BAD_REQUEST,
                "validation_error",
                "schedule definition is invalid",
                Some(serde_json::json!([schedule_validation_issue_from_error(
                    &error.to_string(),
                )])),
            )
        }
    };
    if let Err(error) = state.kernel.cron_scheduler.persist() {
        return schedule_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "definition_persist_failed",
            "Failed to persist schedule definition",
            Some(serde_json::json!([{
                "message": error.to_string(),
            }])),
        );
    }
    let runtime = match schedule_runtime_record_for_meta(&state, &meta) {
        Ok(runtime) => runtime,
        Err(response) => return response,
    };

    (
        StatusCode::OK,
        Json(serde_json::json!(schedule_response(&meta, &runtime))),
    )
}

/// DELETE /api/v1/schedules/{id} — Delete one persisted schedule definition.
pub async fn delete_schedule_definition_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_schedule_definition_id(&id) {
        return response.into_response();
    }
    let _write_guard = SCHEDULE_DEFINITION_WRITE_LOCK.lock().await;
    match state.kernel.cron_scheduler.remove_job_by_definition_id(&id) {
        Ok(_) => {}
        Err(_) => return schedule_not_found_response().into_response(),
    }
    if let Err(error) = state.kernel.cron_scheduler.persist() {
        return schedule_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "definition_delete_failed",
            "Failed to persist schedule deletion",
            Some(serde_json::json!([{
                "message": error.to_string(),
            }])),
        )
        .into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

/// POST /api/v1/schedules/validate — Validate a schedule definition.
pub async fn validate_schedule_definition_v1(
    State(state): State<Arc<AppState>>,
    payload: Result<Json<ScheduleValidateRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return schedule_json_rejection(rejection),
    };
    let (issues, validated) = validate_schedule_definition_value(&state, &request.definition);
    let normalized = validated.map(|validated| validated.definition);
    let valid = schedule_definition_is_valid(&issues, request.strict.unwrap_or(false));

    (
        StatusCode::OK,
        Json(serde_json::json!(ScheduleValidateResponse {
            valid,
            issues,
            normalized,
        })),
    )
}

/// POST /api/v1/schedules/{id}/fork — Fork an existing schedule definition.
pub async fn fork_schedule_definition_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<ScheduleForkRequest>, JsonRejection>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_schedule_definition_id(&id) {
        return response;
    }
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return schedule_json_rejection(rejection),
    };
    let _write_guard = SCHEDULE_DEFINITION_WRITE_LOCK.lock().await;
    if request.mode.as_deref().unwrap_or("shadow") != "shadow" {
        return schedule_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Schedule forks currently support only `shadow` mode",
            Some(serde_json::json!([{
                "path": "mode",
                "value": request.mode,
            }])),
        );
    }
    let Some(existing) = state.kernel.cron_scheduler.get_meta_by_definition_id(&id) else {
        return schedule_not_found_response();
    };

    let forked_id = generate_schedule_definition_id();
    let validated = ValidatedScheduleDefinition {
        runtime_agent_id: existing.job.agent_id,
        definition: ScheduleDefinition {
            agent: existing.agent_ref.clone(),
            name: format!("{} Fork", existing.job.name),
            enabled: existing.job.enabled,
            schedule: existing.job.schedule.clone(),
            action: existing.job.action.clone(),
            delivery: existing.job.delivery.clone(),
        },
    };
    let mut meta = schedule_definition_to_meta(forked_id.clone(), validated, None);
    meta.origin = CronDefinitionOrigin::user();
    meta.forked_from = Some(CronDefinitionForkedFrom {
        kind: existing.origin.kind.clone(),
        pack_id: existing.origin.pack_id.clone(),
        pack_version: existing.origin.pack_version.clone(),
        resource_type: "schedule".to_string(),
        resource_id: existing.definition_id.clone(),
    });

    match state.kernel.cron_scheduler.add_job_meta(meta.clone()) {
        Ok(_) => {}
        Err(error) => {
            return schedule_error_response(
                StatusCode::BAD_REQUEST,
                "validation_error",
                "schedule definition is invalid",
                Some(serde_json::json!([schedule_validation_issue_from_error(
                    &error.to_string(),
                )])),
            )
        }
    }
    if let Err(error) = state.kernel.cron_scheduler.persist() {
        return schedule_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "definition_persist_failed",
            "Failed to persist schedule definition",
            Some(serde_json::json!([{
                "message": error.to_string(),
            }])),
        );
    }
    let runtime = match schedule_runtime_record_for_meta(&state, &meta) {
        Ok(runtime) => runtime,
        Err(response) => return response,
    };

    (
        StatusCode::OK,
        Json(serde_json::json!(schedule_response(&meta, &runtime))),
    )
}

/// GET /api/v1/schedules/{id}/runtime — Load runtime state for one schedule definition.
pub async fn get_schedule_runtime_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_schedule_definition_id(&id) {
        return response;
    }
    let Some(meta) = state.kernel.cron_scheduler.get_meta_by_definition_id(&id) else {
        return schedule_not_found_response();
    };
    let runtime = match schedule_runtime_record_for_meta(&state, &meta) {
        Ok(runtime) => runtime,
        Err(response) => return response,
    };

    (
        StatusCode::OK,
        Json(serde_json::json!(schedule_runtime_response_from_record(
            &runtime
        ))),
    )
}

fn schedule_action_accepted_response(
    id: &str,
    execution: ScheduleExecutionResult,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!(ScheduleAcceptedActionResponse {
            accepted: true,
            resource_id: id.to_string(),
            status: "accepted".to_string(),
            session_id: execution.session_id,
            run_id: execution.run_id,
        })),
    )
}

/// POST /api/v1/schedules/{id}/enable — Enable a schedule definition.
pub async fn enable_schedule_definition_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_schedule_definition_id(&id) {
        return response;
    }
    let _write_guard = SCHEDULE_DEFINITION_WRITE_LOCK.lock().await;
    match state
        .kernel
        .cron_scheduler
        .set_enabled_by_definition_id(&id, true)
    {
        Ok(_) => {}
        Err(_) => return schedule_not_found_response(),
    }
    if let Err(error) = state.kernel.cron_scheduler.persist() {
        return schedule_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "definition_persist_failed",
            "Failed to persist schedule definition",
            Some(serde_json::json!([{
                "message": error.to_string(),
            }])),
        );
    }

    schedule_action_accepted_response(&id, ScheduleExecutionResult::default())
}

/// POST /api/v1/schedules/{id}/disable — Disable a schedule definition.
pub async fn disable_schedule_definition_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_schedule_definition_id(&id) {
        return response;
    }
    let _write_guard = SCHEDULE_DEFINITION_WRITE_LOCK.lock().await;
    match state
        .kernel
        .cron_scheduler
        .set_enabled_by_definition_id(&id, false)
    {
        Ok(_) => {}
        Err(_) => return schedule_not_found_response(),
    }
    if let Err(error) = state.kernel.cron_scheduler.persist() {
        return schedule_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "definition_persist_failed",
            "Failed to persist schedule definition",
            Some(serde_json::json!([{
                "message": error.to_string(),
            }])),
        );
    }

    schedule_action_accepted_response(&id, ScheduleExecutionResult::default())
}

/// POST /api/v1/schedules/{id}/run-now — Trigger a schedule action immediately.
pub async fn run_schedule_definition_now_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<ScheduleRunNowRequest>, JsonRejection>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_schedule_definition_id(&id) {
        return response;
    }
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return schedule_json_rejection(rejection),
    };
    let Some(meta) = state.kernel.cron_scheduler.get_meta_by_definition_id(&id) else {
        return schedule_not_found_response();
    };
    let metadata = request
        .metadata
        .or_else(|| Some(serde_json::json!({ "source": "api" })));
    match state.kernel.execute_schedule_now(&meta, metadata).await {
        Ok(execution) => schedule_action_accepted_response(&id, execution),
        Err(error) => schedule_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "schedule_execution_failed",
            "Failed to execute schedule action",
            Some(serde_json::json!([{
                "message": error,
            }])),
        ),
    }
}

/// POST /api/v1/schedules/{id}/run-now/dry-run — Simulate an immediate schedule execution.
pub async fn dry_run_schedule_definition_now_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Result<Json<ScheduleRunNowRequest>, JsonRejection>,
) -> impl IntoResponse {
    if let Err(response) = ensure_safe_schedule_definition_id(&id) {
        return response;
    }
    let Json(_request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return schedule_json_rejection(rejection),
    };
    let Some(meta) = state.kernel.cron_scheduler.get_meta_by_definition_id(&id) else {
        return schedule_not_found_response();
    };

    (
        StatusCode::OK,
        Json(serde_json::json!(ScheduleDryRunResponse {
            would_execute: true,
            resolved: ScheduleDryRunResolved {
                schedule_id: id,
                action: meta.job.action.clone(),
            },
            effects: ScheduleDryRunEffects {
                schedule_fire: true,
            },
            explanation: ScheduleDryRunExplanation {
                delivery: meta.job.delivery.clone(),
            },
        })),
    )
}

// ---------------------------------------------------------------------------
// Agent Identity endpoint
// ---------------------------------------------------------------------------

/// Request body for updating agent visual identity.
#[derive(serde::Deserialize)]
pub struct UpdateIdentityRequest {
    pub emoji: Option<String>,
    pub avatar_url: Option<String>,
    pub color: Option<String>,
    #[serde(default)]
    pub archetype: Option<String>,
    #[serde(default)]
    pub vibe: Option<String>,
    #[serde(default)]
    pub greeting_style: Option<String>,
}

/// PATCH /api/agents/{id}/identity — Update an agent's visual identity.
pub async fn update_agent_identity(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateIdentityRequest>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            );
        }
    };

    // Validate color format if provided
    if let Some(ref color) = req.color {
        if !color.is_empty() && !color.starts_with('#') {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Color must be a hex code starting with '#'"})),
            );
        }
    }

    // Validate avatar_url if provided
    if let Some(ref url) = req.avatar_url {
        if !url.is_empty()
            && !url.starts_with("http://")
            && !url.starts_with("https://")
            && !url.starts_with("data:")
        {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Avatar URL must be http/https or data URI"})),
            );
        }
    }

    let identity = AgentIdentity {
        emoji: req.emoji,
        avatar_url: req.avatar_url,
        color: req.color,
        archetype: req.archetype,
        vibe: req.vibe,
        greeting_style: req.greeting_style,
    };

    match state.kernel.registry.update_identity(agent_id, identity) {
        Ok(()) => {
            // Persist identity to SQLite
            if let Some(entry) = state.kernel.registry.get(agent_id) {
                let _ = state.kernel.memory.save_agent(&entry);
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "ok", "agent_id": id})),
            )
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Agent not found"})),
        ),
    }
}

// ---------------------------------------------------------------------------
// Agent Config Hot-Update
// ---------------------------------------------------------------------------

/// Request body for patching agent config (name, description, prompt, identity, model).
#[derive(serde::Deserialize)]
pub struct PatchAgentConfigRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub system_prompt: Option<String>,
    pub emoji: Option<String>,
    pub avatar_url: Option<String>,
    pub color: Option<String>,
    pub archetype: Option<String>,
    pub vibe: Option<String>,
    pub greeting_style: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub api_key_env: Option<String>,
    pub base_url: Option<String>,
    pub fallback_models: Option<Vec<openfang_types::agent::FallbackModel>>,
}

/// PATCH /api/agents/{id}/config — Hot-update agent name, description, system prompt, and identity.
pub async fn patch_agent_config(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<PatchAgentConfigRequest>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            );
        }
    };

    // Input length limits
    const MAX_NAME_LEN: usize = 256;
    const MAX_DESC_LEN: usize = 4096;
    const MAX_PROMPT_LEN: usize = 65_536;

    if let Some(ref name) = req.name {
        if name.len() > MAX_NAME_LEN {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(
                    serde_json::json!({"error": format!("Name exceeds max length ({MAX_NAME_LEN} chars)")}),
                ),
            );
        }
    }
    if let Some(ref desc) = req.description {
        if desc.len() > MAX_DESC_LEN {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(
                    serde_json::json!({"error": format!("Description exceeds max length ({MAX_DESC_LEN} chars)")}),
                ),
            );
        }
    }
    if let Some(ref prompt) = req.system_prompt {
        if prompt.len() > MAX_PROMPT_LEN {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(
                    serde_json::json!({"error": format!("System prompt exceeds max length ({MAX_PROMPT_LEN} chars)")}),
                ),
            );
        }
    }

    // Validate color format if provided
    if let Some(ref color) = req.color {
        if !color.is_empty() && !color.starts_with('#') {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Color must be a hex code starting with '#'"})),
            );
        }
    }

    // Validate avatar_url if provided
    if let Some(ref url) = req.avatar_url {
        if !url.is_empty()
            && !url.starts_with("http://")
            && !url.starts_with("https://")
            && !url.starts_with("data:")
        {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Avatar URL must be http/https or data URI"})),
            );
        }
    }

    // Update name
    if let Some(ref new_name) = req.name {
        if !new_name.is_empty() {
            if let Err(e) = state
                .kernel
                .registry
                .update_name(agent_id, new_name.clone())
            {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({"error": format!("{e}")})),
                );
            }
        }
    }

    // Update description
    if let Some(ref new_desc) = req.description {
        if state
            .kernel
            .registry
            .update_description(agent_id, new_desc.clone())
            .is_err()
        {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent not found"})),
            );
        }
    }

    // Update system prompt (hot-swap — takes effect on next message)
    if let Some(ref new_prompt) = req.system_prompt {
        if state
            .kernel
            .registry
            .update_system_prompt(agent_id, new_prompt.clone())
            .is_err()
        {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent not found"})),
            );
        }
    }

    // Update identity fields (merge — only overwrite provided fields)
    let has_identity_field = req.emoji.is_some()
        || req.avatar_url.is_some()
        || req.color.is_some()
        || req.archetype.is_some()
        || req.vibe.is_some()
        || req.greeting_style.is_some();

    if has_identity_field {
        // Read current identity, merge with provided fields
        let current = state
            .kernel
            .registry
            .get(agent_id)
            .map(|e| e.identity)
            .unwrap_or_default();
        let merged = AgentIdentity {
            emoji: req.emoji.or(current.emoji),
            avatar_url: req.avatar_url.or(current.avatar_url),
            color: req.color.or(current.color),
            archetype: req.archetype.or(current.archetype),
            vibe: req.vibe.or(current.vibe),
            greeting_style: req.greeting_style.or(current.greeting_style),
        };
        if state
            .kernel
            .registry
            .update_identity(agent_id, merged)
            .is_err()
        {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent not found"})),
            );
        }
    }

    // Update model/provider — use set_agent_model for catalog-based provider
    // resolution when provider is not explicitly provided (fixes #387/#466:
    // changing model from another provider without specifying provider now
    // auto-resolves the correct provider from the model catalog).
    if let Some(ref new_model) = req.model {
        if !new_model.is_empty() {
            if let Some(ref new_provider) = req.provider {
                if !new_provider.is_empty() {
                    // Explicit provider given — still route through set_agent_model
                    // so provider-specific auth/env hints stay in sync.
                    if let Err(e) =
                        state
                            .kernel
                            .set_agent_model(agent_id, new_model, Some(new_provider))
                    {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({"error": format!("{e}")})),
                        );
                    }
                } else {
                    // Provider is empty string — resolve from catalog
                    if let Err(e) = state.kernel.set_agent_model(agent_id, new_model, None) {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({"error": format!("{e}")})),
                        );
                    }
                }
            } else {
                // No provider field at all — resolve from catalog
                if let Err(e) = state.kernel.set_agent_model(agent_id, new_model, None) {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": format!("{e}")})),
                    );
                }
            }
        }
    }

    // Update fallback model chain
    if let Some(fallbacks) = req.fallback_models {
        if state
            .kernel
            .registry
            .update_fallback_models(agent_id, fallbacks)
            .is_err()
        {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent not found"})),
            );
        }
    }

    // Persist updated manifest to database so changes survive restart
    if let Some(entry) = state.kernel.registry.get(agent_id) {
        if let Err(e) = state.kernel.memory.save_agent(&entry) {
            tracing::warn!("Failed to persist agent config update: {e}");
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "ok", "agent_id": id})),
    )
}

// ---------------------------------------------------------------------------
// Agent Cloning
// ---------------------------------------------------------------------------

/// Request body for cloning an agent.
#[derive(serde::Deserialize)]
pub struct CloneAgentRequest {
    pub new_name: String,
}

/// POST /api/agents/{id}/clone — Clone an agent with its workspace files.
pub async fn clone_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<CloneAgentRequest>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            );
        }
    };

    if req.new_name.len() > 256 {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({"error": "Name exceeds max length (256 chars)"})),
        );
    }

    if req.new_name.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "new_name cannot be empty"})),
        );
    }

    let source = match state.kernel.registry.get(agent_id) {
        Some(e) => e,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent not found"})),
            );
        }
    };

    // Deep-clone manifest with new name
    let mut cloned_manifest = source.manifest.clone();
    cloned_manifest.name = req.new_name.clone();
    cloned_manifest.workspace = None; // Let kernel assign a new workspace

    // Spawn the cloned agent
    let new_id = match state.kernel.spawn_agent(cloned_manifest) {
        Ok(id) => id,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Clone spawn failed: {e}")})),
            );
        }
    };

    // Copy workspace files from source to destination
    let new_entry = state.kernel.registry.get(new_id);
    if let (Some(ref src_ws), Some(ref new_entry)) = (source.manifest.workspace, new_entry) {
        if let Some(ref dst_ws) = new_entry.manifest.workspace {
            // Security: canonicalize both paths
            if let (Ok(src_can), Ok(dst_can)) = (src_ws.canonicalize(), dst_ws.canonicalize()) {
                for &fname in KNOWN_IDENTITY_FILES {
                    let src_file = src_can.join(fname);
                    let dst_file = dst_can.join(fname);
                    if src_file.exists() {
                        let _ = std::fs::copy(&src_file, &dst_file);
                    }
                }
            }
        }
    }

    // Copy identity from source
    let _ = state
        .kernel
        .registry
        .update_identity(new_id, source.identity.clone());

    // Register in channel router so binding resolution finds the cloned agent
    if let Some(ref mgr) = *state.bridge_manager.lock().await {
        mgr.router().register_agent(req.new_name.clone(), new_id);
    }

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "agent_id": new_id.to_string(),
            "name": req.new_name,
        })),
    )
}

// ---------------------------------------------------------------------------
// Workspace File Editor endpoints
// ---------------------------------------------------------------------------

/// Whitelisted workspace identity files that can be read/written via API.
const KNOWN_IDENTITY_FILES: &[&str] = &[
    "SOUL.md",
    "IDENTITY.md",
    "USER.md",
    "TOOLS.md",
    "MEMORY.md",
    "AGENTS.md",
    "BOOTSTRAP.md",
    "HEARTBEAT.md",
];

/// GET /api/agents/{id}/files — List workspace identity files.
pub async fn list_agent_files(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            );
        }
    };

    let entry = match state.kernel.registry.get(agent_id) {
        Some(e) => e,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent not found"})),
            );
        }
    };

    let workspace = match entry.manifest.workspace {
        Some(ref ws) => ws.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent has no workspace"})),
            );
        }
    };

    let mut files = Vec::new();
    for &name in KNOWN_IDENTITY_FILES {
        let path = workspace.join(name);
        let (exists, size_bytes) = if path.exists() {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            (true, size)
        } else {
            (false, 0u64)
        };
        files.push(serde_json::json!({
            "name": name,
            "exists": exists,
            "size_bytes": size_bytes,
        }));
    }

    (StatusCode::OK, Json(serde_json::json!({ "files": files })))
}

/// GET /api/agents/{id}/files/{filename} — Read a workspace identity file.
pub async fn get_agent_file(
    State(state): State<Arc<AppState>>,
    Path((id, filename)): Path<(String, String)>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            );
        }
    };

    // Validate filename whitelist
    if !KNOWN_IDENTITY_FILES.contains(&filename.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "File not in whitelist"})),
        );
    }

    let entry = match state.kernel.registry.get(agent_id) {
        Some(e) => e,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent not found"})),
            );
        }
    };

    let workspace = match entry.manifest.workspace {
        Some(ref ws) => ws.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent has no workspace"})),
            );
        }
    };

    // Security: canonicalize and verify stays inside workspace
    let file_path = workspace.join(&filename);
    let canonical = match file_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "File not found"})),
            );
        }
    };
    let ws_canonical = match workspace.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Workspace path error"})),
            );
        }
    };
    if !canonical.starts_with(&ws_canonical) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Path traversal denied"})),
        );
    }

    let content = match std::fs::read_to_string(&canonical) {
        Ok(c) => c,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "File not found"})),
            );
        }
    };

    let size_bytes = content.len();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "name": filename,
            "content": content,
            "size_bytes": size_bytes,
        })),
    )
}

/// Request body for writing a workspace identity file.
#[derive(serde::Deserialize)]
pub struct SetAgentFileRequest {
    pub content: String,
}

/// PUT /api/agents/{id}/files/{filename} — Write a workspace identity file.
pub async fn set_agent_file(
    State(state): State<Arc<AppState>>,
    Path((id, filename)): Path<(String, String)>,
    Json(req): Json<SetAgentFileRequest>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            );
        }
    };

    // Validate filename whitelist
    if !KNOWN_IDENTITY_FILES.contains(&filename.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "File not in whitelist"})),
        );
    }

    // Max 32KB content
    const MAX_FILE_SIZE: usize = 32_768;
    if req.content.len() > MAX_FILE_SIZE {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({"error": "File content too large (max 32KB)"})),
        );
    }

    let entry = match state.kernel.registry.get(agent_id) {
        Some(e) => e,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent not found"})),
            );
        }
    };

    let workspace = match entry.manifest.workspace {
        Some(ref ws) => ws.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent has no workspace"})),
            );
        }
    };

    // Security: verify workspace path and target stays inside it
    let ws_canonical = match workspace.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Workspace path error"})),
            );
        }
    };

    let file_path = workspace.join(&filename);
    // For new files, check the parent directory instead
    let check_path = if file_path.exists() {
        file_path
            .canonicalize()
            .unwrap_or_else(|_| file_path.clone())
    } else {
        // Parent must be inside workspace
        file_path
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .map(|p| p.join(&filename))
            .unwrap_or_else(|| file_path.clone())
    };
    if !check_path.starts_with(&ws_canonical) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Path traversal denied"})),
        );
    }

    // Atomic write: write to .tmp, then rename
    let tmp_path = workspace.join(format!(".{filename}.tmp"));
    if let Err(e) = std::fs::write(&tmp_path, &req.content) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Write failed: {e}")})),
        );
    }
    if let Err(e) = std::fs::rename(&tmp_path, &file_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Rename failed: {e}")})),
        );
    }

    let size_bytes = req.content.len();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "name": filename,
            "size_bytes": size_bytes,
        })),
    )
}

// ---------------------------------------------------------------------------
// File Upload endpoints
// ---------------------------------------------------------------------------

/// Response body for file uploads.
#[derive(serde::Serialize)]
struct UploadResponse {
    file_id: String,
    filename: String,
    content_type: String,
    size: usize,
    /// Transcription text for audio uploads (populated via Whisper STT).
    #[serde(skip_serializing_if = "Option::is_none")]
    transcription: Option<String>,
}

/// Metadata stored alongside uploaded files.
struct UploadMeta {
    #[allow(dead_code)]
    filename: String,
    content_type: String,
}

/// In-memory upload metadata registry.
static UPLOAD_REGISTRY: LazyLock<DashMap<String, UploadMeta>> = LazyLock::new(DashMap::new);

/// Maximum upload size: 10 MB.
const MAX_UPLOAD_SIZE: usize = 10 * 1024 * 1024;

/// Allowed content type prefixes for upload.
const ALLOWED_CONTENT_TYPES: &[&str] = &["image/", "text/", "application/pdf", "audio/"];

fn is_allowed_content_type(ct: &str) -> bool {
    ALLOWED_CONTENT_TYPES
        .iter()
        .any(|prefix| ct.starts_with(prefix))
}

/// POST /api/agents/{id}/upload — Upload a file attachment.
///
/// Accepts raw body bytes. The client must set:
/// - `Content-Type` header (e.g., `image/png`, `text/plain`, `application/pdf`)
/// - `X-Filename` header (original filename)
pub async fn upload_file(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    // Validate agent ID format
    let _agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            );
        }
    };

    // Extract content type
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    if !is_allowed_content_type(&content_type) {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({"error": "Unsupported content type. Allowed: image/*, text/*, audio/*, application/pdf"}),
            ),
        );
    }

    // Extract filename from header
    let filename = headers
        .get("X-Filename")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("upload")
        .to_string();

    // Validate size
    if body.len() > MAX_UPLOAD_SIZE {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(
                serde_json::json!({"error": format!("File too large (max {} MB)", MAX_UPLOAD_SIZE / (1024 * 1024))}),
            ),
        );
    }

    if body.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Empty file body"})),
        );
    }

    // Generate file ID and save
    let file_id = uuid::Uuid::new_v4().to_string();
    let upload_dir = std::env::temp_dir().join("openfang_uploads");
    if let Err(e) = std::fs::create_dir_all(&upload_dir) {
        tracing::warn!("Failed to create upload dir: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to create upload directory"})),
        );
    }

    let file_path = upload_dir.join(&file_id);
    if let Err(e) = std::fs::write(&file_path, &body) {
        tracing::warn!("Failed to write upload: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save file"})),
        );
    }

    let size = body.len();
    UPLOAD_REGISTRY.insert(
        file_id.clone(),
        UploadMeta {
            filename: filename.clone(),
            content_type: content_type.clone(),
        },
    );

    // Auto-transcribe audio uploads using the media engine
    let transcription = if content_type.starts_with("audio/") {
        let attachment = openfang_types::media::MediaAttachment {
            media_type: openfang_types::media::MediaType::Audio,
            mime_type: content_type.clone(),
            source: openfang_types::media::MediaSource::FilePath {
                path: file_path.to_string_lossy().to_string(),
            },
            size_bytes: size as u64,
        };
        match state
            .kernel
            .media_engine
            .transcribe_audio(&attachment)
            .await
        {
            Ok(result) => {
                tracing::info!(chars = result.description.len(), provider = %result.provider, "Audio transcribed");
                Some(result.description)
            }
            Err(e) => {
                tracing::warn!("Audio transcription failed: {e}");
                None
            }
        }
    } else {
        None
    };

    (
        StatusCode::CREATED,
        Json(serde_json::json!(UploadResponse {
            file_id,
            filename,
            content_type,
            size,
            transcription,
        })),
    )
}

/// GET /api/uploads/{file_id} — Serve an uploaded file.
pub async fn serve_upload(Path(file_id): Path<String>) -> impl IntoResponse {
    // Validate file_id is a UUID to prevent path traversal
    if uuid::Uuid::parse_str(&file_id).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            [(
                axum::http::header::CONTENT_TYPE,
                "application/json".to_string(),
            )],
            b"{\"error\":\"Invalid file ID\"}".to_vec(),
        );
    }

    let file_path = std::env::temp_dir().join("openfang_uploads").join(&file_id);

    // Look up metadata from registry; fall back to disk probe for generated images
    // (image_generate saves files without registering in UPLOAD_REGISTRY).
    let content_type = match UPLOAD_REGISTRY.get(&file_id) {
        Some(m) => m.content_type.clone(),
        None => {
            // Infer content type from file magic bytes
            if !file_path.exists() {
                return (
                    StatusCode::NOT_FOUND,
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "application/json".to_string(),
                    )],
                    b"{\"error\":\"File not found\"}".to_vec(),
                );
            }
            "image/png".to_string()
        }
    };

    match std::fs::read(&file_path) {
        Ok(data) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, content_type)],
            data,
        ),
        Err(_) => (
            StatusCode::NOT_FOUND,
            [(
                axum::http::header::CONTENT_TYPE,
                "application/json".to_string(),
            )],
            b"{\"error\":\"File not found on disk\"}".to_vec(),
        ),
    }
}

// ---------------------------------------------------------------------------
// Execution Approval System — backed by kernel.approval_manager
// ---------------------------------------------------------------------------

/// GET /api/approvals — List pending and recent approval requests.
///
/// Transforms field names to match the dashboard template expectations:
/// `action_summary` → `action`, `agent_id` → `agent_name`, `requested_at` → `created_at`.
pub async fn list_approvals(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let pending = state.kernel.approval_manager.list_pending();
    let recent = state.kernel.approval_manager.list_recent(50);

    // Resolve agent names for display
    let registry_agents = state.kernel.registry.list();
    let agent_name_for = |agent_id: &str| {
        registry_agents
            .iter()
            .find(|ag| ag.id.to_string() == agent_id || ag.name == agent_id)
            .map(|ag| ag.name.clone())
            .unwrap_or_else(|| agent_id.to_string())
    };

    let mut approvals: Vec<serde_json::Value> = pending
        .into_iter()
        .map(|a| {
            let agent_name = agent_name_for(&a.agent_id);
            serde_json::json!({
                "id": a.id,
                "agent_id": a.agent_id,
                "agent_name": agent_name,
                "tool_name": a.tool_name,
                "description": a.description,
                "action_summary": a.action_summary,
                "action": a.action_summary,
                "risk_level": a.risk_level,
                "requested_at": a.requested_at,
                "created_at": a.requested_at,
                "timeout_secs": a.timeout_secs,
                "status": "pending"
            })
        })
        .collect();

    approvals.extend(recent.into_iter().map(|record| {
        let request = record.request;
        let agent_name = agent_name_for(&request.agent_id);
        let status = match record.decision {
            openfang_types::approval::ApprovalDecision::Approved => "approved",
            openfang_types::approval::ApprovalDecision::Denied => "rejected",
            openfang_types::approval::ApprovalDecision::TimedOut => "expired",
        };
        serde_json::json!({
            "id": request.id,
            "agent_id": request.agent_id,
            "agent_name": agent_name,
            "tool_name": request.tool_name,
            "description": request.description,
            "action_summary": request.action_summary,
            "action": request.action_summary,
            "risk_level": request.risk_level,
            "requested_at": request.requested_at,
            "created_at": request.requested_at,
            "timeout_secs": request.timeout_secs,
            "status": status,
            "decided_at": record.decided_at,
            "decided_by": record.decided_by,
        })
    }));

    approvals.sort_by(|a, b| {
        let a_pending = a["status"].as_str() == Some("pending");
        let b_pending = b["status"].as_str() == Some("pending");
        b_pending
            .cmp(&a_pending)
            .then_with(|| b["created_at"].as_str().cmp(&a["created_at"].as_str()))
    });

    let total = approvals.len();

    Json(serde_json::json!({"approvals": approvals, "total": total}))
}

/// POST /api/approvals — Create a manual approval request (for external systems).
///
/// Note: Most approval requests are created automatically by the tool_runner
/// when an agent invokes a tool that requires approval. This endpoint exists
/// for external integrations that need to inject approval gates.
#[derive(serde::Deserialize)]
pub struct CreateApprovalRequest {
    pub agent_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub action_summary: String,
}

pub async fn create_approval(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateApprovalRequest>,
) -> impl IntoResponse {
    use openfang_types::approval::{ApprovalRequest, RiskLevel};

    let policy = state.kernel.approval_manager.policy();
    let id = uuid::Uuid::new_v4();
    let approval_req = ApprovalRequest {
        id,
        agent_id: req.agent_id,
        tool_name: req.tool_name.clone(),
        description: if req.description.is_empty() {
            format!("Manual approval request for {}", req.tool_name)
        } else {
            req.description
        },
        action_summary: if req.action_summary.is_empty() {
            req.tool_name.clone()
        } else {
            req.action_summary
        },
        risk_level: RiskLevel::High,
        requested_at: chrono::Utc::now(),
        timeout_secs: policy.timeout_secs,
    };

    // Spawn the request in the background (it will block until resolved or timed out)
    let kernel = Arc::clone(&state.kernel);
    tokio::spawn(async move {
        kernel.approval_manager.request_approval(approval_req).await;
    });

    (
        StatusCode::CREATED,
        Json(serde_json::json!({"id": id.to_string(), "status": "pending"})),
    )
}

/// POST /api/approvals/{id}/approve — Approve a pending request.
pub async fn approve_request(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let uuid = match uuid::Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid approval ID"})),
            );
        }
    };

    match state.kernel.approval_manager.resolve(
        uuid,
        openfang_types::approval::ApprovalDecision::Approved,
        Some("api".to_string()),
    ) {
        Ok(resp) => (
            StatusCode::OK,
            Json(
                serde_json::json!({"id": id, "status": "approved", "decided_at": resp.decided_at.to_rfc3339()}),
            ),
        ),
        Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": e}))),
    }
}

/// POST /api/approvals/{id}/reject — Reject a pending request.
pub async fn reject_request(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let uuid = match uuid::Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid approval ID"})),
            );
        }
    };

    match state.kernel.approval_manager.resolve(
        uuid,
        openfang_types::approval::ApprovalDecision::Denied,
        Some("api".to_string()),
    ) {
        Ok(resp) => (
            StatusCode::OK,
            Json(
                serde_json::json!({"id": id, "status": "rejected", "decided_at": resp.decided_at.to_rfc3339()}),
            ),
        ),
        Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": e}))),
    }
}

// ---------------------------------------------------------------------------
// Config Reload endpoint
// ---------------------------------------------------------------------------

/// POST /api/config/reload — Reload configuration from disk and apply hot-reloadable changes.
///
/// Reads the config file, diffs against current config, validates the new config,
/// and applies hot-reloadable actions (approval policy, cron limits, etc.).
/// Returns the reload plan showing what changed and what was applied.
pub async fn config_reload(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // SECURITY: Record config reload in audit trail
    state.kernel.audit_log.record(
        "system",
        openfang_runtime::audit::AuditAction::ConfigChange,
        "config reload requested via API",
        "pending",
    );
    match state.kernel.reload_config() {
        Ok(plan) => {
            let status = if plan.restart_required {
                "partial"
            } else if plan.has_changes() {
                "applied"
            } else {
                "no_changes"
            };

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": status,
                    "restart_required": plan.restart_required,
                    "restart_reasons": plan.restart_reasons,
                    "hot_actions_applied": plan.hot_actions.iter().map(|a| format!("{a:?}")).collect::<Vec<_>>(),
                    "noop_changes": plan.noop_changes,
                })),
            )
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"status": "error", "error": e})),
        ),
    }
}

// ---------------------------------------------------------------------------
// Config Schema endpoint
// ---------------------------------------------------------------------------

/// GET /api/config/schema — Return a simplified JSON description of the config structure.
pub async fn config_schema(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Build provider/model options from model catalog for dropdowns
    let catalog = state
        .kernel
        .model_catalog
        .read()
        .unwrap_or_else(|e| e.into_inner());
    let provider_options: Vec<String> = catalog
        .list_providers()
        .iter()
        .map(|p| p.id.clone())
        .collect();
    let model_options: Vec<serde_json::Value> = catalog
        .list_models()
        .iter()
        .map(|m| serde_json::json!({"id": m.id, "name": m.display_name, "provider": m.provider}))
        .collect();
    drop(catalog);

    // Helper: normalize field definitions to objects with {name, type, label}
    // so the frontend template can iterate and render inputs correctly.
    let f = |name: &str, ftype: &str, label: &str| -> serde_json::Value {
        serde_json::json!({"name": name, "type": ftype, "label": label})
    };

    Json(serde_json::json!({
        "sections": {
            "general": {
                "root_level": true,
                "fields": [
                    f("api_listen", "string", "API Listen Address"),
                    f("api_key", "string", "API Key"),
                    f("log_level", "string", "Log Level")
                ]
            },
            "default_model": {
                "hot_reloadable": true,
                "fields": [
                    { "name": "provider", "type": "select", "label": "Provider", "options": provider_options },
                    { "name": "model", "type": "select", "label": "Model", "options": model_options },
                    f("api_key_env", "string", "API Key Env Var"),
                    f("base_url", "string", "Base URL")
                ]
            },
            "memory": {
                "fields": [
                    f("decay_rate", "number", "Decay Rate"),
                    f("vector_dims", "number", "Vector Dimensions")
                ]
            },
            "web": {
                "fields": [
                    f("provider", "string", "Search Provider"),
                    f("timeout_secs", "number", "Timeout (seconds)"),
                    f("max_results", "number", "Max Results")
                ]
            },
            "browser": {
                "fields": [
                    f("headless", "boolean", "Headless Mode"),
                    f("timeout_secs", "number", "Timeout (seconds)"),
                    f("executable_path", "string", "Chrome/Chromium Path")
                ]
            },
            "network": {
                "fields": [
                    f("enabled", "boolean", "Enable OFP Network"),
                    f("listen_addr", "string", "Listen Address"),
                    f("shared_secret", "string", "Shared Secret")
                ]
            },
            "extensions": {
                "fields": [
                    f("auto_connect", "boolean", "Auto Connect"),
                    f("health_check_interval_secs", "number", "Health Check Interval (s)")
                ]
            },
            "vault": {
                "fields": [
                    f("path", "string", "Vault Path")
                ]
            },
            "a2a": {
                "fields": [
                    f("enabled", "boolean", "Enable A2A"),
                    f("name", "string", "Agent Name"),
                    f("description", "string", "Description"),
                    f("url", "string", "URL")
                ]
            },
            "channels": {
                "fields": [
                    f("telegram", "object", "Telegram"),
                    f("discord", "object", "Discord"),
                    f("slack", "object", "Slack"),
                    f("whatsapp", "object", "WhatsApp")
                ]
            }
        }
    }))
}

// ---------------------------------------------------------------------------
// Config Set endpoint
// ---------------------------------------------------------------------------

/// POST /api/config/set — Set a single config value and persist to config.toml.
///
/// Accepts JSON `{ "path": "section.key", "value": "..." }`.
/// Writes the value to the TOML config file and triggers a reload.
pub async fn config_set(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let path = match body.get("path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"status": "error", "error": "missing 'path' field"})),
            );
        }
    };
    let value = match body.get("value") {
        Some(v) => v.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"status": "error", "error": "missing 'value' field"})),
            );
        }
    };

    let config_path = state.kernel.config.home_dir.join("config.toml");

    // Read existing config as a TOML table, or start fresh
    let mut table: toml::value::Table = if config_path.exists() {
        match std::fs::read_to_string(&config_path) {
            Ok(content) => toml::from_str(&content).unwrap_or_default(),
            Err(_) => toml::value::Table::new(),
        }
    } else {
        toml::value::Table::new()
    };

    // Convert JSON value to TOML value
    let toml_val = json_to_toml_value(&value);

    // Parse "section.key" path and set value
    let parts: Vec<&str> = path.split('.').collect();
    match parts.len() {
        1 => {
            table.insert(parts[0].to_string(), toml_val);
        }
        2 => {
            let section = table
                .entry(parts[0].to_string())
                .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
            if let toml::Value::Table(ref mut t) = section {
                t.insert(parts[1].to_string(), toml_val);
            }
        }
        3 => {
            let section = table
                .entry(parts[0].to_string())
                .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
            if let toml::Value::Table(ref mut t) = section {
                let sub = t
                    .entry(parts[1].to_string())
                    .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
                if let toml::Value::Table(ref mut t2) = sub {
                    t2.insert(parts[2].to_string(), toml_val);
                }
            }
        }
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(
                    serde_json::json!({"status": "error", "error": "path too deep (max 3 levels)"}),
                ),
            );
        }
    }

    // Write back
    let toml_string = match toml::to_string_pretty(&table) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    serde_json::json!({"status": "error", "error": format!("serialize failed: {e}")}),
                ),
            );
        }
    };
    if let Err(e) = std::fs::write(&config_path, &toml_string) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"status": "error", "error": format!("write failed: {e}")})),
        );
    }

    // Trigger reload
    let reload_status = match state.kernel.reload_config() {
        Ok(plan) => {
            if plan.restart_required {
                "applied_partial"
            } else {
                "applied"
            }
        }
        Err(_) => "saved_reload_failed",
    };

    state.kernel.audit_log.record(
        "system",
        openfang_runtime::audit::AuditAction::ConfigChange,
        format!("config set: {path}"),
        "completed",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({"status": reload_status, "path": path})),
    )
}

/// Convert a serde_json::Value to a toml::Value.
fn json_to_toml_value(value: &serde_json::Value) -> toml::Value {
    match value {
        serde_json::Value::String(s) => toml::Value::String(s.clone()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_u64() {
                toml::Value::Integer(i as i64)
            } else if let Some(i) = n.as_i64() {
                toml::Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                toml::Value::Float(f)
            } else {
                toml::Value::String(n.to_string())
            }
        }
        serde_json::Value::Bool(b) => toml::Value::Boolean(*b),
        _ => toml::Value::String(value.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Delivery tracking endpoints
// ---------------------------------------------------------------------------

/// GET /api/agents/:id/deliveries — List recent delivery receipts for an agent.
pub async fn get_agent_deliveries(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            // Try name lookup
            match state.kernel.registry.find_by_name(&id) {
                Some(entry) => entry.id,
                None => {
                    return (
                        StatusCode::NOT_FOUND,
                        Json(serde_json::json!({"error": "Agent not found"})),
                    );
                }
            }
        }
    };

    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(50)
        .min(500);

    let receipts = state.kernel.delivery_tracker.get_receipts(agent_id, limit);
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "agent_id": agent_id.to_string(),
            "count": receipts.len(),
            "receipts": receipts,
        })),
    )
}

// ---------------------------------------------------------------------------
// Cron job management endpoints
// ---------------------------------------------------------------------------

/// GET /api/cron/jobs — List all cron jobs, optionally filtered by agent_id.
pub async fn list_cron_jobs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let jobs = if let Some(agent_id_str) = params.get("agent_id") {
        match uuid::Uuid::parse_str(agent_id_str) {
            Ok(uuid) => {
                let aid = AgentId(uuid);
                state.kernel.cron_scheduler.list_jobs(aid)
            }
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Invalid agent_id"})),
                );
            }
        }
    } else {
        state.kernel.cron_scheduler.list_all_jobs()
    };
    let total = jobs.len();
    let jobs_json: Vec<serde_json::Value> = jobs
        .into_iter()
        .map(|j| serde_json::to_value(&j).unwrap_or_default())
        .collect();
    (
        StatusCode::OK,
        Json(serde_json::json!({"jobs": jobs_json, "total": total})),
    )
}

/// POST /api/cron/jobs — Create a new cron job.
pub async fn create_cron_job(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let agent_id = body["agent_id"].as_str().unwrap_or("");
    match state.kernel.cron_create(agent_id, body.clone()).await {
        Ok(result) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"result": result})),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        ),
    }
}

/// DELETE /api/cron/jobs/{id} — Delete a cron job.
pub async fn delete_cron_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match uuid::Uuid::parse_str(&id) {
        Ok(uuid) => {
            let job_id = openfang_types::scheduler::CronJobId(uuid);
            match state.kernel.cron_scheduler.remove_job(job_id) {
                Ok(_) => {
                    let _ = state.kernel.cron_scheduler.persist();
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({"status": "deleted"})),
                    )
                }
                Err(e) => (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": format!("{e}")})),
                ),
            }
        }
        Err(_) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid job ID"})),
        ),
    }
}

/// PUT /api/cron/jobs/{id}/enable — Enable or disable a cron job.
pub async fn toggle_cron_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let enabled = body["enabled"].as_bool().unwrap_or(true);
    match uuid::Uuid::parse_str(&id) {
        Ok(uuid) => {
            let job_id = openfang_types::scheduler::CronJobId(uuid);
            match state.kernel.cron_scheduler.set_enabled(job_id, enabled) {
                Ok(()) => {
                    let _ = state.kernel.cron_scheduler.persist();
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({"id": id, "enabled": enabled})),
                    )
                }
                Err(e) => (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": format!("{e}")})),
                ),
            }
        }
        Err(_) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid job ID"})),
        ),
    }
}

/// GET /api/cron/jobs/{id}/status — Get status of a specific cron job.
pub async fn cron_job_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match uuid::Uuid::parse_str(&id) {
        Ok(uuid) => {
            let job_id = openfang_types::scheduler::CronJobId(uuid);
            match state.kernel.cron_scheduler.get_meta(job_id) {
                Some(meta) => (
                    StatusCode::OK,
                    Json(serde_json::to_value(&meta).unwrap_or_default()),
                ),
                None => (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "Job not found"})),
                ),
            }
        }
        Err(_) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid job ID"})),
        ),
    }
}

// ---------------------------------------------------------------------------
// Webhook trigger endpoints
// ---------------------------------------------------------------------------

/// POST /hooks/wake — Inject a system event via webhook trigger.
///
/// Publishes a custom event through the kernel's event system, which can
/// trigger proactive agents that subscribe to the event type.
pub async fn webhook_wake(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<openfang_types::webhook::WakePayload>,
) -> impl IntoResponse {
    // Check if webhook triggers are enabled
    let wh_config = match &state.kernel.config.webhook_triggers {
        Some(c) if c.enabled => c,
        _ => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Webhook triggers not enabled"})),
            );
        }
    };

    // Validate bearer token (constant-time comparison)
    if !validate_webhook_token(&headers, &wh_config.token_env) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid or missing token"})),
        );
    }

    // Validate payload
    if let Err(e) = body.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        );
    }

    // Publish through the kernel's publish_event (KernelHandle trait), which
    // goes through the full event processing pipeline including trigger evaluation.
    let event_payload = serde_json::json!({
        "source": "webhook",
        "mode": body.mode,
        "text": body.text,
    });
    if let Err(e) =
        KernelHandle::publish_event(state.kernel.as_ref(), "webhook.wake", event_payload).await
    {
        tracing::warn!("Webhook wake event publish failed: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Event publish failed: {e}")})),
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "accepted", "mode": body.mode})),
    )
}

/// POST /hooks/agent — Run an isolated agent turn via webhook.
///
/// Sends a message directly to the specified agent and returns the response.
/// This enables external systems (CI/CD, Slack, etc.) to trigger agent work.
pub async fn webhook_agent(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<openfang_types::webhook::AgentHookPayload>,
) -> impl IntoResponse {
    // Check if webhook triggers are enabled
    let wh_config = match &state.kernel.config.webhook_triggers {
        Some(c) if c.enabled => c,
        _ => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Webhook triggers not enabled"})),
            );
        }
    };

    // Validate bearer token
    if !validate_webhook_token(&headers, &wh_config.token_env) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid or missing token"})),
        );
    }

    // Validate payload
    if let Err(e) = body.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        );
    }

    // Resolve the agent by name or ID (if not specified, use the first running agent)
    let agent_id: AgentId = match &body.agent {
        Some(agent_ref) => match agent_ref.parse() {
            Ok(id) => id,
            Err(_) => {
                // Try name lookup
                match state.kernel.registry.find_by_name(agent_ref) {
                    Some(entry) => entry.id,
                    None => {
                        return (
                            StatusCode::NOT_FOUND,
                            Json(
                                serde_json::json!({"error": format!("Agent not found: {}", agent_ref)}),
                            ),
                        );
                    }
                }
            }
        },
        None => {
            // No agent specified — use the first available agent
            match state.kernel.registry.list().first() {
                Some(entry) => entry.id,
                None => {
                    return (
                        StatusCode::NOT_FOUND,
                        Json(serde_json::json!({"error": "No agents available"})),
                    );
                }
            }
        }
    };

    // Actually send the message to the agent and get the response
    match state.kernel.send_message(agent_id, &body.message).await {
        Ok(result) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "completed",
                "agent_id": agent_id.to_string(),
                "response": result.response,
                "usage": {
                    "input_tokens": result.total_usage.input_tokens,
                    "output_tokens": result.total_usage.output_tokens,
                },
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Agent execution failed: {e}")})),
        ),
    }
}

// ─── Agent Bindings API ────────────────────────────────────────────────

/// GET /api/bindings — List all agent bindings.
pub async fn list_bindings(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let bindings = state.kernel.list_bindings();
    (
        StatusCode::OK,
        Json(serde_json::json!({ "bindings": bindings })),
    )
}

/// POST /api/bindings — Add a new agent binding.
pub async fn add_binding(
    State(state): State<Arc<AppState>>,
    Json(binding): Json<openfang_types::config::AgentBinding>,
) -> impl IntoResponse {
    // Validate agent exists
    let agents = state.kernel.registry.list();
    let agent_exists = agents.iter().any(|e| e.name == binding.agent)
        || binding.agent.parse::<uuid::Uuid>().is_ok();
    if !agent_exists {
        tracing::warn!(agent = %binding.agent, "Binding references unknown agent");
    }

    state.kernel.add_binding(binding);
    (
        StatusCode::CREATED,
        Json(serde_json::json!({ "status": "created" })),
    )
}

/// DELETE /api/bindings/:index — Remove a binding by index.
pub async fn remove_binding(
    State(state): State<Arc<AppState>>,
    Path(index): Path<usize>,
) -> impl IntoResponse {
    match state.kernel.remove_binding(index) {
        Some(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "removed" })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Binding index out of range" })),
        ),
    }
}

// ─── Device Pairing endpoints ───────────────────────────────────────────

/// POST /api/pairing/request — Create a new pairing request (returns token + QR URI).
pub async fn pairing_request(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if !state.kernel.config.pairing.enabled {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Pairing not enabled"})),
        )
            .into_response();
    }
    match state.kernel.pairing.create_pairing_request() {
        Ok(req) => {
            let qr_uri = format!("openfang://pair?token={}", req.token);
            Json(serde_json::json!({
                "token": req.token,
                "qr_uri": qr_uri,
                "expires_at": req.expires_at.to_rfc3339(),
            }))
            .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

/// POST /api/pairing/complete — Complete pairing with token + device info.
pub async fn pairing_complete(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if !state.kernel.config.pairing.enabled {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Pairing not enabled"})),
        )
            .into_response();
    }
    let token = body.get("token").and_then(|v| v.as_str()).unwrap_or("");
    let display_name = body
        .get("display_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let platform = body
        .get("platform")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let push_token = body
        .get("push_token")
        .and_then(|v| v.as_str())
        .map(String::from);
    let device_info = openfang_kernel::pairing::PairedDevice {
        device_id: uuid::Uuid::new_v4().to_string(),
        display_name: display_name.to_string(),
        platform: platform.to_string(),
        paired_at: chrono::Utc::now(),
        last_seen: chrono::Utc::now(),
        push_token,
    };
    match state.kernel.pairing.complete_pairing(token, device_info) {
        Ok(device) => Json(serde_json::json!({
            "device_id": device.device_id,
            "display_name": device.display_name,
            "platform": device.platform,
            "paired_at": device.paired_at.to_rfc3339(),
        }))
        .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

/// GET /api/pairing/devices — List paired devices.
pub async fn pairing_devices(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if !state.kernel.config.pairing.enabled {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Pairing not enabled"})),
        )
            .into_response();
    }
    let devices: Vec<_> = state
        .kernel
        .pairing
        .list_devices()
        .into_iter()
        .map(|d| {
            serde_json::json!({
                "device_id": d.device_id,
                "display_name": d.display_name,
                "platform": d.platform,
                "paired_at": d.paired_at.to_rfc3339(),
                "last_seen": d.last_seen.to_rfc3339(),
            })
        })
        .collect();
    Json(serde_json::json!({"devices": devices})).into_response()
}

/// DELETE /api/pairing/devices/{id} — Remove a paired device.
pub async fn pairing_remove_device(
    State(state): State<Arc<AppState>>,
    Path(device_id): Path<String>,
) -> impl IntoResponse {
    if !state.kernel.config.pairing.enabled {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Pairing not enabled"})),
        )
            .into_response();
    }
    match state.kernel.pairing.remove_device(&device_id) {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

/// POST /api/pairing/notify — Push a notification to all paired devices.
pub async fn pairing_notify(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if !state.kernel.config.pairing.enabled {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Pairing not enabled"})),
        )
            .into_response();
    }
    let title = body
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("OpenFang");
    let message = body.get("message").and_then(|v| v.as_str()).unwrap_or("");
    if message.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "message is required"})),
        )
            .into_response();
    }
    state.kernel.pairing.notify_devices(title, message).await;
    Json(serde_json::json!({"ok": true, "notified": state.kernel.pairing.list_devices().len()}))
        .into_response()
}

/// GET /api/commands — List available chat commands (for dynamic slash menu).
pub async fn list_commands(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut commands = vec![
        serde_json::json!({"cmd": "/help", "desc": "Show available commands"}),
        serde_json::json!({"cmd": "/new", "desc": "Reset session (clear history)"}),
        serde_json::json!({"cmd": "/compact", "desc": "Trigger LLM session compaction"}),
        serde_json::json!({"cmd": "/model", "desc": "Show or switch model (/model [name])"}),
        serde_json::json!({"cmd": "/stop", "desc": "Cancel current agent run"}),
        serde_json::json!({"cmd": "/usage", "desc": "Show session token usage & cost"}),
        serde_json::json!({"cmd": "/think", "desc": "Toggle extended thinking (/think [on|off|stream])"}),
        serde_json::json!({"cmd": "/context", "desc": "Show context window usage & pressure"}),
        serde_json::json!({"cmd": "/verbose", "desc": "Cycle tool detail level (/verbose [off|on|full])"}),
        serde_json::json!({"cmd": "/queue", "desc": "Check if agent is processing"}),
        serde_json::json!({"cmd": "/status", "desc": "Show system status"}),
        serde_json::json!({"cmd": "/clear", "desc": "Clear chat display"}),
        serde_json::json!({"cmd": "/exit", "desc": "Disconnect from agent"}),
    ];

    // Add skill-registered tool names as potential commands
    if let Ok(registry) = state.kernel.skill_registry.read() {
        for skill in registry.list() {
            let desc: String = skill.manifest.skill.description.chars().take(80).collect();
            commands.push(serde_json::json!({
                "cmd": format!("/{}", skill.manifest.skill.name),
                "desc": if desc.is_empty() { format!("Skill: {}", skill.manifest.skill.name) } else { desc },
                "source": "skill",
            }));
        }
    }

    Json(serde_json::json!({"commands": commands}))
}

/// SECURITY: Validate webhook bearer token using constant-time comparison.
fn validate_webhook_token(headers: &axum::http::HeaderMap, token_env: &str) -> bool {
    let expected = match std::env::var(token_env) {
        Ok(t) if t.len() >= 32 => t,
        _ => return false,
    };

    let provided = match headers.get("authorization") {
        Some(v) => match v.to_str() {
            Ok(s) if s.starts_with("Bearer ") => &s[7..],
            _ => return false,
        },
        None => return false,
    };

    use subtle::ConstantTimeEq;
    if provided.len() != expected.len() {
        return false;
    }
    provided.as_bytes().ct_eq(expected.as_bytes()).into()
}

// ══════════════════════════════════════════════════════════════════════
// GitHub Copilot OAuth Device Flow
// ══════════════════════════════════════════════════════════════════════

/// State for an in-progress device flow.
struct CopilotFlowState {
    device_code: String,
    interval: u64,
    expires_at: Instant,
}

/// Active device flows, keyed by poll_id. Auto-expire after the flow's TTL.
static COPILOT_FLOWS: LazyLock<DashMap<String, CopilotFlowState>> = LazyLock::new(DashMap::new);

/// POST /api/providers/github-copilot/oauth/start
///
/// Initiates a GitHub device flow for Copilot authentication.
/// Returns a user code and verification URI that the user visits in their browser.
pub async fn copilot_oauth_start() -> impl IntoResponse {
    // Clean up expired flows first
    COPILOT_FLOWS.retain(|_, state| state.expires_at > Instant::now());

    match openfang_runtime::copilot_oauth::start_device_flow().await {
        Ok(resp) => {
            let poll_id = uuid::Uuid::new_v4().to_string();

            COPILOT_FLOWS.insert(
                poll_id.clone(),
                CopilotFlowState {
                    device_code: resp.device_code,
                    interval: resp.interval,
                    expires_at: Instant::now() + std::time::Duration::from_secs(resp.expires_in),
                },
            );

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "user_code": resp.user_code,
                    "verification_uri": resp.verification_uri,
                    "poll_id": poll_id,
                    "expires_in": resp.expires_in,
                    "interval": resp.interval,
                })),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        ),
    }
}

/// GET /api/providers/github-copilot/oauth/poll/{poll_id}
///
/// Poll the status of a GitHub device flow.
/// Returns `pending`, `complete`, `expired`, `denied`, or `error`.
/// On `complete`, saves the token to secrets.env and sets GITHUB_TOKEN.
pub async fn copilot_oauth_poll(
    State(state): State<Arc<AppState>>,
    Path(poll_id): Path<String>,
) -> impl IntoResponse {
    let flow = match COPILOT_FLOWS.get(&poll_id) {
        Some(f) => f,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"status": "not_found", "error": "Unknown poll_id"})),
            )
        }
    };

    if flow.expires_at <= Instant::now() {
        drop(flow);
        COPILOT_FLOWS.remove(&poll_id);
        return (
            StatusCode::OK,
            Json(serde_json::json!({"status": "expired"})),
        );
    }

    let device_code = flow.device_code.clone();
    drop(flow);

    match openfang_runtime::copilot_oauth::poll_device_flow(&device_code).await {
        openfang_runtime::copilot_oauth::DeviceFlowStatus::Pending => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "pending"})),
        ),
        openfang_runtime::copilot_oauth::DeviceFlowStatus::Complete { access_token } => {
            // Store in vault (best-effort)
            state.kernel.store_credential("GITHUB_TOKEN", &access_token);

            // Save to secrets.env (dual-write)
            let secrets_path = state.kernel.config.home_dir.join("secrets.env");
            if let Err(e) = write_secret_env(&secrets_path, "GITHUB_TOKEN", &access_token) {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(
                        serde_json::json!({"status": "error", "error": format!("Failed to save token: {e}")}),
                    ),
                );
            }

            // Set in current process
            std::env::set_var("GITHUB_TOKEN", access_token.as_str());

            // Refresh auth detection
            state
                .kernel
                .model_catalog
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .detect_auth();

            // Clean up flow state
            COPILOT_FLOWS.remove(&poll_id);

            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "complete"})),
            )
        }
        openfang_runtime::copilot_oauth::DeviceFlowStatus::SlowDown { new_interval } => {
            // Update interval
            if let Some(mut f) = COPILOT_FLOWS.get_mut(&poll_id) {
                f.interval = new_interval;
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "pending", "interval": new_interval})),
            )
        }
        openfang_runtime::copilot_oauth::DeviceFlowStatus::Expired => {
            COPILOT_FLOWS.remove(&poll_id);
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "expired"})),
            )
        }
        openfang_runtime::copilot_oauth::DeviceFlowStatus::AccessDenied => {
            COPILOT_FLOWS.remove(&poll_id);
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "denied"})),
            )
        }
        openfang_runtime::copilot_oauth::DeviceFlowStatus::Error(e) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "error", "error": e})),
        ),
    }
}

// ---------------------------------------------------------------------------
// Agent Communication (Comms) endpoints
// ---------------------------------------------------------------------------

/// GET /api/comms/topology — Build agent topology graph from registry.
pub async fn comms_topology(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    use openfang_types::comms::{EdgeKind, TopoEdge, TopoNode, Topology};

    let agents = state.kernel.registry.list();

    let nodes: Vec<TopoNode> = agents
        .iter()
        .map(|e| TopoNode {
            id: e.id.to_string(),
            name: e.name.clone(),
            state: format!("{:?}", e.state),
            model: e.manifest.model.model.clone(),
        })
        .collect();

    let mut edges: Vec<TopoEdge> = Vec::new();

    // Parent-child edges from registry
    for agent in &agents {
        for child_id in &agent.children {
            edges.push(TopoEdge {
                from: agent.id.to_string(),
                to: child_id.to_string(),
                kind: EdgeKind::ParentChild,
            });
        }
    }

    // Peer message edges from event bus history
    let events = state.kernel.event_bus.history(500).await;
    let mut peer_pairs = std::collections::HashSet::new();
    for event in &events {
        if let openfang_types::event::EventPayload::Message(_) = &event.payload {
            if let openfang_types::event::EventTarget::Agent(target_id) = &event.target {
                let from = event.source.to_string();
                let to = target_id.to_string();
                // Deduplicate: only one edge per pair, skip self-loops
                if from != to {
                    let key = if from < to {
                        (from.clone(), to.clone())
                    } else {
                        (to.clone(), from.clone())
                    };
                    if peer_pairs.insert(key) {
                        edges.push(TopoEdge {
                            from,
                            to,
                            kind: EdgeKind::Peer,
                        });
                    }
                }
            }
        }
    }

    Json(serde_json::to_value(Topology { nodes, edges }).unwrap_or_default())
}

/// Filter a kernel event into a CommsEvent, if it represents inter-agent communication.
fn filter_to_comms_event(
    event: &openfang_types::event::Event,
    agents: &[openfang_types::agent::AgentEntry],
) -> Option<openfang_types::comms::CommsEvent> {
    use openfang_types::comms::{CommsEvent, CommsEventKind};
    use openfang_types::event::{EventPayload, EventTarget, LifecycleEvent};

    let resolve_name = |id: &str| -> String {
        agents
            .iter()
            .find(|a| a.id.to_string() == id)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| id.to_string())
    };

    match &event.payload {
        EventPayload::Message(msg) => {
            let target_id = match &event.target {
                EventTarget::Agent(id) => id.to_string(),
                _ => String::new(),
            };
            Some(CommsEvent {
                id: event.id.to_string(),
                timestamp: event.timestamp.to_rfc3339(),
                kind: CommsEventKind::AgentMessage,
                source_id: event.source.to_string(),
                source_name: resolve_name(&event.source.to_string()),
                target_id: target_id.clone(),
                target_name: resolve_name(&target_id),
                detail: openfang_types::truncate_str(&msg.content, 200).to_string(),
            })
        }
        EventPayload::Lifecycle(lifecycle) => match lifecycle {
            LifecycleEvent::Spawned { agent_id, name } => Some(CommsEvent {
                id: event.id.to_string(),
                timestamp: event.timestamp.to_rfc3339(),
                kind: CommsEventKind::AgentSpawned,
                source_id: event.source.to_string(),
                source_name: resolve_name(&event.source.to_string()),
                target_id: agent_id.to_string(),
                target_name: name.clone(),
                detail: format!("Agent '{}' spawned", name),
            }),
            LifecycleEvent::Terminated { agent_id, reason } => Some(CommsEvent {
                id: event.id.to_string(),
                timestamp: event.timestamp.to_rfc3339(),
                kind: CommsEventKind::AgentTerminated,
                source_id: event.source.to_string(),
                source_name: resolve_name(&event.source.to_string()),
                target_id: agent_id.to_string(),
                target_name: resolve_name(&agent_id.to_string()),
                detail: format!("Terminated: {}", reason),
            }),
            _ => None,
        },
        _ => None,
    }
}

/// Convert an audit entry into a CommsEvent if it represents inter-agent activity.
fn audit_to_comms_event(
    entry: &openfang_runtime::audit::AuditEntry,
    agents: &[openfang_types::agent::AgentEntry],
) -> Option<openfang_types::comms::CommsEvent> {
    use openfang_types::comms::{CommsEvent, CommsEventKind};

    let resolve_name = |id: &str| -> String {
        agents
            .iter()
            .find(|a| a.id.to_string() == id)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| {
                if id.is_empty() || id == "system" {
                    "system".to_string()
                } else {
                    openfang_types::truncate_str(id, 12).to_string()
                }
            })
    };

    let action_str = format!("{:?}", entry.action);
    let (kind, detail, target_label) = match action_str.as_str() {
        "AgentMessage" => {
            // Format detail: "tokens_in=X, tokens_out=Y" → readable summary
            let detail = if entry.detail.starts_with("tokens_in=") {
                let parts: Vec<&str> = entry.detail.split(", ").collect();
                let in_tok = parts
                    .first()
                    .and_then(|p| p.strip_prefix("tokens_in="))
                    .unwrap_or("?");
                let out_tok = parts
                    .get(1)
                    .and_then(|p| p.strip_prefix("tokens_out="))
                    .unwrap_or("?");
                if entry.outcome == "ok" {
                    format!("{} in / {} out tokens", in_tok, out_tok)
                } else {
                    format!(
                        "{} in / {} out — {}",
                        in_tok,
                        out_tok,
                        openfang_types::truncate_str(&entry.outcome, 80)
                    )
                }
            } else if entry.outcome != "ok" {
                format!(
                    "{} — {}",
                    openfang_types::truncate_str(&entry.detail, 80),
                    openfang_types::truncate_str(&entry.outcome, 80)
                )
            } else {
                openfang_types::truncate_str(&entry.detail, 200).to_string()
            };
            (CommsEventKind::AgentMessage, detail, "user")
        }
        "AgentSpawn" => (
            CommsEventKind::AgentSpawned,
            format!(
                "Agent spawned: {}",
                openfang_types::truncate_str(&entry.detail, 100)
            ),
            "",
        ),
        "AgentKill" => (
            CommsEventKind::AgentTerminated,
            format!(
                "Agent killed: {}",
                openfang_types::truncate_str(&entry.detail, 100)
            ),
            "",
        ),
        _ => return None,
    };

    Some(CommsEvent {
        id: format!("audit-{}", entry.seq),
        timestamp: entry.timestamp.clone(),
        kind,
        source_id: entry.agent_id.clone(),
        source_name: resolve_name(&entry.agent_id),
        target_id: if target_label.is_empty() {
            String::new()
        } else {
            target_label.to_string()
        },
        target_name: if target_label.is_empty() {
            String::new()
        } else {
            target_label.to_string()
        },
        detail,
    })
}

/// GET /api/comms/events — Return recent inter-agent communication events.
///
/// Sources from both the event bus (for lifecycle events with full context)
/// and the audit log (for message/spawn/kill events that are always captured).
pub async fn comms_events(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(100)
        .min(500);

    let agents = state.kernel.registry.list();

    // Primary source: event bus (has full source/target context)
    let bus_events = state.kernel.event_bus.history(500).await;
    let mut comms_events: Vec<openfang_types::comms::CommsEvent> = bus_events
        .iter()
        .filter_map(|e| filter_to_comms_event(e, &agents))
        .collect();

    // Secondary source: audit log (always populated, wider coverage)
    let audit_entries = state.kernel.audit_log.recent(500);
    let seen_ids: std::collections::HashSet<String> =
        comms_events.iter().map(|e| e.id.clone()).collect();

    for entry in audit_entries.iter().rev() {
        if let Some(ev) = audit_to_comms_event(entry, &agents) {
            if !seen_ids.contains(&ev.id) {
                comms_events.push(ev);
            }
        }
    }

    // Sort by timestamp descending (newest first)
    comms_events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    comms_events.truncate(limit);

    Json(comms_events)
}

/// GET /api/comms/events/stream — SSE stream of inter-agent communication events.
///
/// Polls the audit log every 500ms for new inter-agent events.
pub async fn comms_events_stream(State(state): State<Arc<AppState>>) -> axum::response::Response {
    use axum::response::sse::{Event, KeepAlive, Sse};

    let (tx, rx) = tokio::sync::mpsc::channel::<
        Result<axum::response::sse::Event, std::convert::Infallible>,
    >(256);

    tokio::spawn(async move {
        let mut last_seq: u64 = {
            let entries = state.kernel.audit_log.recent(1);
            entries.last().map(|e| e.seq).unwrap_or(0)
        };

        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            let agents = state.kernel.registry.list();
            let entries = state.kernel.audit_log.recent(50);

            for entry in &entries {
                if entry.seq <= last_seq {
                    continue;
                }
                if let Some(comms_event) = audit_to_comms_event(entry, &agents) {
                    let data = serde_json::to_string(&comms_event).unwrap_or_default();
                    if tx.send(Ok(Event::default().data(data))).await.is_err() {
                        return; // Client disconnected
                    }
                }
            }

            if let Some(last) = entries.last() {
                last_seq = last.seq;
            }
        }
    });

    let rx_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Sse::new(rx_stream)
        .keep_alive(
            KeepAlive::new()
                .interval(std::time::Duration::from_secs(15))
                .text("ping"),
        )
        .into_response()
}

/// POST /api/comms/send — Send a message from one agent to another.
pub async fn comms_send(
    State(state): State<Arc<AppState>>,
    Json(req): Json<openfang_types::comms::CommsSendRequest>,
) -> impl IntoResponse {
    // Validate from agent exists
    let from_id: openfang_types::agent::AgentId = match req.from_agent_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid from_agent_id"})),
            )
        }
    };
    if state.kernel.registry.get(from_id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Source agent not found"})),
        );
    }

    // Validate to agent exists
    let to_id: openfang_types::agent::AgentId = match req.to_agent_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid to_agent_id"})),
            )
        }
    };
    if state.kernel.registry.get(to_id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Target agent not found"})),
        );
    }

    // SECURITY: Limit message size
    if req.message.len() > 64 * 1024 {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({"error": "Message too large (max 64KB)"})),
        );
    }

    match state.kernel.send_message(to_id, &req.message).await {
        Ok(result) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "response": result.response,
                "input_tokens": result.total_usage.input_tokens,
                "output_tokens": result.total_usage.output_tokens,
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Message delivery failed: {e}")})),
        ),
    }
}

/// POST /api/comms/task — Post a task to the agent task queue.
pub async fn comms_task(
    State(state): State<Arc<AppState>>,
    Json(req): Json<openfang_types::comms::CommsTaskRequest>,
) -> impl IntoResponse {
    if req.title.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Title is required"})),
        );
    }

    match state
        .kernel
        .memory
        .task_post(
            &req.title,
            &req.description,
            req.assigned_to.as_deref(),
            Some("ui-user"),
        )
        .await
    {
        Ok(task_id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "ok": true,
                "task_id": task_id,
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to post task: {e}")})),
        ),
    }
}

// ── Dashboard Authentication (username/password sessions) ──

/// POST /api/auth/login — Authenticate with username/password, returns session token.
pub async fn auth_login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<serde_json::Value>,
) -> axum::response::Response {
    use axum::body::Body;
    use axum::response::Response;

    let auth_cfg = &state.kernel.config.auth;
    if !auth_cfg.enabled {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"error": "Auth not enabled"}).to_string(),
            ))
            .unwrap();
    }

    let username = req.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let password = req.get("password").and_then(|v| v.as_str()).unwrap_or("");

    // Constant-time username comparison to prevent timing attacks
    let username_ok = {
        use subtle::ConstantTimeEq;
        let stored = auth_cfg.username.as_bytes();
        let provided = username.as_bytes();
        if stored.len() != provided.len() {
            false
        } else {
            bool::from(stored.ct_eq(provided))
        }
    };

    if !username_ok || !crate::session_auth::verify_password(password, &auth_cfg.password_hash) {
        // Audit log the failed attempt
        state.kernel.audit_log.record(
            "system",
            openfang_runtime::audit::AuditAction::AuthAttempt,
            "dashboard login failed",
            format!("username: {username}"),
        );
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"error": "Invalid credentials"}).to_string(),
            ))
            .unwrap();
    }

    // Derive the session secret the same way as server.rs
    let api_key = state.kernel.config.api_key.trim().to_string();
    let secret = if !api_key.is_empty() {
        api_key
    } else {
        auth_cfg.password_hash.clone()
    };

    let token =
        crate::session_auth::create_session_token(username, &secret, auth_cfg.session_ttl_hours);
    let ttl_secs = auth_cfg.session_ttl_hours * 3600;
    let cookie =
        format!("openfang_session={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={ttl_secs}");

    state.kernel.audit_log.record(
        "system",
        openfang_runtime::audit::AuditAction::AuthAttempt,
        "dashboard login success",
        format!("username: {username}"),
    );

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .header("set-cookie", &cookie)
        .body(Body::from(
            serde_json::json!({
                "status": "ok",
                "token": token,
                "username": username,
            })
            .to_string(),
        ))
        .unwrap()
}

/// POST /api/auth/logout — Clear the session cookie.
pub async fn auth_logout() -> impl IntoResponse {
    let cookie = "openfang_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0";
    (
        StatusCode::OK,
        [("content-type", "application/json"), ("set-cookie", cookie)],
        serde_json::json!({"status": "ok"}).to_string(),
    )
}

/// GET /api/auth/check — Check current authentication state.
pub async fn auth_check(
    State(state): State<Arc<AppState>>,
    request: axum::http::Request<axum::body::Body>,
) -> impl IntoResponse {
    let auth_cfg = &state.kernel.config.auth;
    if !auth_cfg.enabled {
        return Json(serde_json::json!({
            "authenticated": true,
            "mode": "none",
        }));
    }

    // Derive the session secret the same way as server.rs
    let api_key = state.kernel.config.api_key.trim().to_string();
    let secret = if !api_key.is_empty() {
        api_key
    } else {
        auth_cfg.password_hash.clone()
    };

    // Check session cookie
    let session_user = request
        .headers()
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|c| {
                c.trim()
                    .strip_prefix("openfang_session=")
                    .map(|v| v.to_string())
            })
        })
        .and_then(|token| crate::session_auth::verify_session_token(&token, &secret));

    if let Some(username) = session_user {
        Json(serde_json::json!({
            "authenticated": true,
            "mode": "session",
            "username": username,
        }))
    } else {
        Json(serde_json::json!({
            "authenticated": false,
            "mode": "session",
        }))
    }
}

/// Remove a `[section]` and its contents from a TOML string.
#[allow(dead_code)]
fn backup_config(config_path: &std::path::Path) {
    let backup = config_path.with_extension("toml.bak");
    let _ = std::fs::copy(config_path, backup);
}

fn remove_toml_section(content: &str, section: &str) -> String {
    let header = format!("[{}]", section);
    let mut result = String::new();
    let mut skipping = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == header {
            skipping = true;
            continue;
        }
        if skipping && trimmed.starts_with('[') {
            skipping = false;
        }
        if !skipping {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

#[cfg(test)]
mod channel_config_tests {
    use super::*;

    #[test]
    fn test_is_channel_configured_wecom_none() {
        let config = openfang_types::config::ChannelsConfig::default();
        assert!(!is_channel_configured(&config, "wecom"));
    }

    #[test]
    fn test_is_channel_configured_wecom_some() {
        let mut config = openfang_types::config::ChannelsConfig::default();
        config.wecom = Some(openfang_types::config::WeComConfig {
            corp_id: "test_corp".to_string(),
            agent_id: "test_agent".to_string(),
            secret_env: "WECOM_SECRET".to_string(),
            webhook_port: 8454,
            token: Some("token".to_string()),
            encoding_aes_key: Some("aes_key".to_string()),
            default_agent: Some("assistant".to_string()),
            overrides: openfang_types::config::ChannelOverrides::default(),
        });
        assert!(is_channel_configured(&config, "wecom"));
    }

    #[test]
    fn test_wecom_in_channel_registry() {
        let wecom_meta = CHANNEL_REGISTRY.iter().find(|c| c.name == "wecom");
        assert!(wecom_meta.is_some());
        let meta = wecom_meta.unwrap();
        assert_eq!(meta.display_name, "WeCom");
        assert_eq!(meta.category, "messaging");
        assert!(
            meta.fields
                .iter()
                .find(|f| f.key == "corp_id")
                .unwrap()
                .required
        );
        assert!(
            meta.fields
                .iter()
                .find(|f| f.key == "secret_env")
                .unwrap()
                .required
        );
    }
}

#[cfg(test)]
mod agent_definition_route_tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use axum::Router;
    use openfang_types::config::{DefaultModelConfig, KernelConfig};
    use serde_json::{json, Value};
    use tempfile::TempDir;
    use tower::ServiceExt;

    struct RouteTestContext {
        state: Arc<AppState>,
        _tmp: TempDir,
    }

    impl Drop for RouteTestContext {
        fn drop(&mut self) {
            self.state.kernel.shutdown();
        }
    }

    async fn route_test_context() -> RouteTestContext {
        let tmp = tempfile::tempdir().expect("temporary directory should be created");
        let config = KernelConfig {
            home_dir: tmp.path().to_path_buf(),
            data_dir: tmp.path().join("data"),
            default_model: DefaultModelConfig {
                provider: "claude_code".to_string(),
                model: "sonnet".to_string(),
                api_key_env: "ANTHROPIC_API_KEY".to_string(),
                base_url: None,
            },
            ..KernelConfig::default()
        };

        let kernel =
            Arc::new(OpenFangKernel::boot_with_config(config).expect("kernel should boot"));
        kernel.set_self_handle();
        *kernel
            .skill_registry
            .write()
            .unwrap_or_else(|error| error.into_inner()) =
            openfang_skills::registry::SkillRegistry::new(tmp.path().join("skills"));

        let state = Arc::new(AppState {
            kernel,
            started_at: Instant::now(),
            peer_registry: None,
            bridge_manager: tokio::sync::Mutex::new(None),
            channels_config: tokio::sync::RwLock::new(Default::default()),
            shutdown_notify: Arc::new(tokio::sync::Notify::new()),
            clawhub_cache: DashMap::new(),
            provider_probe_cache: openfang_runtime::provider_health::ProbeCache::new(),
        });

        RouteTestContext { state, _tmp: tmp }
    }

    async fn json_response(response: impl IntoResponse) -> (StatusCode, Value) {
        let response = response.into_response();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should be readable");
        let json = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).expect("response body should be valid JSON")
        };
        (status, json)
    }

    fn prd_writer_definition_value(id: &str, name: &str) -> Value {
        json!({
            "id": id,
            "name": name,
            "version": "1.0.0",
            "description": "Writes and iterates on product requirement documents",
            "enabled": true,
            "group": "sdlc",
            "tags": ["docs", "prd", "planning"],
            "provider": {
                "driver": "claude_code",
                "model": "sonnet",
                "profile": "default",
                "defaults": {
                    "reasoning_effort": "high",
                    "max_tokens": 8000
                },
                "config": {
                    "continue_conversation": true,
                    "fork_session": false,
                    "allowed_tools": ["Read", "Write", "Bash"],
                    "disallowed_tools": [],
                    "additional_directories": ["./docs"],
                    "max_budget_usd": 5.0,
                    "fallback_model": "sonnet"
                }
            },
            "prompt": {
                "system": "You are a senior product writer.",
                "instructions": "Write clear, implementation-ready PRDs.",
                "skills": ["writing", "prd"]
            },
            "capabilities": {
                "tools": ["*"],
                "primitives": ["issue.read", "artifact.*", "doc.*", "hitl.*"],
                "delegation": ["call", "send"],
                "workspace": "none",
                "network": true
            },
            "runtime": {
                "autonomous": true,
                "memory_policy": "session",
                "hitl": "explicit_only"
            },
            "input": {
                "kind": "object"
            },
            "output": {
                "kind": "artifact_ref",
                "artifact_type": "prd"
            }
        })
    }

    fn prd_writer_definition(id: &str, name: &str) -> AgentDefinition {
        serde_json::from_value(prd_writer_definition_value(id, name))
            .expect("fixture should deserialize")
    }

    fn codex_definition_value(id: &str, name: &str) -> Value {
        json!({
            "id": id,
            "name": name,
            "version": "1.0.0",
            "description": "Executes simple prompts through Codex",
            "enabled": true,
            "group": "tests",
            "tags": ["tests", "messages"],
            "provider": {
                "driver": "codex",
                "model": "gpt-4.1",
                "defaults": {
                    "max_tokens": 512
                },
                "config": {
                    "web_search": false
                }
            },
            "prompt": {
                "system": "You are a concise test assistant.",
                "instructions": "Answer briefly and directly.",
                "skills": ["testing"]
            },
            "capabilities": {
                "tools": [],
                "primitives": [],
                "delegation": [],
                "workspace": "none",
                "network": true
            },
            "runtime": {
                "autonomous": true,
                "memory_policy": "session",
                "hitl": "explicit_only"
            },
            "input": {
                "kind": "object"
            },
            "output": {
                "kind": "any"
            }
        })
    }

    fn codex_definition(id: &str, name: &str) -> AgentDefinition {
        serde_json::from_value(codex_definition_value(id, name))
            .expect("fixture should deserialize")
    }

    async fn create_codex_definition(context: &RouteTestContext, id: &str, name: &str) {
        let (status, body) = json_response(
            create_agent(
                State(Arc::clone(&context.state)),
                Ok(Json(CreateAgentRequest {
                    definition: codex_definition(id, name),
                })),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["id"], json!(id));
    }

    fn codex_live_available() -> bool {
        std::env::var("OPENAI_API_KEY").is_ok()
            || std::env::var("CODEX_HOME")
                .map(std::path::PathBuf::from)
                .ok()
                .or_else(|| {
                    std::env::var("HOME")
                        .ok()
                        .map(|home| std::path::PathBuf::from(home).join(".codex"))
                })
                .map(|path| path.join("auth.json").is_file())
                .unwrap_or(false)
    }

    fn message_request(session_id: &str, text: &str) -> MessageRequest {
        MessageRequest {
            session_id: session_id.to_owned(),
            input: MessageInputPayload {
                items: vec![MessageInputItem {
                    item_type: "text".to_owned(),
                    text: Some(text.to_owned()),
                }],
            },
            metadata: Some(json!({
                "source": "tests",
            })),
        }
    }

    async fn create_definition(context: &RouteTestContext, id: &str, name: &str) {
        let (status, body) = json_response(
            create_agent(
                State(Arc::clone(&context.state)),
                Ok(Json(CreateAgentRequest {
                    definition: prd_writer_definition(id, name),
                })),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["id"], json!(id));
    }

    #[tokio::test]
    async fn validate_agent_definition_should_return_normalized_response_for_valid_definition() {
        let context = route_test_context().await;

        let (status, body) = json_response(
            validate_agent_definition(
                State(Arc::clone(&context.state)),
                Ok(Json(AgentValidateRequest {
                    definition: prd_writer_definition("prd-writer", "PRD Writer"),
                    strict: Some(true),
                    context: None,
                })),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["valid"], json!(true));
        assert_eq!(body["issues"], json!([]));
        assert_eq!(body["normalized"]["id"], json!("prd-writer"));
        assert_eq!(body["normalized"]["name"], json!("PRD Writer"));
    }

    #[tokio::test]
    async fn validate_agent_definition_should_report_missing_driver() {
        let context = route_test_context().await;
        let mut definition = prd_writer_definition_value("broken-writer", "Broken Writer");
        definition["provider"]["driver"] = Value::String(String::new());

        let (status, body) = json_response(
            validate_agent_definition(
                State(Arc::clone(&context.state)),
                Ok(Json(AgentValidateRequest {
                    definition: serde_json::from_value(definition)
                        .expect("fixture should deserialize"),
                    strict: Some(false),
                    context: None,
                })),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["valid"], json!(false));
        assert!(body["issues"]
            .as_array()
            .expect("issues should be an array")
            .iter()
            .any(|issue| issue["path"] == json!("provider.driver")));
    }

    #[tokio::test]
    async fn compile_agent_definition_should_return_all_compiled_layers() {
        let context = route_test_context().await;

        let (status, body) = json_response(
            compile_agent_definition(
                State(Arc::clone(&context.state)),
                Ok(Json(AgentCompileRequest {
                    definition: prd_writer_definition("compile-writer", "Compile Writer"),
                    context: None,
                })),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["definition_id"], json!("compile-writer"));
        assert!(body["compiled"]["agent_manifest"].is_object());
        assert!(body["compiled"]["provider_binding"].is_object());
        assert!(body["compiled"]["product_metadata"].is_object());
    }

    #[tokio::test]
    async fn get_agent_compiled_should_return_not_found_for_unknown_definition() {
        let context = route_test_context().await;

        let (status, body) = json_response(
            get_agent_compiled(
                State(Arc::clone(&context.state)),
                Path("missing-definition".to_string()),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], json!("not_found"));
        assert_eq!(
            body["error"]["message"],
            json!("Agent definition not found")
        );
    }

    #[tokio::test]
    async fn create_and_update_agent_should_return_full_resource_objects() {
        let context = route_test_context().await;

        let (create_status, create_body) = json_response(
            create_agent(
                State(Arc::clone(&context.state)),
                Ok(Json(CreateAgentRequest {
                    definition: prd_writer_definition("resource-writer", "Resource Writer"),
                })),
            )
            .await,
        )
        .await;

        assert_eq!(create_status, StatusCode::CREATED);
        assert_eq!(create_body["id"], json!("resource-writer"));
        assert_eq!(create_body["name"], json!("Resource Writer"));
        assert_eq!(create_body["origin"]["kind"], json!("user"));
        assert!(create_body["forked_from"].is_null());
        assert!(create_body["created_at"].is_string());
        assert!(create_body["updated_at"].is_string());
        assert!(create_body.get("agent_id").is_none());

        let mut updated = prd_writer_definition_value("resource-writer", "Updated Writer");
        updated["description"] = json!("Updated description");

        let (update_status, update_body) = json_response(
            update_agent(
                State(Arc::clone(&context.state)),
                Path("resource-writer".to_string()),
                Ok(Json(UpdateAgentRequest {
                    definition: serde_json::from_value(updated)
                        .expect("fixture should deserialize"),
                })),
            )
            .await,
        )
        .await;

        assert_eq!(update_status, StatusCode::OK);
        assert_eq!(update_body["id"], json!("resource-writer"));
        assert_eq!(update_body["name"], json!("Updated Writer"));
        assert_eq!(update_body["description"], json!("Updated description"));
        assert_eq!(update_body["origin"]["kind"], json!("user"));
        assert!(update_body["forked_from"].is_null());
        assert!(update_body.get("agent_id").is_none());
    }

    #[tokio::test]
    async fn get_agent_runtime_should_return_runtime_resource_shape() {
        let context = route_test_context().await;
        create_definition(&context, "runtime-writer", "Runtime Writer").await;

        let (status, body) = json_response(
            get_agent_runtime(
                State(Arc::clone(&context.state)),
                Path("runtime-writer".to_string()),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["agent_id"], json!("runtime-writer"));
        assert_eq!(body["loaded"], json!(false));
        assert_eq!(body["state"], json!("created"));
        assert_eq!(body["mode"], json!("full"));
        assert_eq!(body["healthy"], json!(false));
        assert_eq!(body["active_sessions"], json!(0));
        assert_eq!(body["active_dispatches"], json!(0));
    }

    #[tokio::test]
    async fn start_agent_runtime_should_return_accepted_action_response() {
        let context = route_test_context().await;
        create_definition(&context, "start-writer", "Start Writer").await;

        let (status, body) = json_response(
            start_agent_runtime(
                State(Arc::clone(&context.state)),
                Path("start-writer".to_string()),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["accepted"], json!(true));
        assert_eq!(body["resource_id"], json!("start-writer"));
        assert_eq!(body["status"], json!("accepted"));
    }

    #[tokio::test]
    async fn set_agent_runtime_mode_should_return_bad_request_for_unknown_mode() {
        let context = route_test_context().await;
        create_definition(&context, "mode-writer", "Mode Writer").await;

        let router = Router::new()
            .route(
                "/api/v1/agents/{id}/runtime/mode",
                axum::routing::put(set_agent_runtime_mode),
            )
            .with_state(Arc::clone(&context.state));

        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/agents/mode-writer/runtime/mode")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"mode":"unknown"}"#))
                    .expect("request should build"),
            )
            .await
            .expect("router should respond");

        let status = response.status();
        let body = serde_json::from_slice::<Value>(
            &to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("response body should be readable"),
        )
        .expect("response should be JSON");

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], json!("invalid_request"));
        assert!(body["error"]["message"].is_string());
    }

    #[tokio::test]
    async fn list_agent_sessions_v1_should_return_items_and_next_cursor_shape() {
        let context = route_test_context().await;
        create_definition(&context, "list-sessions", "List Sessions").await;

        let _ = json_response(
            start_agent_runtime(
                State(Arc::clone(&context.state)),
                Path("list-sessions".to_string()),
            )
            .await,
        )
        .await;

        let (status, body) = json_response(
            list_agent_sessions_v1(
                State(Arc::clone(&context.state)),
                Path("list-sessions".to_string()),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(body["items"].is_array());
        assert!(body["next_cursor"].is_null());
    }

    #[tokio::test]
    async fn create_agent_session_v1_should_return_session_detail_with_generated_session_id() {
        let context = route_test_context().await;
        create_definition(&context, "create-session", "Create Session").await;

        let (status, body) = json_response(
            create_agent_session_v1(
                State(Arc::clone(&context.state)),
                Path("create-session".to_string()),
                Ok(Json(CreateSessionRequest {
                    label: Some("Planning".to_string()),
                })),
            )
            .await,
        )
        .await;

        let session_id = body["session_id"]
            .as_str()
            .expect("session_id should be present")
            .to_string();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["id"], json!(session_id));
        assert_eq!(body["label"], json!("Planning"));
        assert_eq!(body["active"], json!(true));
        assert_eq!(body["message_count"], json!(0));
        assert_eq!(body["dispatch_count"], json!(0));
        assert!(body["created_at"].is_string());
        assert!(body["updated_at"].is_string());
    }

    #[tokio::test]
    async fn get_agent_session_v1_should_return_not_found_for_unknown_session_id() {
        let context = route_test_context().await;
        create_definition(&context, "missing-session", "Missing Session").await;

        let (status, body) = json_response(
            get_agent_session_v1(
                State(Arc::clone(&context.state)),
                Path((
                    "missing-session".to_string(),
                    uuid::Uuid::new_v4().to_string(),
                )),
                Query(AgentSessionDetailQuery::default()),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], json!("not_found"));
        assert_eq!(body["error"]["message"], json!("Agent session not found"));
    }

    #[tokio::test]
    async fn submit_agent_message_should_return_not_found_for_unknown_agent_id() {
        let context = route_test_context().await;

        let (status, body) = json_response(
            submit_agent_message(
                State(Arc::clone(&context.state)),
                Path("unknown-agent".to_string()),
                Ok(Json(message_request(
                    &uuid::Uuid::new_v4().to_string(),
                    "Hello",
                ))),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], json!("not_found"));
        assert_eq!(
            body["error"]["message"],
            json!("Agent definition not found")
        );
    }

    #[tokio::test]
    async fn submit_agent_message_should_return_conflict_when_runtime_not_started() {
        let context = route_test_context().await;
        create_definition(&context, "message-runtime", "Message Runtime").await;
        let session_id = uuid::Uuid::new_v4().to_string();

        let (status, body) = json_response(
            submit_agent_message(
                State(Arc::clone(&context.state)),
                Path("message-runtime".to_string()),
                Ok(Json(message_request(&session_id, "Hello"))),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], json!("runtime_not_started"));
        assert_eq!(
            body["error"]["message"],
            json!("Agent runtime is not started")
        );
    }

    #[tokio::test]
    async fn dry_run_agent_message_should_return_resolution_without_dispatch() {
        let context = route_test_context().await;
        create_definition(&context, "dry-run-writer", "Dry Run Writer").await;

        let _ = json_response(
            start_agent_runtime(
                State(Arc::clone(&context.state)),
                Path("dry-run-writer".to_string()),
            )
            .await,
        )
        .await;

        let sessions = context
            .state
            .kernel
            .runtime_stores
            .agent_session
            .list_agent_sessions_for_agent(stable_runtime_agent_id("dry-run-writer"))
            .expect("session projections should load");
        let session_id = sessions
            .first()
            .expect("runtime start should create a default session")
            .session_id
            .to_string();

        let (status, body) = json_response(
            dry_run_agent_message(
                State(Arc::clone(&context.state)),
                Path("dry-run-writer".to_string()),
                Ok(Json(message_request(&session_id, "Create a short outline"))),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["would_execute"], json!(true));
        assert_eq!(body["resolved"]["agent_id"], json!("dry-run-writer"));
        assert_eq!(body["resolved"]["session_id"], json!(session_id));
        assert_eq!(body["resolved"]["provider"]["driver"], json!("claude_code"));
        assert_eq!(body["resolved"]["provider"]["model"], json!("sonnet"));
        assert!(body["resolved"]["tools"].is_array());
        assert_eq!(body["effects"]["message_submit"], json!(true));
        assert!(body["effects"]["estimated_tokens"].is_u64());
        assert!(body["effects"]["estimated_cost"].is_number());
        assert!(body["explanation"]["steps"].is_array());
    }

    #[tokio::test]
    async fn dry_run_agent_message_should_work_when_runtime_is_stopped() {
        let context = route_test_context().await;
        create_definition(&context, "dry-run-stopped", "Dry Run Stopped").await;

        let _ = json_response(
            start_agent_runtime(
                State(Arc::clone(&context.state)),
                Path("dry-run-stopped".to_string()),
            )
            .await,
        )
        .await;
        let agent_entry = find_runtime_agent_for_definition(&context.state, "dry-run-stopped")
            .expect("runtime should be present");
        let session_id = agent_entry.session_id.to_string();

        let _ = json_response(
            stop_agent_runtime(
                State(Arc::clone(&context.state)),
                Path("dry-run-stopped".to_string()),
            )
            .await,
        )
        .await;

        let (status, body) = json_response(
            dry_run_agent_message(
                State(Arc::clone(&context.state)),
                Path("dry-run-stopped".to_string()),
                Ok(Json(message_request(
                    &session_id,
                    "Summarize what would happen",
                ))),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["would_execute"], json!(true));
        assert_eq!(body["resolved"]["session"]["id"], json!(session_id));
        assert_eq!(body["resolved"]["session"]["active"], json!(true));
    }

    #[tokio::test]
    async fn dry_run_agent_message_should_reject_session_from_another_agent() {
        let context = route_test_context().await;
        create_definition(&context, "dry-run-owner", "Dry Run Owner").await;
        create_definition(&context, "dry-run-other", "Dry Run Other").await;

        let _ = json_response(
            start_agent_runtime(
                State(Arc::clone(&context.state)),
                Path("dry-run-owner".to_string()),
            )
            .await,
        )
        .await;
        let owner_entry = find_runtime_agent_for_definition(&context.state, "dry-run-owner")
            .expect("owner runtime should be present");
        let foreign_session_id = owner_entry.session_id.to_string();

        let (status, body) = json_response(
            dry_run_agent_message(
                State(Arc::clone(&context.state)),
                Path("dry-run-other".to_string()),
                Ok(Json(message_request(
                    &foreign_session_id,
                    "Attempt to inspect another agent session",
                ))),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], json!("not_found"));
        assert_eq!(body["error"]["message"], json!("Agent session not found"));
    }

    #[tokio::test]
    async fn submit_agent_message_should_return_accepted_response_for_valid_request() {
        if !codex_live_available() {
            eprintln!(
                "Codex credentials not available, skipping live submit_agent_message unit test"
            );
            return;
        }

        let context = route_test_context().await;
        create_codex_definition(&context, "submit-live", "Submit Live").await;

        let _ = json_response(
            start_agent_runtime(
                State(Arc::clone(&context.state)),
                Path("submit-live".to_string()),
            )
            .await,
        )
        .await;
        let agent_entry = find_runtime_agent_for_definition(&context.state, "submit-live")
            .expect("runtime should be present");
        let session_id = agent_entry.session_id.to_string();

        let (status, body) = json_response(
            submit_agent_message(
                State(Arc::clone(&context.state)),
                Path("submit-live".to_string()),
                Ok(Json(message_request(
                    &session_id,
                    "Say hello in exactly three words.",
                ))),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(body["accepted"], json!(true));
        assert_eq!(body["resource_id"], json!("submit-live"));
        assert_eq!(body["session_id"], json!(session_id));
        assert!(body["message_id"].is_string());
    }

    #[tokio::test]
    async fn stream_agent_message_should_return_sse_error_event_for_unknown_agent_id() {
        let context = route_test_context().await;

        let response = stream_agent_message(
            State(Arc::clone(&context.state)),
            Path("unknown-agent".to_string()),
            Ok(Json(message_request(
                &uuid::Uuid::new_v4().to_string(),
                "Hello from SSE",
            ))),
        )
        .await;

        let status = response.status();
        let content_type = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let body = String::from_utf8(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("response body should be readable")
                .to_vec(),
        )
        .expect("SSE response body should be valid UTF-8");

        assert_eq!(status, StatusCode::OK);
        assert!(content_type.starts_with("text/event-stream"));
        assert!(body.contains("event: error"));
        assert!(body.contains("\"code\":\"not_found\""));
    }

    fn noop_workflow_definition(id: &str) -> openfang_types::workflow::WorkflowV2Definition {
        serde_json::from_value(json!({
            "id": id,
            "name": format!("Workflow {id}"),
            "version": "1.0.0",
            "description": "Schedule route test workflow",
            "enabled": true,
            "tags": ["tests", "schedules"],
            "input": {
                "kind": "object",
                "required": [],
                "open": true,
                "fields": {}
            },
            "output": {
                "kind": "object",
                "required": ["result"],
                "open": false,
                "fields": {
                    "result": { "kind": "string" }
                }
            },
            "steps": [{
                "id": "noop-step",
                "name": "Noop Step",
                "kind": "noop",
                "save_as": "result",
                "flow": { "mode": "sequential" }
            }],
            "outputs": {
                "result": "{{ vars.result }}"
            }
        }))
        .expect("fixture should deserialize")
    }

    async fn register_schedule_workflow(context: &RouteTestContext, id: &str) {
        let (status, body) = json_response(
            create_workflow_definition_v1(
                State(Arc::clone(&context.state)),
                Ok(Json(
                    serde_json::to_value(noop_workflow_definition(id))
                        .expect("workflow definition should serialize"),
                )),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["id"], json!(id));
    }

    fn workflow_run_schedule_definition(agent: &str, workflow_id: &str, enabled: bool) -> Value {
        json!({
            "agent": agent,
            "name": "Nightly Repo Review",
            "enabled": enabled,
            "schedule": {
                "kind": "cron",
                "expr": "0 2 * * *",
                "tz": "UTC"
            },
            "action": {
                "kind": "workflow_run",
                "workflow_id": workflow_id,
                "input": {
                    "scope": "open_prs"
                },
                "timeout_secs": 300
            },
            "delivery": {
                "kind": "none"
            }
        })
    }

    #[tokio::test]
    async fn validate_schedule_definition_should_accept_valid_five_field_cron() {
        let context = route_test_context().await;
        create_definition(&context, "schedule-writer", "Schedule Writer").await;
        register_schedule_workflow(&context, "repo-review").await;

        let (status, body) = json_response(
            validate_schedule_definition_v1(
                State(Arc::clone(&context.state)),
                Ok(Json(ScheduleValidateRequest {
                    definition: workflow_run_schedule_definition(
                        "schedule-writer",
                        "repo-review",
                        true,
                    ),
                    strict: Some(true),
                    context: None,
                })),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["valid"], json!(true));
        assert_eq!(body["issues"], json!([]));
        assert_eq!(body["normalized"]["schedule"]["expr"], json!("0 2 * * *"));
    }

    #[tokio::test]
    async fn validate_schedule_definition_should_report_invalid_cron_expression() {
        let context = route_test_context().await;
        create_definition(&context, "schedule-writer", "Schedule Writer").await;
        register_schedule_workflow(&context, "repo-review").await;
        let mut definition =
            workflow_run_schedule_definition("schedule-writer", "repo-review", true);
        definition["schedule"]["expr"] = json!("99 99 99 99 99");

        let (status, body) = json_response(
            validate_schedule_definition_v1(
                State(Arc::clone(&context.state)),
                Ok(Json(ScheduleValidateRequest {
                    definition,
                    strict: Some(false),
                    context: None,
                })),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["valid"], json!(false));
        assert!(body["issues"]
            .as_array()
            .expect("issues should be an array")
            .iter()
            .any(|issue| issue["path"] == json!("schedule.expr")));
    }

    #[tokio::test]
    async fn validate_schedule_definition_should_report_invalid_timezone() {
        let context = route_test_context().await;
        create_definition(&context, "schedule-writer", "Schedule Writer").await;
        register_schedule_workflow(&context, "repo-review").await;
        let mut definition =
            workflow_run_schedule_definition("schedule-writer", "repo-review", true);
        definition["schedule"]["tz"] = json!("Mars/Phobos");

        let (status, body) = json_response(
            validate_schedule_definition_v1(
                State(Arc::clone(&context.state)),
                Ok(Json(ScheduleValidateRequest {
                    definition,
                    strict: Some(false),
                    context: None,
                })),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["valid"], json!(false));
        assert!(body["issues"]
            .as_array()
            .expect("issues should be an array")
            .iter()
            .any(|issue| issue["path"] == json!("schedule.tz")));
    }

    #[tokio::test]
    async fn validate_schedule_definition_should_require_workflow_id_for_workflow_run() {
        let context = route_test_context().await;
        create_definition(&context, "schedule-writer", "Schedule Writer").await;
        register_schedule_workflow(&context, "repo-review").await;
        let mut definition =
            workflow_run_schedule_definition("schedule-writer", "repo-review", true);
        definition["action"]
            .as_object_mut()
            .expect("action should be an object")
            .remove("workflow_id");

        let (status, body) = json_response(
            validate_schedule_definition_v1(
                State(Arc::clone(&context.state)),
                Ok(Json(ScheduleValidateRequest {
                    definition,
                    strict: Some(false),
                    context: None,
                })),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["valid"], json!(false));
        assert!(body["issues"]
            .as_array()
            .expect("issues should be an array")
            .iter()
            .any(|issue| issue["path"] == json!("action.workflow_id")));
    }

    #[tokio::test]
    async fn validate_schedule_definition_should_report_unsupported_action_kind() {
        let context = route_test_context().await;
        create_definition(&context, "schedule-writer", "Schedule Writer").await;
        register_schedule_workflow(&context, "repo-review").await;
        let mut definition =
            workflow_run_schedule_definition("schedule-writer", "repo-review", true);
        definition["action"]["kind"] = json!("launch_missiles");

        let (status, body) = json_response(
            validate_schedule_definition_v1(
                State(Arc::clone(&context.state)),
                Ok(Json(ScheduleValidateRequest {
                    definition,
                    strict: Some(false),
                    context: None,
                })),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["valid"], json!(false));
        assert!(body["issues"]
            .as_array()
            .expect("issues should be an array")
            .iter()
            .any(|issue| {
                issue["path"] == json!("action.kind")
                    && issue["code"] == json!("unsupported_action_kind")
            }));
    }

    #[tokio::test]
    async fn validate_schedule_definition_should_require_workflow_signal_selector() {
        let context = route_test_context().await;
        create_definition(&context, "schedule-writer", "Schedule Writer").await;
        register_schedule_workflow(&context, "release-prep").await;
        let definition = json!({
            "agent": "schedule-writer",
            "name": "Signal Release",
            "enabled": true,
            "schedule": {
                "kind": "cron",
                "expr": "0 2 * * *",
                "tz": "UTC"
            },
            "action": {
                "kind": "workflow_signal",
                "signal": "deadline_reached",
                "payload": {
                    "reason": "scheduled_check"
                }
            },
            "delivery": {
                "kind": "none"
            }
        });

        let (status, body) = json_response(
            validate_schedule_definition_v1(
                State(Arc::clone(&context.state)),
                Ok(Json(ScheduleValidateRequest {
                    definition,
                    strict: Some(false),
                    context: None,
                })),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["valid"], json!(false));
        assert!(body["issues"]
            .as_array()
            .expect("issues should be an array")
            .iter()
            .any(|issue| issue["path"] == json!("action.selector.workflow_id")));
    }

    #[tokio::test]
    async fn disable_schedule_definition_should_update_runtime_and_active_queue_synchronously() {
        let context = route_test_context().await;
        create_definition(&context, "schedule-writer", "Schedule Writer").await;
        register_schedule_workflow(&context, "repo-review").await;

        let (create_status, create_body) = json_response(
            create_schedule_definition_v1(
                State(Arc::clone(&context.state)),
                Ok(Json(workflow_run_schedule_definition(
                    "schedule-writer",
                    "repo-review",
                    true,
                ))),
            )
            .await,
        )
        .await;

        assert_eq!(create_status, StatusCode::CREATED);
        let schedule_id = create_body["id"]
            .as_str()
            .expect("schedule ID should be returned")
            .to_string();

        let (disable_status, disable_body) = json_response(
            disable_schedule_definition_v1(
                State(Arc::clone(&context.state)),
                Path(schedule_id.clone()),
            )
            .await,
        )
        .await;

        assert_eq!(disable_status, StatusCode::ACCEPTED);
        assert_eq!(disable_body["accepted"], json!(true));
        assert_eq!(disable_body["resource_id"], json!(schedule_id.clone()));

        let meta = context
            .state
            .kernel
            .cron_scheduler
            .get_meta_by_definition_id(&schedule_id)
            .expect("schedule should still exist");
        assert!(!meta.job.enabled);
        assert!(meta.job.next_run.is_none());

        let runtime = context
            .state
            .kernel
            .runtime_stores
            .schedule_runtime
            .get_schedule_runtime(&schedule_id)
            .expect("runtime projection should load")
            .expect("runtime projection should exist");
        assert!(!runtime.enabled);
        assert!(runtime.next_run.is_none());
    }
}

#[cfg(test)]
mod trigger_definition_route_tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::StatusCode;
    use openfang_types::config::{DefaultModelConfig, KernelConfig};
    use serde_json::{json, Value};
    use tempfile::TempDir;

    struct RouteTestContext {
        state: Arc<AppState>,
        _tmp: TempDir,
    }

    impl Drop for RouteTestContext {
        fn drop(&mut self) {
            self.state.kernel.shutdown();
        }
    }

    async fn route_test_context() -> RouteTestContext {
        let tmp = tempfile::tempdir().expect("temporary directory should be created");
        let config = KernelConfig {
            home_dir: tmp.path().to_path_buf(),
            data_dir: tmp.path().join("data"),
            default_model: DefaultModelConfig {
                provider: "ollama".to_string(),
                model: "test-model".to_string(),
                api_key_env: "OLLAMA_API_KEY".to_string(),
                base_url: None,
            },
            ..KernelConfig::default()
        };

        let kernel =
            Arc::new(OpenFangKernel::boot_with_config(config).expect("kernel should boot"));
        kernel.set_self_handle();
        kernel.bootstrap_workflow_definitions().await;

        let state = Arc::new(AppState {
            kernel,
            started_at: Instant::now(),
            peer_registry: None,
            bridge_manager: tokio::sync::Mutex::new(None),
            channels_config: tokio::sync::RwLock::new(Default::default()),
            shutdown_notify: Arc::new(tokio::sync::Notify::new()),
            clawhub_cache: DashMap::new(),
            provider_probe_cache: openfang_runtime::provider_health::ProbeCache::new(),
        });

        RouteTestContext { state, _tmp: tmp }
    }

    async fn json_response(response: impl IntoResponse) -> (StatusCode, Value) {
        let response = response.into_response();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should be readable");
        let json = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).expect("response body should be valid JSON")
        };
        (status, json)
    }

    fn noop_workflow_definition(id: &str) -> Value {
        json!({
            "id": id,
            "name": format!("Workflow {id}"),
            "version": "1.0.0",
            "description": "Trigger route test workflow",
            "enabled": true,
            "input": {
                "kind": "object",
                "required": [],
                "open": true,
                "fields": {}
            },
            "output": {
                "kind": "object",
                "required": ["result"],
                "open": false,
                "fields": {
                    "result": { "kind": "string" }
                }
            },
            "steps": [{
                "id": "noop-step",
                "name": "Noop Step",
                "kind": "noop",
                "save_as": "result",
                "flow": { "mode": "sequential" }
            }],
            "outputs": {
                "result": "{{ vars.result }}"
            }
        })
    }

    async fn create_workflow_definition(context: &RouteTestContext, id: &str) {
        let (status, body) = json_response(
            create_workflow_definition_v1(
                State(Arc::clone(&context.state)),
                Ok(Json(noop_workflow_definition(id))),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["id"], json!(id));
    }

    fn workflow_start_trigger(id: &str, workflow_id: &str) -> Value {
        json!({
            "id": id,
            "name": format!("Trigger {id}"),
            "description": "Trigger route test definition",
            "enabled": true,
            "max_fires": 3,
            "cooldown_secs": 15,
            "match": {
                "event": "issue.created"
            },
            "target": {
                "kind": "workflow_start",
                "workflow": workflow_id,
                "input": {
                    "source": "tests"
                }
            }
        })
    }

    #[tokio::test]
    async fn validate_trigger_definition_should_treat_warnings_as_invalid_when_strict() {
        let context = route_test_context().await;
        create_workflow_definition(&context, "trigger-validate-workflow").await;

        let mut definition =
            workflow_start_trigger("broad-match-trigger", "trigger-validate-workflow");
        definition["match"] = json!({});

        let (status, body) = json_response(
            validate_trigger_definition_v1(
                State(Arc::clone(&context.state)),
                Ok(Json(TriggerValidateRequest {
                    definition,
                    strict: Some(true),
                    context: None,
                })),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["valid"], json!(false));
        assert!(body["issues"]
            .as_array()
            .expect("issues should be an array")
            .iter()
            .any(|issue| issue["code"] == json!("broad_match")));
    }

    #[tokio::test]
    async fn compile_trigger_definition_should_return_trigger_ir_payload() {
        let context = route_test_context().await;
        create_workflow_definition(&context, "trigger-compile-workflow").await;

        let (status, body) = json_response(
            compile_trigger_definition_v1(
                State(Arc::clone(&context.state)),
                Ok(Json(TriggerCompileRequest {
                    definition: workflow_start_trigger(
                        "compile-trigger",
                        "trigger-compile-workflow",
                    ),
                    context: None,
                })),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["definition_id"], json!("compile-trigger"));
        assert_eq!(body["normalized"]["id"], json!("compile-trigger"));
        assert_eq!(
            body["compiled"]["trigger_ir"]["trigger_id"],
            json!("compile-trigger")
        );
    }

    #[tokio::test]
    async fn enable_disable_trigger_definition_should_update_active_matching_set() {
        let context = route_test_context().await;
        create_workflow_definition(&context, "trigger-enable-workflow").await;

        let (create_status, _) = json_response(
            create_trigger_definition_v1(
                State(Arc::clone(&context.state)),
                Ok(Json(workflow_start_trigger(
                    "toggle-trigger",
                    "trigger-enable-workflow",
                ))),
            )
            .await,
        )
        .await;

        assert_eq!(create_status, StatusCode::CREATED);
        assert_eq!(
            context
                .state
                .kernel
                .trigger_v2
                .list_active_trigger_ids()
                .await,
            vec!["toggle-trigger".to_string()]
        );

        let (disable_status, disable_body) = json_response(
            disable_trigger_definition_v1(
                State(Arc::clone(&context.state)),
                Path("toggle-trigger".to_string()),
            )
            .await,
        )
        .await;

        assert_eq!(disable_status, StatusCode::ACCEPTED);
        assert_eq!(disable_body["accepted"], json!(true));
        assert!(context
            .state
            .kernel
            .trigger_v2
            .list_active_trigger_ids()
            .await
            .is_empty());

        let (enable_status, enable_body) = json_response(
            enable_trigger_definition_v1(
                State(Arc::clone(&context.state)),
                Path("toggle-trigger".to_string()),
            )
            .await,
        )
        .await;

        assert_eq!(enable_status, StatusCode::ACCEPTED);
        assert_eq!(enable_body["accepted"], json!(true));
        assert_eq!(
            context
                .state
                .kernel
                .trigger_v2
                .list_active_trigger_ids()
                .await,
            vec!["toggle-trigger".to_string()]
        );
    }
}

#[cfg(test)]
mod skill_v1_route_tests {
    use super::*;

    fn skill(id: &str, name: &str, description: &str) -> SkillResponse {
        SkillResponse {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            source: format!("/tmp/skills/{id}/skill.toml"),
            created_at: "2026-03-24T00:00:00Z".to_string(),
            updated_at: "2026-03-24T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn skill_matches_search_should_be_case_insensitive_for_name_and_description() {
        let writing = skill(
            "writing",
            "Writing Coach",
            "Structured document authoring guidance",
        );
        let testing = skill("testing", "Regression Guard", "Catches hidden regressions");

        assert!(skill_matches_search(&writing, "writing"));
        assert!(skill_matches_search(&writing, "AUTHORING"));
        assert!(!skill_matches_search(&testing, "writing"));
    }

    #[test]
    fn paginate_skill_summaries_should_return_items_and_next_cursor_across_pages() {
        let items = (0..5)
            .map(|index| SkillSummary {
                id: format!("skill-{index}"),
                name: format!("Skill {index}"),
                description: format!("Description {index}"),
                source: format!("/tmp/skills/skill-{index}/skill.toml"),
            })
            .collect::<Vec<_>>();

        let first_page = paginate_skill_summaries(items.clone(), 2, 0);
        assert_eq!(first_page.items.len(), 2);
        assert_eq!(first_page.next_cursor.as_deref(), Some("2"));

        let second_page = paginate_skill_summaries(items.clone(), 2, 2);
        assert_eq!(second_page.items.len(), 2);
        assert_eq!(second_page.next_cursor.as_deref(), Some("4"));

        let final_page = paginate_skill_summaries(items, 2, 4);
        assert_eq!(final_page.items.len(), 1);
        assert!(final_page.next_cursor.is_none());
    }
}

#[cfg(test)]
mod task_control_plane_route_tests {
    use super::*;
    use axum::body::to_bytes;
    use openfang_types::config::{DefaultModelConfig, KernelConfig};
    use serde_json::{json, Value};
    use tempfile::TempDir;

    struct RouteTestContext {
        state: Arc<AppState>,
        _tmp: TempDir,
    }

    impl Drop for RouteTestContext {
        fn drop(&mut self) {
            self.state.kernel.shutdown();
        }
    }

    async fn route_test_context() -> RouteTestContext {
        let tmp = tempfile::tempdir().expect("temporary directory should be created");
        let config = KernelConfig {
            home_dir: tmp.path().to_path_buf(),
            data_dir: tmp.path().join("data"),
            default_model: DefaultModelConfig {
                provider: "ollama".to_string(),
                model: "test-model".to_string(),
                api_key_env: "OLLAMA_API_KEY".to_string(),
                base_url: None,
            },
            ..KernelConfig::default()
        };

        let kernel =
            Arc::new(OpenFangKernel::boot_with_config(config).expect("kernel should boot"));
        kernel.set_self_handle();
        kernel.bootstrap_workflow_definitions().await;

        let state = Arc::new(AppState {
            kernel,
            started_at: Instant::now(),
            peer_registry: None,
            bridge_manager: tokio::sync::Mutex::new(None),
            channels_config: tokio::sync::RwLock::new(Default::default()),
            shutdown_notify: Arc::new(tokio::sync::Notify::new()),
            clawhub_cache: DashMap::new(),
            provider_probe_cache: openfang_runtime::provider_health::ProbeCache::new(),
        });

        RouteTestContext { state, _tmp: tmp }
    }

    async fn json_response(response: impl IntoResponse) -> (StatusCode, Value) {
        let response = response.into_response();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should be readable");
        let json = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).expect("response body should be valid JSON")
        };
        (status, json)
    }

    fn sample_task(id: &str, slug: &str) -> TaskRecord {
        TaskRecord {
            task_id: TaskId::new(id),
            slug: slug.to_string(),
            title: format!("Task {id}"),
            description: "Task description".to_string(),
            status: openfang_types::task::TaskStatus::Planned,
            priority: openfang_types::task::Priority::High,
            complexity: openfang_types::task::Complexity::Medium,
            position: 1,
            source: TaskSource::Manual,
            owner: openfang_types::task::OwnerRef {
                kind: openfang_types::task::ActorKind::AgentGroup,
                ref_id: "sdlc".to_string(),
            },
            created_by: openfang_types::task::OwnerRef {
                kind: openfang_types::task::ActorKind::Agent,
                ref_id: "planner".to_string(),
            },
            repository_refs: vec![],
            label_refs: vec![],
            artifact_refs: vec![],
            doc_refs: vec![],
            file_refs: vec![],
            metadata: json!({}),
            created_at: "2026-03-25T12:00:00Z".to_string(),
            updated_at: "2026-03-25T12:00:00Z".to_string(),
            completed_at: None,
        }
    }

    fn sample_subtask(id: &str, task_id: &TaskId, position: i64) -> SubtaskRecord {
        SubtaskRecord {
            subtask_id: SubtaskId::new(id),
            task_id: task_id.clone(),
            title: format!("Subtask {id}"),
            description: "Subtask description".to_string(),
            kind: openfang_types::task::SubtaskKind::DocChange,
            status: SubtaskStatus::Planned,
            complexity: openfang_types::task::Complexity::Medium,
            position,
            assignee: Some(openfang_types::task::AssigneeRef {
                kind: openfang_types::task::ActorKind::Agent,
                ref_id: "prd-writer".to_string(),
            }),
            depends_on: Vec::new(),
            parallelizable: false,
            input: json!({}),
            result: None,
            metadata: json!({}),
            created_at: "2026-03-25T12:01:00Z".to_string(),
            updated_at: "2026-03-25T12:01:00Z".to_string(),
            completed_at: None,
        }
    }

    #[tokio::test]
    async fn replan_task_v1_should_return_structured_422_for_foreign_cancel() {
        let context = route_test_context().await;
        let task = sample_task("task_local", "task-local");
        let other_task = sample_task("task_other", "task-other");
        context
            .state
            .kernel
            .workflow_stores
            .task
            .create(&task)
            .expect("task should be created");
        context
            .state
            .kernel
            .workflow_stores
            .task
            .create(&other_task)
            .expect("other task should be created");

        let foreign_subtask = sample_subtask("subtask_foreign", &other_task.task_id, 1);
        context
            .state
            .kernel
            .workflow_stores
            .subtask
            .create(&foreign_subtask)
            .expect("foreign subtask should be created");

        let (status, body) = json_response(
            replan_task_v1(
                State(Arc::clone(&context.state)),
                Path(task.task_id.to_string()),
                Ok(Json(TaskReplanRequest {
                    reason: "cancel the wrong subtask".to_string(),
                    operations: vec![openfang_types::task::TaskReplanOperation::CancelSubtasks {
                        subtask_ids: vec![foreign_subtask.subtask_id.clone()],
                    }],
                    metadata: json!({}),
                })),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], json!("invalid_subtask_reference"));
        assert_eq!(
            body["error"]["details"][0]["subtask_id"],
            json!("subtask_foreign")
        );
    }

    #[tokio::test]
    async fn replan_task_v1_should_return_structured_422_for_missing_dependency_without_writes() {
        let context = route_test_context().await;
        let task = sample_task("task_missing_dep", "task-missing-dep");
        context
            .state
            .kernel
            .workflow_stores
            .task
            .create(&task)
            .expect("task should be created");

        let (status, body) = json_response(
            replan_task_v1(
                State(Arc::clone(&context.state)),
                Path(task.task_id.to_string()),
                Ok(Json(TaskReplanRequest {
                    reason: "introduce an invalid dependency".to_string(),
                    operations: vec![openfang_types::task::TaskReplanOperation::CreateSubtasks {
                        items: vec![openfang_types::task::PlannedSubtask {
                            subtask_id: Some(SubtaskId::new("subtask_new")),
                            title: "Invalid dependency".to_string(),
                            description: "Should fail".to_string(),
                            kind: openfang_types::task::SubtaskKind::ReviewItem,
                            status: None,
                            complexity: None,
                            position: 1,
                            assignee: None,
                            depends_on: vec![SubtaskId::new("missing_dependency")],
                            parallelizable: false,
                            input: json!({}),
                            result: None,
                            metadata: json!({}),
                        }],
                    }],
                    metadata: json!({}),
                })),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], json!("invalid_dependency"));

        let page = context
            .state
            .kernel
            .workflow_stores
            .subtask
            .list_for_task(&task.task_id, &SubtaskListQuery::default())
            .expect("subtask list should load");
        assert_eq!(page.items.len(), 0);
    }

    #[tokio::test]
    async fn list_subtasks_v1_should_filter_ready_and_blocked() {
        let context = route_test_context().await;
        let task = sample_task("task_filters", "task-filters");
        context
            .state
            .kernel
            .workflow_stores
            .task
            .create(&task)
            .expect("task should be created");

        let mut completed = sample_subtask("subtask_completed", &task.task_id, 1);
        completed.status = SubtaskStatus::Completed;
        completed.updated_at = "2026-03-25T12:02:00Z".to_string();
        completed.completed_at = Some("2026-03-25T12:02:00Z".to_string());
        context
            .state
            .kernel
            .workflow_stores
            .subtask
            .create(&completed)
            .expect("completed subtask should be created");

        let mut ready = sample_subtask("subtask_ready", &task.task_id, 2);
        ready.depends_on = vec![completed.subtask_id.clone()];
        context
            .state
            .kernel
            .workflow_stores
            .subtask
            .create(&ready)
            .expect("ready subtask should be created");

        let pending = sample_subtask("subtask_pending", &task.task_id, 3);
        context
            .state
            .kernel
            .workflow_stores
            .subtask
            .create(&pending)
            .expect("pending subtask should be created");

        let mut blocked = sample_subtask("subtask_blocked", &task.task_id, 4);
        blocked.depends_on = vec![pending.subtask_id.clone()];
        context
            .state
            .kernel
            .workflow_stores
            .subtask
            .create(&blocked)
            .expect("blocked subtask should be created");

        let (ready_status, ready_body) = json_response(
            list_task_subtasks_v1(
                State(Arc::clone(&context.state)),
                Path(task.task_id.to_string()),
                Ok(Query(SubtaskListQueryParams {
                    ready: Some(true),
                    ..SubtaskListQueryParams::default()
                })),
            )
            .await,
        )
        .await;
        assert_eq!(ready_status, StatusCode::OK);
        assert_eq!(
            ready_body["items"]
                .as_array()
                .expect("ready items should be an array")
                .iter()
                .map(|item| item["id"].as_str().expect("id should be a string"))
                .collect::<Vec<_>>(),
            vec!["subtask_completed", "subtask_ready", "subtask_pending"]
        );

        let (blocked_status, blocked_body) = json_response(
            list_task_subtasks_v1(
                State(Arc::clone(&context.state)),
                Path(task.task_id.to_string()),
                Ok(Query(SubtaskListQueryParams {
                    blocked: Some(true),
                    ..SubtaskListQueryParams::default()
                })),
            )
            .await,
        )
        .await;
        assert_eq!(blocked_status, StatusCode::OK);
        assert_eq!(
            blocked_body["items"]
                .as_array()
                .expect("blocked items should be an array")
                .iter()
                .map(|item| item["id"].as_str().expect("id should be a string"))
                .collect::<Vec<_>>(),
            vec!["subtask_blocked"]
        );
    }
}
