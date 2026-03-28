//! Real HTTP integration tests for the dispatch/HITL v1 control-plane surface.

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use openfang_api::routes::AppState;
use openfang_api::server::build_router;
use openfang_kernel::error::KernelError;
use openfang_kernel::OpenFangKernel;
use openfang_memory::{
    now_timestamp, DispatchKind, DispatchRecord, DispatchRepository, DispatchStatus, HitlKind,
    HitlRepository, NewHitlRequest, WorkflowRunRecord, WorkflowRunStatus,
};
use openfang_types::artifact::{
    ArtifactId, ArtifactType, ArtifactVersionId, NewArtifact, ProvenanceKind, ProvenanceRef,
};
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use openfang_types::doc::{DocId, DocType, DocVersionId, NewDoc};
use openfang_types::error::OpenFangError;
use openfang_types::pack::{PackObjectCounts, PackRecord, PackSourceKind};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout, Duration};
use uuid::Uuid;

struct TestServer {
    base_url: String,
    state: Arc<AppState>,
    home_dir: PathBuf,
    data_dir: PathBuf,
    _tmp: Option<tempfile::TempDir>,
    _mock: MockLlmServer,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.state.kernel.shutdown();
    }
}

#[derive(Clone)]
struct MockLlmState {
    responses: Arc<Mutex<VecDeque<String>>>,
}

struct MockLlmServer {
    base_url: String,
}

async fn mock_chat_completion(State(state): State<MockLlmState>) -> Json<Value> {
    let queued = state
        .responses
        .lock()
        .await
        .pop_front()
        .unwrap_or_else(|| "default mock completion".to_string());
    let (delay_ms, content) = queued
        .strip_prefix("DELAY_MS=")
        .and_then(|rest| {
            let (delay_ms, content) = rest.split_once("::")?;
            let delay_ms = delay_ms.parse::<u64>().ok()?;
            Some((delay_ms, content.to_string()))
        })
        .unwrap_or((0, queued));

    if delay_ms > 0 {
        sleep(Duration::from_millis(delay_ms)).await;
    }

    Json(json!({
        "id": format!("chatcmpl-{}", Uuid::new_v4().simple()),
        "object": "chat.completion",
        "created": 0,
        "model": "test-model",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content,
            },
            "finish_reason": "stop",
        }],
        "usage": {
            "prompt_tokens": 8,
            "completion_tokens": 4,
            "total_tokens": 12,
        },
    }))
}

async fn mock_models() -> Json<Value> {
    Json(json!({
        "object": "list",
        "data": [{
            "id": "test-model",
            "object": "model",
        }],
    }))
}

async fn mock_ollama_tags() -> Json<Value> {
    Json(json!({
        "models": [{
            "name": "test-model",
        }],
    }))
}

async fn start_mock_llm_server(responses: Vec<String>) -> MockLlmServer {
    let state = MockLlmState {
        responses: Arc::new(Mutex::new(VecDeque::from(responses))),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock LLM listener should bind");
    let address = listener
        .local_addr()
        .expect("mock LLM listener should expose an address");
    let app = Router::new()
        .route("/v1/chat/completions", post(mock_chat_completion))
        .route("/v1/models", get(mock_models))
        .route("/api/tags", get(mock_ollama_tags))
        .with_state(state);

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("mock LLM server should stay available");
    });

    MockLlmServer {
        base_url: format!("http://{address}"),
    }
}

const TEST_MANIFEST: &str = r#"
name = "dispatch-tester"
version = "0.1.0"
description = "Dispatch/HITL API integration test agent"
author = "test"
module = "builtin:chat"

[model]
provider = "ollama"
model = "test-model"
system_prompt = "You are a dispatch integration test agent."

[capabilities]
tools = []
memory_read = ["*"]
memory_write = ["self.*"]
"#;

async fn start_dispatch_hitl_test_server(mock_responses: Vec<String>) -> TestServer {
    let tmp = tempfile::tempdir().expect("temp dir should be created");
    let home_dir = tmp.path().to_path_buf();
    let data_dir = home_dir.join("data");
    start_dispatch_hitl_test_server_with_paths_and_tmp(
        home_dir,
        data_dir,
        mock_responses,
        Some(tmp),
    )
    .await
}

async fn start_dispatch_hitl_test_server_with_paths(
    home_dir: PathBuf,
    data_dir: PathBuf,
    mock_responses: Vec<String>,
) -> TestServer {
    start_dispatch_hitl_test_server_with_paths_and_tmp(home_dir, data_dir, mock_responses, None)
        .await
}

async fn start_dispatch_hitl_test_server_with_paths_and_tmp(
    home_dir: PathBuf,
    data_dir: PathBuf,
    mock_responses: Vec<String>,
    tmp: Option<tempfile::TempDir>,
) -> TestServer {
    let mock = start_mock_llm_server(mock_responses).await;
    let mut provider_urls = HashMap::new();
    provider_urls.insert("ollama".to_string(), format!("{}/v1", mock.base_url));

    let config = KernelConfig {
        home_dir: home_dir.clone(),
        data_dir: data_dir.clone(),
        default_model: DefaultModelConfig {
            provider: "ollama".to_string(),
            model: "test-model".to_string(),
            api_key_env: "OLLAMA_API_KEY".to_string(),
            base_url: None,
        },
        provider_urls,
        ..KernelConfig::default()
    };

    let kernel = Arc::new(OpenFangKernel::boot_with_config(config).expect("kernel should boot"));
    kernel.set_self_handle();
    kernel.bootstrap_workflow_definitions().await;

    let manifest =
        toml::from_str(TEST_MANIFEST).expect("dispatch integration manifest should deserialize");
    match kernel.spawn_agent(manifest) {
        Ok(_) => {}
        Err(KernelError::OpenFang(OpenFangError::AgentAlreadyExists(_))) => {}
        Err(error) => panic!("dispatch integration test agent should spawn: {error}"),
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener should bind");
    let address = listener
        .local_addr()
        .expect("test listener should expose an address");
    let (app, state) = build_router(kernel, address).await;

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("dispatch/HITL test server should stay available");
    });

    TestServer {
        base_url: format!("http://{address}"),
        state,
        home_dir,
        data_dir,
        _tmp: tmp,
        _mock: mock,
    }
}

async fn restart_dispatch_hitl_server(
    server: TestServer,
    mock_responses: Vec<String>,
) -> TestServer {
    let home_dir = server.home_dir.clone();
    let data_dir = server.data_dir.clone();
    drop(server);
    sleep(Duration::from_millis(150)).await;
    start_dispatch_hitl_test_server_with_paths(home_dir, data_dir, mock_responses).await
}

fn workflow_definition(id: &str) -> Value {
    workflow_definition_with_dispatch_mode(id, "call")
}

fn workflow_definition_with_dispatch_mode(id: &str, dispatch_mode: &str) -> Value {
    json!({
        "id": id,
        "name": format!("Workflow {id}"),
        "version": "1.0.0",
        "description": "Dispatch/HITL API integration workflow",
        "enabled": true,
        "tags": ["dispatch-hitl"],
        "input": {
            "kind": "object",
            "required": ["topic"],
            "open": false,
            "fields": {
                "topic": { "kind": "string" }
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
            "id": "write-prd",
            "name": "Write PRD",
            "kind": "agent",
            "uses": { "agent": "dispatch-tester" },
            "with": {
                "message": "Review {{ input.topic }}"
            },
            "runtime": {
                "dispatch": dispatch_mode,
            },
            "save_as": "result",
            "flow": { "mode": "sequential" }
        }],
        "outputs": {
            "result": "{{ vars.result }}"
        }
    })
}

