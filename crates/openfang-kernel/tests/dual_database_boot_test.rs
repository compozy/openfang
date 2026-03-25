//! Integration coverage for dual-database kernel bootstrap.

use chrono::{DateTime, Utc};
use openfang_kernel::workflow::{WorkflowRunId, WorkflowRunState};
use openfang_kernel::OpenFangKernel;
use openfang_memory::{
    AgentRuntimeRecord, CheckpointKind, DispatchKind, DispatchRecord, DispatchRepository,
    DispatchStatus, HitlKind, HitlRepository, HitlStatus, NewHitlRequest, WorkflowRunRecord,
    WorkflowRunStatus,
};
use openfang_types::agent::{AgentMode, AgentState};
use openfang_types::config::KernelConfig;
use openfang_types::scheduler::{CronAction, CronDelivery, CronJob, CronJobId, CronSchedule};
use pretty_assertions::assert_eq;
use rusqlite::{Connection, OptionalExtension};
use serde_json::json;
use std::collections::HashSet;
use uuid::Uuid;

fn table_exists(path: &std::path::Path, table_name: &str) -> bool {
    let conn = Connection::open(path).expect("open sqlite file");
    conn.query_row(
        "SELECT name
         FROM sqlite_master
         WHERE type = 'table' AND name = ?1",
        [table_name],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .expect("table exists query")
    .is_some()
}

fn boot_test_config(root: &std::path::Path) -> KernelConfig {
    KernelConfig {
        home_dir: root.to_path_buf(),
        data_dir: root.join("data"),
        ..KernelConfig::default()
    }
}

fn schema_migration_exists(path: &std::path::Path) -> bool {
    let conn = Connection::open(path).expect("open sqlite file");
    conn.query_row(
        "SELECT name
         FROM sqlite_master
         WHERE type = 'table' AND name = 'schema_migration'",
        [],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .expect("schema_migration exists query")
    .is_some()
}

fn schema_migration_rows(path: &std::path::Path) -> Vec<(u32, String, String)> {
    let conn = Connection::open(path).expect("open sqlite file");
    let mut stmt = conn
        .prepare("SELECT version, name, applied_at FROM schema_migration ORDER BY version")
        .expect("prepare schema_migration query");
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("query schema_migration rows");

    rows.map(|row| row.expect("schema_migration row")).collect()
}

fn table_names(path: &std::path::Path) -> Vec<String> {
    let conn = Connection::open(path).expect("open sqlite file");
    let mut stmt = conn
        .prepare(
            "SELECT name
             FROM sqlite_master
             WHERE type = 'table'
             ORDER BY name",
        )
        .expect("prepare table list query");
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query table names");

    rows.map(|row| row.expect("table row")).collect()
}

fn sample_workflow_run(run_id: &str) -> WorkflowRunRecord {
    WorkflowRunRecord {
        run_id: Uuid::new_v5(&Uuid::NAMESPACE_URL, run_id.as_bytes()).to_string(),
        workflow_id: Uuid::new_v4().to_string(),
        workflow_version: "2026.03.23".to_string(),
        status: WorkflowRunStatus::Pending,
        input_json: "\"hello workflow\"".to_string(),
        vars_json: "{}".to_string(),
        current_step_id: None,
        waiting_kind: None,
        waiting_ref: None,
        active_dispatch_id: None,
        active_hitl_request_id: None,
        labels_json: "[]".to_string(),
        metadata_json: "{}".to_string(),
        error_json: None,
        started_at: "2026-03-23T12:00:00Z".to_string(),
        updated_at: "2026-03-23T12:00:00Z".to_string(),
        completed_at: None,
    }
}

fn fixed_test_timestamp() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-03-23T12:00:00Z")
        .expect("valid fixed timestamp")
        .with_timezone(&Utc)
}

fn sample_waiting_hitl_dispatch(
    dispatch_id: &str,
    run_id: &str,
    step_id: &str,
    updated_at: &str,
) -> DispatchRecord {
    DispatchRecord {
        dispatch_id: dispatch_id.to_string(),
        run_id: run_id.to_string(),
        step_id: Some(step_id.to_string()),
        kind: DispatchKind::Call,
        target_agent: "review-agent".to_string(),
        status: DispatchStatus::WaitingHitl,
        input_json: json!({
            "prompt": "Please review the pending item",
        }),
        result_json: None,
        error_json: None,
        attempt: 1,
        parent_dispatch_id: None,
        spawned_agent_id: None,
        provider_driver: Some("openfang".to_string()),
        session_id: Some(Uuid::new_v4().to_string()),
        provider_resume_token: None,
        started_at: updated_at.to_string(),
        updated_at: updated_at.to_string(),
        completed_at: None,
    }
}

fn sample_pending_hitl_request(
    hitl_request_id: &str,
    run_id: &str,
    step_id: &str,
    dispatch_id: &str,
) -> NewHitlRequest {
    NewHitlRequest {
        hitl_request_id: hitl_request_id.to_string(),
        run_id: run_id.to_string(),
        step_id: step_id.to_string(),
        dispatch_id: Some(dispatch_id.to_string()),
        kind: HitlKind::Clarification,
        question: "Is this ready to ship?".to_string(),
        context_json: json!({
            "source": "dual_database_boot_test",
        }),
        created_at: fixed_test_timestamp(),
        timeout_at: None,
    }
}

#[test]
fn boot_should_create_both_database_files_on_fresh_boot() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let config = boot_test_config(tmp.path());
    let runtime_db = config.persistence.resolve_runtime_db(&config.data_dir);
    let compozy_db = config.persistence.resolve_compozy_db(&config.data_dir);

    let kernel = OpenFangKernel::boot_with_config(config).expect("kernel should boot");

    assert!(
        runtime_db.exists(),
        "runtime.db should be created on first boot"
    );
    assert!(
        compozy_db.exists(),
        "compozy.db should be created on first boot"
    );
    assert!(
        kernel.db_health().is_healthy(),
        "both databases should be healthy"
    );

    kernel.shutdown();
}

