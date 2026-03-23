//! Integration coverage for dual-database kernel bootstrap.

use openfang_kernel::OpenFangKernel;
use openfang_types::config::KernelConfig;
use rusqlite::{Connection, OptionalExtension};

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

    assert_eq!(runtime_rows.len(), 1);
    assert_eq!(compozy_rows.len(), 1);
    assert_eq!(runtime_rows[0].0, 1);
    assert_eq!(compozy_rows[0].0, 1);
    assert_eq!(runtime_rows[0].1, "schema_migrations_bootstrap");
    assert_eq!(compozy_rows[0].1, "schema_migrations_bootstrap");

    kernel.shutdown();
}
