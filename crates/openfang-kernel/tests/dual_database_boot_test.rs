//! Integration coverage for dual-database kernel bootstrap.

use openfang_kernel::OpenFangKernel;
use openfang_memory::{AgentRuntimeRecord, CheckpointKind, WorkflowRunRecord, WorkflowRunStatus};
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
        run_id: run_id.to_string(),
        workflow_id: Uuid::new_v4().to_string(),
        workflow_version: Some("2026.03.23".to_string()),
        status: WorkflowRunStatus::Pending,
        input_json: "\"hello workflow\"".to_string(),
        vars_json: "{}".to_string(),
        current_step_id: Some("step-1".to_string()),
        waiting_kind: None,
        waiting_ref: None,
        active_dispatch_id: None,
        active_hitl_request_id: None,
        labels_json: "[]".to_string(),
        metadata_json: "{}".to_string(),
        error_json: None,
        started_at: None,
        updated_at: "2026-03-23T12:00:00Z".to_string(),
        completed_at: None,
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

    assert_eq!(runtime_rows.len(), 4);
    assert_eq!(compozy_rows.len(), 4);
    assert_eq!(runtime_rows[0].0, 1);
    assert_eq!(compozy_rows[0].0, 1);
    assert_eq!(runtime_rows[0].1, "schema_migrations_bootstrap");
    assert_eq!(runtime_rows[1].1, "0002_agent_runtime_core");
    assert_eq!(runtime_rows[2].1, "0003_agent_sessions_and_messages");
    assert_eq!(runtime_rows[3].1, "0004_schedule_runtime_core");
    assert_eq!(compozy_rows[0].1, "schema_migrations_bootstrap");
    assert_eq!(compozy_rows[1].1, "0002_workflow_run_core");
    assert_eq!(compozy_rows[2].1, "0003_workflow_checkpoint");
    assert_eq!(compozy_rows[3].1, "0004_workflow_signal");

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
fn boot_should_create_compozy_db_with_all_phase_1_tables() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let config = boot_test_config(tmp.path());
    let compozy_db = config.persistence.resolve_compozy_db(&config.data_dir);

    let kernel = OpenFangKernel::boot_with_config(config).expect("kernel should boot");

    for table_name in [
        "schema_migration",
        "workflow_run",
        "workflow_checkpoint",
        "workflow_signal",
    ] {
        assert!(
            table_exists(&compozy_db, table_name),
            "missing compozy.db table: {table_name}"
        );
    }

    kernel.shutdown();
}

#[test]
fn boot_should_run_recovery_scan_before_workflow_engine_starts() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let config = boot_test_config(tmp.path());
    let record = sample_workflow_run("run-recovery-boot");

    let first_kernel = OpenFangKernel::boot_with_config(config.clone()).expect("first boot");
    first_kernel
        .workflow_stores
        .workflow_run
        .create_run(&record)
        .expect("store workflow run");
    first_kernel
        .workflow_stores
        .workflow_run
        .update_run_status(&record.run_id, WorkflowRunStatus::Running)
        .expect("mark workflow run as running");
    first_kernel.shutdown();

    let second_kernel = OpenFangKernel::boot_with_config(config).expect("second boot");
    let loaded = second_kernel
        .workflow_stores
        .workflow_run
        .get_run(&record.run_id)
        .expect("load recovered workflow run")
        .expect("workflow run should persist");
    let checkpoints = second_kernel
        .workflow_stores
        .workflow_checkpoint
        .list_checkpoints_for_run(&record.run_id)
        .expect("load checkpoints");

    assert_eq!(loaded.status, WorkflowRunStatus::Paused);
    assert_eq!(
        checkpoints
            .iter()
            .filter(|checkpoint| checkpoint.kind == CheckpointKind::RunRecoveredNeedsResume)
            .count(),
        1
    );

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
        .create_run(&record)
        .expect("store workflow run");
    first_kernel.shutdown();

    let second_kernel = OpenFangKernel::boot_with_config(config).expect("second boot");
    let loaded = second_kernel
        .workflow_stores
        .workflow_run
        .get_run(&record.run_id)
        .expect("load workflow run")
        .expect("workflow run should persist");

    assert_eq!(loaded, record);

    second_kernel.shutdown();
}

#[test]
fn waiting_signal_run_should_survive_restart_and_remain_resumable() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let config = boot_test_config(tmp.path());
    let record = sample_workflow_run("run-waiting-survival");

    let first_kernel = OpenFangKernel::boot_with_config(config.clone()).expect("first boot");
    first_kernel
        .workflow_stores
        .workflow_run
        .create_run(&record)
        .expect("store workflow run");
    first_kernel
        .workflow_stores
        .workflow_run
        .update_run_waiting_state(&record.run_id, Some("signal"), Some("approval-42"))
        .expect("update waiting state");
    first_kernel
        .workflow_stores
        .workflow_run
        .update_run_status(&record.run_id, WorkflowRunStatus::WaitingSignal)
        .expect("mark workflow run as waiting_signal");
    first_kernel.shutdown();

    let second_kernel = OpenFangKernel::boot_with_config(config).expect("second boot");
    let loaded = second_kernel
        .workflow_stores
        .workflow_run
        .get_run(&record.run_id)
        .expect("load workflow run")
        .expect("workflow run should persist");

    assert_eq!(loaded.status, WorkflowRunStatus::WaitingSignal);
    assert_eq!(loaded.waiting_kind.as_deref(), Some("signal"));
    assert_eq!(loaded.waiting_ref.as_deref(), Some("approval-42"));

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
            text: "ping".to_string(),
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
