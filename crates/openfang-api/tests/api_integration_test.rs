//! Real HTTP integration tests for the OpenFang API.
//!
//! These tests boot a real kernel, start a real axum HTTP server on a random
//! port, and hit actual endpoints with reqwest.  No mocking.
//!
//! Tests that require an LLM API call are gated behind GROQ_API_KEY.
//!
//! Run: cargo test -p openfang-api --test api_integration_test -- --nocapture

use axum::Router;
use openfang_api::middleware;
use openfang_api::routes::{self, AppState};
use openfang_api::ws;
use openfang_kernel::workflow::{
    ErrorMode, StepAgent, StepMode, Workflow, WorkflowId, WorkflowStep,
};
use openfang_kernel::workflow_compiler::{compile_workflow_definition, WorkflowCompileRegistry};
use openfang_kernel::OpenFangKernel;
use openfang_memory::{
    now_timestamp, DispatchKind, DispatchRecord, DispatchRepository, DispatchStatus,
    WorkflowRunRecord, WorkflowRunStatus, WorkflowSignalRecord,
};
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use openfang_types::workflow::{
    FlowBlock, FlowMode, ResolvedRuntimeSettings, WorkflowIr, WorkflowIrStep, WorkflowIrStepKind,
    WorkflowV2Definition,
};
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{sleep, Duration};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Test infrastructure
// ---------------------------------------------------------------------------

struct TestServer {
    base_url: String,
    state: Arc<AppState>,
    _tmp: tempfile::TempDir,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.state.kernel.shutdown();
    }
}

/// Start a test server using ollama as default provider (no API key needed).
/// This lets the kernel boot without any real LLM credentials.
/// Tests that need actual LLM calls should use `start_test_server_with_llm()`.
async fn start_test_server() -> TestServer {
    start_test_server_with_provider("ollama", "test-model", "OLLAMA_API_KEY").await
}

/// Start a test server with Groq as the LLM provider (requires GROQ_API_KEY).
async fn start_test_server_with_llm() -> TestServer {
    start_test_server_with_provider("groq", "llama-3.3-70b-versatile", "GROQ_API_KEY").await
}

async fn start_test_server_with_provider(
    provider: &str,
    model: &str,
    api_key_env: &str,
) -> TestServer {
    let tmp = tempfile::tempdir().expect("Failed to create temp dir");

    let config = KernelConfig {
        home_dir: tmp.path().to_path_buf(),
        data_dir: tmp.path().join("data"),
        default_model: DefaultModelConfig {
            provider: provider.to_string(),
            model: model.to_string(),
            api_key_env: api_key_env.to_string(),
            base_url: None,
        },
        ..KernelConfig::default()
    };

    let kernel = OpenFangKernel::boot_with_config(config).expect("Kernel should boot");
    let kernel = Arc::new(kernel);
    kernel.set_self_handle();
    kernel.bootstrap_workflow_definitions().await;

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
        .route("/api/health", axum::routing::get(routes::health))
        .route("/api/status", axum::routing::get(routes::status))
        .route("/api/config", axum::routing::get(routes::get_config))
        .route(
            "/api/agents",
            axum::routing::get(routes::list_agents_legacy).post(routes::spawn_agent),
        )
        .route(
            "/api/agents/{id}/message",
            axum::routing::post(routes::send_message),
        )
        .route(
            "/api/agents/{id}/session",
            axum::routing::get(routes::get_agent_session),
        )
        .route("/api/agents/{id}/ws", axum::routing::get(ws::agent_ws))
        .route(
            "/api/agents/{id}",
            axum::routing::delete(routes::kill_agent),
        )
        .route(
            "/api/triggers",
            axum::routing::get(routes::list_triggers).post(routes::create_trigger),
        )
        .route(
            "/api/triggers/{id}",
            axum::routing::delete(routes::delete_trigger),
        )
        .route(
            "/api/workflows",
            axum::routing::get(routes::list_workflows).post(routes::create_workflow),
        )
        .route(
            "/api/workflows/{id}/run",
            axum::routing::post(routes::run_workflow),
        )
        .route(
            "/api/workflows/{id}/runs",
            axum::routing::get(routes::list_workflow_runs),
        )
        .route(
            "/api/v1/workflows",
            axum::routing::get(routes::list_workflow_definitions_v1)
                .post(routes::create_workflow_definition_v1),
        )
        .route(
            "/api/v1/workflows/{id}/runs",
            axum::routing::get(routes::list_workflow_runs_v1).post(routes::start_workflow_run_v1),
        )
        .route("/api/v1/runs", axum::routing::get(routes::list_runs_v1))
        .route("/api/v1/runs/{id}", axum::routing::get(routes::get_run_v1))
        .route(
            "/api/v1/runs/{id}/checkpoints",
            axum::routing::get(routes::get_run_checkpoints_v1),
        )
        .route(
            "/api/v1/runs/{id}/dispatches",
            axum::routing::get(routes::get_run_dispatches_v1),
        )
        .route(
            "/api/v1/runs/{id}/signals",
            axum::routing::get(routes::get_run_signals_v1).post(routes::post_run_signal_v1),
        )
        .route(
            "/api/v1/runs/{id}/pause",
            axum::routing::post(routes::pause_run_v1),
        )
        .route(
            "/api/v1/runs/{id}/resume",
            axum::routing::post(routes::resume_run_v1),
        )
        .route(
            "/api/v1/runs/{id}/cancel",
            axum::routing::post(routes::cancel_run_v1),
        )
        .route("/api/shutdown", axum::routing::post(routes::shutdown))
        .layer(axum::middleware::from_fn(middleware::request_logging))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind test server");
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    TestServer {
        base_url: format!("http://{}", addr),
        state,
        _tmp: tmp,
    }
}

/// Manifest that uses ollama (no API key required, won't make real LLM calls).
const TEST_MANIFEST: &str = r#"
name = "test-agent"
version = "0.1.0"
description = "Integration test agent"
author = "test"
module = "builtin:chat"

[model]
provider = "ollama"
model = "test-model"
system_prompt = "You are a test agent. Reply concisely."

[capabilities]
tools = ["file_read"]
memory_read = ["*"]
memory_write = ["self.*"]
"#;

/// Manifest that uses Groq for real LLM tests.
const LLM_MANIFEST: &str = r#"
name = "test-agent"
version = "0.1.0"
description = "Integration test agent"
author = "test"
module = "builtin:chat"

[model]
provider = "groq"
model = "llama-3.3-70b-versatile"
system_prompt = "You are a test agent. Reply concisely."

[capabilities]
tools = ["file_read"]
memory_read = ["*"]
memory_write = ["self.*"]
"#;

