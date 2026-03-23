//! Reusable SQLite migration runner for kernel-owned databases.
//!
//! The runner keeps `runtime.db` and `compozy.db` on independent migration
//! streams while sharing a single ordered execution path.

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

const RUNTIME_BOOTSTRAP_MIGRATIONS: &[MigrationStep<'static>] = &[MigrationStep::new(
    1,
    "schema_migrations_bootstrap",
    SCHEMA_MIGRATION_BOOTSTRAP_SQL,
)];

const COMPOZY_BOOTSTRAP_MIGRATIONS: &[MigrationStep<'static>] = &[MigrationStep::new(
    1,
    "schema_migrations_bootstrap",
    SCHEMA_MIGRATION_BOOTSTRAP_SQL,
)];

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
