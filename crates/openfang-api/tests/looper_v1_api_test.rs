//! Real HTTP integration tests for the looper v1 control-plane surface.

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use openfang_api::routes::AppState;
use openfang_api::server::build_router;
use openfang_kernel::OpenFangKernel;
use openfang_memory::{DispatchRepository, NewLooperRun};
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use openfang_types::looper::{
    LooperExecutionMode, LooperExecutionPolicy, LooperRunId, LooperRunResource, LooperRunStatus,
    LooperSelectionStrategy,
};
use openfang_types::task::{
    ActorKind, AssigneeRef, Complexity, OwnerRef, SubtaskId, SubtaskKind, SubtaskRecord,
    SubtaskStatus, TaskId, TaskRecord, TaskSource,
};
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
    requests: Arc<Mutex<Vec<String>>>,
}

struct MockLlmServer {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

#[derive(Debug)]
struct ParsedSseEvent {
    id: u64,
    event: String,
    data: Value,
}

async fn mock_chat_completion(State(state): State<MockLlmState>) -> Json<Value> {
    state
        .requests
        .lock()
        .await
        .push("POST /v1/chat/completions".to_string());
    let queued = state
        .responses
        .lock()
        .await
        .pop_front()
        .unwrap_or_else(|| "default looper completion".to_string());
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

async fn mock_models(State(state): State<MockLlmState>) -> Json<Value> {
    state
        .requests
        .lock()
        .await
        .push("GET /v1/models".to_string());
    Json(json!({
        "object": "list",
        "data": [{
            "id": "test-model",
            "object": "model",
        }],
    }))
}

async fn mock_ollama_tags(State(state): State<MockLlmState>) -> Json<Value> {
    state
        .requests
        .lock()
        .await
        .push("GET /api/tags".to_string());
    Json(json!({
        "models": [{
            "name": "test-model",
        }],
    }))
}

async fn start_mock_llm_server(responses: Vec<String>) -> MockLlmServer {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let state = MockLlmState {
        responses: Arc::new(Mutex::new(VecDeque::from(responses))),
        requests: Arc::clone(&requests),
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
        requests,
    }
}

const TEST_MANIFEST: &str = r#"
name = "looper-test-agent"
version = "0.1.0"
description = "Looper API integration test agent"
author = "test"
module = "builtin:chat"

[model]
provider = "ollama"
model = "test-model"
system_prompt = "You are a looper integration test agent."

[capabilities]
tools = []
memory_read = ["*"]
memory_write = ["self.*"]
"#;

async fn start_looper_test_server(mock_responses: Vec<String>) -> TestServer {
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
        toml::from_str(TEST_MANIFEST).expect("looper integration manifest should deserialize");
    kernel
        .spawn_agent(manifest)
        .expect("looper integration test agent should spawn");

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
            .expect("looper test server should stay available");
    });

    TestServer {
        base_url: format!("http://{address}"),
        state,
        _tmp: tmp,
        _mock: mock,
    }
}

fn sample_task(id: &str) -> TaskRecord {
    TaskRecord {
        task_id: TaskId::new(id),
        slug: format!("{id}-slug"),
        title: format!("Task {id}"),
        description: "Looper API integration task".to_string(),
        status: openfang_types::task::TaskStatus::Planned,
        priority: openfang_types::task::Priority::High,
        complexity: Complexity::Medium,
        position: 1,
        source: TaskSource::Manual,
        owner: OwnerRef {
            kind: ActorKind::AgentGroup,
            ref_id: "sdlc".to_string(),
        },
        created_by: OwnerRef {
            kind: ActorKind::Agent,
            ref_id: "planner".to_string(),
        },
        repository_refs: vec![],
        label_refs: vec![],
        artifact_refs: vec![],
        doc_refs: vec![],
        file_refs: vec![],
        metadata: json!({}),
        created_at: "2026-03-25T12:00:00Z".to_string(),
        updated_at: "2026-03-25T12:00:00Z".to_string(),
        completed_at: None,
    }
}

