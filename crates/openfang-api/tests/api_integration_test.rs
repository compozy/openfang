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
use openfang_kernel::OpenFangKernel;
use openfang_memory::{WorkflowRunRecord, WorkflowRunStatus};
use openfang_types::agent::AgentId;
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use openfang_types::workflow::{
    FlowBlock, FlowMode, ResolvedRuntimeSettings, WorkflowIr, WorkflowIrStep, WorkflowIrStepKind,
};
use std::sync::Arc;
use std::time::Instant;
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
            axum::routing::get(routes::list_agents).post(routes::spawn_agent),
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
        .post(format!("{}/api/workflows", server.base_url))
        .json(&serde_json::json!({
            "name": name,
            "description": "Durable workflow integration test",
            "steps": [],
        }))
        .send()
        .await
        .expect("workflow creation request should succeed");

    assert_eq!(response.status(), 201);
    let body: serde_json::Value = response
        .json()
        .await
        .expect("workflow creation response should deserialize");
    body["workflow_id"]
        .as_str()
        .expect("workflow id should be present")
        .to_string()
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
        .post(format!("{}/api/workflows", server.base_url))
        .json(&serde_json::json!({
            "name": "test-workflow",
            "description": "Integration test workflow",
            "steps": [
                {
                    "name": "step1",
                    "agent_name": agent_name,
                    "prompt": "Echo: {{input}}",
                    "mode": "sequential",
                    "timeout_secs": 30
                }
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    let workflow_id = body["workflow_id"].as_str().unwrap().to_string();
    assert!(!workflow_id.is_empty());

    // List workflows
    let resp = client
        .get(format!("{}/api/workflows", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let workflows: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(workflows.len(), 1);
    assert_eq!(workflows[0]["name"], "test-workflow");
    assert_eq!(workflows[0]["steps"], 1);
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
                kind: WorkflowIrStepKind::Agent {
                    agent: "alpha".to_string(),
                },
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
                kind: WorkflowIrStepKind::Agent {
                    agent: "beta".to_string(),
                },
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
        .workflows
        .execute_run(
            run_id,
            workflow_ir,
            |agent| Some((AgentId::new(), agent.to_string())),
            |_agent_id: AgentId, message: String| async move {
                Ok((format!("processed {message}"), 1, 1))
            },
        )
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
async fn test_v1_interrupted_run_is_visible_after_restart() {
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
    let first_kernel = OpenFangKernel::boot_with_config(config.clone()).expect("first boot");
    first_kernel
        .workflow_stores
        .workflow_run
        .insert_run(&WorkflowRunRecord {
            run_id: run_id.clone(),
            workflow_id: Uuid::new_v4().to_string(),
            workflow_version: "1.0.0".to_string(),
            status: WorkflowRunStatus::Running,
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
        })
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

    assert_eq!(body["id"], run_id);
    assert_eq!(body["status"], "interrupted");
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
            axum::routing::get(routes::list_agents).post(routes::spawn_agent),
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
