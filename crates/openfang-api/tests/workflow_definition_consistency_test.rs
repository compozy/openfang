//! Workflow definition consistency regressions.
//!
//! These tests exercise the workflow CRUD routes against a real kernel and
//! confirm that file-backed definitions remain the canonical source of truth.

use axum::Router;
use openfang_api::routes::{self, AppState};
use openfang_kernel::workflow::{Workflow, WorkflowId};
use openfang_kernel::OpenFangKernel;
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

struct WorkflowTestServer {
    base_url: String,
    state: Arc<AppState>,
    _temp_dir: tempfile::TempDir,
    workflows_dir: PathBuf,
}

impl Drop for WorkflowTestServer {
    fn drop(&mut self) {
        self.state.kernel.shutdown();
    }
}

impl WorkflowTestServer {
    fn workflow_path(&self, workflow_id: &str) -> PathBuf {
        self.workflows_dir.join(format!("{workflow_id}.json"))
    }
}

async fn start_workflow_test_server(workflows_dir: PathBuf) -> WorkflowTestServer {
    start_workflow_test_server_with_bootstrap(workflows_dir, true).await
}

async fn start_workflow_test_server_with_bootstrap(
    workflows_dir: PathBuf,
    bootstrap_workflows: bool,
) -> WorkflowTestServer {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let config = KernelConfig {
        home_dir: temp_dir.path().to_path_buf(),
        data_dir: temp_dir.path().join("data"),
        workflows_dir: Some(workflows_dir.clone()),
        default_model: DefaultModelConfig {
            provider: "ollama".to_string(),
            model: "test-model".to_string(),
            api_key_env: "OLLAMA_API_KEY".to_string(),
            base_url: None,
        },
        ..KernelConfig::default()
    };

    let kernel = OpenFangKernel::boot_with_config(config).expect("kernel should boot");
    let kernel = Arc::new(kernel);
    kernel.set_self_handle();
    if bootstrap_workflows {
        kernel.bootstrap_workflow_definitions().await;
    }

    let state = Arc::new(AppState {
        kernel,
        started_at: Instant::now(),
        peer_registry: None,
        bridge_manager: tokio::sync::Mutex::new(None),
        channels_config: tokio::sync::RwLock::new(Default::default()),
        shutdown_notify: Arc::new(tokio::sync::Notify::new()),
        clawhub_cache: dashmap::DashMap::new(),
        provider_probe_cache: openfang_runtime::provider_health::ProbeCache::new(),
    });

    let app = Router::new()
        .route(
            "/api/workflows",
            axum::routing::get(routes::list_workflows).post(routes::create_workflow),
        )
        .route(
            "/api/workflows/{id}",
            axum::routing::get(routes::get_workflow)
                .put(routes::update_workflow)
                .delete(routes::delete_workflow),
        )
        .route(
            "/api/workflows/{id}/runtime",
            axum::routing::get(routes::get_workflow_runtime),
        )
        .route(
            "/api/workflows/{id}/run",
            axum::routing::post(routes::run_workflow),
        )
        .route(
            "/api/v1/workflows/{id}/runtime",
            axum::routing::get(routes::get_workflow_runtime),
        )
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let address = listener.local_addr().expect("address should resolve");

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server should run");
    });

    WorkflowTestServer {
        base_url: format!("http://{address}"),
        state,
        _temp_dir: temp_dir,
        workflows_dir,
    }
}

fn stored_workflow(name: &str, description: &str) -> Workflow {
    Workflow {
        id: WorkflowId::new(),
        name: name.to_string(),
        description: description.to_string(),
        steps: Vec::new(),
        created_at: chrono::Utc::now(),
    }
}

fn write_workflow_definition(workflows_dir: &Path, file_name: &str, workflow: &Workflow) {
    std::fs::create_dir_all(workflows_dir).expect("workflow dir should be created");
    std::fs::write(
        workflows_dir.join(file_name),
        serde_json::to_string_pretty(workflow).expect("workflow should serialize"),
    )
    .expect("workflow file should be written");
}

