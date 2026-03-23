//! Reusable SQLite migration runner for kernel-owned databases.
//!
//! The runner keeps `runtime.db` and `compozy.db` on independent migration
//! streams while sharing a single ordered execution path.

use openfang_memory::{
    AGENT_RUNTIME_CORE_MIGRATION_SQL, AGENT_SESSIONS_AND_MESSAGES_MIGRATION_SQL,
    SCHEDULE_RUNTIME_CORE_MIGRATION_SQL, WORKFLOW_CHECKPOINT_MIGRATION_SQL,
    WORKFLOW_RUNTIME_DURABILITY_MIGRATION_SQL, WORKFLOW_RUN_CORE_MIGRATION_SQL,
    WORKFLOW_SIGNAL_MIGRATION_SQL, WORKFLOW_SIGNAL_WAITING_STATE_MIGRATION_SQL,
};
use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;

/// Stable migration identity for kernel-owned SQLite databases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseIdentity {
    /// The platform runtime database.
    Runtime,
    /// The durable Compozy domain database.
    Compozy,
}

impl DatabaseIdentity {
    /// Returns the user-facing database file name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Runtime => "runtime.db",
            Self::Compozy => "compozy.db",
        }
    }
}

impl std::fmt::Display for DatabaseIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One ordered migration unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationStep<'a> {
    /// Per-database migration version.
    pub version: u32,
    /// Human-readable migration name.
    pub name: &'a str,
    /// SQL batch executed when the step has not yet been applied.
    pub sql: &'a str,
}

impl<'a> MigrationStep<'a> {
    /// Creates a static migration descriptor.
    pub const fn new(version: u32, name: &'a str, sql: &'a str) -> Self {
        Self { version, name, sql }
    }
}

/// Typed failures surfaced by the schema migration runner.
#[derive(Debug, Error)]
pub enum MigrationError {
    /// Failed to ensure the migration tracking table exists.
    #[error("failed to bootstrap schema_migration for {database}: {source}")]
    SchemaBootstrapFailed {
        /// Target database.
        database: DatabaseIdentity,
        /// Underlying SQLite failure.
        #[source]
        source: rusqlite::Error,
    },
    /// Defensive error for duplicate step versions in the provided stream.
    #[error(
        "migration {version} ({name}) is already applied or duplicated in the {database} stream"
    )]
    AlreadyApplied {
        /// Target database.
        database: DatabaseIdentity,
        /// Duplicate or conflicting version.
        version: u32,
        /// Human-readable step name.
        name: String,
    },
    /// Failed while checking whether a version is already recorded.
    #[error("failed to query migration {version} in {database}: {source}")]
    VersionQueryFailed {
        /// Target database.
        database: DatabaseIdentity,
        /// Version whose status was queried.
        version: u32,
        /// Underlying SQLite failure.
        #[source]
        source: rusqlite::Error,
    },
    /// Failed while executing or recording a migration step.
    #[error("failed to execute migration {version} ({name}) for {database}: {source}")]
    ExecutionFailed {
        /// Target database.
        database: DatabaseIdentity,
        /// Failing version.
        version: u32,
        /// Failing step name.
        name: String,
        /// Underlying SQLite failure.
        #[source]
        source: rusqlite::Error,
    },
}

const SCHEMA_MIGRATION_BOOTSTRAP_SQL: &str = "
CREATE TABLE IF NOT EXISTS schema_migration (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL
);";

const RUNTIME_BOOTSTRAP_MIGRATIONS: &[MigrationStep<'static>] = &[
    MigrationStep::new(
        1,
        "schema_migrations_bootstrap",
        SCHEMA_MIGRATION_BOOTSTRAP_SQL,
    ),
    MigrationStep::new(
        2,
        "0002_agent_runtime_core",
        AGENT_RUNTIME_CORE_MIGRATION_SQL,
    ),
    MigrationStep::new(
        3,
        "0003_agent_sessions_and_messages",
        AGENT_SESSIONS_AND_MESSAGES_MIGRATION_SQL,
    ),
    MigrationStep::new(
        4,
        "0004_schedule_runtime_core",
        SCHEDULE_RUNTIME_CORE_MIGRATION_SQL,
    ),
];

