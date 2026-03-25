//! Real HTTP integration tests for the event ingress v1 API surface.

use std::sync::Arc;

use openfang_api::routes::AppState;
use openfang_api::server::build_router;
use openfang_kernel::workflow::WorkflowId;
use openfang_kernel::OpenFangKernel;
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use openfang_types::workflow::WorkflowV2Definition;
use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use tokio::time::{sleep, Duration};

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

async fn start_event_ingress_test_server() -> TestServer {
    let tmp = tempfile::tempdir().expect("temporary directory should be created");
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

    let kernel = Arc::new(OpenFangKernel::boot_with_config(config).expect("kernel should boot"));
    kernel.set_self_handle();
    kernel.bootstrap_workflow_definitions().await;
    *kernel
        .skill_registry
        .write()
        .unwrap_or_else(|error| error.into_inner()) =
        openfang_skills::registry::SkillRegistry::new(tmp.path().join("skills"));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener should bind");
    let address = listener
        .local_addr()
        .expect("bound listener should expose an address");
    let (app, state) = build_router(kernel, address).await;

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("test server should stay available");
    });

    TestServer {
        base_url: format!("http://{address}"),
        state,
        _tmp: tmp,
    }
}

fn noop_workflow_definition(id: &str) -> Value {
    json!({
        "id": id,
        "name": format!("Workflow {id}"),
        "version": "1.0.0",
        "description": "Event ingress workflow",
        "enabled": true,
        "input": {
            "kind": "object",
            "required": [],
            "open": true,
            "fields": {}
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
    })
}

fn workflow_start_trigger(id: &str, workflow_id: &str, enabled: bool) -> Value {
    json!({
        "id": id,
        "name": format!("Trigger {id}"),
        "description": "Starts a workflow from event ingress",
        "enabled": enabled,
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
                "issue_id": "{{ event.payload.issue_id }}"
            }
        }
    })
}

fn workflow_signal_trigger(id: &str, workflow_id: &str) -> Value {
    json!({
        "id": id,
        "name": format!("Trigger {id}"),
        "description": "Signals a waiting workflow from event ingress",
        "enabled": true,
        "max_fires": 0,
        "cooldown_secs": 0,
        "match": {
            "event": "issue.created",
            "source": "api"
        },
        "target": {
            "kind": "workflow_signal",
            "signal": "approval",
            "selector": {
                "workflow_id": workflow_id
            },
            "payload": {
                "approved": true
            }
        }
    })
}

fn agent_definition_value(id: &str, name: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "version": "1.0.0",
        "description": "Receives event ingress messages",
        "enabled": true,
        "group": "tests",
        "tags": ["tests", "events"],
        "provider": {
            "driver": "codex",
            "model": "gpt-4.1",
            "defaults": {
                "max_tokens": 256
            },
            "config": {
                "web_search": false
            }
        },
        "prompt": {
            "system": "You are a concise test assistant.",
            "instructions": "Reply briefly.",
            "skills": ["testing"]
        },
        "capabilities": {
            "tools": [],
            "primitives": [],
            "delegation": [],
            "workspace": "none",
            "network": true
        },
        "runtime": {
            "autonomous": true,
            "memory_policy": "session",
            "hitl": "explicit_only"
        },
        "input": {
            "kind": "object"
        },
        "output": {
            "kind": "any"
        }
    })
}

fn agent_message_trigger(id: &str, agent_id: &str) -> Value {
    json!({
        "id": id,
        "name": format!("Trigger {id}"),
        "description": "Sends an agent message from event ingress",
        "enabled": true,
        "max_fires": 0,
        "cooldown_secs": 0,
        "match": {
            "event": "issue.created",
            "source": "api"
        },
        "target": {
            "kind": "agent_message",
            "agent": agent_id,
            "input": {
                "items": [{
                    "type": "text",
                    "text": "Review the incoming issue."
                }]
            }
        }
    })
}