async fn create_empty_workflow(
    client: &reqwest::Client,
    server: &TestServer,
    name: &str,
) -> String {
    let response = client
        .post(format!("{}/api/v1/workflows", server.base_url))
        .json(&serde_json::json!({
            "id": name,
            "name": name,
            "version": "1.0.0",
            "description": "Durable workflow integration test",
            "enabled": true,
            "tags": ["durable"],
            "input": {
                "kind": "object",
                "required": ["issue_id"],
                "open": false,
                "fields": {
                    "issue_id": { "kind": "string" }
                }
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
        .send()
        .await
        .expect("workflow creation request should succeed");

    assert_eq!(response.status(), 201);
    let body: serde_json::Value = response
        .json()
        .await
        .expect("workflow creation response should deserialize");
    body["id"]
        .as_str()
        .expect("workflow id should be present")
        .to_string()
}

fn wait_signal_definition(workflow_id: WorkflowId) -> WorkflowV2Definition {
    serde_json::from_value(serde_json::json!({
        "id": workflow_id.to_string(),
        "name": "wait-signal-api-test",
        "version": "1.0.0",
        "description": "Wait signal api test",
        "input": { "kind": "any" },
        "output": {
            "kind": "object",
            "required": ["result"],
            "open": false,
            "fields": {
                "result": { "kind": "any" }
            }
        },
        "steps": [
            {
                "id": "await-approval",
                "name": "Await approval",
                "kind": "wait_signal",
                "uses": { "signal_name": "approval" },
                "flow": { "mode": "sequential" }
            },
            {
                "id": "after-approval",
                "name": "After approval",
                "kind": "noop",
                "save_as": "result",
                "flow": { "mode": "sequential" }
            }
        ],
        "outputs": {
            "result": "{{ vars.result }}"
        }
    }))
    .expect("wait signal definition should deserialize")
}

async fn create_waiting_signal_run(server: &TestServer) -> String {
    let workflow_id = WorkflowId::new();
    let definition = wait_signal_definition(workflow_id);
    let mut registry = WorkflowCompileRegistry::new();
    registry.set_workflows(std::iter::once(definition.id.clone()));
    let workflow_ir =
        compile_workflow_definition(&definition, &registry).expect("workflow should compile");

    server
        .state
        .kernel
        .workflows
        .register_workflow_v2_definition(definition.clone(), Vec::<String>::new())
        .await
        .expect("workflow v2 definition should register");
    server
        .state
        .kernel
        .workflows
        .register(Workflow {
            id: workflow_id,
            name: definition.name.clone(),
            description: definition.description.clone(),
            steps: Vec::new(),
            created_at: chrono::Utc::now(),
        })
        .await
        .expect("workflow registration should succeed");

    let run_id = server
        .state
        .kernel
        .workflows
        .create_run(workflow_id, "waiting input".to_string())
        .await
        .expect("workflow run should be created");
    server
        .state
        .kernel
        .execute_compiled_workflow_run(run_id, workflow_ir)
        .await
        .expect("workflow should park");

    run_id.to_string()
}

async fn wait_for_run_status(
    client: &reqwest::Client,
    server: &TestServer,
    run_id: &str,
    expected_status: &str,
) -> serde_json::Value {
    for _ in 0..40 {
        let response = client
            .get(format!("{}/api/v1/runs/{run_id}", server.base_url))
            .send()
            .await
            .expect("run detail request should succeed");
        let body: serde_json::Value = response
            .json()
            .await
            .expect("run detail response should deserialize");
        if body["status"] == expected_status {
            return body;
        }
        sleep(Duration::from_millis(25)).await;
    }

    panic!("run {run_id} did not reach status {expected_status}");
}

fn durable_run_record(run_id: &str, status: WorkflowRunStatus) -> WorkflowRunRecord {
    WorkflowRunRecord {
        run_id: run_id.to_string(),
        workflow_id: Uuid::new_v4().to_string(),
        workflow_version: "1.0.0".to_string(),
        status,
        input_json: serde_json::json!({ "ticket": "T-42" }).to_string(),
        vars_json: "{}".to_string(),
        current_step_id: Some("analyze".to_string()),
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_health_endpoint() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/health", server.base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    // Middleware injects x-request-id
    assert!(resp.headers().contains_key("x-request-id"));

    let body: serde_json::Value = resp.json().await.unwrap();
    // Public health endpoint returns minimal info (redacted for security)
    assert_eq!(body["status"], "ok");
    assert!(body["version"].is_string());
    // Detailed fields should NOT appear in public health endpoint
    assert!(body["database"].is_null());
    assert!(body["agent_count"].is_null());
}

#[tokio::test]
async fn test_status_endpoint() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/status", server.base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "running");
    assert_eq!(body["agent_count"], 1); // default assistant auto-spawned
    assert!(body["uptime_seconds"].is_number());
    assert_eq!(body["default_provider"], "ollama");
    assert_eq!(body["agents"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn test_config_endpoint_includes_resolved_persistence_paths() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/config", server.base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["persistence"]["runtime_db"].as_str(),
        Some(
            server
                .state
                .kernel
                .config
                .persistence
                .resolve_runtime_db(&server.state.kernel.config.data_dir)
                .to_string_lossy()
                .as_ref()
        )
    );
    assert_eq!(
        body["persistence"]["compozy_db"].as_str(),
        Some(
            server
                .state
                .kernel
                .config
                .persistence
                .resolve_compozy_db(&server.state.kernel.config.data_dir)
                .to_string_lossy()
                .as_ref()
        )
    );
}

#[tokio::test]
async fn test_fresh_boot_creates_both_database_files() {
    let server = start_test_server().await;
    let runtime_db = server
        .state
        .kernel
        .config
        .persistence
        .resolve_runtime_db(&server.state.kernel.config.data_dir);
    let compozy_db = server
        .state
        .kernel
        .config
        .persistence
        .resolve_compozy_db(&server.state.kernel.config.data_dir);

    assert!(runtime_db.exists(), "runtime.db should exist after boot");
    assert!(compozy_db.exists(), "compozy.db should exist after boot");
    assert!(
        server.state.kernel.db_health().is_healthy(),
        "dual-database boot should report healthy"
    );
}

#[tokio::test]
async fn test_second_boot_against_existing_dual_databases_succeeds() {
    let tmp = tempfile::tempdir().expect("Failed to create temp dir");

    let first_config = KernelConfig {
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
    let first_kernel = OpenFangKernel::boot_with_config(first_config).expect("first boot");
    first_kernel.shutdown();

    let second_config = KernelConfig {
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
    let second_kernel = OpenFangKernel::boot_with_config(second_config).expect("second boot");

    assert!(
        second_kernel.db_health().is_healthy(),
        "second boot should preserve dual-database readiness"
    );
    second_kernel.shutdown();
}

#[tokio::test]
async fn test_spawn_list_kill_agent() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();

    // --- Spawn ---
    let resp = client
        .post(format!("{}/api/agents", server.base_url))
        .json(&serde_json::json!({"manifest_toml": TEST_MANIFEST}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "test-agent");
    let agent_id = body["agent_id"].as_str().unwrap().to_string();
    assert!(!agent_id.is_empty());

    // --- List (2 agents: default assistant + test-agent) ---
    let resp = client
        .get(format!("{}/api/agents", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let agents: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(agents.len(), 2);
    let test_agent = agents.iter().find(|a| a["name"] == "test-agent").unwrap();
    assert_eq!(test_agent["id"], agent_id);
    assert_eq!(test_agent["model_provider"], "ollama");

    // --- Kill ---
    let resp = client
        .delete(format!("{}/api/agents/{}", server.base_url, agent_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "killed");

    // --- List (only default assistant remains) ---
    let resp = client
        .get(format!("{}/api/agents", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let agents: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["name"], "assistant");
}

#[tokio::test]
async fn test_agent_session_empty() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();

    // Spawn agent
    let resp = client
        .post(format!("{}/api/agents", server.base_url))
        .json(&serde_json::json!({"manifest_toml": TEST_MANIFEST}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let agent_id = body["agent_id"].as_str().unwrap();

    // Session should be empty — no messages sent yet
    let resp = client
        .get(format!(
            "{}/api/agents/{}/session",
            server.base_url, agent_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["message_count"], 0);
    assert_eq!(body["messages"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_send_message_with_llm() {
    if std::env::var("GROQ_API_KEY").is_err() {
        eprintln!("GROQ_API_KEY not set, skipping LLM integration test");
        return;
    }

    let server = start_test_server_with_llm().await;
    let client = reqwest::Client::new();

    // Spawn
    let resp = client
        .post(format!("{}/api/agents", server.base_url))
        .json(&serde_json::json!({"manifest_toml": LLM_MANIFEST}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let agent_id = body["agent_id"].as_str().unwrap().to_string();

    // Send message through the real HTTP endpoint → kernel → Groq LLM
    let resp = client
        .post(format!(
            "{}/api/agents/{}/message",
            server.base_url, agent_id
        ))
        .json(&serde_json::json!({"message": "Say hello in exactly 3 words."}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let response_text = body["response"].as_str().unwrap();
    assert!(
        !response_text.is_empty(),
        "LLM response should not be empty"
    );
    assert!(body["input_tokens"].as_u64().unwrap() > 0);
    assert!(body["output_tokens"].as_u64().unwrap() > 0);

    // Session should now have messages
    let resp = client
        .get(format!(
            "{}/api/agents/{}/session",
            server.base_url, agent_id
        ))
        .send()
        .await
        .unwrap();
    let session: serde_json::Value = resp.json().await.unwrap();
    assert!(session["message_count"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn test_workflow_crud() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();

    // Spawn agent for workflow
    let resp = client
        .post(format!("{}/api/agents", server.base_url))
        .json(&serde_json::json!({"manifest_toml": TEST_MANIFEST}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let agent_name = body["name"].as_str().unwrap().to_string();

    // Create workflow
    let resp = client
        .post(format!("{}/api/v1/workflows", server.base_url))
        .json(&serde_json::json!({
            "id": "test-workflow",
            "name": "test-workflow",
            "version": "1.0.0",
            "description": "Integration test workflow",
            "enabled": true,
            "tags": ["integration"],
            "input": {
                "kind": "object",
                "required": ["message"],
                "open": false,
                "fields": {
                    "message": { "kind": "string" }
                }
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
                "id": "agent-step",
                "name": "step1",
                "kind": "agent",
                "uses": { "agent": agent_name },
                "with": {
                    "message": "Echo: {{ input.message }}"
                },
                "save_as": "result",
                "flow": { "mode": "sequential" }
            }],
            "outputs": {
                "result": "{{ vars.result }}"
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    let workflow_id = body["id"].as_str().unwrap().to_string();
    assert!(!workflow_id.is_empty());

    // List workflows
    let resp = client
        .get(format!("{}/api/v1/workflows", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let workflows: serde_json::Value = resp.json().await.unwrap();
    let items = workflows["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"], "test-workflow");
    assert_eq!(items[0]["steps"], 1);
}

#[tokio::test]
async fn test_v1_start_workflow_creates_durable_run_record_immediately() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();
    let workflow_id = create_empty_workflow(&client, &server, "durable-start").await;

    let response = client
        .post(format!(
            "{}/api/v1/workflows/{workflow_id}/runs",
            server.base_url
        ))
        .json(&serde_json::json!({
            "input": { "issue_id": "ISSUE-123" },
            "labels": ["manual"],
            "metadata": { "source": "api" },
        }))
        .send()
        .await
        .expect("workflow start request should succeed");

    assert_eq!(response.status(), 202);
    let start_body: serde_json::Value = response
        .json()
        .await
        .expect("workflow start response should deserialize");
    let run_id = start_body["run_id"]
        .as_str()
        .expect("run id should be present")
        .to_string();

    let detail_response = client
        .get(format!("{}/api/v1/runs/{run_id}", server.base_url))
        .send()
        .await
        .expect("run detail request should succeed");

    assert_eq!(detail_response.status(), 200);
    let detail_body: serde_json::Value = detail_response
        .json()
        .await
        .expect("run detail response should deserialize");
    let durable_record = server
        .state
        .kernel
        .workflow_stores
        .workflow_run
        .find_by_id(&run_id)
        .expect("durable run query should succeed")
        .expect("durable run should exist");

    assert_eq!(detail_body["id"], run_id);
    assert_eq!(detail_body["workflow_id"], workflow_id);
    assert!(!detail_body["status"]
        .as_str()
        .expect("status should be a string")
        .is_empty());
    assert_eq!(detail_body["input"]["issue_id"], "ISSUE-123");
    assert_eq!(durable_record.run_id, run_id);
}

#[tokio::test]
async fn test_v1_workflow_run_lists_read_from_durable_store_after_cache_clear() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();
    let workflow_id = create_empty_workflow(&client, &server, "durable-list").await;

    let response = client
        .post(format!(
            "{}/api/v1/workflows/{workflow_id}/runs",
            server.base_url
        ))
        .json(&serde_json::json!({
            "input": { "ticket": "T-1" }
        }))
        .send()
        .await
        .expect("workflow start request should succeed");
    let start_body: serde_json::Value = response
        .json()
        .await
        .expect("workflow start response should deserialize");
    let run_id = start_body["run_id"]
        .as_str()
        .expect("run id should be present")
        .to_string();

    server.state.kernel.workflows.clear_run_cache().await;

    let v1_list = client
        .get(format!(
            "{}/api/v1/workflows/{workflow_id}/runs",
            server.base_url
        ))
        .send()
        .await
        .expect("v1 workflow run list should succeed");
    let legacy_list = client
        .get(format!(
            "{}/api/workflows/{workflow_id}/runs",
            server.base_url
        ))
        .send()
        .await
        .expect("legacy workflow run list should succeed");

    assert_eq!(v1_list.status(), 200);
    assert_eq!(legacy_list.status(), 200);

    let v1_items = v1_list
        .json::<serde_json::Value>()
        .await
        .expect("v1 run list should deserialize");
    let legacy_items = legacy_list
        .json::<Vec<serde_json::Value>>()
        .await
        .expect("legacy run list should deserialize");

    assert!(v1_items["items"]
        .as_array()
        .expect("v1 items should be an array")
        .iter()
        .any(|item| item["id"] == run_id));
    assert!(legacy_items.iter().any(|item| item["id"] == run_id));
}

#[tokio::test]
async fn test_v1_run_checkpoints_reflect_full_lifecycle() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();
    let workflow = Workflow {
        id: WorkflowId::new(),
        name: "checkpoint-workflow".to_string(),
        description: "Checkpoint listing integration test".to_string(),
        steps: vec![
            WorkflowStep {
                name: "analyze".to_string(),
                agent: StepAgent::ByName {
                    name: "alpha".to_string(),
                },
                prompt_template: "{{input}}".to_string(),
                mode: StepMode::Sequential,
                timeout_secs: 10,
                error_mode: ErrorMode::Fail,
                output_var: Some("analysis".to_string()),
            },
            WorkflowStep {
                name: "summarize".to_string(),
                agent: StepAgent::ByName {
                    name: "beta".to_string(),
                },
                prompt_template: "{{input}}".to_string(),
                mode: StepMode::Sequential,
                timeout_secs: 10,
                error_mode: ErrorMode::Fail,
                output_var: Some("summary".to_string()),
            },
        ],
        created_at: chrono::Utc::now(),
    };
    let workflow_id = server
        .state
        .kernel
        .workflows
        .register(workflow.clone())
        .await
        .expect("workflow registration should succeed");
    let run_id = server
        .state
        .kernel
        .workflows
        .create_run(workflow_id, "checkpoint input".to_string())
        .await
        .expect("workflow run should be created");
    let workflow_ir = WorkflowIr {
        workflow_id: workflow_id.to_string(),
        workflow_version: "legacy".to_string(),
        defaults: ResolvedRuntimeSettings::default(),
        input_contract: serde_json::from_value(serde_json::json!({ "kind": "any" }))
            .expect("static any contract should deserialize"),
        output_contract: serde_json::from_value(serde_json::json!({ "kind": "any" }))
            .expect("static any contract should deserialize"),
        steps: vec![
            WorkflowIrStep {
                id: "analyze".to_string(),
                name: "analyze".to_string(),
                kind: WorkflowIrStepKind::Noop,
                flow: FlowBlock {
                    mode: FlowMode::Sequential,
                },
                runtime: ResolvedRuntimeSettings::default(),
                with: std::collections::BTreeMap::new(),
                save_as: Some("analysis".to_string()),
            },
            WorkflowIrStep {
                id: "summarize".to_string(),
                name: "summarize".to_string(),
                kind: WorkflowIrStepKind::Noop,
                flow: FlowBlock {
                    mode: FlowMode::Sequential,
                },
                runtime: ResolvedRuntimeSettings::default(),
                with: std::collections::BTreeMap::new(),
                save_as: Some("summary".to_string()),
            },
        ],
        symbol_table: std::collections::BTreeMap::from([
            ("analysis".to_string(), "analyze".to_string()),
            ("summary".to_string(), "summarize".to_string()),
        ]),
        outputs: std::collections::BTreeMap::new(),
    };

    server
        .state
        .kernel
        .execute_compiled_workflow_run(run_id, workflow_ir)
        .await
        .expect("workflow execution should succeed");

    let response = client
        .get(format!(
            "{}/api/v1/runs/{}/checkpoints",
            server.base_url, run_id
        ))
        .send()
        .await
        .expect("checkpoint request should succeed");

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response
        .json()
        .await
        .expect("checkpoint response should deserialize");
    let kinds = body["items"]
        .as_array()
        .expect("checkpoint items should be an array")
        .iter()
        .map(|item| item["kind"].as_str().expect("kind should be a string"))
        .collect::<Vec<_>>();

    assert_eq!(
        kinds,
        vec![
            "run_created",
            "run_started",
            "step_started",
            "step_completed",
            "step_started",
            "step_completed",
            "run_completed",
        ]
    );
}

#[tokio::test]
async fn test_v1_recovered_run_is_paused_after_restart() {
    let tmp = tempfile::tempdir().expect("temp dir should be created");
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
    let run_id = Uuid::new_v4().to_string();
    let first_kernel =
        Arc::new(OpenFangKernel::boot_with_config(config.clone()).expect("first boot"));
    first_kernel.set_self_handle();
    first_kernel
        .workflow_stores
        .workflow_run
        .insert_run(&durable_run_record(&run_id, WorkflowRunStatus::Running))
        .expect("running workflow run should persist");
    first_kernel.shutdown();

    let kernel = Arc::new(OpenFangKernel::boot_with_config(config).expect("second boot"));
    kernel.set_self_handle();
    kernel.bootstrap_workflow_definitions().await;
    let state = Arc::new(AppState {
        kernel: Arc::clone(&kernel),
        started_at: Instant::now(),
        peer_registry: None,
        bridge_manager: tokio::sync::Mutex::new(None),
        channels_config: tokio::sync::RwLock::new(Default::default()),
        shutdown_notify: Arc::new(tokio::sync::Notify::new()),
        clawhub_cache: dashmap::DashMap::new(),
        provider_probe_cache: openfang_runtime::provider_health::ProbeCache::new(),
    });
    let app = Router::new()
        .route("/api/v1/runs/{id}", axum::routing::get(routes::get_run_v1))
        .route(
            "/api/v1/runs/{id}/checkpoints",
            axum::routing::get(routes::get_run_checkpoints_v1),
        )
        .route(
            "/api/v1/runs/{id}/signals",
            axum::routing::get(routes::get_run_signals_v1).post(routes::post_run_signal_v1),
        )
        .with_state(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener should expose local address");

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("api should serve");
    });

    let server = TestServer {
        base_url: format!("http://{}", address),
        state,
        _tmp: tmp,
    };
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/api/v1/runs/{run_id}", server.base_url))
        .send()
        .await
        .expect("run detail request should succeed");

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response
        .json()
        .await
        .expect("run detail response should deserialize");
    let checkpoint_response = client
        .get(format!(
            "{}/api/v1/runs/{run_id}/checkpoints",
            server.base_url
        ))
        .send()
        .await
        .expect("run checkpoint request should succeed");
    let checkpoint_body: serde_json::Value = checkpoint_response
        .json()
        .await
        .expect("run checkpoint response should deserialize");

    assert_eq!(body["id"], run_id);
    assert_eq!(body["status"], "paused");
    assert!(checkpoint_body["items"]
        .as_array()
        .expect("checkpoint items should be an array")
        .iter()
        .any(|item| {
            item["kind"] == "run_recovered_needs_resume"
                && item["data"]["previous_status"] == "running"
        }));
}

#[tokio::test]
async fn get_run_list_reflects_recovered_state() {
    let tmp = tempfile::tempdir().expect("temp dir should be created");
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
    let first_kernel =
        Arc::new(OpenFangKernel::boot_with_config(config.clone()).expect("first boot"));
    first_kernel.set_self_handle();
    let paused_candidates = [Uuid::new_v4().to_string(), Uuid::new_v4().to_string()];
    for run_id in &paused_candidates {
        first_kernel
            .workflow_stores
            .workflow_run
            .insert_run(&durable_run_record(run_id, WorkflowRunStatus::Running))
            .expect("running workflow run should persist");
    }
    let mut waiting = durable_run_record(
        &Uuid::new_v4().to_string(),
        WorkflowRunStatus::WaitingSignal,
    );
    waiting.current_step_id = Some("await-approval".to_string());
    waiting.waiting_kind = Some("signal".to_string());
    waiting.waiting_ref = Some("approval".to_string());
    first_kernel
        .workflow_stores
        .workflow_run
        .insert_run(&waiting)
        .expect("waiting workflow run should persist");
    first_kernel.shutdown();

    let kernel = Arc::new(OpenFangKernel::boot_with_config(config).expect("second boot"));
    kernel.set_self_handle();
    kernel.bootstrap_workflow_definitions().await;
    let state = Arc::new(AppState {
        kernel: Arc::clone(&kernel),
        started_at: Instant::now(),
        peer_registry: None,
        bridge_manager: tokio::sync::Mutex::new(None),
        channels_config: tokio::sync::RwLock::new(Default::default()),
        shutdown_notify: Arc::new(tokio::sync::Notify::new()),
        clawhub_cache: dashmap::DashMap::new(),
        provider_probe_cache: openfang_runtime::provider_health::ProbeCache::new(),
    });
    let app = Router::new()
        .route("/api/v1/runs", axum::routing::get(routes::list_runs_v1))
        .with_state(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener should expose local address");

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("api should serve");
    });

    let server = TestServer {
        base_url: format!("http://{}", address),
        state,
        _tmp: tmp,
    };
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/api/v1/runs?status=paused", server.base_url))
        .send()
        .await
        .expect("paused run list request should succeed");

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response
        .json()
        .await
        .expect("paused run list response should deserialize");
    let items = body["items"]
        .as_array()
        .expect("paused run list should be an array");

    assert_eq!(items.len(), paused_candidates.len());
    for run_id in &paused_candidates {
        assert!(items.iter().any(|item| item["id"] == *run_id));
    }
    assert!(items.iter().all(|item| item["status"] == "paused"));
}

#[tokio::test]
async fn get_run_dispatches_reads_from_compozy_db_not_memory() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string();
    server
        .state
        .kernel
        .workflow_stores
        .workflow_run
        .insert_run(&durable_run_record(&run_id, WorkflowRunStatus::Running))
        .expect("durable run should persist");
    server
        .state
        .kernel
        .workflow_stores
        .dispatch
        .create(&DispatchRecord {
            dispatch_id: "dispatch-1".to_string(),
            run_id: run_id.clone(),
            step_id: Some("step-1".to_string()),
            kind: DispatchKind::Call,
            target_agent: "agent-alpha".to_string(),
            status: DispatchStatus::Running,
            input_json: serde_json::json!({ "prompt": "hello" }),
            result_json: None,
            error_json: None,
            attempt: 1,
            parent_dispatch_id: None,
            spawned_agent_id: None,
            provider_driver: None,
            session_id: None,
            provider_resume_token: None,
            started_at: "2026-03-23T12:00:00Z".to_string(),
            updated_at: "2026-03-23T12:01:00Z".to_string(),
            completed_at: None,
        })
        .await
        .expect("dispatch row should persist");
    server.state.kernel.workflows.clear_run_cache().await;

    let response = client
        .get(format!(
            "{}/api/v1/runs/{run_id}/dispatches",
            server.base_url
        ))
        .send()
        .await
        .expect("dispatch list request should succeed");

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response
        .json()
        .await
        .expect("dispatch list response should deserialize");

    assert_eq!(body["items"][0]["id"], "dispatch-1");
    assert_eq!(body["items"][0]["run_id"], run_id);
    assert_eq!(body["items"][0]["status"], "running");
}

#[tokio::test]
async fn pause_resume_cancel_round_trip_through_db() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string();
    server
        .state
        .kernel
        .workflow_stores
        .workflow_run
        .insert_run(&durable_run_record(&run_id, WorkflowRunStatus::Running))
        .expect("durable run should persist");

    let pause_response = client
        .post(format!("{}/api/v1/runs/{run_id}/pause", server.base_url))
        .send()
        .await
        .expect("pause request should succeed");
    assert_eq!(pause_response.status(), 200);
    server.state.kernel.workflows.clear_run_cache().await;
    let paused = wait_for_run_status(&client, &server, &run_id, "paused").await;

    let resume_response = client
        .post(format!("{}/api/v1/runs/{run_id}/resume", server.base_url))
        .send()
        .await
        .expect("resume request should succeed");
    assert_eq!(resume_response.status(), 200);
    server.state.kernel.workflows.clear_run_cache().await;
    let resumed = wait_for_run_status(&client, &server, &run_id, "running").await;

    let cancel_response = client
        .post(format!("{}/api/v1/runs/{run_id}/cancel", server.base_url))
        .send()
        .await
        .expect("cancel request should succeed");
    assert_eq!(cancel_response.status(), 200);
    server.state.kernel.workflows.clear_run_cache().await;
    let cancelled = wait_for_run_status(&client, &server, &run_id, "cancelled").await;

    let checkpoint_response = client
        .get(format!(
            "{}/api/v1/runs/{run_id}/checkpoints",
            server.base_url
        ))
        .send()
        .await
        .expect("checkpoint request should succeed");
    let checkpoint_body: serde_json::Value = checkpoint_response
        .json()
        .await
        .expect("checkpoint response should deserialize");
    let checkpoint_items = checkpoint_body["items"]
        .as_array()
        .expect("checkpoint items should be an array");

    assert_eq!(paused["status"], "paused");
    assert_eq!(resumed["status"], "running");
    assert_eq!(cancelled["status"], "cancelled");
    assert!(cancelled["completed_at"].is_string());
    assert!(checkpoint_items
        .iter()
        .any(|item| { item["kind"] == "run_paused" && item["data"]["actor_source"] == "api" }));
    assert!(checkpoint_items
        .iter()
        .any(|item| { item["kind"] == "run_resumed" && item["data"]["actor_source"] == "api" }));
    assert!(checkpoint_items
        .iter()
        .any(|item| { item["kind"] == "run_cancelled" && item["data"]["actor_source"] == "api" }));
}

#[tokio::test]
async fn post_run_signal_persists_and_affects_run_state() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();
    let run_id = create_waiting_signal_run(&server).await;

    let response = client
        .post(format!("{}/api/v1/runs/{run_id}/signals", server.base_url))
        .json(&serde_json::json!({
            "name": "approval",
            "payload": { "decision": "approved" },
            "source": "api",
            "idempotency_key": "idem-post-signal",
        }))
        .send()
        .await
        .expect("signal request should succeed");

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response
        .json()
        .await
        .expect("signal response should deserialize");
    let detail_response = client
        .get(format!("{}/api/v1/runs/{run_id}", server.base_url))
        .send()
        .await
        .expect("run detail request should succeed");
    let detail_body: serde_json::Value = detail_response
        .json()
        .await
        .expect("run detail response should deserialize");

    assert_eq!(body["run_id"], run_id);
    assert_eq!(body["name"], "approval");
    assert_eq!(body["payload"]["decision"], "approved");
    assert_eq!(body["source"], "api");
    assert_eq!(body["consumed"], true);
    assert!(body["consumed_at"].is_string());
    assert!(detail_body["waiting_kind"].is_null());
    assert_ne!(detail_body["status"], "waiting_signal");
}

#[tokio::test]
async fn waiting_workflow_resumes_after_durable_signal_delivery() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();
    let run_id = create_waiting_signal_run(&server).await;

    let response = client
        .post(format!("{}/api/v1/runs/{run_id}/signals", server.base_url))
        .json(&serde_json::json!({
            "name": "approval",
            "payload": { "decision": "approved" },
            "source": "api",
            "idempotency_key": "idem-resume-complete",
        }))
        .send()
        .await
        .expect("signal request should succeed");

    assert_eq!(response.status(), 200);
    let run = wait_for_run_status(&client, &server, &run_id, "completed").await;

    assert_eq!(run["waiting_kind"], serde_json::Value::Null);
    assert_eq!(run["waiting_ref"], serde_json::Value::Null);
}

#[tokio::test]
async fn duplicate_signal_idempotency_returns_existing_record() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();
    let run_id = create_waiting_signal_run(&server).await;
    let request_body = serde_json::json!({
        "name": "approval",
        "payload": { "decision": "approved" },
        "source": "api",
        "idempotency_key": "idem-duplicate-signal",
    });

    let first = client
        .post(format!("{}/api/v1/runs/{run_id}/signals", server.base_url))
        .json(&request_body)
        .send()
        .await
        .expect("first signal request should succeed");
    let second = client
        .post(format!("{}/api/v1/runs/{run_id}/signals", server.base_url))
        .json(&request_body)
        .send()
        .await
        .expect("second signal request should succeed");

    assert_eq!(first.status(), 200);
    assert_eq!(second.status(), 200);
    let first_body: serde_json::Value = first
        .json()
        .await
        .expect("first signal response should deserialize");
    let second_body: serde_json::Value = second
        .json()
        .await
        .expect("second signal response should deserialize");
    let list_response = client
        .get(format!("{}/api/v1/runs/{run_id}/signals", server.base_url))
        .send()
        .await
        .expect("signal list request should succeed");
    let list_body: serde_json::Value = list_response
        .json()
        .await
        .expect("signal list response should deserialize");

    assert_eq!(first_body["id"], second_body["id"]);
    assert_eq!(
        list_body["items"]
            .as_array()
            .expect("signal list should be an array")
            .len(),
        1
    );
}

#[tokio::test]
async fn restart_preserves_waiting_state_and_outstanding_signals() {
    let tmp = tempfile::tempdir().expect("temp dir should be created");
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
    let run_id = Uuid::new_v4().to_string();
    let first_kernel =
        Arc::new(OpenFangKernel::boot_with_config(config.clone()).expect("first boot"));
    first_kernel.set_self_handle();
    first_kernel
        .workflow_stores
        .workflow_run
        .insert_run(&WorkflowRunRecord {
            run_id: run_id.clone(),
            workflow_id: Uuid::new_v4().to_string(),
            workflow_version: "1.0.0".to_string(),
            status: WorkflowRunStatus::WaitingSignal,
            input_json: serde_json::json!({ "ticket": "T-42" }).to_string(),
            vars_json: "{}".to_string(),
            current_step_id: Some("await-approval".to_string()),
            waiting_kind: Some("signal".to_string()),
            waiting_ref: Some("approval".to_string()),
            active_dispatch_id: None,
            active_hitl_request_id: None,
            labels_json: "[]".to_string(),
            metadata_json: "{}".to_string(),
            error_json: None,
            started_at: "2026-03-23T12:00:00Z".to_string(),
            updated_at: "2026-03-23T12:00:00Z".to_string(),
            completed_at: None,
        })
        .expect("waiting workflow run should persist");
    first_kernel
        .workflow_stores
        .workflow_checkpoint
        .append(&openfang_memory::WorkflowCheckpointRecord {
            checkpoint_id: "chk-restart-waiting".to_string(),
            run_id: run_id.clone(),
            step_id: Some("await-approval".to_string()),
            kind: openfang_memory::CheckpointKind::WaitingSignal,
            data_json: serde_json::json!({
                "signal_name": "approval",
                "resume_input": { "ticket": "T-42" },
            })
            .to_string(),
            created_at: "2026-03-23T12:00:05Z".to_string(),
        })
        .expect("waiting checkpoint should persist");
    first_kernel
        .workflow_stores
        .workflow_signal
        .insert(&WorkflowSignalRecord {
            signal_id: "signal-restart".to_string(),
            run_id: run_id.clone(),
            name: "approval".to_string(),
            payload_json: serde_json::json!({ "decision": "approved" }).to_string(),
            source: "schedule".to_string(),
            idempotency_key: "idem-restart".to_string(),
            consumed: false,
            created_at: now_timestamp(),
            consumed_at: None,
        })
        .expect("outstanding signal should persist");
    first_kernel.shutdown();

    let kernel = Arc::new(OpenFangKernel::boot_with_config(config).expect("second boot"));
    kernel.set_self_handle();
    kernel.bootstrap_workflow_definitions().await;
    let state = Arc::new(AppState {
        kernel: Arc::clone(&kernel),
        started_at: Instant::now(),
        peer_registry: None,
        bridge_manager: tokio::sync::Mutex::new(None),
        channels_config: tokio::sync::RwLock::new(Default::default()),
        shutdown_notify: Arc::new(tokio::sync::Notify::new()),
        clawhub_cache: dashmap::DashMap::new(),
        provider_probe_cache: openfang_runtime::provider_health::ProbeCache::new(),
    });
    let app = Router::new()
        .route("/api/v1/runs/{id}", axum::routing::get(routes::get_run_v1))
        .route(
            "/api/v1/runs/{id}/checkpoints",
            axum::routing::get(routes::get_run_checkpoints_v1),
        )
        .route(
            "/api/v1/runs/{id}/signals",
            axum::routing::get(routes::get_run_signals_v1),
        )
        .with_state(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener should expose local address");

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("api should serve");
    });

    let server = TestServer {
        base_url: format!("http://{}", address),
        state,
        _tmp: tmp,
    };
    let client = reqwest::Client::new();
    let run_response = client
        .get(format!("{}/api/v1/runs/{run_id}", server.base_url))
        .send()
        .await
        .expect("run detail request should succeed");
    let run_body: serde_json::Value = run_response
        .json()
        .await
        .expect("run detail response should deserialize");
    let signal_response = client
        .get(format!("{}/api/v1/runs/{run_id}/signals", server.base_url))
        .send()
        .await
        .expect("signal list request should succeed");
    let signal_body: serde_json::Value = signal_response
        .json()
        .await
        .expect("signal list response should deserialize");
    let checkpoint_response = client
        .get(format!(
            "{}/api/v1/runs/{run_id}/checkpoints",
            server.base_url
        ))
        .send()
        .await
        .expect("checkpoint list request should succeed");
    let checkpoint_body: serde_json::Value = checkpoint_response
        .json()
        .await
        .expect("checkpoint list response should deserialize");

    assert_eq!(run_body["status"], "waiting_signal");
    assert_eq!(run_body["waiting_ref"], "approval");
    assert_eq!(signal_body["items"][0]["name"], "approval");
    assert_eq!(signal_body["items"][0]["consumed"], false);
    assert!(checkpoint_body["items"]
        .as_array()
        .expect("checkpoint items should be an array")
        .iter()
        .any(|item| item["kind"] == "waiting_signal"));
}

#[tokio::test]
async fn get_run_signals_reads_from_compozy_db_not_memory() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();
    let run_id = create_waiting_signal_run(&server).await;

    server
        .state
        .kernel
        .workflow_stores
        .workflow_signal
        .insert(&WorkflowSignalRecord {
            signal_id: "signal-db-only".to_string(),
            run_id: run_id.clone(),
            name: "approval".to_string(),
            payload_json: serde_json::json!({ "decision": "approved" }).to_string(),
            source: "schedule".to_string(),
            idempotency_key: "idem-db-only".to_string(),
            consumed: false,
            created_at: now_timestamp(),
            consumed_at: None,
        })
        .expect("signal should persist");
    server.state.kernel.workflows.clear_run_cache().await;

    let response = client
        .get(format!(
            "{}/api/v1/runs/{run_id}/signals?consumed=false",
            server.base_url
        ))
        .send()
        .await
        .expect("signal list request should succeed");

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response
        .json()
        .await
        .expect("signal list response should deserialize");

    assert_eq!(body["items"][0]["id"], "signal-db-only");
    assert_eq!(body["items"][0]["source"], "schedule");
}

#[tokio::test]
async fn waiting_signal_run_still_accepts_signal_after_restart() {
    let tmp = tempfile::tempdir().expect("temp dir should be created");
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
    let first_kernel =
        Arc::new(OpenFangKernel::boot_with_config(config.clone()).expect("first boot"));
    first_kernel.set_self_handle();
    let workflow_id = WorkflowId::new();
    let definition = wait_signal_definition(workflow_id);
    let mut registry = WorkflowCompileRegistry::new();
    registry.set_workflows(std::iter::once(definition.id.clone()));
    let workflow_ir =
        compile_workflow_definition(&definition, &registry).expect("workflow should compile");

    first_kernel
        .workflows
        .register_workflow_v2_definition(definition.clone(), Vec::<String>::new())
        .await
        .expect("workflow v2 definition should register");
    first_kernel
        .workflows
        .register(Workflow {
            id: workflow_id,
            name: definition.name.clone(),
            description: definition.description.clone(),
            steps: Vec::new(),
            created_at: chrono::Utc::now(),
        })
        .await
        .expect("workflow registration should succeed");

    let run_id = first_kernel
        .workflows
        .create_run(workflow_id, "waiting input".to_string())
        .await
        .expect("workflow run should be created");
    first_kernel
        .execute_compiled_workflow_run(run_id, workflow_ir)
        .await
        .expect("workflow should park");
    first_kernel.shutdown();

    let kernel = Arc::new(OpenFangKernel::boot_with_config(config).expect("second boot"));
    kernel.set_self_handle();
    kernel.bootstrap_workflow_definitions().await;
    kernel
        .workflows
        .register_workflow_v2_definition(definition, Vec::<String>::new())
        .await
        .expect("workflow definition should be re-registered after restart");

    let state = Arc::new(AppState {
        kernel: Arc::clone(&kernel),
        started_at: Instant::now(),
        peer_registry: None,
        bridge_manager: tokio::sync::Mutex::new(None),
        channels_config: tokio::sync::RwLock::new(Default::default()),
        shutdown_notify: Arc::new(tokio::sync::Notify::new()),
        clawhub_cache: dashmap::DashMap::new(),
        provider_probe_cache: openfang_runtime::provider_health::ProbeCache::new(),
    });
    let app = Router::new()
        .route("/api/v1/runs/{id}", axum::routing::get(routes::get_run_v1))
        .route(
            "/api/v1/runs/{id}/signals",
            axum::routing::get(routes::get_run_signals_v1).post(routes::post_run_signal_v1),
        )
        .with_state(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener should expose local address");

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("api should serve");
    });

    let server = TestServer {
        base_url: format!("http://{}", address),
        state,
        _tmp: tmp,
    };
    let client = reqwest::Client::new();
    let response = client
        .post(format!(
            "{}/api/v1/runs/{}/signals",
            server.base_url, run_id
        ))
        .json(&serde_json::json!({
            "name": "approval",
            "payload": { "decision": "approved" },
            "source": "api",
            "idempotency_key": "idem-after-restart",
        }))
        .send()
        .await
        .expect("signal request should succeed");
    assert_eq!(response.status(), 200);

    let completed = wait_for_run_status(&client, &server, &run_id.to_string(), "completed").await;
    let signals_response = client
        .get(format!(
            "{}/api/v1/runs/{}/signals",
            server.base_url, run_id
        ))
        .send()
        .await
        .expect("signal list request should succeed");
    let signals_body: serde_json::Value = signals_response
        .json()
        .await
        .expect("signal list response should deserialize");

    assert_eq!(completed["status"], "completed");
    assert_eq!(signals_body["items"][0]["name"], "approval");
    assert_eq!(signals_body["items"][0]["consumed"], true);
}

#[tokio::test]
async fn concurrent_signal_delivery_does_not_double_consume() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();
    let run_id = create_waiting_signal_run(&server).await;

    let first = client
        .post(format!("{}/api/v1/runs/{run_id}/signals", server.base_url))
        .json(&serde_json::json!({
            "name": "approval",
            "payload": { "decision": "approved" },
            "source": "api",
            "idempotency_key": "idem-concurrent-a",
        }))
        .send();
    let second = client
        .post(format!("{}/api/v1/runs/{run_id}/signals", server.base_url))
        .json(&serde_json::json!({
            "name": "approval",
            "payload": { "decision": "approved-again" },
            "source": "trigger",
            "idempotency_key": "idem-concurrent-b",
        }))
        .send();

    let (first_response, second_response) = tokio::join!(first, second);
    assert_eq!(
        first_response
            .expect("first signal request should succeed")
            .status(),
        200
    );
    assert_eq!(
        second_response
            .expect("second signal request should succeed")
            .status(),
        200
    );

    let list_response = client
        .get(format!("{}/api/v1/runs/{run_id}/signals", server.base_url))
        .send()
        .await
        .expect("signal list request should succeed");
    let list_body: serde_json::Value = list_response
        .json()
        .await
        .expect("signal list response should deserialize");
    let items = list_body["items"]
        .as_array()
        .expect("signal list should be an array");
    let consumed = items.iter().filter(|item| item["consumed"] == true).count();
    let unconsumed = items
        .iter()
        .filter(|item| item["consumed"] == false)
        .count();

    assert_eq!(items.len(), 2);
    assert_eq!(consumed, 1);
    assert_eq!(unconsumed, 1);
}

#[tokio::test]
async fn test_trigger_crud() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();

    // Spawn agent for trigger
    let resp = client
        .post(format!("{}/api/agents", server.base_url))
        .json(&serde_json::json!({"manifest_toml": TEST_MANIFEST}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let agent_id = body["agent_id"].as_str().unwrap().to_string();

    // Create trigger (Lifecycle pattern — simplest variant)
    let resp = client
        .post(format!("{}/api/triggers", server.base_url))
        .json(&serde_json::json!({
            "agent_id": agent_id,
            "pattern": "lifecycle",
            "prompt_template": "Handle: {{event}}",
            "max_fires": 5
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    let trigger_id = body["trigger_id"].as_str().unwrap().to_string();
    assert_eq!(body["agent_id"], agent_id);

    // List triggers (unfiltered)
    let resp = client
        .get(format!("{}/api/triggers", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let triggers: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(triggers.len(), 1);
    assert_eq!(triggers[0]["agent_id"], agent_id);
    assert_eq!(triggers[0]["enabled"], true);
    assert_eq!(triggers[0]["max_fires"], 5);

    // List triggers (filtered by agent_id)
    let resp = client
        .get(format!(
            "{}/api/triggers?agent_id={}",
            server.base_url, agent_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let triggers: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(triggers.len(), 1);

    // Delete trigger
    let resp = client
        .delete(format!("{}/api/triggers/{}", server.base_url, trigger_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // List triggers (should be empty)
    let resp = client
        .get(format!("{}/api/triggers", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let triggers: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(triggers.len(), 0);
}

#[tokio::test]
async fn test_invalid_agent_id_returns_400() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();

    // Send message to invalid ID
    let resp = client
        .post(format!("{}/api/agents/not-a-uuid/message", server.base_url))
        .json(&serde_json::json!({"message": "hello"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("Invalid"));

    // Kill invalid ID
    let resp = client
        .delete(format!("{}/api/agents/not-a-uuid", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    // Session for invalid ID
    let resp = client
        .get(format!("{}/api/agents/not-a-uuid/session", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn test_kill_nonexistent_agent_returns_404() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();

    let fake_id = uuid::Uuid::new_v4();
    let resp = client
        .delete(format!("{}/api/agents/{}", server.base_url, fake_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_spawn_invalid_manifest_returns_400() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/api/agents", server.base_url))
        .json(&serde_json::json!({"manifest_toml": "this is {{ not valid toml"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("Invalid manifest"));
}

#[tokio::test]
async fn test_request_id_header_is_uuid() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/health", server.base_url))
        .send()
        .await
        .unwrap();

    let request_id = resp
        .headers()
        .get("x-request-id")
        .expect("x-request-id header should be present");
    let id_str = request_id.to_str().unwrap();
    assert!(
        uuid::Uuid::parse_str(id_str).is_ok(),
        "x-request-id should be a valid UUID, got: {}",
        id_str
    );
}

#[tokio::test]
async fn test_multiple_agents_lifecycle() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();

    // Spawn 3 agents
    let mut ids = Vec::new();
    for i in 0..3 {
        let manifest = format!(
            r#"
name = "agent-{i}"
version = "0.1.0"
description = "Multi-agent test {i}"
author = "test"
module = "builtin:chat"

[model]
provider = "ollama"
model = "test-model"
system_prompt = "Agent {i}."

[capabilities]
memory_read = ["*"]
memory_write = ["self.*"]
"#
        );

        let resp = client
            .post(format!("{}/api/agents", server.base_url))
            .json(&serde_json::json!({"manifest_toml": manifest}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        let body: serde_json::Value = resp.json().await.unwrap();
        ids.push(body["agent_id"].as_str().unwrap().to_string());
    }

    // List should show 4 (3 spawned + default assistant)
    let resp = client
        .get(format!("{}/api/agents", server.base_url))
        .send()
        .await
        .unwrap();
    let agents: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(agents.len(), 4);

    // Status should agree
    let resp = client
        .get(format!("{}/api/status", server.base_url))
        .send()
        .await
        .unwrap();
    let status: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(status["agent_count"], 4);

    // Kill one
    let resp = client
        .delete(format!("{}/api/agents/{}", server.base_url, ids[1]))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // List should show 3 (2 spawned + default assistant)
    let resp = client
        .get(format!("{}/api/agents", server.base_url))
        .send()
        .await
        .unwrap();
    let agents: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(agents.len(), 3);

    // Kill the rest
    for id in [&ids[0], &ids[2]] {
        client
            .delete(format!("{}/api/agents/{}", server.base_url, id))
            .send()
            .await
            .unwrap();
    }

    // List should have only default assistant
    let resp = client
        .get(format!("{}/api/agents", server.base_url))
        .send()
        .await
        .unwrap();
    let agents: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(agents.len(), 1);
}

// ---------------------------------------------------------------------------
// Auth integration tests
// ---------------------------------------------------------------------------

/// Start a test server with Bearer-token authentication enabled.
async fn start_test_server_with_auth(api_key: &str) -> TestServer {
    let tmp = tempfile::tempdir().expect("Failed to create temp dir");

    let config = KernelConfig {
        home_dir: tmp.path().to_path_buf(),
        data_dir: tmp.path().join("data"),
        api_key: api_key.to_string(),
        default_model: DefaultModelConfig {
            provider: "ollama".to_string(),
            model: "test-model".to_string(),
            api_key_env: "OLLAMA_API_KEY".to_string(),
            base_url: None,
        },
        ..KernelConfig::default()
    };

    let kernel = OpenFangKernel::boot_with_config(config).expect("Kernel should boot");
    let kernel = Arc::new(kernel);
    kernel.set_self_handle();
    kernel.bootstrap_workflow_definitions().await;

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

    let api_key = state.kernel.config.api_key.trim().to_string();
    let auth_state = middleware::AuthState {
        api_key: api_key.clone(),
        auth_enabled: state.kernel.config.auth.enabled,
        session_secret: if !api_key.is_empty() {
            api_key.clone()
        } else if state.kernel.config.auth.enabled {
            state.kernel.config.auth.password_hash.clone()
        } else {
            String::new()
        },
    };

    let app = Router::new()
        .route("/api/health", axum::routing::get(routes::health))
        .route("/api/status", axum::routing::get(routes::status))
        .route(
            "/api/agents",
            axum::routing::get(routes::list_agents_legacy).post(routes::spawn_agent),
        )
        .route(
            "/api/agents/{id}/message",
            axum::routing::post(routes::send_message),
        )
        .route(
            "/api/agents/{id}/session",
            axum::routing::get(routes::get_agent_session),
        )
        .route("/api/agents/{id}/ws", axum::routing::get(ws::agent_ws))
        .route(
            "/api/agents/{id}",
            axum::routing::delete(routes::kill_agent),
        )
        .route(
            "/api/triggers",
            axum::routing::get(routes::list_triggers).post(routes::create_trigger),
        )
        .route(
            "/api/triggers/{id}",
            axum::routing::delete(routes::delete_trigger),
        )
        .route(
            "/api/workflows",
            axum::routing::get(routes::list_workflows).post(routes::create_workflow),
        )
        .route(
            "/api/workflows/{id}/run",
            axum::routing::post(routes::run_workflow),
        )
        .route(
            "/api/workflows/{id}/runs",
            axum::routing::get(routes::list_workflow_runs),
        )
        .route(
            "/api/v1/workflows/{id}/runs",
            axum::routing::get(routes::list_workflow_runs_v1).post(routes::start_workflow_run_v1),
        )
        .route("/api/v1/runs", axum::routing::get(routes::list_runs_v1))
        .route("/api/v1/runs/{id}", axum::routing::get(routes::get_run_v1))
        .route(
            "/api/v1/runs/{id}/checkpoints",
            axum::routing::get(routes::get_run_checkpoints_v1),
        )
        .route(
            "/api/v1/runs/{id}/dispatches",
            axum::routing::get(routes::get_run_dispatches_v1),
        )
        .route(
            "/api/v1/runs/{id}/signals",
            axum::routing::get(routes::get_run_signals_v1).post(routes::post_run_signal_v1),
        )
        .route(
            "/api/v1/runs/{id}/pause",
            axum::routing::post(routes::pause_run_v1),
        )
        .route(
            "/api/v1/runs/{id}/resume",
            axum::routing::post(routes::resume_run_v1),
        )
        .route(
            "/api/v1/runs/{id}/cancel",
            axum::routing::post(routes::cancel_run_v1),
        )
        .route("/api/shutdown", axum::routing::post(routes::shutdown))
        .layer(axum::middleware::from_fn_with_state(
            auth_state,
            middleware::auth,
        ))
        .layer(axum::middleware::from_fn(middleware::request_logging))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind test server");
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    TestServer {
        base_url: format!("http://{}", addr),
        state,
        _tmp: tmp,
    }
}

#[tokio::test]
async fn test_auth_health_is_public() {
    let server = start_test_server_with_auth("secret-key-123").await;
    let client = reqwest::Client::new();

    // /api/health should be accessible without auth
    let resp = client
        .get(format!("{}/api/health", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_auth_rejects_no_token() {
    let server = start_test_server_with_auth("secret-key-123").await;
    let client = reqwest::Client::new();

    // Protected endpoint without auth header → 401
    // Note: /api/status is public (dashboard needs it), so use a protected endpoint
    let resp = client
        .get(format!("{}/api/commands", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("Missing"));
}

#[tokio::test]
async fn test_auth_rejects_wrong_token() {
    let server = start_test_server_with_auth("secret-key-123").await;
    let client = reqwest::Client::new();

    // Wrong bearer token → 401
    // Note: /api/status is public (dashboard needs it), so use a protected endpoint
    let resp = client
        .get(format!("{}/api/commands", server.base_url))
        .header("authorization", "Bearer wrong-key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("Invalid"));
}

#[tokio::test]
async fn test_auth_accepts_correct_token() {
    let server = start_test_server_with_auth("secret-key-123").await;
    let client = reqwest::Client::new();

    // Correct bearer token → 200
    let resp = client
        .get(format!("{}/api/status", server.base_url))
        .header("authorization", "Bearer secret-key-123")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "running");
}

#[tokio::test]
async fn test_auth_disabled_when_no_key() {
    // Empty API key = auth disabled
    let server = start_test_server().await;
    let client = reqwest::Client::new();

    // Protected endpoint accessible without auth when no key is configured
    let resp = client
        .get(format!("{}/api/status", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}