#[test]
fn boot_should_reopen_existing_dual_databases() {
    let tmp = tempfile::tempdir().expect("temp dir");

    let first_config = boot_test_config(tmp.path());
    let runtime_db = first_config
        .persistence
        .resolve_runtime_db(&first_config.data_dir);
    let compozy_db = first_config
        .persistence
        .resolve_compozy_db(&first_config.data_dir);
    let first_kernel = OpenFangKernel::boot_with_config(first_config).expect("first boot");
    first_kernel.shutdown();

    let second_config = boot_test_config(tmp.path());
    let second_kernel = OpenFangKernel::boot_with_config(second_config).expect("second boot");

    assert!(runtime_db.exists(), "runtime.db should still exist");
    assert!(compozy_db.exists(), "compozy.db should still exist");
    assert!(
        second_kernel.db_health().is_healthy(),
        "second boot should keep both databases healthy"
    );

    second_kernel.shutdown();
}

#[test]
fn boot_should_create_schema_migration_in_both_databases() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let config = boot_test_config(tmp.path());
    let runtime_db = config.persistence.resolve_runtime_db(&config.data_dir);
    let compozy_db = config.persistence.resolve_compozy_db(&config.data_dir);

    let kernel = OpenFangKernel::boot_with_config(config).expect("kernel should boot");

    assert!(schema_migration_exists(&runtime_db));
    assert!(schema_migration_exists(&compozy_db));
    assert!(!schema_migration_rows(&runtime_db).is_empty());
    assert!(!schema_migration_rows(&compozy_db).is_empty());

    kernel.shutdown();
}

#[test]
fn second_boot_against_migrated_databases_succeeds_without_error() {
    let tmp = tempfile::tempdir().expect("temp dir");

    let first_config = boot_test_config(tmp.path());
    let first_kernel = OpenFangKernel::boot_with_config(first_config).expect("first boot");
    first_kernel.shutdown();

    let second_config = boot_test_config(tmp.path());
    let second_kernel = OpenFangKernel::boot_with_config(second_config).expect("second boot");

    assert!(
        second_kernel.db_health().is_healthy(),
        "second boot should keep both databases healthy"
    );

    second_kernel.shutdown();
}