fn event_request(event: &str) -> Value {
    json!({
        "event": event,
        "source": "api",
        "payload": {
            "issue_id": "ISSUE-123",
            "issue": {
                "id": "ISSUE-123"
            }
        },
        "idempotency_key": "event-test-key",
        "occurred_at": "2026-03-21T14:10:00Z",
        "metadata": {
            "actor": "system"
        }
    })
}

fn wait_signal_definition(workflow_id: WorkflowId) -> WorkflowV2Definition {
    serde_json::from_value(serde_json::json!({
        "id": workflow_id.to_string(),
        "name": "wait-signal-event-ingress-test",
        "version": "1.0.0",
        "description": "Wait signal event ingress test",
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

async fn create_waiting_signal_run(server: &TestServer) -> (String, String) {
    let workflow_id = WorkflowId::new();
    let workflow_id_text = workflow_id.to_string();
    let definition = serde_json::to_value(wait_signal_definition(workflow_id))
        .expect("workflow definition should serialize");
    let client = reqwest::Client::new();

    let (create_status, body) = post_json(&client, server, "/api/v1/workflows", definition).await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);
    assert_eq!(body["id"], json!(workflow_id_text.clone()));

    let (start_status, start_body) = post_json(
        &client,
        server,
        &format!("/api/v1/workflows/{workflow_id_text}/runs"),
        json!({
            "input": {
                "ticket": "ISSUE-123"
            },
            "metadata": {
                "source": "event-ingress-test"
            }
        }),
    )
    .await;
    assert_eq!(start_status, reqwest::StatusCode::ACCEPTED);
    let run_id = start_body["run_id"]
        .as_str()
        .expect("run id should be present")
        .to_string();

    for _ in 0..40 {
        let (_, run_body) = get_json(&client, server, &format!("/api/v1/runs/{run_id}")).await;
        if run_body["status"] == json!("waiting_signal") {
            return (workflow_id_text, run_id);
        }
        sleep(Duration::from_millis(25)).await;
    }

    panic!("run {run_id} did not reach waiting_signal state");
}

async fn post_json(
    client: &reqwest::Client,
    server: &TestServer,
    path: &str,
    body: Value,
) -> (reqwest::StatusCode, Value) {
    let response = client
        .post(format!("{}{path}", server.base_url))
        .json(&body)
        .send()
        .await
        .expect("request should succeed");
    let status = response.status();
    let body = response
        .json()
        .await
        .expect("response body should deserialize");
    (status, body)
}

async fn post_empty(
    client: &reqwest::Client,
    server: &TestServer,
    path: &str,
) -> (reqwest::StatusCode, Value) {
    let response = client
        .post(format!("{}{path}", server.base_url))
        .send()
        .await
        .expect("request should succeed");
    let status = response.status();
    let body = if status == reqwest::StatusCode::NO_CONTENT {
        Value::Null
    } else {
        response
            .json()
            .await
            .expect("response body should deserialize")
    };
    (status, body)
}

async fn get_json(
    client: &reqwest::Client,
    server: &TestServer,
    path: &str,
) -> (reqwest::StatusCode, Value) {
    let response = client
        .get(format!("{}{path}", server.base_url))
        .send()
        .await
        .expect("request should succeed");
    let status = response.status();
    let body = response
        .json()
        .await
        .expect("response body should deserialize");
    (status, body)
}

async fn delete_json(
    client: &reqwest::Client,
    server: &TestServer,
    path: &str,
) -> (reqwest::StatusCode, Value) {
    let response = client
        .delete(format!("{}{path}", server.base_url))
        .send()
        .await
        .expect("request should succeed");
    let status = response.status();
    let body = if status == reqwest::StatusCode::NO_CONTENT {
        Value::Null
    } else {
        response
            .json()
            .await
            .expect("response body should deserialize")
    };
    (status, body)
}

async fn create_workflow(client: &reqwest::Client, server: &TestServer, id: &str) {
    let (status, body) = post_json(
        client,
        server,
        "/api/v1/workflows",
        noop_workflow_definition(id),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::CREATED);
    assert_eq!(body["id"], json!(id));
}

async fn create_agent_definition(client: &reqwest::Client, server: &TestServer, id: &str) {
    let (status, body) = post_json(
        client,
        server,
        "/api/v1/agents",
        agent_definition_value(id, "Event Reviewer"),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::CREATED);
    assert_eq!(body["id"], json!(id));
}

async fn wait_for_workflow_run_count(server: &TestServer, workflow_id: &str, expected: usize) {
    for _ in 0..40 {
        let runs = server
            .state
            .kernel
            .workflow_stores
            .workflow_run
            .list_for_workflow(workflow_id)
            .expect("workflow runs should list");
        if runs.len() == expected {
            return;
        }
        sleep(Duration::from_millis(25)).await;
    }

    panic!("workflow {workflow_id} did not reach run count {expected}");
}

async fn wait_for_signal_count(server: &TestServer, run_id: &str, expected: usize) {
    for _ in 0..40 {
        let signals = server
            .state
            .kernel
            .workflow_stores
            .workflow_signal
            .list_for_run(run_id, None)
            .expect("workflow signals should list");
        if signals.len() == expected {
            return;
        }
        sleep(Duration::from_millis(25)).await;
    }

    panic!("run {run_id} did not reach signal count {expected}");
}

async fn wait_for_agent_session_count(
    client: &reqwest::Client,
    server: &TestServer,
    definition_id: &str,
    expected: usize,
) {
    for _ in 0..40 {
        let (status, body) = get_json(
            client,
            server,
            &format!("/api/v1/agents/{definition_id}/sessions"),
        )
        .await;
        assert_eq!(status, reqwest::StatusCode::OK);
        if body["items"]
            .as_array()
            .expect("items should be an array")
            .len()
            == expected
        {
            return;
        }
        sleep(Duration::from_millis(25)).await;
    }

    panic!("agent {definition_id} did not reach session count {expected}");
}

#[tokio::test]
async fn event_ingress_should_start_workflow_and_record_fire_state() {
    let server = start_event_ingress_test_server().await;
    let client = reqwest::Client::new();
    create_workflow(&client, &server, "event-ingress-workflow").await;

    let (create_status, _) = post_json(
        &client,
        &server,
        "/api/v1/triggers",
        workflow_start_trigger("event-start-trigger", "event-ingress-workflow", true),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/events",
        event_request("issue.created"),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
    assert_eq!(body["accepted"], json!(true));
    assert_eq!(body["status"], json!("accepted"));
    assert_eq!(body["matched_triggers"], json!(["event-start-trigger"]));
    assert_eq!(body["effects"]["workflow_starts"], json!(1));
    assert_eq!(body["effects"]["workflow_signals"], json!(0));
    assert_eq!(body["effects"]["agent_messages"], json!(0));
    assert!(body["failures"].is_null());
    assert!(body["event_id"]
        .as_str()
        .is_some_and(|value| value.starts_with("evt_")));

    wait_for_workflow_run_count(&server, "event-ingress-workflow", 1).await;

    let (runtime_status, runtime_body) = get_json(
        &client,
        &server,
        "/api/v1/triggers/event-start-trigger/runtime",
    )
    .await;
    assert_eq!(runtime_status, reqwest::StatusCode::OK);
    assert_eq!(runtime_body["fire_count"], json!(1));
    assert_eq!(
        runtime_body["last_fired_at"],
        json!("2026-03-21T14:10:00+00:00")
    );
}

#[tokio::test]
async fn disabled_trigger_should_not_fire_without_restart() {
    let server = start_event_ingress_test_server().await;
    let client = reqwest::Client::new();
    create_workflow(&client, &server, "disabled-trigger-workflow").await;

    let (create_status, _) = post_json(
        &client,
        &server,
        "/api/v1/triggers",
        workflow_start_trigger("disabled-trigger", "disabled-trigger-workflow", true),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);

    let (disable_status, _) = post_empty(
        &client,
        &server,
        "/api/v1/triggers/disabled-trigger/disable",
    )
    .await;
    assert_eq!(disable_status, reqwest::StatusCode::ACCEPTED);

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/events",
        event_request("issue.created"),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
    assert_eq!(body["matched_triggers"], json!([]));
    assert_eq!(body["effects"]["workflow_starts"], json!(0));
    assert_eq!(body["effects"]["workflow_signals"], json!(0));
    assert_eq!(body["effects"]["agent_messages"], json!(0));
    assert!(body["failures"].is_null());

    wait_for_workflow_run_count(&server, "disabled-trigger-workflow", 0).await;

    let (runtime_status, runtime_body) = get_json(
        &client,
        &server,
        "/api/v1/triggers/disabled-trigger/runtime",
    )
    .await;
    assert_eq!(runtime_status, reqwest::StatusCode::OK);
    assert_eq!(runtime_body["enabled"], json!(false));
    assert_eq!(runtime_body["fire_count"], json!(0));
}

#[tokio::test]
async fn dry_run_should_report_effects_without_dispatching() {
    let server = start_event_ingress_test_server().await;
    let client = reqwest::Client::new();
    create_workflow(&client, &server, "dry-run-workflow").await;

    let (create_status, _) = post_json(
        &client,
        &server,
        "/api/v1/triggers",
        workflow_start_trigger("dry-run-trigger", "dry-run-workflow", true),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/events/dry-run",
        event_request("issue.created"),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::OK);
    assert_eq!(body["would_execute"], json!(true));
    assert_eq!(body["resolved"]["event"], json!("issue.created"));
    assert_eq!(body["resolved"]["source"], json!("api"));
    assert_eq!(
        body["effects"]["matched_triggers"],
        json!(["dry-run-trigger"])
    );
    assert_eq!(body["effects"]["workflow_starts"], json!(1));
    assert_eq!(body["effects"]["workflow_signals"], json!(0));
    assert_eq!(body["effects"]["agent_messages"], json!(0));
    assert_eq!(
        body["explanation"]["matching_mode"],
        json!("trigger_engine")
    );

    wait_for_workflow_run_count(&server, "dry-run-workflow", 0).await;

    let (runtime_status, runtime_body) =
        get_json(&client, &server, "/api/v1/triggers/dry-run-trigger/runtime").await;
    assert_eq!(runtime_status, reqwest::StatusCode::OK);
    assert_eq!(runtime_body["fire_count"], json!(0));
    assert!(runtime_body["last_fired_at"].is_null());
}

#[tokio::test]
async fn event_ingress_should_dispatch_multiple_matching_targets() {
    let server = start_event_ingress_test_server().await;
    let client = reqwest::Client::new();
    create_workflow(&client, &server, "multi-trigger-start-workflow").await;
    let (signal_workflow_id, signal_run_id) = create_waiting_signal_run(&server).await;

    let (start_status, _) = post_json(
        &client,
        &server,
        "/api/v1/triggers",
        workflow_start_trigger("multi-trigger-start", "multi-trigger-start-workflow", true),
    )
    .await;
    assert_eq!(start_status, reqwest::StatusCode::CREATED);

    let (signal_status, _) = post_json(
        &client,
        &server,
        "/api/v1/triggers",
        workflow_signal_trigger("multi-trigger-signal", &signal_workflow_id),
    )
    .await;
    assert_eq!(signal_status, reqwest::StatusCode::CREATED);

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/events",
        event_request("issue.created"),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
    assert_eq!(
        body["matched_triggers"],
        json!(["multi-trigger-signal", "multi-trigger-start"])
    );
    assert_eq!(body["effects"]["workflow_starts"], json!(1));
    assert_eq!(body["effects"]["workflow_signals"], json!(1));
    assert_eq!(body["effects"]["agent_messages"], json!(0));
    assert!(body["failures"].is_null());

    wait_for_workflow_run_count(&server, "multi-trigger-start-workflow", 1).await;
    wait_for_signal_count(&server, &signal_run_id, 1).await;
}

#[tokio::test]
async fn event_ingress_should_isolate_dispatch_errors() {
    let server = start_event_ingress_test_server().await;
    let client = reqwest::Client::new();
    create_workflow(&client, &server, "missing-target-workflow").await;
    create_workflow(&client, &server, "valid-target-workflow").await;

    let (broken_status, _) = post_json(
        &client,
        &server,
        "/api/v1/triggers",
        workflow_start_trigger("broken-start-trigger", "missing-target-workflow", true),
    )
    .await;
    assert_eq!(broken_status, reqwest::StatusCode::CREATED);

    let (valid_status, _) = post_json(
        &client,
        &server,
        "/api/v1/triggers",
        workflow_start_trigger("valid-start-trigger", "valid-target-workflow", true),
    )
    .await;
    assert_eq!(valid_status, reqwest::StatusCode::CREATED);

    let (delete_status, delete_body) = delete_json(
        &client,
        &server,
        "/api/v1/workflows/missing-target-workflow",
    )
    .await;
    assert_eq!(delete_status, reqwest::StatusCode::NO_CONTENT);
    assert_eq!(delete_body, Value::Null);

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/events",
        event_request("issue.created"),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
    assert_eq!(
        body["matched_triggers"],
        json!(["broken-start-trigger", "valid-start-trigger"])
    );
    assert_eq!(body["effects"]["workflow_starts"], json!(1));
    assert_eq!(body["effects"]["workflow_signals"], json!(0));
    assert_eq!(body["effects"]["agent_messages"], json!(0));
    assert_eq!(
        body["failures"][0]["trigger_id"],
        json!("broken-start-trigger")
    );
    assert_eq!(body["failures"][0]["target_kind"], json!("workflow_start"));
    assert_eq!(
        body["failures"][0]["message"],
        json!("workflow definition not found: missing-target-workflow")
    );

    wait_for_workflow_run_count(&server, "valid-target-workflow", 1).await;

    let (runtime_status, runtime_body) = get_json(
        &client,
        &server,
        "/api/v1/triggers/valid-start-trigger/runtime",
    )
    .await;
    assert_eq!(runtime_status, reqwest::StatusCode::OK);
    assert_eq!(runtime_body["fire_count"], json!(1));

    let (broken_runtime_status, broken_runtime_body) = get_json(
        &client,
        &server,
        "/api/v1/triggers/broken-start-trigger/runtime",
    )
    .await;
    assert_eq!(broken_runtime_status, reqwest::StatusCode::OK);
    assert_eq!(broken_runtime_body["fire_count"], json!(0));
}

#[tokio::test]
async fn event_ingress_should_dispatch_agent_messages() {
    let server = start_event_ingress_test_server().await;
    let client = reqwest::Client::new();
    create_agent_definition(&client, &server, "event-reviewer").await;

    let (trigger_status, _) = post_json(
        &client,
        &server,
        "/api/v1/triggers",
        agent_message_trigger("agent-message-trigger", "event-reviewer"),
    )
    .await;
    assert_eq!(trigger_status, reqwest::StatusCode::CREATED);

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/events",
        event_request("issue.created"),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
    assert_eq!(body["matched_triggers"], json!(["agent-message-trigger"]));
    assert_eq!(body["effects"]["workflow_starts"], json!(0));
    assert_eq!(body["effects"]["workflow_signals"], json!(0));
    assert_eq!(body["effects"]["agent_messages"], json!(1));

    wait_for_agent_session_count(&client, &server, "event-reviewer", 1).await;

    let (runtime_status, runtime_body) = get_json(
        &client,
        &server,
        "/api/v1/triggers/agent-message-trigger/runtime",
    )
    .await;
    assert_eq!(runtime_status, reqwest::StatusCode::OK);
    assert_eq!(runtime_body["fire_count"], json!(1));
}