const COMPOZY_BOOTSTRAP_MIGRATIONS: &[MigrationStep<'static>] = &[
    MigrationStep::new(
        1,
        "schema_migrations_bootstrap",
        SCHEMA_MIGRATION_BOOTSTRAP_SQL,
    ),
    MigrationStep::new(2, "0002_workflow_run_core", WORKFLOW_RUN_CORE_MIGRATION_SQL),
    MigrationStep::new(
        3,
        "0003_workflow_checkpoint",
        WORKFLOW_CHECKPOINT_MIGRATION_SQL,
    ),
    MigrationStep::new(4, "0004_workflow_signal", WORKFLOW_SIGNAL_MIGRATION_SQL),
    MigrationStep::new(
        5,
        "0005_workflow_runtime_durability",
        WORKFLOW_RUNTIME_DURABILITY_MIGRATION_SQL,
    ),
    MigrationStep::new(
        6,
        "0006_workflow_signal_waiting_state",
        WORKFLOW_SIGNAL_WAITING_STATE_MIGRATION_SQL,
    ),
];

/// Returns the current `runtime.db` migration slice.
pub(crate) const fn runtime_migration_steps() -> &'static [MigrationStep<'static>] {
    RUNTIME_BOOTSTRAP_MIGRATIONS
}

/// Returns the current `compozy.db` migration slice.
pub(crate) const fn compozy_migration_steps() -> &'static [MigrationStep<'static>] {
    COMPOZY_BOOTSTRAP_MIGRATIONS
}