#[test]
fn migration_status_is_queryable_after_boot() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let config = boot_test_config(tmp.path());
    let runtime_db = config.persistence.resolve_runtime_db(&config.data_dir);
    let compozy_db = config.persistence.resolve_compozy_db(&config.data_dir);

    let kernel = OpenFangKernel::boot_with_config(config).expect("kernel should boot");
    let runtime_rows = schema_migration_rows(&runtime_db);
    let compozy_rows = schema_migration_rows(&compozy_db);

    assert_eq!(runtime_rows.len(), 5);
    assert_eq!(compozy_rows.len(), 11);
    assert_eq!(runtime_rows[0].0, 1);
    assert_eq!(compozy_rows[0].0, 1);
    assert_eq!(runtime_rows[0].1, "schema_migrations_bootstrap");
    assert_eq!(runtime_rows[1].1, "0002_agent_runtime_core");
    assert_eq!(runtime_rows[2].1, "0003_agent_sessions_and_messages");
    assert_eq!(runtime_rows[3].1, "0004_schedule_runtime_core");
    assert_eq!(runtime_rows[4].1, "0005_trigger_runtime_core");
    assert_eq!(compozy_rows[0].1, "schema_migrations_bootstrap");
    assert_eq!(compozy_rows[1].1, "0002_workflow_run_core");
    assert_eq!(compozy_rows[2].1, "0003_workflow_checkpoint");
    assert_eq!(compozy_rows[3].1, "0004_workflow_signal");
    assert_eq!(compozy_rows[4].1, "0005_workflow_runtime_durability");
    assert_eq!(compozy_rows[5].1, "0006_workflow_signal_waiting_state");
    assert_eq!(compozy_rows[6].1, "0007_workflow_run_control_plane");
    assert_eq!(compozy_rows[7].1, "0008_agent_dispatch");
    assert_eq!(compozy_rows[8].1, "0009_hitl_request");
    assert_eq!(compozy_rows[9].1, "0010_task_subtask");
    assert_eq!(compozy_rows[10].1, "0011_looper_runtime");

    kernel.shutdown();
}

#[test]
fn boot_should_create_runtime_db_with_all_initial_tables() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let config = boot_test_config(tmp.path());
    let runtime_db = config.persistence.resolve_runtime_db(&config.data_dir);

    let kernel = OpenFangKernel::boot_with_config(config).expect("kernel should boot");

    for table_name in [
        "schema_migration",
        "agent_runtime",
        "agent_session",
        "agent_message",
        "schedule_runtime",
        "schedule_execution",
    ] {
        assert!(
            table_exists(&runtime_db, table_name),
            "missing runtime.db table: {table_name}"
        );
    }

    kernel.shutdown();
}

#[test]
fn boot_should_create_compozy_db_with_all_durable_runtime_tables() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let config = boot_test_config(tmp.path());
    let compozy_db = config.persistence.resolve_compozy_db(&config.data_dir);

    let kernel = OpenFangKernel::boot_with_config(config).expect("kernel should boot");

    for table_name in [
        "schema_migration",
        "workflow_run",
        "workflow_checkpoint",
        "workflow_signal",
        "agent_dispatch",
        "hitl_request",
        "task",
        "subtask",
        "looper_run",
        "looper_subtask",
    ] {
        assert!(
            table_exists(&compozy_db, table_name),
            "missing compozy.db table: {table_name}"
        );
    }

    kernel.shutdown();
}

