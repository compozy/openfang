//! Integration coverage for dual-database kernel bootstrap.

use openfang_kernel::OpenFangKernel;
use openfang_types::config::KernelConfig;

fn boot_test_config(root: &std::path::Path) -> KernelConfig {
    KernelConfig {
        home_dir: root.to_path_buf(),
        data_dir: root.join("data"),
        ..KernelConfig::default()
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
