//! Memory substrate for the OpenFang Agent Operating System.
//!
//! Provides a unified memory API over three storage backends:
//! - **Structured store** (SQLite): Key-value pairs, sessions, agent state
//! - **Semantic store**: Text-based search (Phase 1: LIKE matching, Phase 2: Qdrant vectors)
//! - **Knowledge graph** (SQLite): Entities and relations
//!
//! Agents interact with a single `Memory` trait that abstracts over all three stores.

pub mod artifact;
pub mod consolidation;
pub mod dispatch;
pub mod doc;
pub mod hitl;
#[cfg(feature = "http-memory")]
pub mod http_client;
pub mod knowledge;
pub mod looper;
pub mod migration;
pub mod pack;
pub mod runtime_store;
pub mod semantic;
pub mod session;
pub mod structured;
pub mod task;
pub mod usage;
pub mod workflow_store;

mod substrate;
pub use artifact::{ArtifactRepository, ArtifactStoreError, ARTIFACT_DOC_VERSIONING_MIGRATION_SQL};
pub use dispatch::{
    DispatchKind, DispatchListPage, DispatchListQuery, DispatchRecord, DispatchRepository,
    DispatchStatus, DispatchStore, DispatchStoreError, DispatchSummaryRecord,
    SqliteDispatchRepository, AGENT_DISPATCH_MIGRATION_SQL,
};
pub use doc::{DocRepository, DocStoreError};
pub use hitl::{
    ExpiredHitlRequest, HitlKind, HitlListPage, HitlListQuery, HitlRecord, HitlRepository,
    HitlStatus, HitlStore, HitlStoreError, NewHitlRequest, SqliteHitlRepository,
    HITL_REQUEST_MIGRATION_SQL,
};
pub use looper::{
    LooperRunRepository, LooperStoreError, LooperSubtaskRepository, NewLooperRun,
    LOOPER_RUNTIME_MIGRATION_SQL,
};
pub use pack::{PackRepository, PackStoreError, PACK_MIGRATION_SQL};
pub use runtime_store::{
    AgentMessageRecord, AgentMessageStore, AgentRuntimeRecord, AgentRuntimeStore,
    AgentSessionRecord, AgentSessionStore, RuntimeStoreSet, ScheduleExecutionRecord,
    ScheduleExecutionStore, ScheduleRuntimeRecord, ScheduleRuntimeStore, TriggerRuntimeRecord,
    TriggerRuntimeStore, AGENT_RUNTIME_CORE_MIGRATION_SQL,
    AGENT_SESSIONS_AND_MESSAGES_MIGRATION_SQL, SCHEDULE_RUNTIME_CORE_MIGRATION_SQL,
    TRIGGER_RUNTIME_CORE_MIGRATION_SQL,
};
pub use substrate::MemorySubstrate;
pub use task::{
    SubtaskRepository, TaskRepository, TaskStoreError, TaskStoreSet, TASK_SUBTASK_MIGRATION_SQL,
};
pub use workflow_store::{
    now_timestamp, CheckpointKind, HitlAnswerTransition, SubmittedSignalResume,
    WorkflowCheckpointRecord, WorkflowCheckpointRepository, WorkflowCheckpointStore,
    WorkflowDispatchSummaryRecord, WorkflowRunListQuery, WorkflowRunRecord,
    WorkflowRunRecoveryRecord, WorkflowRunRepository, WorkflowRunStatus, WorkflowRunStore,
    WorkflowSignalRecord, WorkflowSignalRepository, WorkflowSignalStore, WorkflowStoreError,
    WorkflowStoreSet, WORKFLOW_CHECKPOINT_MIGRATION_SQL,
    WORKFLOW_CHECKPOINT_RETENTION_MIGRATION_SQL, WORKFLOW_RUNTIME_DURABILITY_MIGRATION_SQL,
    WORKFLOW_RUN_CONTROL_PLANE_MIGRATION_SQL, WORKFLOW_RUN_CORE_MIGRATION_SQL,
    WORKFLOW_SIGNAL_MIGRATION_SQL, WORKFLOW_SIGNAL_WAITING_STATE_MIGRATION_SQL,
};