fn sample_subtask(
    task_id: &TaskId,
    id: &str,
    position: i64,
    depends_on: Vec<SubtaskId>,
    parallelizable: bool,
) -> SubtaskRecord {
    SubtaskRecord {
        subtask_id: SubtaskId::new(id),
        task_id: task_id.clone(),
        title: format!("Subtask {id}"),
        description: "Looper API integration subtask".to_string(),
        kind: SubtaskKind::DocChange,
        status: SubtaskStatus::Planned,
        complexity: Complexity::Medium,
        position,
        assignee: Some(AssigneeRef {
            kind: ActorKind::Agent,
            ref_id: "looper-test-agent".to_string(),
        }),
        depends_on,
        parallelizable,
        input: json!({ "topic": id }),
        result: None,
        metadata: json!({}),
        created_at: "2026-03-25T12:00:00Z".to_string(),
        updated_at: "2026-03-25T12:00:00Z".to_string(),
        completed_at: None,
    }
}

fn looper_policy(mode: LooperExecutionMode, max_parallelism: u32) -> Value {
    json!({
        "mode": mode.as_str(),
        "max_parallelism": max_parallelism,
        "selection": "priority",
    })
}

fn seed_task(server: &TestServer, task_id: &str) -> TaskRecord {
    let task = sample_task(task_id);
    server
        .state
        .kernel
        .workflow_stores
        .task
        .create(&task)
        .expect("task should persist");
    task
}

fn seed_subtask(server: &TestServer, subtask: &SubtaskRecord) {
    server
        .state
        .kernel
        .workflow_stores
        .subtask
        .create(subtask)
        .expect("subtask should persist");
}

fn seed_looper_run(
    server: &TestServer,
    task_id: &TaskId,
    looper_run_id: &str,
    status: LooperRunStatus,
) {
    server
        .state
        .kernel
        .workflow_stores
        .looper_run
        .create(&NewLooperRun {
            looper_run_id: LooperRunId::new(looper_run_id),
            task_id: task_id.clone(),
            source_run_id: Some("run_seeded".to_string()),
            status,
            execution_policy_json: serde_json::to_value(LooperExecutionPolicy {
                mode: LooperExecutionMode::Parallel,
                max_parallelism: 4,
                selection: LooperSelectionStrategy::Priority,
            })
            .expect("looper policy should encode"),
            current_subtask_id: None,
            progress_json: json!({
                "total": 1,
                "completed": 0,
                "failed": 0,
            }),
            error_json: None,
            started_at: "2026-03-25T12:00:00Z".to_string(),
            updated_at: "2026-03-25T12:00:00Z".to_string(),
            completed_at: None,
        })
        .expect("looper run should persist");
}