#[tokio::test]
async fn boot_should_recover_running_workflow_runs_before_cache_projection() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let config = boot_test_config(tmp.path());
    let mut record = sample_workflow_run("run-recovery-boot");
    record.status = WorkflowRunStatus::Running;
    record.current_step_id = Some("step-1".to_string());

    let first_kernel = OpenFangKernel::boot_with_config(config.clone()).expect("first boot");
    first_kernel
        .workflow_stores
        .workflow_run
        .insert_run(&record)
        .expect("store workflow run");
    first_kernel.shutdown();

    let second_kernel = OpenFangKernel::boot_with_config(config).expect("second boot");
    let loaded_before_bootstrap = second_kernel
        .workflow_stores
        .workflow_run
        .find_by_id(&record.run_id)
        .expect("load recovered workflow run")
        .expect("workflow run should persist");
    let checkpoints_before_bootstrap = second_kernel
        .workflow_stores
        .workflow_checkpoint
        .list_for_run(&record.run_id)
        .expect("load checkpoints");

    assert_eq!(loaded_before_bootstrap.status, WorkflowRunStatus::Paused);
    assert_eq!(
        checkpoints_before_bootstrap
            .iter()
            .filter(|checkpoint| checkpoint.kind == CheckpointKind::RunRecoveredNeedsResume)
            .count(),
        1
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&checkpoints_before_bootstrap[0].data_json)
            .expect("recovery checkpoint data should deserialize"),
        json!({ "previous_status": "running" })
    );

    second_kernel.bootstrap_workflow_definitions().await;
    let loaded = second_kernel
        .workflow_stores
        .workflow_run
        .find_by_id(&record.run_id)
        .expect("load recovered workflow run after bootstrap")
        .expect("workflow run should persist");
    let checkpoints = second_kernel
        .workflow_stores
        .workflow_checkpoint
        .list_for_run(&record.run_id)
        .expect("load checkpoints after bootstrap");
    let cached = second_kernel
        .workflows
        .get_run(WorkflowRunId(
            Uuid::parse_str(&record.run_id).expect("valid run id"),
        ))
        .await
        .expect("workflow run should be projected into cache");

    assert_eq!(loaded.status, WorkflowRunStatus::Paused);
    assert_eq!(
        checkpoints
            .iter()
            .filter(|checkpoint| checkpoint.kind == CheckpointKind::RunRecoveredNeedsResume)
            .count(),
        1
    );
    assert!(matches!(cached.state, WorkflowRunState::Paused));

    second_kernel.shutdown();
}

#[test]
fn workflow_run_should_survive_restart() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let config = boot_test_config(tmp.path());
    let record = sample_workflow_run("run-survives-restart");

    let first_kernel = OpenFangKernel::boot_with_config(config.clone()).expect("first boot");
    first_kernel
        .workflow_stores
        .workflow_run
        .insert_run(&record)
        .expect("store workflow run");
    first_kernel.shutdown();

    let second_kernel = OpenFangKernel::boot_with_config(config).expect("second boot");
    let loaded = second_kernel
        .workflow_stores
        .workflow_run
        .find_by_id(&record.run_id)
        .expect("load workflow run")
        .expect("workflow run should persist");

    assert_eq!(loaded, record);

    second_kernel.shutdown();
}

#[tokio::test]
async fn waiting_run_should_survive_restart_and_remain_resumable() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let config = boot_test_config(tmp.path());
    let mut record = sample_workflow_run("run-waiting-survival");
    record.status = WorkflowRunStatus::WaitingSignal;
    record.current_step_id = Some("step-approval".to_string());
    record.waiting_kind = Some("signal".to_string());
    record.waiting_ref = Some("approval-42".to_string());

    let first_kernel = OpenFangKernel::boot_with_config(config.clone()).expect("first boot");
    first_kernel
        .workflow_stores
        .workflow_run
        .insert_run(&record)
        .expect("store workflow run");
    first_kernel.shutdown();

    let second_kernel = OpenFangKernel::boot_with_config(config).expect("second boot");
    second_kernel.bootstrap_workflow_definitions().await;
    let loaded = second_kernel
        .workflow_stores
        .workflow_run
        .find_by_id(&record.run_id)
        .expect("load workflow run")
        .expect("workflow run should persist");
    let cached = second_kernel
        .workflows
        .get_run(WorkflowRunId(
            Uuid::parse_str(&record.run_id).expect("valid run id"),
        ))
        .await
        .expect("waiting workflow run should be projected into cache");

    assert_eq!(loaded.status, WorkflowRunStatus::WaitingSignal);
    assert_eq!(loaded.waiting_kind.as_deref(), Some("signal"));
    assert_eq!(loaded.waiting_ref.as_deref(), Some("approval-42"));
    assert!(matches!(cached.state, WorkflowRunState::WaitingSignal));

    second_kernel.shutdown();
}