fn workflow_payload(name: &str, description: &str, prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "description": description,
        "steps": [
            {
                "name": "step1",
                "agent_name": "assistant",
                "prompt": prompt,
                "mode": "sequential",
                "timeout_secs": 30
            }
        ]
    })
}

async fn create_workflow(
    client: &reqwest::Client,
    server: &WorkflowTestServer,
    payload: serde_json::Value,
) -> (reqwest::StatusCode, serde_json::Value) {
    let response = client
        .post(format!("{}/api/workflows", server.base_url))
        .json(&payload)
        .send()
        .await
        .expect("create workflow request should succeed");
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("create workflow body should deserialize");
    (status, body)
}

async fn update_workflow(
    client: &reqwest::Client,
    server: &WorkflowTestServer,
    workflow_id: &str,
    payload: serde_json::Value,
) -> (reqwest::StatusCode, serde_json::Value) {
    let response = client
        .put(format!("{}/api/workflows/{workflow_id}", server.base_url))
        .json(&payload)
        .send()
        .await
        .expect("update workflow request should succeed");
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("update workflow body should deserialize");
    (status, body)
}

async fn delete_workflow(
    client: &reqwest::Client,
    server: &WorkflowTestServer,
    workflow_id: &str,
) -> (reqwest::StatusCode, serde_json::Value) {
    let response = client
        .delete(format!("{}/api/workflows/{workflow_id}", server.base_url))
        .send()
        .await
        .expect("delete workflow request should succeed");
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("delete workflow body should deserialize");
    (status, body)
}

async fn get_workflow_runtime(
    client: &reqwest::Client,
    server: &WorkflowTestServer,
    workflow_id: &str,
) -> (reqwest::StatusCode, serde_json::Value) {
    let response = client
        .get(format!(
            "{}/api/v1/workflows/{workflow_id}/runtime",
            server.base_url
        ))
        .send()
        .await
        .expect("workflow runtime request should succeed");
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("workflow runtime body should deserialize");
    (status, body)
}

fn load_workflow_file(path: &Path) -> Workflow {
    let content = std::fs::read_to_string(path).expect("workflow file should exist");
    serde_json::from_str(&content).expect("workflow file should deserialize")
}

fn workflow_ids_from_disk(workflows_dir: &Path) -> HashSet<WorkflowId> {
    let mut workflow_ids = HashSet::new();
    let Ok(entries) = std::fs::read_dir(workflows_dir) else {
        return workflow_ids;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        workflow_ids.insert(load_workflow_file(&path).id);
    }

    workflow_ids
}

#[tokio::test]
async fn api_server_workflow_list_is_populated_before_first_request() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let workflows_dir = temp_dir.path().join("workflows");
    let preloaded_workflow = stored_workflow("startup-ready", "loaded before bind");
    write_workflow_definition(&workflows_dir, "b-startup.json", &preloaded_workflow);

    let server = start_workflow_test_server(workflows_dir).await;
    let client = reqwest::Client::new();

    let list_response = client
        .get(format!("{}/api/workflows", server.base_url))
        .send()
        .await
        .expect("list workflows request should succeed");
    assert_eq!(list_response.status(), reqwest::StatusCode::OK);
    let workflows = list_response
        .json::<Vec<serde_json::Value>>()
        .await
        .expect("workflow list should deserialize");
    let returned_ids = workflows
        .iter()
        .filter_map(|workflow| workflow["id"].as_str().map(ToOwned::to_owned))
        .collect::<HashSet<_>>();
    assert!(returned_ids.contains(&preloaded_workflow.id.to_string()));

    let run_response = client
        .post(format!(
            "{}/api/workflows/{}/run",
            server.base_url, preloaded_workflow.id
        ))
        .json(&serde_json::json!({ "input": "boot input" }))
        .send()
        .await
        .expect("workflow run request should succeed");
    assert_eq!(run_response.status(), reqwest::StatusCode::OK);
    let run_body = run_response
        .json::<serde_json::Value>()
        .await
        .expect("workflow run body should deserialize");
    assert_eq!(run_body["status"], "completed");
}

