//! Integration coverage for workflow bootstrap semantics.

use chrono::Utc;
use openfang_kernel::workflow::{Workflow, WorkflowBootstrapErrorLevel, WorkflowId};
use openfang_kernel::OpenFangKernel;
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use pretty_assertions::assert_eq;
use std::collections::HashSet;
use std::path::Path;

fn workflow_bootstrap_config(home_dir: &Path, workflows_dir: &Path) -> KernelConfig {
    KernelConfig {
        home_dir: home_dir.to_path_buf(),
        data_dir: home_dir.join("data"),
        workflows_dir: Some(workflows_dir.to_path_buf()),
        default_model: DefaultModelConfig {
            provider: "ollama".to_string(),
            model: "test-model".to_string(),
            api_key_env: "OLLAMA_API_KEY".to_string(),
            base_url: None,
        },
        ..KernelConfig::default()
    }
}

fn stored_workflow(name: &str) -> Workflow {
    Workflow {
        id: WorkflowId::new(),
        name: name.to_string(),
        description: format!("workflow {name}"),
        steps: Vec::new(),
        created_at: Utc::now(),
    }
}

fn write_workflow_json(workflows_dir: &Path, file_name: &str, workflow: &Workflow) {
    std::fs::create_dir_all(workflows_dir).expect("workflow dir should be created");
    std::fs::write(
        workflows_dir.join(file_name),
        serde_json::to_string_pretty(workflow).expect("workflow should serialize"),
    )
    .expect("workflow file should be written");
}

fn normalize_error_set(
    errors: &[openfang_kernel::workflow::WorkflowBootstrapError],
) -> Vec<(String, String, WorkflowBootstrapErrorLevel)> {
    errors
        .iter()
        .map(|error| {
            (
                error.path.display().to_string(),
                error.message.clone(),
                error.level,
            )
        })
        .collect::<Vec<_>>()
}

#[tokio::test]
async fn restart_with_existing_workflow_files_yields_stable_registry() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let workflows_dir = temp_dir.path().join("workflows");
    let alpha = stored_workflow("alpha");
    let beta = stored_workflow("beta");
    write_workflow_json(&workflows_dir, "a-alpha.json", &alpha);
    write_workflow_json(&workflows_dir, "b-beta.json", &beta);

    let first_home = temp_dir.path().join("home-first");
    let first_kernel =
        OpenFangKernel::boot_with_config(workflow_bootstrap_config(&first_home, &workflows_dir))
            .expect("first kernel should boot");
    let first_bootstrap = first_kernel.bootstrap_workflow_definitions().await;
    assert_eq!(first_bootstrap.loaded, 2);
    assert!(first_kernel.workflows.is_ready());
    let first_registry = first_kernel
        .workflows
        .list_workflows()
        .await
        .into_iter()
        .map(|workflow| workflow.id)
        .collect::<HashSet<_>>();

    let second_home = temp_dir.path().join("home-second");
    let second_kernel =
        OpenFangKernel::boot_with_config(workflow_bootstrap_config(&second_home, &workflows_dir))
            .expect("second kernel should boot");
    let second_bootstrap = second_kernel.bootstrap_workflow_definitions().await;
    assert_eq!(second_bootstrap.loaded, 2);
    assert!(second_kernel.workflows.is_ready());
    let second_registry = second_kernel
        .workflows
        .list_workflows()
        .await
        .into_iter()
        .map(|workflow| workflow.id)
        .collect::<HashSet<_>>();

    assert_eq!(first_registry, second_registry);
    assert_eq!(first_registry, HashSet::from([alpha.id, beta.id]));
}

#[tokio::test]
async fn broken_workflow_files_surface_startup_behavior_consistently() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let workflows_dir = temp_dir.path().join("workflows");
    write_workflow_json(&workflows_dir, "a-valid.json", &stored_workflow("valid"));
    std::fs::write(workflows_dir.join("b-invalid.json"), "{invalid json")
        .expect("invalid workflow file should be written");

    let first_home = temp_dir.path().join("home-first");
    let first_kernel =
        OpenFangKernel::boot_with_config(workflow_bootstrap_config(&first_home, &workflows_dir))
            .expect("first kernel should boot");
    let first_bootstrap = first_kernel.bootstrap_workflow_definitions().await;

    let second_home = temp_dir.path().join("home-second");
    let second_kernel =
        OpenFangKernel::boot_with_config(workflow_bootstrap_config(&second_home, &workflows_dir))
            .expect("second kernel should boot");
    let second_bootstrap = second_kernel.bootstrap_workflow_definitions().await;

    assert_eq!(first_bootstrap.loaded, 1);
    assert_eq!(first_bootstrap.skipped, 1);
    assert_eq!(second_bootstrap.loaded, 1);
    assert_eq!(second_bootstrap.skipped, 1);
    assert_eq!(
        normalize_error_set(&first_bootstrap.errors),
        normalize_error_set(&second_bootstrap.errors)
    );
}