/// Applies the ordered migration slice for one database.
pub(crate) fn run_migrations(
    conn: &Connection,
    database: DatabaseIdentity,
    steps: &[MigrationStep<'_>],
) -> Result<(), MigrationError> {
    ensure_schema_migration_table(conn, database)?;

    let mut ordered_steps: Vec<_> = steps.iter().collect();
    ordered_steps.sort_by_key(|step| step.version);

    let mut previous_version = None;
    for step in ordered_steps {
        if previous_version == Some(step.version) {
            return Err(MigrationError::AlreadyApplied {
                database,
                version: step.version,
                name: step.name.to_string(),
            });
        }
        previous_version = Some(step.version);

        if migration_is_applied(conn, database, step.version)? {
            continue;
        }

        let transaction =
            conn.unchecked_transaction()
                .map_err(|source| MigrationError::ExecutionFailed {
                    database,
                    version: step.version,
                    name: step.name.to_string(),
                    source,
                })?;

        if let Err(source) = transaction.execute_batch(step.sql) {
            return Err(MigrationError::ExecutionFailed {
                database,
                version: step.version,
                name: step.name.to_string(),
                source,
            });
        }

        if let Err(source) = transaction.execute(
            "INSERT INTO schema_migration (version, name, applied_at)
             VALUES (?1, ?2, datetime('now'))",
            params![step.version, step.name],
        ) {
            return Err(MigrationError::ExecutionFailed {
                database,
                version: step.version,
                name: step.name.to_string(),
                source,
            });
        }

        transaction
            .commit()
            .map_err(|source| MigrationError::ExecutionFailed {
                database,
                version: step.version,
                name: step.name.to_string(),
                source,
            })?;
    }

    Ok(())
}

fn ensure_schema_migration_table(
    conn: &Connection,
    database: DatabaseIdentity,
) -> Result<(), MigrationError> {
    conn.execute_batch(SCHEMA_MIGRATION_BOOTSTRAP_SQL)
        .map_err(|source| MigrationError::SchemaBootstrapFailed { database, source })
}

fn migration_is_applied(
    conn: &Connection,
    database: DatabaseIdentity,
    version: u32,
) -> Result<bool, MigrationError> {
    conn.query_row(
        "SELECT 1 FROM schema_migration WHERE version = ?1",
        [version],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map(|maybe| maybe.is_some())
    .map_err(|source| MigrationError::VersionQueryFailed {
        database,
        version,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;
    use pretty_assertions::assert_eq;
    use rusqlite::params;

    fn applied_versions(conn: &Connection) -> Vec<u32> {
        let mut stmt = conn
            .prepare("SELECT version FROM schema_migration ORDER BY version")
            .expect("prepare applied versions query");
        let rows = stmt
            .query_map([], |row| row.get::<_, u32>(0))
            .expect("query applied versions");

        rows.map(|row| row.expect("version row")).collect()
    }

    fn migration_rows(conn: &Connection) -> Vec<(u32, String, String)> {
        let mut stmt = conn
            .prepare("SELECT version, name, applied_at FROM schema_migration ORDER BY version")
            .expect("prepare migration rows query");
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .expect("query migration rows");

        rows.map(|row| row.expect("migration row")).collect()
    }

    fn table_exists(conn: &Connection, table_name: &str) -> bool {
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

    fn index_exists(conn: &Connection, index_name: &str) -> bool {
        conn.query_row(
            "SELECT name
             FROM sqlite_master
             WHERE type = 'index' AND name = ?1",
            [index_name],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .expect("index exists query")
        .is_some()
    }

    fn table_columns(conn: &Connection, table_name: &str) -> Vec<String> {
        let pragma = format!("PRAGMA table_info('{table_name}')");
        let mut stmt = conn.prepare(&pragma).expect("prepare table info query");
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query table info rows");

        rows.map(|row| row.expect("column row")).collect()
    }

    fn apply_runtime_migrations(conn: &Connection) {
        run_migrations(conn, DatabaseIdentity::Runtime, runtime_migration_steps())
            .expect("runtime migrations should succeed");
    }

    fn apply_compozy_migrations(conn: &Connection) {
        run_migrations(conn, DatabaseIdentity::Compozy, compozy_migration_steps())
            .expect("compozy migrations should succeed");
    }

    #[test]
    fn migration_runner_should_apply_steps_in_version_order() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        let steps = [
            MigrationStep::new(
                3,
                "insert_three",
                "INSERT INTO applied_order (value) VALUES (3);",
            ),
            MigrationStep::new(
                1,
                "create_table",
                "
                CREATE TABLE applied_order (value INTEGER NOT NULL);
                INSERT INTO applied_order (value) VALUES (1);
                ",
            ),
            MigrationStep::new(
                2,
                "insert_two",
                "INSERT INTO applied_order (value) VALUES (2);",
            ),
        ];

        run_migrations(&conn, DatabaseIdentity::Runtime, &steps).expect("runner should succeed");

        let mut stmt = conn
            .prepare("SELECT value FROM applied_order ORDER BY rowid")
            .expect("prepare applied order query");
        let rows = stmt
            .query_map([], |row| row.get::<_, i64>(0))
            .expect("query applied order");
        let applied: Vec<i64> = rows.map(|row| row.expect("applied row")).collect();

        assert_eq!(applied, vec![1, 2, 3]);
        assert_eq!(applied_versions(&conn), vec![1, 2, 3]);
    }

    #[test]
    fn migration_runner_should_skip_already_applied_steps() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        let steps = [
            MigrationStep::new(
                1,
                "create_counter",
                "CREATE TABLE counter (value INTEGER NOT NULL DEFAULT 0);",
            ),
            MigrationStep::new(
                2,
                "insert_counter",
                "INSERT INTO counter (value) VALUES (1);",
            ),
        ];

        run_migrations(&conn, DatabaseIdentity::Runtime, &steps).expect("first run succeeds");
        let initial_row_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migration", [], |row| {
                row.get(0)
            })
            .expect("initial migration row count");
        let initial_counter_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM counter", [], |row| row.get(0))
            .expect("initial counter rows");

        run_migrations(&conn, DatabaseIdentity::Runtime, &steps).expect("second run succeeds");
        let second_row_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migration", [], |row| {
                row.get(0)
            })
            .expect("second migration row count");
        let second_counter_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM counter", [], |row| row.get(0))
            .expect("second counter rows");

        assert_eq!(second_row_count, initial_row_count);
        assert_eq!(second_counter_rows, initial_counter_rows);
    }

    #[test]
    fn migration_runner_should_record_name_and_applied_at_for_each_step() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        let steps = [
            MigrationStep::new(
                1,
                "create_alpha",
                "CREATE TABLE alpha (id INTEGER PRIMARY KEY);",
            ),
            MigrationStep::new(
                2,
                "create_beta",
                "CREATE TABLE beta (id INTEGER PRIMARY KEY);",
            ),
        ];

        run_migrations(&conn, DatabaseIdentity::Runtime, &steps).expect("runner should succeed");

        for (version, name, applied_at) in migration_rows(&conn) {
            assert!(version >= 1);
            assert!(!name.is_empty(), "migration name should be recorded");
            assert!(
                NaiveDateTime::parse_from_str(&applied_at, "%Y-%m-%d %H:%M:%S").is_ok(),
                "applied_at should contain a SQLite datetime string: {applied_at}"
            );
        }
    }

    #[test]
    fn migration_runner_should_surface_failure_from_bad_sql() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        let steps = [
            MigrationStep::new(
                1,
                "create_ok",
                "CREATE TABLE ok_table (id INTEGER PRIMARY KEY);",
            ),
            MigrationStep::new(2, "bad_sql", "CREATE TABL definitely_invalid (id INTEGER);"),
        ];

        let error = run_migrations(&conn, DatabaseIdentity::Runtime, &steps)
            .expect_err("runner should fail");

        match error {
            MigrationError::ExecutionFailed {
                version,
                name,
                database,
                ..
            } => {
                assert_eq!(database, DatabaseIdentity::Runtime);
                assert_eq!(version, 2);
                assert_eq!(name, "bad_sql");
            }
            other => panic!("expected execution failure, got {other:?}"),
        }

        let bad_step_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migration WHERE version = ?1",
                params![2_u32],
                |row| row.get(0),
            )
            .expect("bad step row count");
        assert_eq!(bad_step_rows, 0);
    }

    #[test]
    fn migration_runner_should_roll_back_failed_step() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        let steps = [MigrationStep::new(
            1,
            "partial_failure",
            "
            CREATE TABLE rolled_back_table (id INTEGER PRIMARY KEY);
            CREATE TABLE rolled_back_table (id INTEGER PRIMARY KEY);
            ",
        )];

        let error = run_migrations(&conn, DatabaseIdentity::Runtime, &steps)
            .expect_err("runner should fail");

        match error {
            MigrationError::ExecutionFailed { version, .. } => assert_eq!(version, 1),
            other => panic!("expected execution failure, got {other:?}"),
        }

        assert!(
            !table_exists(&conn, "rolled_back_table"),
            "failed step should roll back prior DDL changes"
        );
    }

    #[test]
    fn schema_migration_bootstrap_should_be_idempotent() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(SCHEMA_MIGRATION_BOOTSTRAP_SQL)
            .expect("manual bootstrap");
        let steps = [MigrationStep::new(
            1,
            "schema_migrations_bootstrap",
            SCHEMA_MIGRATION_BOOTSTRAP_SQL,
        )];

        run_migrations(&conn, DatabaseIdentity::Runtime, &steps).expect("first run succeeds");
        run_migrations(&conn, DatabaseIdentity::Runtime, &steps).expect("second run succeeds");

        assert_eq!(applied_versions(&conn), vec![1]);
    }

    #[test]
    fn migration_runner_should_bootstrap_schema_migration_table_if_absent() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        let steps = [MigrationStep::new(
            1,
            "schema_migrations_bootstrap",
            SCHEMA_MIGRATION_BOOTSTRAP_SQL,
        )];

        run_migrations(&conn, DatabaseIdentity::Runtime, &steps).expect("runner should succeed");

        assert!(table_exists(&conn, "schema_migration"));
        assert_eq!(applied_versions(&conn), vec![1]);
    }

    #[test]
    fn runtime_db_migration_should_create_agent_runtime_table() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        apply_runtime_migrations(&conn);

        assert!(table_exists(&conn, "agent_runtime"));
    }

    #[test]
    fn runtime_db_migration_should_create_agent_session_and_message_tables() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        apply_runtime_migrations(&conn);

        assert!(table_exists(&conn, "agent_session"));
        assert!(table_exists(&conn, "agent_message"));
    }

    #[test]
    fn runtime_db_migration_should_create_schedule_tables() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        apply_runtime_migrations(&conn);

        assert!(table_exists(&conn, "schedule_runtime"));
        assert!(table_exists(&conn, "schedule_execution"));
    }

    #[test]
    fn runtime_db_migration_should_create_required_indexes() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        apply_runtime_migrations(&conn);

        for index_name in [
            "idx_agent_session_agent_id",
            "idx_agent_message_agent_session",
            "idx_agent_message_session",
            "idx_schedule_execution_schedule",
            "idx_schedule_execution_fired",
        ] {
            assert!(
                index_exists(&conn, index_name),
                "missing required runtime.db index: {index_name}"
            );
        }
    }

    #[test]
    fn compozy_db_migration_should_create_workflow_run_table() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        apply_compozy_migrations(&conn);

        assert!(table_exists(&conn, "workflow_run"));
        assert_eq!(
            table_columns(&conn, "workflow_run"),
            vec![
                "run_id".to_string(),
                "workflow_id".to_string(),
                "workflow_version".to_string(),
                "status".to_string(),
                "input_json".to_string(),
                "vars_json".to_string(),
                "current_step_id".to_string(),
                "waiting_kind".to_string(),
                "waiting_ref".to_string(),
                "active_dispatch_id".to_string(),
                "active_hitl_request_id".to_string(),
                "labels_json".to_string(),
                "metadata_json".to_string(),
                "error_json".to_string(),
                "started_at".to_string(),
                "updated_at".to_string(),
                "completed_at".to_string(),
            ]
        );
    }

    #[test]
    fn compozy_db_migration_should_create_workflow_checkpoint_table() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        apply_compozy_migrations(&conn);

        assert!(table_exists(&conn, "workflow_checkpoint"));
        assert_eq!(
            table_columns(&conn, "workflow_checkpoint"),
            vec![
                "checkpoint_id".to_string(),
                "run_id".to_string(),
                "step_id".to_string(),
                "kind".to_string(),
                "data_json".to_string(),
                "created_at".to_string(),
            ]
        );
    }

    #[test]
    fn compozy_db_migration_should_create_workflow_signal_table() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        apply_compozy_migrations(&conn);

        assert!(table_exists(&conn, "workflow_signal"));
        assert_eq!(
            table_columns(&conn, "workflow_signal"),
            vec![
                "signal_id".to_string(),
                "run_id".to_string(),
                "name".to_string(),
                "payload_json".to_string(),
                "source".to_string(),
                "idempotency_key".to_string(),
                "consumed".to_string(),
                "created_at".to_string(),
                "consumed_at".to_string(),
            ]
        );
    }

    #[test]
    fn compozy_db_migration_should_create_all_required_indexes() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        apply_compozy_migrations(&conn);

        for index_name in [
            "idx_workflow_run_workflow_id",
            "idx_workflow_run_status",
            "idx_workflow_run_updated_at",
            "idx_workflow_checkpoint_run",
            "idx_workflow_signal_run",
            "idx_workflow_signal_run_consumed",
            "idx_workflow_signal_run_name",
            "idx_workflow_signal_run_idempotency",
        ] {
            assert!(
                index_exists(&conn, index_name),
                "missing required compozy.db index: {index_name}"
            );
        }
    }

    #[test]
    fn runtime_db_migration_should_not_include_compozy_domain_tables() {
        let runtime_sql = runtime_migration_steps()
            .iter()
            .map(|step| step.sql)
            .collect::<Vec<_>>()
            .join("\n");

        for disallowed_table in [
            "workflow_run",
            "workflow_checkpoint",
            "workflow_signal",
            "task",
            "subtask",
            "artifact",
            "looper_run",
        ] {
            assert!(
                !runtime_sql.contains(disallowed_table),
                "runtime.db migrations unexpectedly reference {disallowed_table}"
            );
        }
    }

    #[test]
    fn runtime_db_migration_should_not_include_agent_definition_fields() {
        let runtime_sql = runtime_migration_steps()
            .iter()
            .map(|step| step.sql)
            .collect::<Vec<_>>()
            .join("\n");

        for disallowed_column in [
            "system_prompt",
            "skills",
            "model",
            "agent_name",
            "target_agent",
            "cron_expression",
        ] {
            assert!(
                !runtime_sql.contains(disallowed_column),
                "runtime.db migrations unexpectedly reference definition field {disallowed_column}"
            );
        }
    }

    #[test]
    fn compozy_db_migration_should_not_include_later_phase_tables_or_cross_database_sql() {
        let compozy_sql = compozy_migration_steps()
            .iter()
            .map(|step| step.sql)
            .collect::<Vec<_>>()
            .join("\n");

        for disallowed_fragment in [
            "CREATE TABLE agent_dispatch",
            "CREATE TABLE IF NOT EXISTS agent_dispatch",
            "CREATE TABLE hitl_request",
            "CREATE TABLE IF NOT EXISTS hitl_request",
            "CREATE TABLE task",
            "CREATE TABLE IF NOT EXISTS task",
            "CREATE TABLE subtask",
            "CREATE TABLE IF NOT EXISTS subtask",
            "ATTACH DATABASE",
            "attach database",
        ] {
            assert!(
                !compozy_sql.contains(disallowed_fragment),
                "compozy.db migrations unexpectedly reference {disallowed_fragment}"
            );
        }
    }

    #[test]
    fn compozy_db_migration_should_store_workflow_definition_references_only() {
        let workflow_run_sql = compozy_migration_steps()
            .iter()
            .find(|step| step.name == "0002_workflow_run_core")
            .expect("workflow_run migration should exist")
            .sql;

        assert!(workflow_run_sql.contains("workflow_id"));
        assert!(workflow_run_sql.contains("workflow_version"));
        for disallowed_column in [
            "workflow_toml",
            "workflow_json",
            "step_definition",
            "step_definitions",
            "compiled_workflow",
        ] {
            assert!(
                !workflow_run_sql.contains(disallowed_column),
                "workflow_run migration unexpectedly embeds definition column {disallowed_column}"
            );
        }
    }

    #[test]
    fn migration_error_should_carry_failing_version_and_name() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        let steps = [MigrationStep::new(
            7,
            "broken_step",
            "CREATE TABLE broken_step (id INTEGER PRIMARY KEY",
        )];

        let error = run_migrations(&conn, DatabaseIdentity::Compozy, &steps)
            .expect_err("runner should fail");

        match error {
            MigrationError::ExecutionFailed {
                version,
                name,
                database,
                ..
            } => {
                assert_eq!(database, DatabaseIdentity::Compozy);
                assert_eq!(version, 7);
                assert_eq!(name, "broken_step");
            }
            other => panic!("expected execution failure, got {other:?}"),
        }
    }
}