#[tokio::test]
async fn workflow_runtime_endpoint_reports_not_loaded_until_bootstrap_finishes() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let workflows_dir = temp_dir.path().join("workflows");
    let preloaded_workflow = stored_workflow("runtime-ready", "runtime check");
    write_workflow_definition(&workflows_dir, "a-runtime.json", &preloaded_workflow);

    let server = start_workflow_test_server_with_bootstrap(workflows_dir, false).await;
    let client = reqwest::Client::new();

    let (status, body) =
        get_workflow_runtime(&client, &server, &preloaded_workflow.id.to_string()).await;
    assert_eq!(status, reqwest::StatusCode::OK);
    assert_eq!(body["loaded"], false);
    assert_eq!(body["healthy"], false);

    let bootstrap = server.state.kernel.bootstrap_workflow_definitions().await;
    assert_eq!(bootstrap.loaded, 1);
    assert_eq!(bootstrap.skipped, 0);

    let (status, body) =
        get_workflow_runtime(&client, &server, &preloaded_workflow.id.to_string()).await;
    assert_eq!(status, reqwest::StatusCode::OK);
    assert_eq!(body["loaded"], true);
    assert_eq!(body["healthy"], true);
    assert_eq!(body["active_runs"], 0);
    assert_eq!(body["waiting_runs"], 0);
    assert!(body["last_run_at"].is_null());
}

#[tokio::test]
async fn create_returns_internal_server_error_when_definition_persist_fails() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let blocked_parent = temp_dir.path().join("blocked-parent");
    std::fs::write(&blocked_parent, "not a directory").expect("blocked parent should exist");
    let server = start_workflow_test_server(blocked_parent.join("workflows")).await;
    let client = reqwest::Client::new();

    let (status, _body) = create_workflow(
        &client,
        &server,
        workflow_payload("broken-create", "should fail", "create {{input}}"),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::INTERNAL_SERVER_ERROR);
    assert!(server
        .state
        .kernel
        .workflows
        .list_workflows()
        .await
        .is_empty());
}

#[tokio::test]
async fn update_returns_internal_server_error_when_definition_persist_fails() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let workflows_dir = temp_dir.path().join("workflows");
    let server = start_workflow_test_server(workflows_dir.clone()).await;
    let client = reqwest::Client::new();

    let (create_status, create_body) = create_workflow(
        &client,
        &server,
        workflow_payload("before-failure", "v1", "v1 {{input}}"),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);
    let workflow_id = create_body["workflow_id"]
        .as_str()
        .expect("workflow id should be present")
        .to_string();

    std::fs::create_dir_all(server.workflows_dir.join(format!("{workflow_id}.json.tmp")))
        .expect("blocking temp path should be created");

    let (update_status, _update_body) = update_workflow(
        &client,
        &server,
        &workflow_id,
        workflow_payload("after-failure", "v2", "updated {{input}}"),
    )
    .await;

    assert_eq!(update_status, reqwest::StatusCode::INTERNAL_SERVER_ERROR);
    let current_workflow = server
        .state
        .kernel
        .workflows
        .get_workflow(WorkflowId(
            workflow_id
                .parse()
                .expect("workflow id should parse into a UUID"),
        ))
        .await
        .expect("workflow should still be present");
    assert_eq!(current_workflow.name, "before-failure");

    let persisted_workflow = load_workflow_file(&server.workflow_path(&workflow_id));
    assert_eq!(persisted_workflow.name, "before-failure");
}

#[tokio::test]
async fn create_then_restart_then_reload_returns_same_definition() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let server = start_workflow_test_server(temp_dir.path().join("workflows")).await;
    let client = reqwest::Client::new();

    let (status, body) = create_workflow(
        &client,
        &server,
        workflow_payload("create-reload", "v1", "created {{input}}"),
    )
    .await;
    assert_eq!(status, reqwest::StatusCode::CREATED);

    let workflow_id = body["workflow_id"]
        .as_str()
        .expect("workflow id should be present")
        .to_string();
    let on_disk = load_workflow_file(&server.workflow_path(&workflow_id));

    let reloaded = server.state.kernel.bootstrap_workflow_definitions().await;
    assert_eq!(reloaded.loaded, 1);

    let in_memory = server
        .state
        .kernel
        .workflows
        .get_workflow(on_disk.id)
        .await
        .expect("workflow should be reloaded");
    assert_eq!(in_memory, on_disk);
}