#[tokio::test]
async fn waiting_hitl_run_should_survive_restart_unchanged() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let config = boot_test_config(tmp.path());
    let mut record = sample_workflow_run("run-waiting-hitl-survival");
    let dispatch_id = "dispatch-hitl-42".to_string();
    let hitl_request_id = "hitl-request-42".to_string();

    record.status = WorkflowRunStatus::WaitingHitl;
    record.current_step_id = Some("step-review".to_string());
    record.waiting_kind = Some("hitl".to_string());
    record.waiting_ref = Some(hitl_request_id.clone());
    record.active_dispatch_id = Some(dispatch_id.clone());
    record.active_hitl_request_id = Some(hitl_request_id.clone());

    let first_kernel = OpenFangKernel::boot_with_config(config.clone()).expect("first boot");
    first_kernel
        .workflow_stores
        .workflow_run
        .insert_run(&record)
        .expect("store workflow run");
    first_kernel
        .workflow_stores
        .dispatch
        .create(&sample_waiting_hitl_dispatch(
            &dispatch_id,
            &record.run_id,
            "step-review",
            &record.updated_at,
        ))
        .await
        .expect("store waiting HITL dispatch");
    let created_hitl = first_kernel
        .workflow_stores
        .hitl
        .create(sample_pending_hitl_request(
            &hitl_request_id,
            &record.run_id,
            "step-review",
            &dispatch_id,
        ))
        .await
        .expect("store pending HITL request");
    first_kernel.shutdown();

    let second_kernel = OpenFangKernel::boot_with_config(config).expect("second boot");
    second_kernel.bootstrap_workflow_definitions().await;
    let loaded = second_kernel
        .workflow_stores
        .workflow_run
        .find_by_id(&record.run_id)
        .expect("load workflow run")
        .expect("workflow run should persist");
    let cached = second_kernel
        .workflows
        .get_run(WorkflowRunId(
            Uuid::parse_str(&record.run_id).expect("valid run id"),
        ))
        .await
        .expect("waiting workflow run should be projected into cache");
    let loaded_hitl = second_kernel
        .workflow_stores
        .hitl
        .find_by_id(&created_hitl.hitl_request_id)
        .await
        .expect("load pending HITL request")
        .expect("pending HITL request should persist");
    let checkpoints = second_kernel
        .workflow_stores
        .workflow_checkpoint
        .list_for_run(&record.run_id)
        .expect("load checkpoints");

    assert_eq!(loaded.status, WorkflowRunStatus::WaitingHitl);
    assert_eq!(loaded.waiting_kind.as_deref(), Some("hitl"));
    assert_eq!(loaded.waiting_ref.as_deref(), Some("hitl-request-42"));
    assert_eq!(
        loaded.active_hitl_request_id.as_deref(),
        Some(hitl_request_id.as_str())
    );
    assert_eq!(
        loaded.active_dispatch_id.as_deref(),
        Some(dispatch_id.as_str())
    );
    assert_eq!(loaded_hitl.status, HitlStatus::Pending);
    assert!(matches!(cached.state, WorkflowRunState::WaitingHitl));
    assert!(checkpoints.is_empty());

    second_kernel.shutdown();
}

#[test]
fn compozy_db_and_runtime_db_should_coexist_after_both_boot() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let config = boot_test_config(tmp.path());
    let runtime_db = config.persistence.resolve_runtime_db(&config.data_dir);
    let compozy_db = config.persistence.resolve_compozy_db(&config.data_dir);

    let kernel = OpenFangKernel::boot_with_config(config).expect("kernel should boot");
    let runtime_tables = table_names(&runtime_db).into_iter().collect::<HashSet<_>>();
    let compozy_tables = table_names(&compozy_db).into_iter().collect::<HashSet<_>>();
    let overlap = runtime_tables
        .intersection(&compozy_tables)
        .cloned()
        .collect::<HashSet<_>>();

    assert!(runtime_db.exists(), "runtime.db should exist");
    assert!(compozy_db.exists(), "compozy.db should exist");
    assert!(schema_migration_exists(&runtime_db));
    assert!(schema_migration_exists(&compozy_db));
    assert_eq!(overlap, HashSet::from([String::from("schema_migration")]));

    kernel.shutdown();
}

