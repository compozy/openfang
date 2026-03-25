//! Real HTTP integration tests for the dispatch/HITL v1 control-plane surface.

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::StreamExt;
use openfang_api::routes::AppState;
use openfang_api::server::build_router;
use openfang_kernel::OpenFangKernel;
use openfang_memory::{
    now_timestamp, DispatchKind, DispatchRecord, DispatchRepository, DispatchStatus, HitlKind,
    HitlRepository, NewHitlRequest, WorkflowRunRecord, WorkflowRunStatus,
};
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout, Duration};
use uuid::Uuid;

struct TestServer {
    base_url: String,
    state: Arc<AppState>,
    _tmp: tempfile::TempDir,
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
    let mock = start_mock_llm_server(mock_responses).await;
    let mut provider_urls = HashMap::new();
    provider_urls.insert("ollama".to_string(), format!("{}/v1", mock.base_url));

    let config = KernelConfig {
        home_dir: tmp.path().to_path_buf(),
        data_dir: tmp.path().join("data"),
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
    kernel
        .spawn_agent(manifest)
        .expect("dispatch integration test agent should spawn");

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
        _tmp: tmp,
        _mock: mock,
    }
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

async fn create_workflow_definition(
    client: &reqwest::Client,
    server: &TestServer,
    workflow_id: &str,
) {
    let response = client
        .post(format!("{}/api/v1/workflows", server.base_url))
        .json(&workflow_definition(workflow_id))
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
    for _ in 0..80 {
        let response = client
            .get(format!("{}/api/v1/runs/{run_id}", server.base_url))
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

async fn wait_for_dispatch_status(
    client: &reqwest::Client,
    server: &TestServer,
    dispatch_id: &str,
    expected_statuses: &[&str],
) -> Value {
    for _ in 0..80 {
        let response = client
            .get(format!(
                "{}/api/v1/dispatches/{dispatch_id}",
                server.base_url
            ))
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
    for _ in 0..80 {
        let response = client
            .get(format!(
                "{}/api/v1/dispatches?run_id={run_id}",
                server.base_url
            ))
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
    for _ in 0..80 {
        let response = client
            .get(format!(
                "{}/api/v1/hitl-requests?run_id={run_id}&status=pending",
                server.base_url
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
    for _ in 0..80 {
        let response = client
            .get(format!(
                "{}/api/v1/hitl-requests/{hitl_request_id}",
                server.base_url
            ))
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

    let response = client
        .get(format!(
            "{}/api/v1/dispatches/dispatch-sse/events",
            server.base_url
        ))
        .send()
        .await
        .expect("dispatch SSE request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.starts_with("text/event-stream")),
        Some(true)
    );

    let stream_text = timeout(Duration::from_secs(3), async {
        let mut text = String::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.expect("SSE chunk should be readable");
            text.push_str(std::str::from_utf8(&chunk).expect("SSE chunk should be valid UTF-8"));
            if text.contains("event: stream.snapshot") && text.contains("event: keepalive") {
                return text;
            }
        }
        text
    })
    .await
    .expect("SSE stream should emit snapshot and keepalive");

    assert!(stream_text.contains("event: stream.snapshot"));
    assert!(stream_text.contains("event: keepalive"));
}