#[tokio::test]
async fn update_then_restart_then_reload_reflects_updated_definition() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let server = start_workflow_test_server(temp_dir.path().join("workflows")).await;
    let client = reqwest::Client::new();

    let (create_status, create_body) = create_workflow(
        &client,
        &server,
        workflow_payload("before-update", "v1", "v1 {{input}}"),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);
    let workflow_id = create_body["workflow_id"]
        .as_str()
        .expect("workflow id should be present")
        .to_string();

    let (update_status, _update_body) = update_workflow(
        &client,
        &server,
        &workflow_id,
        workflow_payload("after-update", "v2", "updated {{input}}"),
    )
    .await;
    assert_eq!(update_status, reqwest::StatusCode::OK);

    let reloaded = server.state.kernel.bootstrap_workflow_definitions().await;
    assert_eq!(reloaded.loaded, 1);

    let reloaded_workflow = load_workflow_file(&server.workflow_path(&workflow_id));
    let in_memory = server
        .state
        .kernel
        .workflows
        .get_workflow(reloaded_workflow.id)
        .await
        .expect("workflow should still exist");
    assert_eq!(in_memory, reloaded_workflow);
    assert_eq!(in_memory.name, "after-update");
    assert_eq!(in_memory.description, "v2");
    assert_eq!(in_memory.steps[0].prompt_template, "updated {{input}}");
}

#[tokio::test]
async fn delete_then_restart_does_not_resurrect_workflow() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let server = start_workflow_test_server(temp_dir.path().join("workflows")).await;
    let client = reqwest::Client::new();

    let (create_status, create_body) = create_workflow(
        &client,
        &server,
        workflow_payload("before-delete", "v1", "delete {{input}}"),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);
    let workflow_id = create_body["workflow_id"]
        .as_str()
        .expect("workflow id should be present")
        .to_string();
    let workflow_uuid = WorkflowId(
        workflow_id
            .parse()
            .expect("workflow id should parse into a UUID"),
    );

    let (delete_status, _delete_body) = delete_workflow(&client, &server, &workflow_id).await;
    assert_eq!(delete_status, reqwest::StatusCode::OK);
    assert!(!server.workflow_path(&workflow_id).exists());

    let reloaded = server.state.kernel.bootstrap_workflow_definitions().await;
    assert_eq!(reloaded.loaded, 0);
    assert!(server
        .state
        .kernel
        .workflows
        .get_workflow(workflow_uuid)
        .await
        .is_none());
}