#[test]
fn agent_runtime_state_should_survive_restart() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let config = boot_test_config(tmp.path());

    let first_kernel = OpenFangKernel::boot_with_config(config.clone()).expect("first boot");
    let record = AgentRuntimeRecord {
        agent_id: openfang_types::agent::AgentId::new(),
        loaded: true,
        state: AgentState::Running,
        mode: AgentMode::Observe,
        healthy: true,
        active_session_id: None,
        active_dispatches: 0,
        last_active_at: Some("2026-03-23T12:00:00Z".to_string()),
        updated_at: "2026-03-23T12:00:00Z".to_string(),
    };

    first_kernel
        .runtime_stores
        .agent_runtime
        .upsert_agent_runtime(&record)
        .expect("store runtime projection");
    first_kernel.shutdown();

    let second_kernel = OpenFangKernel::boot_with_config(config).expect("second boot");
    let loaded = second_kernel
        .runtime_stores
        .agent_runtime
        .get_agent_runtime(record.agent_id)
        .expect("load runtime projection")
        .expect("runtime projection should persist");

    assert_eq!(loaded, record);

    second_kernel.shutdown();
}

#[test]
fn schedule_runtime_state_should_survive_restart() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let config = boot_test_config(tmp.path());

    let first_kernel = OpenFangKernel::boot_with_config(config.clone()).expect("first boot");
    let schedule_id = CronJobId::new();
    let job = CronJob {
        id: schedule_id,
        agent_id: first_kernel.registry.list()[0].id,
        name: "runtime-db-survival".to_string(),
        enabled: true,
        schedule: CronSchedule::Every { every_secs: 600 },
        action: CronAction::SystemEvent {
            event: "ping".to_string(),
            payload: serde_json::Value::Null,
        },
        delivery: CronDelivery::None,
        created_at: chrono::Utc::now(),
        last_run: None,
        next_run: None,
    };
    first_kernel
        .cron_scheduler
        .add_job(job, false)
        .expect("add cron job");
    first_kernel
        .cron_scheduler
        .persist()
        .expect("persist cron jobs");

    let first_runtime = first_kernel
        .runtime_stores
        .schedule_runtime
        .get_schedule_runtime(&schedule_id.to_string())
        .expect("load stored schedule runtime")
        .expect("schedule runtime should exist");
    first_kernel.shutdown();

    let second_kernel = OpenFangKernel::boot_with_config(config).expect("second boot");
    let loaded = second_kernel
        .runtime_stores
        .schedule_runtime
        .get_schedule_runtime(&schedule_id.to_string())
        .expect("load schedule runtime after restart")
        .expect("schedule runtime should persist after restart");

    assert_eq!(loaded.schedule_id, first_runtime.schedule_id);
    assert_eq!(loaded.enabled, first_runtime.enabled);
    assert_eq!(loaded.one_shot, first_runtime.one_shot);
    assert!(
        loaded.next_run.is_some(),
        "next_run should be recomputed on load"
    );

    second_kernel.shutdown();
}

#[test]
fn existing_boot_path_should_not_regress() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let config = boot_test_config(tmp.path());

    let kernel = OpenFangKernel::boot_with_config(config).expect("kernel should boot");
    let agent_id = kernel.registry.list()[0].id;

    kernel
        .memory
        .structured_set(agent_id, "task6_probe", json!({"status": "ok"}))
        .expect("store structured memory");
    let stored = kernel
        .memory
        .structured_get(agent_id, "task6_probe")
        .expect("load structured memory");
    assert_eq!(stored, Some(json!({"status": "ok"})));

    let session = kernel
        .memory
        .create_session(agent_id)
        .expect("create legacy session");
    let loaded_session = kernel
        .memory
        .get_session(session.id)
        .expect("load legacy session");
    assert!(
        loaded_session.is_some(),
        "legacy session path should keep working"
    );

    kernel.shutdown();
}