async fn start_looper_run(
    client: &reqwest::Client,
    server: &TestServer,
    task_id: &TaskId,
    execution_mode: LooperExecutionMode,
    max_parallelism: u32,
) -> Value {
    let response = client
        .post(format!("{}/api/v1/looper-runs", server.base_url))
        .json(&json!({
            "task_id": task_id,
            "subtask_ids": Value::Null,
            "execution_policy": looper_policy(execution_mode, max_parallelism),
            "metadata": { "source": "integration-test" },
        }))
        .send()
        .await
        .expect("looper create request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::ACCEPTED);
    response
        .json::<Value>()
        .await
        .expect("looper create response should deserialize")
}

async fn get_looper_run(
    client: &reqwest::Client,
    server: &TestServer,
    looper_run_id: &str,
) -> Value {
    let response = client
        .get(format!(
            "{}/api/v1/looper-runs/{looper_run_id}",
            server.base_url
        ))
        .send()
        .await
        .expect("looper get request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    response
        .json::<Value>()
        .await
        .expect("looper get response should deserialize")
}

async fn wait_for_looper_status(
    client: &reqwest::Client,
    server: &TestServer,
    looper_run_id: &str,
    expected: &str,
    timeout_duration: Duration,
) -> Value {
    let deadline = tokio::time::Instant::now() + timeout_duration;
    loop {
        let detail = get_looper_run(client, server, looper_run_id).await;
        if detail["status"] == json!(expected) {
            return detail;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for looper run {looper_run_id} to reach status {expected}; last detail: {detail}; looper_subtasks: {}; dispatches: {}; mock_requests: {:?}",
            {
                let records = server
                    .state
                    .kernel
                    .list_looper_subtasks(&LooperRunId::new(looper_run_id))
                    .expect("looper subtask list should load");
                serde_json::to_string(&records).expect("looper subtask diagnostics should encode")
            },
            {
                let mut dispatches = Vec::new();
                for record in server
                    .state
                    .kernel
                    .list_looper_subtasks(&LooperRunId::new(looper_run_id))
                    .expect("looper subtask list should load")
                {
                    let Some(dispatch_id) = record.dispatch_id.as_deref() else {
                        continue;
                    };
                    dispatches.push(
                        server
                            .state
                            .kernel
                            .workflow_stores
                            .dispatch
                            .find_by_id(dispatch_id)
                            .await
                            .expect("dispatch diagnostics should load"),
                    );
                }
                serde_json::to_string(&dispatches).expect("dispatch diagnostics should encode")
            },
            server._mock.requests.lock().await.clone(),
        );
        sleep(Duration::from_millis(100)).await;
    }
}

async fn open_looper_event_stream(
    client: &reqwest::Client,
    server: &TestServer,
    looper_run_id: &str,
    last_event_id: Option<u64>,
) -> reqwest::Response {
    let mut request = client
        .get(format!(
            "{}/api/v1/looper-runs/{looper_run_id}/events",
            server.base_url
        ))
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
                .expect("SSE stream should remain open");
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
    ParsedSseEvent {
        id: id.expect("SSE event should include an id"),
        event: event.expect("SSE event should include an event name"),
        data: serde_json::from_str(&data_text).expect("SSE data should be valid JSON"),
    }
}

fn looper_run_resource(server: &TestServer, looper_run_id: &str) -> LooperRunResource {
    server
        .state
        .kernel
        .get_looper_run(&LooperRunId::new(looper_run_id))
        .expect("looper run lookup should succeed")
        .map(|record| LooperRunResource::from(&record))
        .expect("looper run should exist")
}

#[tokio::test]
async fn looper_full_api_round_trip_should_complete_all_subtasks() {
    let server =
        start_looper_test_server(vec!["first done".to_string(), "second done".to_string()]).await;
    let client = reqwest::Client::new();
    let task = seed_task(&server, "task_looper_round_trip");
    seed_subtask(
        &server,
        &sample_subtask(&task.task_id, "subtask_001", 1, vec![], false),
    );
    seed_subtask(
        &server,
        &sample_subtask(
            &task.task_id,
            "subtask_002",
            2,
            vec![SubtaskId::new("subtask_001")],
            false,
        ),
    );

    let created = start_looper_run(
        &client,
        &server,
        &task.task_id,
        LooperExecutionMode::Sequential,
        1,
    )
    .await;
    let looper_run_id = created["looper_run_id"]
        .as_str()
        .expect("looper_run_id should be present");

    let detail = wait_for_looper_status(
        &client,
        &server,
        looper_run_id,
        "completed",
        Duration::from_secs(45),
    )
    .await;

    assert_eq!(detail["progress"]["total"], json!(2));
    assert_eq!(detail["progress"]["completed"], json!(2));
    assert_eq!(detail["progress"]["failed"], json!(0));
}

#[tokio::test]
async fn looper_sse_should_emit_snapshot_first_and_buffer_subtask_events() {
    let server = start_looper_test_server(vec!["single done".to_string()]).await;
    let client = reqwest::Client::new();
    let task = seed_task(&server, "task_looper_sse");
    seed_subtask(
        &server,
        &sample_subtask(&task.task_id, "subtask_001", 1, vec![], false),
    );

    let created = start_looper_run(
        &client,
        &server,
        &task.task_id,
        LooperExecutionMode::Sequential,
        1,
    )
    .await;
    let looper_run_id = created["looper_run_id"]
        .as_str()
        .expect("looper_run_id should be present")
        .to_string();

    wait_for_looper_status(
        &client,
        &server,
        &looper_run_id,
        "completed",
        Duration::from_secs(30),
    )
    .await;

    let mut fresh_response = open_looper_event_stream(&client, &server, &looper_run_id, None).await;
    let mut fresh_buffer = String::new();
    let snapshot = read_sse_event(
        &mut fresh_response,
        &mut fresh_buffer,
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(snapshot.event, "stream.snapshot");
    assert_eq!(snapshot.data["id"], json!(looper_run_id));

    let mut replay_response =
        open_looper_event_stream(&client, &server, &looper_run_id, Some(0)).await;
    let mut replay_buffer = String::new();
    let mut saw_started = false;
    let mut saw_completed = false;
    for _ in 0..10 {
        let event = read_sse_event(
            &mut replay_response,
            &mut replay_buffer,
            Duration::from_secs(5),
        )
        .await;
        if event.event == "subtask.started" {
            saw_started = true;
        }
        if event.event == "subtask.completed" {
            saw_completed = true;
        }
        if saw_started && saw_completed {
            break;
        }
    }

    assert!(saw_started, "expected at least one subtask.started event");
    assert!(
        saw_completed,
        "expected at least one subtask.completed event"
    );
}

#[tokio::test]
async fn looper_sse_should_replay_events_within_ring_buffer() {
    let server = start_looper_test_server(vec![]).await;
    let client = reqwest::Client::new();
    let task = seed_task(&server, "task_looper_replay");
    seed_looper_run(
        &server,
        &task.task_id,
        "loop_replay",
        LooperRunStatus::Paused,
    );

    let mut first_response = open_looper_event_stream(&client, &server, "loop_replay", None).await;
    let mut first_buffer = String::new();
    let snapshot = read_sse_event(
        &mut first_response,
        &mut first_buffer,
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(snapshot.event, "stream.snapshot");

    let handle = server
        .state
        .looper_runtime_registry
        .handle(&LooperRunId::new("loop_replay"));
    let base_resource = looper_run_resource(&server, "loop_replay");
    let mut published_ids = Vec::new();
    for index in 1..=10 {
        let mut payload = base_resource.clone();
        payload.updated_at = format!("2026-03-25T12:00:{index:02}Z");
        let event = handle.publish(
            "run.updated",
            serde_json::to_value(payload).expect("looper resource should encode"),
        );
        published_ids.push(event.id);
    }

    let mut received_ids = Vec::new();
    for _ in 0..10 {
        let event = read_sse_event(
            &mut first_response,
            &mut first_buffer,
            Duration::from_secs(5),
        )
        .await;
        received_ids.push(event.id);
    }
    assert_eq!(received_ids, published_ids);
    drop(first_response);

    let resume_from = published_ids[4];
    let mut replay_response =
        open_looper_event_stream(&client, &server, "loop_replay", Some(resume_from)).await;
    let mut replay_buffer = String::new();
    let mut replayed_ids = Vec::new();
    for _ in 0..5 {
        let event = read_sse_event(
            &mut replay_response,
            &mut replay_buffer,
            Duration::from_secs(5),
        )
        .await;
        replayed_ids.push(event.id);
    }
    assert_eq!(replayed_ids, published_ids[5..].to_vec());

    let mut payload = base_resource.clone();
    payload.updated_at = "2026-03-25T12:00:11Z".to_string();
    let live_event = handle.publish(
        "run.updated",
        serde_json::to_value(payload).expect("looper resource should encode"),
    );
    let event = read_sse_event(
        &mut replay_response,
        &mut replay_buffer,
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(event.id, live_event.id);
    assert_eq!(event.event, "run.updated");
}

#[tokio::test]
async fn looper_sse_should_reset_when_last_event_id_falls_outside_buffer() {
    let server = start_looper_test_server(vec![]).await;
    let client = reqwest::Client::new();
    let task = seed_task(&server, "task_looper_reset");
    seed_looper_run(
        &server,
        &task.task_id,
        "loop_reset",
        LooperRunStatus::Running,
    );

    let handle = server
        .state
        .looper_runtime_registry
        .handle(&LooperRunId::new("loop_reset"));
    let base_resource = looper_run_resource(&server, "loop_reset");
    let mut published_ids = Vec::new();
    for index in 1..=60 {
        let mut payload = base_resource.clone();
        payload.updated_at = format!("2026-03-25T12:01:{:02}Z", index % 60);
        let event = handle.publish(
            "run.updated",
            serde_json::to_value(payload).expect("looper resource should encode"),
        );
        published_ids.push(event.id);
    }

    let mut response =
        open_looper_event_stream(&client, &server, "loop_reset", Some(published_ids[0])).await;
    let mut buffer = String::new();

    let reset = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
    assert_eq!(reset.event, "stream.reset");
    let snapshot = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
    assert_eq!(snapshot.event, "stream.snapshot");
    assert_eq!(snapshot.data["id"], json!("loop_reset"));

    let mut payload = base_resource.clone();
    payload.updated_at = "2026-03-25T12:02:01Z".to_string();
    let live_event = handle.publish(
        "run.updated",
        serde_json::to_value(payload).expect("looper resource should encode"),
    );
    let event = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
    assert_eq!(event.event, "run.updated");
    assert_eq!(event.id, live_event.id);
}

#[tokio::test]
async fn looper_sse_should_emit_keepalive_for_idle_paused_runs() {
    let server = start_looper_test_server(vec![]).await;
    let client = reqwest::Client::new();
    let task = seed_task(&server, "task_looper_keepalive");
    seed_looper_run(
        &server,
        &task.task_id,
        "loop_keepalive",
        LooperRunStatus::Paused,
    );

    let mut response = open_looper_event_stream(&client, &server, "loop_keepalive", None).await;
    let mut buffer = String::new();

    let snapshot = read_sse_event(&mut response, &mut buffer, Duration::from_secs(5)).await;
    assert_eq!(snapshot.event, "stream.snapshot");

    let keepalive = read_sse_event(&mut response, &mut buffer, Duration::from_secs(20)).await;
    assert_eq!(keepalive.event, "keepalive");
    assert_eq!(keepalive.data, json!({}));
}

#[tokio::test]
async fn looper_pause_then_resume_round_trip_should_update_statuses() {
    let server = start_looper_test_server(vec![
        "DELAY_MS=1500::first delayed".to_string(),
        "second done".to_string(),
    ])
    .await;
    let client = reqwest::Client::new();
    let task = seed_task(&server, "task_looper_pause_resume");
    seed_subtask(
        &server,
        &sample_subtask(&task.task_id, "subtask_001", 1, vec![], false),
    );
    seed_subtask(
        &server,
        &sample_subtask(
            &task.task_id,
            "subtask_002",
            2,
            vec![SubtaskId::new("subtask_001")],
            false,
        ),
    );

    let created = start_looper_run(
        &client,
        &server,
        &task.task_id,
        LooperExecutionMode::Sequential,
        1,
    )
    .await;
    let looper_run_id = created["looper_run_id"]
        .as_str()
        .expect("looper_run_id should be present");

    wait_for_looper_status(
        &client,
        &server,
        looper_run_id,
        "running",
        Duration::from_secs(5),
    )
    .await;

    let pause = client
        .post(format!(
            "{}/api/v1/looper-runs/{looper_run_id}/pause",
            server.base_url
        ))
        .send()
        .await
        .expect("pause request should succeed");
    assert_eq!(pause.status(), reqwest::StatusCode::ACCEPTED);
    let pause_body = pause
        .json::<Value>()
        .await
        .expect("pause response should deserialize");
    assert_eq!(pause_body["accepted"], json!(true));

    let paused = wait_for_looper_status(
        &client,
        &server,
        looper_run_id,
        "paused",
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(paused["status"], json!("paused"));

    let resume = client
        .post(format!(
            "{}/api/v1/looper-runs/{looper_run_id}/resume",
            server.base_url
        ))
        .send()
        .await
        .expect("resume request should succeed");
    assert_eq!(resume.status(), reqwest::StatusCode::ACCEPTED);
    let resume_body = resume
        .json::<Value>()
        .await
        .expect("resume response should deserialize");
    assert_eq!(resume_body["accepted"], json!(true));

    let running = wait_for_looper_status(
        &client,
        &server,
        looper_run_id,
        "running",
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(running["status"], json!("running"));

    let completed = wait_for_looper_status(
        &client,
        &server,
        looper_run_id,
        "completed",
        Duration::from_secs(45),
    )
    .await;
    assert_eq!(completed["progress"]["completed"], json!(2));
}