#[tokio::test]
async fn concurrent_update_and_reload_is_coherent() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let server = start_workflow_test_server(temp_dir.path().join("workflows")).await;
    let client = reqwest::Client::new();

    let (create_status, create_body) = create_workflow(
        &client,
        &server,
        workflow_payload("before-concurrency", "v1", "v1 {{input}}"),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);

    let workflow_id = create_body["workflow_id"]
        .as_str()
        .expect("workflow id should be present")
        .to_string();
    let workflow_uuid = WorkflowId(
        workflow_id
            .parse()
            .expect("workflow id should parse into a UUID"),
    );

    let observed_prompts = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let stop_reading = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reader_kernel = server.state.kernel.clone();
    let observed_prompts_reader = observed_prompts.clone();
    let stop_reading_reader = stop_reading.clone();
    let reader = tokio::spawn(async move {
        while !stop_reading_reader.load(std::sync::atomic::Ordering::SeqCst) {
            if let Some(workflow) = reader_kernel.workflows.get_workflow(workflow_uuid).await {
                observed_prompts_reader
                    .lock()
                    .await
                    .push(workflow.steps[0].prompt_template.clone());
            }
            tokio::task::yield_now().await;
        }
    });

    let update_task = {
        let client = client.clone();
        let base_url = server.base_url.clone();
        let workflow_id = workflow_id.clone();
        tokio::spawn(async move {
            client
                .put(format!("{base_url}/api/workflows/{workflow_id}"))
                .json(&workflow_payload(
                    "after-concurrency",
                    "v2",
                    "updated {{input}}",
                ))
                .send()
                .await
                .expect("update request should succeed")
        })
    };
    let reload_task = {
        let kernel = server.state.kernel.clone();
        tokio::spawn(async move { kernel.bootstrap_workflow_definitions().await })
    };

    let update_response = update_task
        .await
        .expect("update task should join successfully");
    let reloaded = reload_task
        .await
        .expect("reload task should join successfully");

    stop_reading.store(true, std::sync::atomic::Ordering::SeqCst);
    reader.await.expect("reader task should join successfully");

    assert_eq!(update_response.status(), reqwest::StatusCode::OK);
    assert_eq!(reloaded.loaded, 1);

    let final_workflow = server
        .state
        .kernel
        .workflows
        .get_workflow(workflow_uuid)
        .await
        .expect("workflow should still exist");
    let persisted_workflow = load_workflow_file(&server.workflow_path(&workflow_id));
    assert_eq!(final_workflow, persisted_workflow);
    assert_eq!(final_workflow.name, "after-concurrency");
    assert_eq!(final_workflow.steps[0].prompt_template, "updated {{input}}");

    let observed_prompts = observed_prompts.lock().await;
    assert!(!observed_prompts.is_empty());
    assert!(observed_prompts
        .iter()
        .all(|prompt| { prompt == "v1 {{input}}" || prompt == "updated {{input}}" }));
}

#[tokio::test]
async fn runtime_registry_and_file_store_stay_aligned_after_route_mutations() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let server = start_workflow_test_server(temp_dir.path().join("workflows")).await;
    let client = reqwest::Client::new();

    let (create_status, create_body) = create_workflow(
        &client,
        &server,
        workflow_payload("aligned-routes", "v1", "v1 {{input}}"),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);
    let workflow_id = create_body["workflow_id"]
        .as_str()
        .expect("workflow id should be present")
        .to_string();
    let workflow_uuid = WorkflowId(
        workflow_id
            .parse()
            .expect("workflow id should parse into a UUID"),
    );

    let memory_ids = server
        .state
        .kernel
        .workflows
        .list_workflows()
        .await
        .into_iter()
        .map(|workflow| workflow.id)
        .collect::<HashSet<_>>();
    assert_eq!(memory_ids, workflow_ids_from_disk(&server.workflows_dir));

    let (update_status, _update_body) = update_workflow(
        &client,
        &server,
        &workflow_id,
        workflow_payload("aligned-routes", "v2", "updated {{input}}"),
    )
    .await;
    assert_eq!(update_status, reqwest::StatusCode::OK);
    assert!(server
        .state
        .kernel
        .workflows
        .get_workflow(workflow_uuid)
        .await
        .is_some());
    let updated_memory_ids = server
        .state
        .kernel
        .workflows
        .list_workflows()
        .await
        .into_iter()
        .map(|workflow| workflow.id)
        .collect::<HashSet<_>>();
    assert_eq!(
        updated_memory_ids,
        workflow_ids_from_disk(&server.workflows_dir)
    );

    let (delete_status, _delete_body) = delete_workflow(&client, &server, &workflow_id).await;
    assert_eq!(delete_status, reqwest::StatusCode::OK);
    let final_memory_ids = server
        .state
        .kernel
        .workflows
        .list_workflows()
        .await
        .into_iter()
        .map(|workflow| workflow.id)
        .collect::<HashSet<_>>();
    assert_eq!(
        final_memory_ids,
        workflow_ids_from_disk(&server.workflows_dir)
    );
    assert!(final_memory_ids.is_empty());
}