fn workflow_definition_with_prep_and_agent_step(id: &str) -> Value {
    json!({
        "id": id,
        "name": format!("Workflow {id}"),
        "version": "1.0.0",
        "description": "Mid-flight restart workflow",
        "enabled": true,
        "tags": ["dispatch-hitl"],
        "input": {
            "kind": "object",
            "required": ["topic"],
            "open": false,
            "fields": {
                "topic": { "kind": "string" }
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
        "steps": [
            {
                "id": "prepare-input",
                "name": "Prepare Input",
                "kind": "noop",
                "save_as": "prepared",
                "flow": { "mode": "sequential" }
            },
            {
                "id": "agent-step",
                "name": "Agent Step",
                "kind": "agent",
                "uses": { "agent": "dispatch-tester" },
                "with": {
                    "message": "Resume {{ vars.prepared }}"
                },
                "runtime": {
                    "dispatch": "call"
                },
                "save_as": "result",
                "flow": { "mode": "sequential" }
            }
        ],
        "outputs": {
            "result": "{{ vars.result }}"
        }
    })
}

fn workflow_start_trigger_definition(id: &str, workflow_id: &str) -> Value {
    json!({
        "id": id,
        "name": format!("Trigger {id}"),
        "description": "Starts workflow from event ingress",
        "enabled": true,
        "max_fires": 0,
        "cooldown_secs": 0,
        "match": {
            "event": "issue.created",
            "source": "api"
        },
        "target": {
            "kind": "workflow_start",
            "workflow": workflow_id,
            "input": {
                "topic": "{{ event.payload.issue_id }}"
            }
        }
    })
}

fn event_ingress_request() -> Value {
    json!({
        "event": "issue.created",
        "source": "api",
        "payload": {
            "issue_id": "ISSUE-123"
        },
        "idempotency_key": "dispatch-hitl-e2e",
        "occurred_at": "2026-03-26T12:00:00Z",
        "metadata": {
            "source": "integration-test"
        }
    })
}

fn seed_filesystem_pack_fixture(home_dir: &Path, pack_id: &str) {
    let pack_root = home_dir.join("packs").join(pack_id);
    fs::create_dir_all(pack_root.join("workflows")).expect("pack workflows directory should exist");
    fs::write(
        pack_root.join("pack.toml"),
        format!(
            r#"
id = "{pack_id}"
name = "Task 43 Fixture Pack"
version = "1.0.0"
description = "Pack fixture for Task 43 endpoint reachability"

[source]
kind = "bundled"

[[objects]]
resource_type = "workflow"
resource_id = "task43-pack-workflow"
"#
        ),
    )
    .expect("pack manifest should be written");
    fs::write(
        pack_root.join("workflows/task43-pack-workflow.toml"),
        r#"
id = "task43-pack-workflow"
name = "Task 43 Pack Workflow"
version = "1.0.0"
description = "Fixture workflow referenced by pack manifest"
enabled = true

[input]
kind = "object"
open = true
required = []

[output]
kind = "object"
open = true
required = []

[[steps]]
id = "noop"
name = "Noop"
kind = "noop"

[steps.flow]
mode = "sequential"
"#,
    )
    .expect("pack workflow fixture should be written");
}

async fn create_workflow_definition(
    client: &reqwest::Client,
    server: &TestServer,
    workflow_id: &str,
) {
    create_workflow_definition_from_value(client, server, workflow_definition(workflow_id)).await;
}

async fn create_workflow_definition_from_value(
    client: &reqwest::Client,
    server: &TestServer,
    workflow: Value,
) {
    let response = client
        .post(format!("{}/api/v1/workflows", server.base_url))
        .json(&workflow)
        .send()
        .await
        .expect("workflow create request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
}

async fn create_workflow_definition_with_dispatch_mode(
    client: &reqwest::Client,
    server: &TestServer,
    workflow_id: &str,
    dispatch_mode: &str,
) {
    let response = client
        .post(format!("{}/api/v1/workflows", server.base_url))
        .json(&workflow_definition_with_dispatch_mode(
            workflow_id,
            dispatch_mode,
        ))
        .send()
        .await
        .expect("workflow create request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
}

async fn create_trigger_definition(
    client: &reqwest::Client,
    server: &TestServer,
    trigger_id: &str,
    workflow_id: &str,
) {
    let response = client
        .post(format!("{}/api/v1/triggers", server.base_url))
        .json(&workflow_start_trigger_definition(trigger_id, workflow_id))
        .send()
        .await
        .expect("trigger create request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
}

async fn emit_event_ingress(client: &reqwest::Client, base_url: &str) -> Value {
    let response = client
        .post(format!("{base_url}/api/v1/events"))
        .json(&event_ingress_request())
        .send()
        .await
        .expect("event ingress request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::ACCEPTED);
    response
        .json::<Value>()
        .await
        .expect("event ingress response should deserialize")
}

async fn start_workflow_run(
    client: &reqwest::Client,
    server: &TestServer,
    workflow_id: &str,
    topic: &str,
) -> String {
    let response = client
        .post(format!(
            "{}/api/v1/workflows/{workflow_id}/runs",
            server.base_url
        ))
        .json(&json!({
            "input": {
                "topic": topic,
            },
            "labels": ["manual"],
            "metadata": {
                "source": "api",
            }
        }))
        .send()
        .await
        .expect("workflow run request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::ACCEPTED);
    response
        .json::<Value>()
        .await
        .expect("workflow run response should deserialize")["run_id"]
        .as_str()
        .expect("run response should include a run_id")
        .to_string()
}

async fn wait_for_run_status(
    client: &reqwest::Client,
    server: &TestServer,
    run_id: &str,
    expected_status: &str,
) -> Value {
    wait_for_run_status_at_base_url(client, &server.base_url, run_id, expected_status).await
}

async fn wait_for_run_status_at_base_url(
    client: &reqwest::Client,
    base_url: &str,
    run_id: &str,
    expected_status: &str,
) -> Value {
    for _ in 0..300 {
        let response = client
            .get(format!("{base_url}/api/v1/runs/{run_id}"))
            .send()
            .await
            .expect("run detail request should succeed");
        let body = response
            .json::<Value>()
            .await
            .expect("run detail should deserialize");
        if body["status"] == expected_status {
            return body;
        }
        sleep(Duration::from_millis(50)).await;
    }

    panic!("run {run_id} did not reach status {expected_status}");
}

async fn wait_for_run_id_for_workflow(
    client: &reqwest::Client,
    base_url: &str,
    workflow_id: &str,
) -> String {
    for _ in 0..120 {
        let response = client
            .get(format!("{base_url}/api/v1/runs?workflow_id={workflow_id}"))
            .send()
            .await
            .expect("run list request should succeed");
        let body = response
            .json::<Value>()
            .await
            .expect("run list response should deserialize");
        if let Some(run_id) = body["items"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item["id"].as_str())
        {
            return run_id.to_string();
        }
        sleep(Duration::from_millis(50)).await;
    }

    panic!("workflow {workflow_id} did not produce a run via event ingress");
}

async fn wait_for_run_status_one_of_at_base_url(
    client: &reqwest::Client,
    base_url: &str,
    run_id: &str,
    expected_statuses: &[&str],
) -> Value {
    for _ in 0..300 {
        let response = client
            .get(format!("{base_url}/api/v1/runs/{run_id}"))
            .send()
            .await
            .expect("run detail request should succeed");
        let body = response
            .json::<Value>()
            .await
            .expect("run detail should deserialize");
        if let Some(status) = body["status"].as_str() {
            if expected_statuses.contains(&status) {
                return body;
            }
        }
        sleep(Duration::from_millis(50)).await;
    }

    panic!("run {run_id} did not reach one of expected statuses: {expected_statuses:?}");
}

async fn wait_for_dispatch_status(
    client: &reqwest::Client,
    server: &TestServer,
    dispatch_id: &str,
    expected_statuses: &[&str],
) -> Value {
    wait_for_dispatch_status_at_base_url(client, &server.base_url, dispatch_id, expected_statuses)
        .await
}

async fn wait_for_dispatch_status_at_base_url(
    client: &reqwest::Client,
    base_url: &str,
    dispatch_id: &str,
    expected_statuses: &[&str],
) -> Value {
    for _ in 0..80 {
        let response = client
            .get(format!("{base_url}/api/v1/dispatches/{dispatch_id}"))
            .send()
            .await
            .expect("dispatch detail request should succeed");
        let body = response
            .json::<Value>()
            .await
            .expect("dispatch detail should deserialize");
        if let Some(status) = body["status"].as_str() {
            if expected_statuses.contains(&status) {
                return body;
            }
        }
        sleep(Duration::from_millis(50)).await;
    }

    panic!(
        "dispatch {dispatch_id} did not reach one of the expected statuses: {expected_statuses:?}"
    );
}

async fn wait_for_first_dispatch_id(
    client: &reqwest::Client,
    server: &TestServer,
    run_id: &str,
) -> String {
    wait_for_first_dispatch_id_at_base_url(client, &server.base_url, run_id).await
}

async fn wait_for_first_dispatch_id_at_base_url(
    client: &reqwest::Client,
    base_url: &str,
    run_id: &str,
) -> String {
    for _ in 0..80 {
        let response = client
            .get(format!("{base_url}/api/v1/dispatches?run_id={run_id}"))
            .send()
            .await
            .expect("dispatch list request should succeed");
        let body = response
            .json::<Value>()
            .await
            .expect("dispatch list should deserialize");
        if let Some(dispatch_id) = body["items"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item["id"].as_str())
        {
            return dispatch_id.to_string();
        }
        sleep(Duration::from_millis(50)).await;
    }

    panic!("run {run_id} did not produce a dispatch");
}

async fn wait_for_pending_hitl_request(
    client: &reqwest::Client,
    server: &TestServer,
    run_id: &str,
) -> Value {
    wait_for_pending_hitl_request_at_base_url(client, &server.base_url, run_id).await
}

async fn wait_for_pending_hitl_request_at_base_url(
    client: &reqwest::Client,
    base_url: &str,
    run_id: &str,
) -> Value {
    for _ in 0..80 {
        let response = client
            .get(format!(
                "{base_url}/api/v1/hitl-requests?run_id={run_id}&status=pending"
            ))
            .send()
            .await
            .expect("HITL list request should succeed");
        let body = response
            .json::<Value>()
            .await
            .expect("HITL list should deserialize");
        if let Some(item) = body["items"].as_array().and_then(|items| items.first()) {
            return item.clone();
        }
        sleep(Duration::from_millis(50)).await;
    }

    panic!("run {run_id} did not produce a pending HITL request");
}

async fn wait_for_hitl_status(
    client: &reqwest::Client,
    server: &TestServer,
    hitl_request_id: &str,
    expected_status: &str,
) -> Value {
    wait_for_hitl_status_at_base_url(client, &server.base_url, hitl_request_id, expected_status)
        .await
}

async fn wait_for_hitl_status_at_base_url(
    client: &reqwest::Client,
    base_url: &str,
    hitl_request_id: &str,
    expected_status: &str,
) -> Value {
    for _ in 0..80 {
        let response = client
            .get(format!("{base_url}/api/v1/hitl-requests/{hitl_request_id}"))
            .send()
            .await
            .expect("HITL detail request should succeed");
        let body = response
            .json::<Value>()
            .await
            .expect("HITL detail should deserialize");
        if body["status"] == expected_status {
            return body;
        }
        sleep(Duration::from_millis(50)).await;
    }

    panic!("HITL request {hitl_request_id} did not reach status {expected_status}");
}

fn durable_run_record(
    run_id: &str,
    workflow_id: &str,
    status: WorkflowRunStatus,
) -> WorkflowRunRecord {
    WorkflowRunRecord {
        run_id: run_id.to_string(),
        workflow_id: workflow_id.to_string(),
        workflow_version: "1.0.0".to_string(),
        status,
        input_json: serde_json::to_string(&json!({ "topic": "dispatch lifecycle" }))
            .expect("run input should serialize"),
        vars_json: "{}".to_string(),
        current_step_id: Some("write-prd".to_string()),
        waiting_kind: None,
        waiting_ref: None,
        active_dispatch_id: None,
        active_hitl_request_id: None,
        labels_json: "[]".to_string(),
        metadata_json: "{}".to_string(),
        error_json: None,
        started_at: "2026-03-25T10:00:00Z".to_string(),
        updated_at: "2026-03-25T10:00:00Z".to_string(),
        completed_at: None,
    }
}

fn durable_dispatch_record(
    dispatch_id: &str,
    run_id: &str,
    status: DispatchStatus,
) -> DispatchRecord {
    let completed_at = if matches!(
        status,
        DispatchStatus::Completed | DispatchStatus::Failed | DispatchStatus::Cancelled
    ) {
        Some("2026-03-25T10:01:00Z".to_string())
    } else {
        None
    };

    DispatchRecord {
        dispatch_id: dispatch_id.to_string(),
        run_id: run_id.to_string(),
        step_id: Some("write-prd".to_string()),
        kind: DispatchKind::Call,
        target_agent: "dispatch-tester".to_string(),
        status,
        input_json: json!({
            "issue": {
                "id": "ISSUE-123"
            }
        }),
        result_json: None,
        error_json: None,
        attempt: 1,
        parent_dispatch_id: None,
        spawned_agent_id: None,
        provider_driver: None,
        session_id: None,
        provider_resume_token: None,
        started_at: "2026-03-25T10:00:00Z".to_string(),
        updated_at: "2026-03-25T10:01:00Z".to_string(),
        completed_at,
    }
}

async fn seed_run(server: &TestServer, record: &WorkflowRunRecord) {
    server
        .state
        .kernel
        .workflow_stores
        .workflow_run
        .insert_run(record)
        .expect("run should be inserted");
}

async fn seed_dispatch(server: &TestServer, record: &DispatchRecord) {
    server
        .state
        .kernel
        .workflow_stores
        .dispatch
        .create(record)
        .await
        .expect("dispatch should be inserted");
}

async fn create_hitl_request(server: &TestServer, run_id: &str, dispatch_id: &str) -> String {
    server
        .state
        .kernel
        .workflow_stores
        .hitl
        .create(NewHitlRequest {
            hitl_request_id: format!("hitl-{}", Uuid::new_v4().simple()),
            run_id: run_id.to_string(),
            step_id: "write-prd".to_string(),
            dispatch_id: Some(dispatch_id.to_string()),
            kind: HitlKind::Clarification,
            question: "Should the PRD prioritize admins or end users first?".to_string(),
            context_json: json!({
                "artifact_type": "prd",
                "artifact_id": "artifact_001",
            }),
            created_at: chrono::Utc::now(),
            timeout_at: None,
        })
        .await
        .expect("HITL request should be inserted")
        .hitl_request_id
}

fn update_run_for_stream_event(server: &TestServer, run_id: &str, step_id: &str, updated_at: &str) {
    let mut run = server
        .state
        .kernel
        .workflow_stores
        .workflow_run
        .find_by_id(run_id)
        .expect("run lookup should succeed")
        .expect("run should exist");
    run.current_step_id = Some(step_id.to_string());
    run.updated_at = updated_at.to_string();
    server
        .state
        .kernel
        .workflow_stores
        .workflow_run
        .replace_run(&run)
        .expect("run update should persist");
}

fn object_keys(value: &Value) -> Vec<String> {
    let mut keys = value
        .as_object()
        .expect("value should be a JSON object")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    keys
}

#[derive(Debug, Clone)]
struct ParsedSseEvent {
    event: String,
    data: Value,
}

async fn open_sse_stream(
    client: &reqwest::Client,
    url: String,
    last_event_id: Option<u64>,
) -> reqwest::Response {
    let mut request = client
        .get(url)
        .header(reqwest::header::ACCEPT, "text/event-stream");
    if let Some(last_event_id) = last_event_id {
        request = request.header("Last-Event-ID", last_event_id.to_string());
    }

    let response = request.send().await.expect("SSE request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    response
}

async fn read_sse_event(
    response: &mut reqwest::Response,
    buffer: &mut String,
    timeout_duration: Duration,
) -> ParsedSseEvent {
    let raw = timeout(timeout_duration, async {
        loop {
            *buffer = buffer.replace("\r\n", "\n");
            if let Some(index) = buffer.find("\n\n") {
                let raw = buffer[..index].to_string();
                *buffer = buffer[index + 2..].to_string();
                return raw;
            }

            let chunk = response
                .chunk()
                .await
                .expect("SSE chunk should be readable")
                .expect("SSE stream should stay open");
            buffer.push_str(&String::from_utf8_lossy(&chunk));
        }
    })
    .await
    .expect("timed out waiting for SSE event");

    let mut id = None;
    let mut event = None;
    let mut data_lines = Vec::new();
    for line in raw.lines() {
        if let Some(value) = line.strip_prefix("id: ") {
            id = Some(
                value
                    .parse::<u64>()
                    .expect("SSE event id should be an unsigned integer"),
            );
        } else if let Some(value) = line.strip_prefix("event: ") {
            event = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("data: ") {
            data_lines.push(value.to_string());
        }
    }

    let data_text = data_lines.join("\n");
    let _event_id = id.expect("SSE event should include an id");
    ParsedSseEvent {
        event: event.expect("SSE event should include an event name"),
        data: serde_json::from_str(&data_text).expect("SSE data should be valid JSON"),
    }
}

#[tokio::test]
async fn dispatch_list_response_should_match_api_spec_summary_shape() {
    let server = start_dispatch_hitl_test_server(Vec::new()).await;
    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string();
    seed_run(
        &server,
        &durable_run_record(
            &run_id,
            "dispatch-summary-shape",
            WorkflowRunStatus::Running,
        ),
    )
    .await;
    seed_dispatch(
        &server,
        &durable_dispatch_record("dispatch-summary", &run_id, DispatchStatus::WaitingHitl),
    )
    .await;

    let response = client
        .get(format!(
            "{}/api/v1/dispatches?run_id={run_id}",
            server.base_url
        ))
        .send()
        .await
        .expect("dispatch list request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body = response
        .json::<Value>()
        .await
        .expect("dispatch list response should deserialize");

    assert_eq!(
        object_keys(&body),
        vec!["items".to_string(), "next_cursor".to_string()]
    );
    let item = body["items"][0].clone();
    assert_eq!(
        object_keys(&item),
        vec![
            "id".to_string(),
            "kind".to_string(),
            "run_id".to_string(),
            "status".to_string(),
            "step_id".to_string(),
            "target_agent".to_string(),
            "updated_at".to_string(),
        ]
    );
    assert_eq!(item["id"], json!("dispatch-summary"));
    assert_eq!(item["status"], json!("waiting_hitl"));
    assert!(body["next_cursor"].is_null());
}

#[tokio::test]
async fn dispatch_detail_response_should_include_all_required_fields() {
    let server = start_dispatch_hitl_test_server(Vec::new()).await;
    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string();
    seed_run(
        &server,
        &durable_run_record(&run_id, "dispatch-detail-shape", WorkflowRunStatus::Running),
    )
    .await;

    let mut record =
        durable_dispatch_record("dispatch-detail", &run_id, DispatchStatus::WaitingHitl);
    record.parent_dispatch_id = None;
    record.spawned_agent_id = None;
    seed_dispatch(&server, &record).await;

    let response = client
        .get(format!(
            "{}/api/v1/dispatches/dispatch-detail",
            server.base_url
        ))
        .send()
        .await
        .expect("dispatch detail request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body = response
        .json::<Value>()
        .await
        .expect("dispatch detail response should deserialize");

    assert_eq!(
        object_keys(&body),
        vec![
            "attempt".to_string(),
            "completed_at".to_string(),
            "error".to_string(),
            "id".to_string(),
            "input".to_string(),
            "kind".to_string(),
            "parent_dispatch_id".to_string(),
            "result".to_string(),
            "run_id".to_string(),
            "spawned_agent_id".to_string(),
            "started_at".to_string(),
            "status".to_string(),
            "step_id".to_string(),
            "target_agent".to_string(),
            "updated_at".to_string(),
        ]
    );
    assert_eq!(body["attempt"], json!(1));
    assert!(body["parent_dispatch_id"].is_null());
    assert!(body["spawned_agent_id"].is_null());
}

#[tokio::test]
async fn hitl_answer_endpoint_should_trigger_dispatch_transition() {
    let server = start_dispatch_hitl_test_server(vec![
        json!({
            "$compozy_hitl": {
                "kind": "clarification",
                "question": "Should the PRD prioritize B2B admins or end users first?",
                "context": {
                    "artifact_type": "prd",
                    "artifact_id": "artifact_001"
                }
            }
        })
        .to_string(),
        "Completed after HITL answer".to_string(),
    ])
    .await;
    let client = reqwest::Client::new();
    create_workflow_definition(&client, &server, "hitl-answer-transition").await;
    let run_id = start_workflow_run(&client, &server, "hitl-answer-transition", "test").await;
    let hitl = wait_for_pending_hitl_request(&client, &server, &run_id).await;
    let dispatch_id = hitl["dispatch_id"]
        .as_str()
        .expect("pending HITL request should include a dispatch_id")
        .to_string();
    let hitl_request_id = hitl["id"]
        .as_str()
        .expect("pending HITL request should include an id")
        .to_string();

    let response = client
        .post(format!(
            "{}/api/v1/hitl-requests/{hitl_request_id}/answer",
            server.base_url
        ))
        .json(&json!({
            "response": {
                "type": "choice",
                "value": "b2b_admins_first",
            },
            "metadata": {
                "source": "api",
            }
        }))
        .send()
        .await
        .expect("HITL answer request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::ACCEPTED);

    let dispatch =
        wait_for_dispatch_status(&client, &server, &dispatch_id, &["running", "completed"]).await;
    assert!(dispatch["status"] == "running" || dispatch["status"] == "completed");
}

#[tokio::test]
async fn hitl_list_with_status_filter_should_return_only_matching_records() {
    let server = start_dispatch_hitl_test_server(Vec::new()).await;
    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string();
    let dispatch_id = "dispatch-hitl-filter";
    seed_run(
        &server,
        &durable_run_record(&run_id, "hitl-filter", WorkflowRunStatus::Running),
    )
    .await;
    seed_dispatch(
        &server,
        &durable_dispatch_record(dispatch_id, &run_id, DispatchStatus::WaitingHitl),
    )
    .await;

    let pending_id = create_hitl_request(&server, &run_id, dispatch_id).await;
    let answered_id = create_hitl_request(&server, &run_id, dispatch_id).await;
    server
        .state
        .kernel
        .workflow_stores
        .hitl
        .answer(
            &answered_id,
            &json!({
                "type": "choice",
                "value": "admins_first",
            }),
            chrono::Utc::now(),
        )
        .await
        .expect("answered HITL request should persist");

    let response = client
        .get(format!(
            "{}/api/v1/hitl-requests?status=pending",
            server.base_url
        ))
        .send()
        .await
        .expect("HITL list request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body = response
        .json::<Value>()
        .await
        .expect("HITL list response should deserialize");
    let items = body["items"]
        .as_array()
        .expect("HITL list should expose an items array");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], json!(pending_id));
    assert_eq!(items[0]["status"], json!("pending"));
}

#[tokio::test]
async fn dispatch_cancel_should_cascade_to_linked_hitl_request() {
    let server = start_dispatch_hitl_test_server(vec![json!({
        "$compozy_hitl": {
            "kind": "clarification",
            "question": "Should the PRD prioritize B2B admins or end users first?",
            "context": {
                "artifact_type": "prd"
            }
        }
    })
    .to_string()])
    .await;
    let client = reqwest::Client::new();
    create_workflow_definition(&client, &server, "dispatch-cancel-hitl").await;
    let run_id = start_workflow_run(&client, &server, "dispatch-cancel-hitl", "cancel").await;
    let hitl = wait_for_pending_hitl_request(&client, &server, &run_id).await;
    let dispatch_id = hitl["dispatch_id"]
        .as_str()
        .expect("pending HITL request should include a dispatch_id")
        .to_string();
    let hitl_request_id = hitl["id"]
        .as_str()
        .expect("pending HITL request should include an id")
        .to_string();

    let response = client
        .post(format!(
            "{}/api/v1/dispatches/{dispatch_id}/cancel",
            server.base_url
        ))
        .send()
        .await
        .expect("dispatch cancel request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::ACCEPTED);

    let hitl_request = wait_for_hitl_status(&client, &server, &hitl_request_id, "cancelled").await;
    let dispatch = wait_for_dispatch_status(&client, &server, &dispatch_id, &["cancelled"]).await;
    assert_eq!(hitl_request["status"], json!("cancelled"));
    assert_eq!(dispatch["status"], json!("cancelled"));
}

#[tokio::test]
async fn dispatch_cancel_should_stop_live_background_send_dispatch() {
    let server =
        start_dispatch_hitl_test_server(vec!["DELAY_MS=1500::send dispatch completed".to_string()])
            .await;
    let client = reqwest::Client::new();
    create_workflow_definition_with_dispatch_mode(&client, &server, "dispatch-send-cancel", "send")
        .await;
    let run_id = start_workflow_run(&client, &server, "dispatch-send-cancel", "cancel send").await;
    let dispatch_id = wait_for_first_dispatch_id(&client, &server, &run_id).await;

    wait_for_dispatch_status(&client, &server, &dispatch_id, &["running"]).await;

    let response = client
        .post(format!(
            "{}/api/v1/dispatches/{dispatch_id}/cancel",
            server.base_url
        ))
        .send()
        .await
        .expect("dispatch cancel request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::ACCEPTED);

    let dispatch = wait_for_dispatch_status(&client, &server, &dispatch_id, &["cancelled"]).await;
    assert_eq!(dispatch["status"], json!("cancelled"));

    sleep(Duration::from_millis(1700)).await;
    let response = client
        .get(format!(
            "{}/api/v1/dispatches/{dispatch_id}",
            server.base_url
        ))
        .send()
        .await
        .expect("dispatch detail request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body = response
        .json::<Value>()
        .await
        .expect("dispatch detail should deserialize");
    assert_eq!(body["status"], json!("cancelled"));
}

#[tokio::test]
async fn dispatch_retry_should_increment_attempt_counter() {
    let server =
        start_dispatch_hitl_test_server(vec!["Retried dispatch completed".to_string()]).await;
    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string();
    let workflow_id = Uuid::new_v4().to_string();
    seed_run(
        &server,
        &durable_run_record(&run_id, &workflow_id, WorkflowRunStatus::Failed),
    )
    .await;

    let mut record = durable_dispatch_record("dispatch-retry", &run_id, DispatchStatus::Failed);
    record.input_json = json!("Retry this dispatch");
    record.error_json = Some(json!({ "message": "first attempt failed" }));
    record.completed_at = Some(now_timestamp());
    seed_dispatch(&server, &record).await;

    let response = client
        .post(format!(
            "{}/api/v1/dispatches/dispatch-retry/retry",
            server.base_url
        ))
        .send()
        .await
        .expect("dispatch retry request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::ACCEPTED);

    let dispatch = wait_for_dispatch_status(
        &client,
        &server,
        "dispatch-retry",
        &["pending", "running", "completed"],
    )
    .await;
    assert_eq!(dispatch["attempt"], json!(2));
}

#[tokio::test]
async fn hitl_answer_on_non_pending_request_should_return_error() {
    let server = start_dispatch_hitl_test_server(Vec::new()).await;
    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string();
    let dispatch_id = "dispatch-answered-hitl";
    seed_run(
        &server,
        &durable_run_record(&run_id, "hitl-non-pending", WorkflowRunStatus::Running),
    )
    .await;
    seed_dispatch(
        &server,
        &durable_dispatch_record(dispatch_id, &run_id, DispatchStatus::Running),
    )
    .await;
    let hitl_request_id = create_hitl_request(&server, &run_id, dispatch_id).await;
    server
        .state
        .kernel
        .workflow_stores
        .hitl
        .answer(
            &hitl_request_id,
            &json!({
                "type": "choice",
                "value": "admins_first",
            }),
            chrono::Utc::now(),
        )
        .await
        .expect("answered HITL request should persist");

    let response = client
        .post(format!(
            "{}/api/v1/hitl-requests/{hitl_request_id}/answer",
            server.base_url
        ))
        .json(&json!({
            "response": {
                "type": "choice",
                "value": "end_users_first",
            },
            "metadata": {}
        }))
        .send()
        .await
        .expect("non-pending HITL answer request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);
    let body = response
        .json::<Value>()
        .await
        .expect("error response should deserialize");
    assert_eq!(body["error"]["code"], json!("invalid_hitl_transition"));
}

#[tokio::test]
async fn run_scoped_dispatch_list_should_return_404_for_missing_run() {
    let server = start_dispatch_hitl_test_server(Vec::new()).await;
    let client = reqwest::Client::new();

    let response = client
        .get(format!(
            "{}/api/v1/runs/{}/dispatches",
            server.base_url,
            Uuid::new_v4()
        ))
        .send()
        .await
        .expect("missing run dispatch list request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn dispatch_lifecycle_visible_through_api_after_runtime_execution() {
    let server =
        start_dispatch_hitl_test_server(vec!["Dispatch lifecycle completed".to_string()]).await;
    let client = reqwest::Client::new();
    create_workflow_definition(&client, &server, "dispatch-lifecycle").await;
    let run_id = start_workflow_run(&client, &server, "dispatch-lifecycle", "lifecycle").await;
    wait_for_run_status(&client, &server, &run_id, "completed").await;

    let response = client
        .get(format!(
            "{}/api/v1/dispatches?run_id={run_id}",
            server.base_url
        ))
        .send()
        .await
        .expect("dispatch list request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body = response
        .json::<Value>()
        .await
        .expect("dispatch list response should deserialize");
    let dispatch = body["items"][0].clone();

    assert_eq!(dispatch["run_id"], json!(run_id));
    assert_eq!(dispatch["status"], json!("completed"));

    let dispatch_id = dispatch["id"]
        .as_str()
        .expect("dispatch summary should include an id");
    let detail = client
        .get(format!(
            "{}/api/v1/dispatches/{dispatch_id}",
            server.base_url
        ))
        .send()
        .await
        .expect("dispatch detail request should succeed")
        .json::<Value>()
        .await
        .expect("dispatch detail response should deserialize");
    assert_eq!(detail["status"], json!("completed"));
    assert_eq!(
        detail["result"]["response"],
        json!("Dispatch lifecycle completed")
    );
}

#[tokio::test]
async fn hitl_answer_end_to_end_through_api() {
    let server = start_dispatch_hitl_test_server(vec![
        json!({
            "$compozy_hitl": {
                "kind": "clarification",
                "question": "Should the PRD prioritize B2B admins or end users first?",
                "context": {
                    "artifact_type": "prd",
                    "artifact_id": "artifact_001"
                }
            }
        })
        .to_string(),
        "Workflow completed after HITL answer".to_string(),
    ])
    .await;
    let client = reqwest::Client::new();
    create_workflow_definition(&client, &server, "hitl-e2e").await;
    let run_id = start_workflow_run(&client, &server, "hitl-e2e", "e2e").await;
    let hitl = wait_for_pending_hitl_request(&client, &server, &run_id).await;
    let hitl_request_id = hitl["id"]
        .as_str()
        .expect("pending HITL request should include an id")
        .to_string();

    let response = client
        .post(format!(
            "{}/api/v1/hitl-requests/{hitl_request_id}/answer",
            server.base_url
        ))
        .json(&json!({
            "response": {
                "type": "choice",
                "value": "b2b_admins_first",
            },
            "metadata": {
                "source": "api",
            }
        }))
        .send()
        .await
        .expect("HITL answer request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::ACCEPTED);

    let run = wait_for_run_status(&client, &server, &run_id, "completed").await;
    assert_eq!(run["status"], json!("completed"));

    let dispatches = client
        .get(format!(
            "{}/api/v1/runs/{run_id}/dispatches",
            server.base_url
        ))
        .send()
        .await
        .expect("run dispatch list request should succeed")
        .json::<Value>()
        .await
        .expect("run dispatch list should deserialize");
    assert_eq!(dispatches["items"][0]["status"], json!("completed"));

    let hitl_requests = client
        .get(format!(
            "{}/api/v1/runs/{run_id}/hitl-requests",
            server.base_url
        ))
        .send()
        .await
        .expect("run HITL list request should succeed")
        .json::<Value>()
        .await
        .expect("run HITL list should deserialize");
    assert_eq!(hitl_requests["items"][0]["status"], json!("answered"));
    assert_eq!(
        hitl_requests["items"][0]["response"]["value"],
        json!("b2b_admins_first")
    );
}

#[tokio::test]
async fn dispatch_children_endpoint_should_return_child_dispatches() {
    let server = start_dispatch_hitl_test_server(Vec::new()).await;
    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string();
    seed_run(
        &server,
        &durable_run_record(&run_id, "dispatch-children", WorkflowRunStatus::Running),
    )
    .await;

    let parent = durable_dispatch_record("dispatch-parent", &run_id, DispatchStatus::Completed);
    seed_dispatch(&server, &parent).await;

    let mut child_a =
        durable_dispatch_record("dispatch-child-a", &run_id, DispatchStatus::Completed);
    child_a.parent_dispatch_id = Some("dispatch-parent".to_string());
    child_a.target_agent = "dispatch-child-a".to_string();
    child_a.updated_at = "2026-03-25T10:02:00Z".to_string();
    seed_dispatch(&server, &child_a).await;

    let mut child_b =
        durable_dispatch_record("dispatch-child-b", &run_id, DispatchStatus::Completed);
    child_b.parent_dispatch_id = Some("dispatch-parent".to_string());
    child_b.target_agent = "dispatch-child-b".to_string();
    child_b.updated_at = "2026-03-25T10:03:00Z".to_string();
    seed_dispatch(&server, &child_b).await;

    let response = client
        .get(format!(
            "{}/api/v1/dispatches/dispatch-parent/children",
            server.base_url
        ))
        .send()
        .await
        .expect("dispatch children request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body = response
        .json::<Value>()
        .await
        .expect("dispatch children response should deserialize");
    let items = body["items"]
        .as_array()
        .expect("children response should include an items array");

    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["id"], json!("dispatch-child-b"));
    assert_eq!(items[1]["id"], json!("dispatch-child-a"));
}

#[tokio::test]
async fn internal_agent_can_use_hitl_api_to_answer_own_question() {
    let server = start_dispatch_hitl_test_server(vec![
        json!({
            "$compozy_hitl": {
                "kind": "clarification",
                "question": "Should the PRD prioritize B2B admins or end users first?",
                "context": {
                    "artifact_type": "prd"
                }
            }
        })
        .to_string(),
        "Internal agent completed the workflow".to_string(),
    ])
    .await;
    let client = reqwest::Client::new();
    create_workflow_definition(&client, &server, "hitl-internal-agent").await;
    let run_id =
        start_workflow_run(&client, &server, "hitl-internal-agent", "internal-agent").await;
    let hitl = wait_for_pending_hitl_request(&client, &server, &run_id).await;
    let hitl_request_id = hitl["id"]
        .as_str()
        .expect("pending HITL request should include an id")
        .to_string();

    let response = client
        .post(format!(
            "{}/api/v1/hitl-requests/{hitl_request_id}/answer",
            server.base_url
        ))
        .json(&json!({
            "response": {
                "type": "choice",
                "value": "agent_selected_b2b_admins_first",
            },
            "metadata": {
                "source": "internal-agent",
            }
        }))
        .send()
        .await
        .expect("internal agent HITL answer request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::ACCEPTED);

    let run = wait_for_run_status(&client, &server, &run_id, "completed").await;
    assert_eq!(run["status"], json!("completed"));
}

#[tokio::test]
async fn sse_dispatch_events_endpoint_should_return_snapshot_and_keepalive() {
    let server = start_dispatch_hitl_test_server(Vec::new()).await;
    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string();
    seed_run(
        &server,
        &durable_run_record(&run_id, "dispatch-sse", WorkflowRunStatus::Running),
    )
    .await;
    seed_dispatch(
        &server,
        &durable_dispatch_record("dispatch-sse", &run_id, DispatchStatus::Running),
    )
    .await;

    let mut response = open_sse_stream(
        &client,
        format!("{}/api/v1/dispatches/dispatch-sse/events", server.base_url),
        None,
    )
    .await;
    assert_eq!(
        response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.starts_with("text/event-stream")),
        Some(true)
    );

    let mut buffer = String::new();
    let snapshot = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
    assert_eq!(snapshot.event, "stream.snapshot");
    assert_eq!(snapshot.data["dispatch"]["id"], json!("dispatch-sse"));

    let keepalive = read_sse_event(&mut response, &mut buffer, Duration::from_secs(20)).await;
    assert_eq!(keepalive.event, "keepalive");
    assert_eq!(keepalive.data, json!({}));
}

#[tokio::test]
async fn sse_dispatch_events_with_expired_last_event_id_should_emit_reset_then_snapshot() {
    let server = start_dispatch_hitl_test_server(Vec::new()).await;
    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string();
    seed_run(
        &server,
        &durable_run_record(&run_id, "dispatch-sse-reset", WorkflowRunStatus::Running),
    )
    .await;
    seed_dispatch(
        &server,
        &durable_dispatch_record("dispatch-sse-reset", &run_id, DispatchStatus::Running),
    )
    .await;

    let handle = server
        .state
        .dispatch_event_stream_registry
        .handle("dispatch-sse-reset");
    let mut oldest_event_id = 0u64;
    for index in 0..60 {
        let event = handle.publish(
            "dispatch.updated",
            json!({
                "id": "dispatch-sse-reset",
                "run_id": run_id,
                "status": "running",
                "updated_at": format!("2026-03-25T10:01:{index:02}Z"),
            }),
        );
        if index == 0 {
            oldest_event_id = event.id;
        }
    }

    let mut response = open_sse_stream(
        &client,
        format!(
            "{}/api/v1/dispatches/dispatch-sse-reset/events",
            server.base_url
        ),
        Some(oldest_event_id),
    )
    .await;
    let mut buffer = String::new();

    let reset = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
    assert_eq!(reset.event, "stream.reset");
    let snapshot = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
    assert_eq!(snapshot.event, "stream.snapshot");
    assert_eq!(snapshot.data["dispatch"]["id"], json!("dispatch-sse-reset"));
}

#[tokio::test]
async fn hitl_stream_should_emit_requested_event_for_new_hitl_request() {
    let server = start_dispatch_hitl_test_server(Vec::new()).await;
    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string();
    seed_run(
        &server,
        &durable_run_record(&run_id, "hitl-stream", WorkflowRunStatus::Running),
    )
    .await;
    seed_dispatch(
        &server,
        &durable_dispatch_record("dispatch-hitl-stream", &run_id, DispatchStatus::WaitingHitl),
    )
    .await;

    let mut response = open_sse_stream(
        &client,
        format!("{}/api/v1/hitl-requests/stream", server.base_url),
        None,
    )
    .await;
    let mut buffer = String::new();
    let snapshot = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
    assert_eq!(snapshot.event, "stream.snapshot");

    let hitl_request_id = create_hitl_request(&server, &run_id, "dispatch-hitl-stream").await;
    let mut requested = None;
    for _ in 0..8 {
        let event = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
        if event.event == "hitl.requested" {
            requested = Some(event);
            break;
        }
    }
    let requested = requested.expect("expected hitl.requested event");
    assert_eq!(requested.data["id"], json!(hitl_request_id));
    assert_eq!(requested.data["run_id"], json!(run_id));
    assert_eq!(
        requested.data["question"],
        json!("Should the PRD prioritize admins or end users first?")
    );
}

#[tokio::test]
async fn hitl_stream_with_pending_filter_should_emit_only_pending_events() {
    let server = start_dispatch_hitl_test_server(Vec::new()).await;
    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string();
    seed_run(
        &server,
        &durable_run_record(&run_id, "hitl-stream-filter", WorkflowRunStatus::Running),
    )
    .await;
    seed_dispatch(
        &server,
        &durable_dispatch_record(
            "dispatch-hitl-filter-stream",
            &run_id,
            DispatchStatus::WaitingHitl,
        ),
    )
    .await;

    let mut response = open_sse_stream(
        &client,
        format!(
            "{}/api/v1/hitl-requests/stream?status=pending",
            server.base_url
        ),
        None,
    )
    .await;
    let mut buffer = String::new();
    let snapshot = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
    assert_eq!(snapshot.event, "stream.snapshot");

    let answered_id = create_hitl_request(&server, &run_id, "dispatch-hitl-filter-stream").await;
    server
        .state
        .kernel
        .workflow_stores
        .hitl
        .answer(
            &answered_id,
            &json!({
                "type": "choice",
                "value": "answered",
            }),
            chrono::Utc::now(),
        )
        .await
        .expect("answered HITL request should persist");
    let pending_id = create_hitl_request(&server, &run_id, "dispatch-hitl-filter-stream").await;

    let mut saw_pending_event = false;
    for _ in 0..10 {
        let event = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
        if event.event != "hitl.requested" {
            continue;
        }
        assert_eq!(event.data["status"], json!("pending"));
        assert_eq!(event.data["id"], json!(pending_id));
        assert!(event.data["id"] != json!(answered_id));
        saw_pending_event = true;
        break;
    }

    assert!(
        saw_pending_event,
        "expected a pending-only hitl.requested event"
    );
}

#[tokio::test]
async fn hitl_stream_with_pending_filter_should_emit_answered_event_for_pending_request() {
    let server = start_dispatch_hitl_test_server(Vec::new()).await;
    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string();
    let dispatch_id = "dispatch-hitl-answer-stream";
    seed_run(
        &server,
        &durable_run_record(&run_id, "hitl-stream-answer", WorkflowRunStatus::Running),
    )
    .await;
    seed_dispatch(
        &server,
        &durable_dispatch_record(dispatch_id, &run_id, DispatchStatus::WaitingHitl),
    )
    .await;

    let mut response = open_sse_stream(
        &client,
        format!(
            "{}/api/v1/hitl-requests/stream?status=pending",
            server.base_url
        ),
        None,
    )
    .await;
    let mut buffer = String::new();
    let snapshot = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
    assert_eq!(snapshot.event, "stream.snapshot");

    let hitl_request_id = create_hitl_request(&server, &run_id, dispatch_id).await;
    for _ in 0..10 {
        let event = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
        if event.event == "hitl.requested" && event.data["id"] == json!(hitl_request_id) {
            break;
        }
    }

    server
        .state
        .kernel
        .workflow_stores
        .hitl
        .answer(
            &hitl_request_id,
            &json!({
                "type": "choice",
                "value": "answered",
            }),
            chrono::Utc::now(),
        )
        .await
        .expect("answered HITL request should persist");

    let mut answered = None;
    for _ in 0..10 {
        let event = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
        if event.event == "hitl.answered" {
            answered = Some(event);
            break;
        }
    }

    let answered = answered.expect("expected hitl.answered event");
    assert_eq!(answered.data["id"], json!(hitl_request_id));
    assert_eq!(answered.data["status"], json!("answered"));
}

#[tokio::test]
async fn hitl_stream_with_pending_filter_should_emit_cancelled_event_for_pending_request() {
    let server = start_dispatch_hitl_test_server(Vec::new()).await;
    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string();
    let dispatch_id = "dispatch-hitl-cancel-stream";
    seed_run(
        &server,
        &durable_run_record(&run_id, "hitl-stream-cancel", WorkflowRunStatus::Running),
    )
    .await;
    seed_dispatch(
        &server,
        &durable_dispatch_record(dispatch_id, &run_id, DispatchStatus::WaitingHitl),
    )
    .await;

    let mut response = open_sse_stream(
        &client,
        format!(
            "{}/api/v1/hitl-requests/stream?status=pending",
            server.base_url
        ),
        None,
    )
    .await;
    let mut buffer = String::new();
    let snapshot = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
    assert_eq!(snapshot.event, "stream.snapshot");

    let hitl_request_id = create_hitl_request(&server, &run_id, dispatch_id).await;
    for _ in 0..10 {
        let event = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
        if event.event == "hitl.requested" && event.data["id"] == json!(hitl_request_id) {
            break;
        }
    }

    server
        .state
        .kernel
        .workflow_stores
        .hitl
        .cancel(&hitl_request_id)
        .await
        .expect("cancelled HITL request should persist");

    let mut cancelled = None;
    for _ in 0..10 {
        let event = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
        if event.event == "hitl.cancelled" {
            cancelled = Some(event);
            break;
        }
    }

    let cancelled = cancelled.expect("expected hitl.cancelled event");
    assert_eq!(cancelled.data["id"], json!(hitl_request_id));
    assert_eq!(cancelled.data["status"], json!("cancelled"));
}

#[tokio::test]
async fn hitl_stream_with_pending_filter_should_emit_timed_out_event_for_pending_request() {
    let server = start_dispatch_hitl_test_server(Vec::new()).await;
    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string();
    let dispatch_id = "dispatch-hitl-timeout-stream";
    seed_run(
        &server,
        &durable_run_record(&run_id, "hitl-stream-timeout", WorkflowRunStatus::Running),
    )
    .await;
    seed_dispatch(
        &server,
        &durable_dispatch_record(dispatch_id, &run_id, DispatchStatus::WaitingHitl),
    )
    .await;

    let mut response = open_sse_stream(
        &client,
        format!(
            "{}/api/v1/hitl-requests/stream?status=pending",
            server.base_url
        ),
        None,
    )
    .await;
    let mut buffer = String::new();
    let snapshot = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
    assert_eq!(snapshot.event, "stream.snapshot");

    let hitl_request_id = create_hitl_request(&server, &run_id, dispatch_id).await;
    for _ in 0..10 {
        let event = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
        if event.event == "hitl.requested" && event.data["id"] == json!(hitl_request_id) {
            break;
        }
    }

    server
        .state
        .kernel
        .workflow_stores
        .hitl
        .mark_timed_out(&hitl_request_id)
        .await
        .expect("timed out HITL request should persist");

    let mut timed_out = None;
    for _ in 0..10 {
        let event = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
        if event.event == "hitl.timed_out" {
            timed_out = Some(event);
            break;
        }
    }

    let timed_out = timed_out.expect("expected hitl.timed_out event");
    assert_eq!(timed_out.data["id"], json!(hitl_request_id));
    assert_eq!(timed_out.data["status"], json!("timed_out"));
}

#[tokio::test]
async fn run_sse_should_emit_run_updated_with_api_spec_summary_shape() {
    let server = start_dispatch_hitl_test_server(Vec::new()).await;
    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string();
    seed_run(
        &server,
        &durable_run_record(&run_id, "run-sse-summary-shape", WorkflowRunStatus::Running),
    )
    .await;

    let mut response = open_sse_stream(
        &client,
        format!("{}/api/v1/runs/{run_id}/events", server.base_url),
        None,
    )
    .await;
    let mut buffer = String::new();
    let snapshot = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
    assert_eq!(snapshot.event, "stream.snapshot");
    assert_eq!(snapshot.data["run"]["id"], json!(run_id));

    update_run_for_stream_event(&server, &run_id, "review-prd", "2026-03-25T10:00:10Z");

    let mut run_updated_event = None;
    for _ in 0..10 {
        let event = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
        if event.event == "run.updated" {
            run_updated_event = Some(event);
            break;
        }
    }
    let run_updated_event = run_updated_event.expect("expected run.updated event");
    assert_eq!(run_updated_event.event, "run.updated");
    assert_eq!(run_updated_event.data["id"], json!(run_id));
    assert_eq!(
        run_updated_event.data["workflow_id"],
        json!("run-sse-summary-shape")
    );
    assert_eq!(run_updated_event.data["status"], json!("running"));
    assert_eq!(
        run_updated_event.data["current_step_id"],
        json!("review-prd")
    );
    assert_eq!(run_updated_event.data["waiting_kind"], Value::Null);
    assert_eq!(
        run_updated_event.data["started_at"],
        json!("2026-03-25T10:00:00Z")
    );
    assert_eq!(
        run_updated_event.data["updated_at"],
        json!("2026-03-25T10:00:10Z")
    );
}

#[tokio::test]
async fn run_sse_should_remain_stable_for_60_seconds_with_multiple_state_changes() {
    let server = start_dispatch_hitl_test_server(Vec::new()).await;
    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string();
    seed_run(
        &server,
        &durable_run_record(&run_id, "run-sse-stability", WorkflowRunStatus::Running),
    )
    .await;

    let mut response = open_sse_stream(
        &client,
        format!("{}/api/v1/runs/{run_id}/events", server.base_url),
        None,
    )
    .await;
    let mut buffer = String::new();
    let snapshot = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
    assert_eq!(snapshot.event, "stream.snapshot");
    assert_eq!(snapshot.data["run"]["id"], json!(run_id));

    let state_for_updates = Arc::clone(&server.state);
    let run_id_for_updates = run_id.clone();
    tokio::spawn(async move {
        for index in 0..5 {
            sleep(Duration::from_secs(3)).await;
            let mut run = state_for_updates
                .kernel
                .workflow_stores
                .workflow_run
                .find_by_id(&run_id_for_updates)
                .expect("run lookup should succeed")
                .expect("run should exist");
            run.current_step_id = Some(format!("step-{index}"));
            run.updated_at = format!("2026-03-25T10:00:{:02}Z", 20 + index);
            state_for_updates
                .kernel
                .workflow_stores
                .workflow_run
                .replace_run(&run)
                .expect("run update should persist");
        }
    });

    let started = tokio::time::Instant::now();
    let mut run_updated_events = 0usize;
    let mut keepalive_events = 0usize;
    while started.elapsed() < Duration::from_secs(62) {
        let event = read_sse_event(&mut response, &mut buffer, Duration::from_secs(20)).await;
        if event.event == "run.updated" {
            run_updated_events += 1;
        } else if event.event == "keepalive" {
            keepalive_events += 1;
            assert_eq!(event.data, json!({}));
        }
    }

    assert!(
        run_updated_events >= 5,
        "expected at least five run.updated events during the stream"
    );
    assert!(
        keepalive_events > 0,
        "expected at least one keepalive event while holding the stream open"
    );
}

#[tokio::test]
async fn task43_e2e_event_ingress_restart_during_hitl_should_complete_and_preserve_durable_state() {
    let tmp = tempfile::tempdir().expect("temp dir should be created");
    let home_dir = tmp.path().to_path_buf();
    let data_dir = home_dir.join("data");
    let fixture_pack_id = "task43-pack-fs";
    seed_filesystem_pack_fixture(&home_dir, fixture_pack_id);

    let workflow_id = "task43-e2e-event";
    let trigger_id = "task43-e2e-trigger";
    let artifact_id = ArtifactId::new("artifact_task43");
    let artifact_version_id = ArtifactVersionId::new("artifact_task43_v1");
    let doc_id = DocId::new("doc_task43");
    let doc_version_id = DocVersionId::new("doc_task43_v1");

    let mut server = start_dispatch_hitl_test_server_with_paths(
        home_dir.clone(),
        data_dir,
        vec![json!({
            "$compozy_hitl": {
                "kind": "clarification",
                "question": "Should the PRD prioritize B2B admins or end users first?",
                "context": {
                    "artifact_type": "prd",
                    "artifact_id": artifact_id.as_ref()
                }
            }
        })
        .to_string()],
    )
    .await;
    let client = reqwest::Client::new();

    create_workflow_definition(&client, &server, workflow_id).await;
    create_trigger_definition(&client, &server, trigger_id, workflow_id).await;

    let seeded_at = now_timestamp();
    server
        .state
        .kernel
        .workflow_stores
        .artifact
        .create(&NewArtifact {
            artifact_id: artifact_id.clone(),
            artifact_version_id: artifact_version_id.clone(),
            type_name: ArtifactType::new("prd"),
            metadata: json!({
                "source": "task43-test",
            }),
            content: json!({
                "title": "Task 43 seeded artifact",
                "body": "This artifact should survive restart.",
            }),
            provenance: Some(ProvenanceRef {
                kind: ProvenanceKind::Run,
                ref_id: "seeded-run".to_string(),
            }),
            created_at: seeded_at.clone(),
        })
        .expect("artifact seed should persist");
    server
        .state
        .kernel
        .workflow_stores
        .doc
        .create(&NewDoc {
            doc_id: doc_id.clone(),
            doc_version_id: doc_version_id.clone(),
            type_name: DocType::new("prd_doc"),
            metadata: json!({
                "source": "task43-test",
            }),
            content: json!({
                "title": "Task 43 seeded doc",
                "body": "This doc should survive restart.",
            }),
            provenance: Some(ProvenanceRef {
                kind: ProvenanceKind::Api,
                ref_id: "seeded-api".to_string(),
            }),
            created_at: seeded_at.clone(),
        })
        .expect("doc seed should persist");
    server
        .state
        .kernel
        .workflow_stores
        .pack
        .upsert(&PackRecord {
            pack_id: "task43-pack-db".to_string(),
            name: "Task 43 Durable Pack".to_string(),
            version: "1.0.0".to_string(),
            source_kind: PackSourceKind::Bundled,
            installed: 1,
            managed: 1,
            installed_at: seeded_at.clone(),
            updated_at: seeded_at.clone(),
            objects: PackObjectCounts {
                agents: 1,
                workflows: 1,
                triggers: 1,
                schedules: 0,
                templates: 0,
            },
        })
        .expect("pack seed should persist");

    let event_response = emit_event_ingress(&client, &server.base_url).await;
    assert_eq!(event_response["accepted"], json!(true));

    let run_id = wait_for_run_id_for_workflow(&client, &server.base_url, workflow_id).await;
    let run_started = wait_for_run_status_one_of_at_base_url(
        &client,
        &server.base_url,
        &run_id,
        &["running", "waiting_hitl"],
    )
    .await;
    assert_eq!(run_started["workflow_id"], json!(workflow_id));

    let dispatch_id = wait_for_first_dispatch_id(&client, &server, &run_id).await;
    let waiting_dispatch =
        wait_for_dispatch_status(&client, &server, &dispatch_id, &["waiting_hitl"]).await;
    assert_eq!(waiting_dispatch["status"], json!("waiting_hitl"));

    let pending_hitl = wait_for_pending_hitl_request(&client, &server, &run_id).await;
    let hitl_request_id = pending_hitl["id"]
        .as_str()
        .expect("pending HITL request should include id")
        .to_string();
    assert_eq!(pending_hitl["status"], json!("pending"));

    let checkpoint_response = client
        .get(format!(
            "{}/api/v1/runs/{run_id}/checkpoints",
            server.base_url
        ))
        .send()
        .await
        .expect("checkpoint list request should succeed");
    assert_eq!(checkpoint_response.status(), reqwest::StatusCode::OK);
    let checkpoint_body = checkpoint_response
        .json::<Value>()
        .await
        .expect("checkpoint list should deserialize");
    let checkpoints_before_restart = checkpoint_body["items"]
        .as_array()
        .expect("checkpoint list should expose items")
        .len();
    assert!(checkpoints_before_restart > 0);

    let packs_response = client
        .get(format!("{}/api/v1/packs", server.base_url))
        .send()
        .await
        .expect("pack list request should succeed");
    assert_eq!(packs_response.status(), reqwest::StatusCode::OK);
    let packs_body = packs_response
        .json::<Value>()
        .await
        .expect("pack list response should deserialize");
    let packs = packs_body["items"]
        .as_array()
        .expect("pack list should return items");
    assert!(packs
        .iter()
        .any(|item| item["id"] == json!(fixture_pack_id)));

    let pack_detail = client
        .get(format!(
            "{}/api/v1/packs/{fixture_pack_id}",
            server.base_url
        ))
        .send()
        .await
        .expect("pack detail request should succeed");
    assert_eq!(pack_detail.status(), reqwest::StatusCode::OK);

    let pack_objects = client
        .get(format!(
            "{}/api/v1/packs/{fixture_pack_id}/objects",
            server.base_url
        ))
        .send()
        .await
        .expect("pack objects request should succeed");
    assert_eq!(pack_objects.status(), reqwest::StatusCode::OK);
    let pack_objects_body = pack_objects
        .json::<Value>()
        .await
        .expect("pack objects response should deserialize");
    let pack_object_items = pack_objects_body["items"]
        .as_array()
        .expect("pack object list should return items");
    assert!(!pack_object_items.is_empty());

    let mut run_stream = open_sse_stream(
        &client,
        format!("{}/api/v1/runs/{run_id}/events", server.base_url),
        None,
    )
    .await;
    let mut run_stream_buffer = String::new();
    let run_snapshot = read_sse_event(
        &mut run_stream,
        &mut run_stream_buffer,
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(run_snapshot.event, "stream.snapshot");
    assert_eq!(run_snapshot.data["run"]["id"], json!(run_id));

    let mut dispatch_stream = open_sse_stream(
        &client,
        format!("{}/api/v1/dispatches/{dispatch_id}/events", server.base_url),
        None,
    )
    .await;
    let mut dispatch_stream_buffer = String::new();
    let dispatch_snapshot = read_sse_event(
        &mut dispatch_stream,
        &mut dispatch_stream_buffer,
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(dispatch_snapshot.event, "stream.snapshot");
    assert_eq!(dispatch_snapshot.data["dispatch"]["id"], json!(dispatch_id));

    let mut hitl_stream = open_sse_stream(
        &client,
        format!("{}/api/v1/hitl-requests/stream", server.base_url),
        None,
    )
    .await;
    let mut hitl_stream_buffer = String::new();
    let hitl_snapshot = read_sse_event(
        &mut hitl_stream,
        &mut hitl_stream_buffer,
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(hitl_snapshot.event, "stream.snapshot");

    server = restart_dispatch_hitl_server(
        server,
        vec![json!({
            "kind": "artifact_ref",
            "artifact_id": artifact_id.as_ref(),
            "artifact_version_id": artifact_version_id.as_ref(),
        })
        .to_string()],
    )
    .await;

    let restored_pending_hitl =
        wait_for_pending_hitl_request_at_base_url(&client, &server.base_url, &run_id).await;
    assert_eq!(restored_pending_hitl["id"], json!(hitl_request_id));
    assert_eq!(restored_pending_hitl["status"], json!("pending"));
    let restored_dispatch = wait_for_dispatch_status_at_base_url(
        &client,
        &server.base_url,
        &dispatch_id,
        &["waiting_hitl"],
    )
    .await;
    assert_eq!(restored_dispatch["status"], json!("waiting_hitl"));

    let answer_response = client
        .post(format!(
            "{}/api/v1/hitl-requests/{hitl_request_id}/answer",
            server.base_url
        ))
        .json(&json!({
            "response": {
                "type": "choice",
                "value": "b2b_admins_first",
            },
            "metadata": {
                "source": "task43-test",
            }
        }))
        .send()
        .await
        .expect("HITL answer request should succeed");
    assert_eq!(answer_response.status(), reqwest::StatusCode::ACCEPTED);

    let answered_hitl =
        wait_for_hitl_status_at_base_url(&client, &server.base_url, &hitl_request_id, "answered")
            .await;
    assert_eq!(answered_hitl["status"], json!("answered"));

    let completed_run =
        wait_for_run_status_at_base_url(&client, &server.base_url, &run_id, "completed").await;
    assert_eq!(completed_run["status"], json!("completed"));
    let output_ref_raw = completed_run["vars"]["result"]
        .as_str()
        .expect("completed run vars.result should be a string");
    let output_ref = serde_json::from_str::<Value>(output_ref_raw)
        .expect("completed run vars.result should be valid JSON");
    assert_eq!(output_ref["kind"], json!("artifact_ref"));
    assert_eq!(output_ref["artifact_id"], json!(artifact_id.as_ref()));
    assert_eq!(
        output_ref["artifact_version_id"],
        json!(artifact_version_id.as_ref())
    );

    let checkpoints_after_response = client
        .get(format!(
            "{}/api/v1/runs/{run_id}/checkpoints",
            server.base_url
        ))
        .send()
        .await
        .expect("checkpoint list request should succeed after restart");
    assert_eq!(checkpoints_after_response.status(), reqwest::StatusCode::OK);
    let checkpoints_after_body = checkpoints_after_response
        .json::<Value>()
        .await
        .expect("checkpoint list response should deserialize after restart");
    let checkpoints_after_restart = checkpoints_after_body["items"]
        .as_array()
        .expect("checkpoint list should expose items after restart")
        .len();
    assert!(checkpoints_after_restart >= checkpoints_before_restart);

    let dispatches_after = client
        .get(format!(
            "{}/api/v1/dispatches?run_id={run_id}",
            server.base_url
        ))
        .send()
        .await
        .expect("dispatch list request should succeed after restart")
        .json::<Value>()
        .await
        .expect("dispatch list should deserialize after restart");
    let dispatch_items = dispatches_after["items"]
        .as_array()
        .expect("dispatch list should expose items");
    assert!(dispatch_items
        .iter()
        .any(|item| item["id"] == json!(dispatch_id)));

    let hitl_after = client
        .get(format!(
            "{}/api/v1/hitl-requests?run_id={run_id}",
            server.base_url
        ))
        .send()
        .await
        .expect("HITL list request should succeed after restart")
        .json::<Value>()
        .await
        .expect("HITL list should deserialize after restart");
    let hitl_items = hitl_after["items"]
        .as_array()
        .expect("HITL list should expose items");
    assert!(hitl_items
        .iter()
        .any(|item| item["id"] == json!(hitl_request_id)));

    let artifact_versions = client
        .get(format!(
            "{}/api/v1/artifacts/{}/versions",
            server.base_url, artifact_id
        ))
        .send()
        .await
        .expect("artifact versions request should succeed")
        .json::<Value>()
        .await
        .expect("artifact versions response should deserialize");
    assert_eq!(
        artifact_versions["items"][0]["id"],
        json!(artifact_version_id.as_ref())
    );

    let doc_versions = client
        .get(format!(
            "{}/api/v1/docs/{}/versions",
            server.base_url, doc_id
        ))
        .send()
        .await
        .expect("doc versions request should succeed")
        .json::<Value>()
        .await
        .expect("doc versions response should deserialize");
    assert_eq!(
        doc_versions["items"][0]["id"],
        json!(doc_version_id.as_ref())
    );

    let artifact_record = server
        .state
        .kernel
        .workflow_stores
        .artifact
        .find_by_id(&artifact_id)
        .expect("artifact lookup should succeed");
    assert!(artifact_record.is_some());
    let artifact_version_record = server
        .state
        .kernel
        .workflow_stores
        .artifact
        .find_version_by_id(&artifact_version_id)
        .expect("artifact version lookup should succeed");
    assert!(artifact_version_record.is_some());

    let doc_record = server
        .state
        .kernel
        .workflow_stores
        .doc
        .find_by_id(&doc_id)
        .expect("doc lookup should succeed");
    assert!(doc_record.is_some());
    let doc_version_record = server
        .state
        .kernel
        .workflow_stores
        .doc
        .find_version_by_id(&doc_version_id)
        .expect("doc version lookup should succeed");
    assert!(doc_version_record.is_some());

    let pack_record = server
        .state
        .kernel
        .workflow_stores
        .pack
        .find_by_id("task43-pack-db")
        .expect("pack lookup should succeed");
    assert!(pack_record.is_some());
}

#[tokio::test]
async fn task43_restart_mid_flight_run_should_resume_from_checkpoint_and_complete() {
    let tmp = tempfile::tempdir().expect("temp dir should be created");
    let home_dir = tmp.path().to_path_buf();
    let data_dir = home_dir.join("data");

    let mut server = start_dispatch_hitl_test_server_with_paths(
        home_dir,
        data_dir,
        vec!["DELAY_MS=7000::this should not complete before restart".to_string()],
    )
    .await;
    let client = reqwest::Client::new();
    let workflow_id = "task43-mid-flight";

    create_workflow_definition_from_value(
        &client,
        &server,
        workflow_definition_with_prep_and_agent_step(workflow_id),
    )
    .await;

    let run_id = start_workflow_run(&client, &server, workflow_id, "restart-mid-flight").await;
    let dispatch_id = wait_for_first_dispatch_id(&client, &server, &run_id).await;
    let running_dispatch =
        wait_for_dispatch_status(&client, &server, &dispatch_id, &["running"]).await;
    assert_eq!(running_dispatch["status"], json!("running"));

    let pre_restart_checkpoints_response = client
        .get(format!(
            "{}/api/v1/runs/{run_id}/checkpoints",
            server.base_url
        ))
        .send()
        .await
        .expect("checkpoint list request should succeed");
    assert_eq!(
        pre_restart_checkpoints_response.status(),
        reqwest::StatusCode::OK
    );
    let pre_restart_checkpoints = pre_restart_checkpoints_response
        .json::<Value>()
        .await
        .expect("checkpoint list should deserialize");
    let pre_restart_checkpoint_items = pre_restart_checkpoints["items"]
        .as_array()
        .expect("checkpoint items should be present");
    assert!(pre_restart_checkpoint_items
        .iter()
        .any(|item| item["kind"] == json!("step_completed")
            && item["step_id"] == json!("prepare-input")));

    server = restart_dispatch_hitl_server(
        server,
        vec!["Recovered after restart and completed".to_string()],
    )
    .await;

    let recovered_run =
        wait_for_run_status_at_base_url(&client, &server.base_url, &run_id, "paused").await;
    assert_eq!(recovered_run["status"], json!("paused"));

    let resume_response = client
        .post(format!("{}/api/v1/runs/{run_id}/resume", server.base_url))
        .send()
        .await
        .expect("run resume request should succeed");
    assert_eq!(resume_response.status(), reqwest::StatusCode::OK);

    let completed_run =
        wait_for_run_status_at_base_url(&client, &server.base_url, &run_id, "completed").await;
    assert_eq!(completed_run["status"], json!("completed"));

    let post_restart_checkpoints_response = client
        .get(format!(
            "{}/api/v1/runs/{run_id}/checkpoints",
            server.base_url
        ))
        .send()
        .await
        .expect("post-restart checkpoint list request should succeed");
    assert_eq!(
        post_restart_checkpoints_response.status(),
        reqwest::StatusCode::OK
    );
    let post_restart_checkpoints = post_restart_checkpoints_response
        .json::<Value>()
        .await
        .expect("post-restart checkpoint list should deserialize");
    let post_restart_checkpoint_items = post_restart_checkpoints["items"]
        .as_array()
        .expect("post-restart checkpoint items should be present");
    assert!(post_restart_checkpoint_items
        .iter()
        .any(|item| item["kind"] == json!("run_recovered_needs_resume")));
    assert!(post_restart_checkpoint_items
        .iter()
        .any(|item| item["kind"] == json!("step_completed")
            && item["step_id"] == json!("prepare-input")));

    let dispatches_after_restart = client
        .get(format!(
            "{}/api/v1/dispatches?run_id={run_id}",
            server.base_url
        ))
        .send()
        .await
        .expect("dispatch list request should succeed after restart")
        .json::<Value>()
        .await
        .expect("dispatch list should deserialize after restart");
    let dispatch_items = dispatches_after_restart["items"]
        .as_array()
        .expect("dispatch items should be present after restart");
    assert!(dispatch_items.iter().any(|item| {
        item["id"] == json!(dispatch_id)
            && (item["status"] == json!("completed") || item["status"] == json!("cancelled"))
    }));
}
