//! Real HTTP integration tests for the Schedule v1 API surface.

use std::sync::Arc;

use openfang_api::routes::AppState;
use openfang_api::server::build_router;
use openfang_kernel::OpenFangKernel;
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use serde_json::{json, Value};

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

async fn start_schedule_v1_test_server() -> TestServer {
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

fn schedule_agent_definition(id: &str, name: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "version": "1.0.0",
        "description": "Schedule integration test agent",
        "enabled": true,
        "group": "tests",
        "tags": ["tests", "schedules"],
        "provider": {
            "driver": "claude_code",
            "model": "sonnet"
        },
        "prompt": {
            "system": "You help with schedule tests.",
            "instructions": "Reply briefly."
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

fn noop_workflow_definition(id: &str) -> Value {
    json!({
        "id": id,
        "name": format!("Workflow {id}"),
        "version": "1.0.0",
        "description": "Schedule integration test workflow",
        "enabled": true,
        "tags": ["tests", "schedules"],
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

fn workflow_run_schedule_definition(
    agent: &str,
    workflow_id: &str,
    enabled: bool,
    name: &str,
) -> Value {
    json!({
        "agent": agent,
        "name": name,
        "enabled": enabled,
        "schedule": {
            "kind": "cron",
            "expr": "0 2 * * *",
            "tz": "UTC"
        },
        "action": {
            "kind": "workflow_run",
            "workflow_id": workflow_id,
            "input": {
                "scope": "open_prs"
            },
            "timeout_secs": 300
        },
        "delivery": {
            "kind": "none"
        }
    })
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
) -> reqwest::StatusCode {
    client
        .post(format!("{}{path}", server.base_url))
        .send()
        .await
        .expect("request should succeed")
        .status()
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

async fn put_json(
    client: &reqwest::Client,
    server: &TestServer,
    path: &str,
    body: Value,
) -> (reqwest::StatusCode, Value) {
    let response = client
        .put(format!("{}{path}", server.base_url))
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

async fn create_schedule_prerequisites(client: &reqwest::Client, server: &TestServer) {
    let (agent_status, _agent_body) = post_json(
        client,
        server,
        "/api/v1/agents",
        schedule_agent_definition("schedule-writer", "Schedule Writer"),
    )
    .await;
    assert_eq!(agent_status, reqwest::StatusCode::CREATED);

    let (workflow_status, _workflow_body) = post_json(
        client,
        server,
        "/api/v1/workflows",
        noop_workflow_definition("repo-review"),
    )
    .await;
    assert_eq!(workflow_status, reqwest::StatusCode::CREATED);
}

#[tokio::test]
async fn full_schedule_lifecycle_should_use_typed_v1_contract() {
    let server = start_schedule_v1_test_server().await;
    let client = reqwest::Client::new();
    create_schedule_prerequisites(&client, &server).await;

    let definition = workflow_run_schedule_definition(
        "schedule-writer",
        "repo-review",
        false,
        "Nightly Repo Review",
    );
    let (create_status, create_body) =
        post_json(&client, &server, "/api/v1/schedules", definition.clone()).await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);
    let schedule_id = create_body["id"]
        .as_str()
        .expect("schedule ID should be present")
        .to_string();
    assert_eq!(create_body["action"]["kind"], json!("workflow_run"));
    assert!(create_body["runtime_status"].is_object());

    let (validate_status, validate_body) = post_json(
        &client,
        &server,
        "/api/v1/schedules/validate",
        json!({
            "definition": definition,
            "strict": true
        }),
    )
    .await;
    assert_eq!(validate_status, reqwest::StatusCode::OK);
    assert_eq!(validate_body["valid"], json!(true));

    let enable_status = post_empty(
        &client,
        &server,
        &format!("/api/v1/schedules/{schedule_id}/enable"),
    )
    .await;
    assert_eq!(enable_status, reqwest::StatusCode::ACCEPTED);

    let (run_now_status, run_now_body) = post_json(
        &client,
        &server,
        &format!("/api/v1/schedules/{schedule_id}/run-now"),
        json!({
            "metadata": {
                "source": "integration-tests"
            }
        }),
    )
    .await;
    assert_eq!(run_now_status, reqwest::StatusCode::ACCEPTED);
    assert_eq!(run_now_body["accepted"], json!(true));
    assert_eq!(run_now_body["resource_id"], json!(schedule_id.clone()));
    assert!(run_now_body["run_id"].is_string());

    let disable_status = post_empty(
        &client,
        &server,
        &format!("/api/v1/schedules/{schedule_id}/disable"),
    )
    .await;
    assert_eq!(disable_status, reqwest::StatusCode::ACCEPTED);

    let (delete_status, delete_body) = delete_json(
        &client,
        &server,
        &format!("/api/v1/schedules/{schedule_id}"),
    )
    .await;
    assert_eq!(delete_status, reqwest::StatusCode::NO_CONTENT);
    assert!(delete_body.is_null());
}

#[tokio::test]
async fn create_schedule_should_persist_runtime_projection_and_runtime_endpoint_shape() {
    let server = start_schedule_v1_test_server().await;
    let client = reqwest::Client::new();
    create_schedule_prerequisites(&client, &server).await;

    let (create_status, create_body) = post_json(
        &client,
        &server,
        "/api/v1/schedules",
        workflow_run_schedule_definition(
            "schedule-writer",
            "repo-review",
            true,
            "Persisted Schedule",
        ),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);
    let schedule_id = create_body["id"]
        .as_str()
        .expect("schedule ID should be present")
        .to_string();

    let runtime_projection = server
        .state
        .kernel
        .runtime_stores
        .schedule_runtime
        .get_schedule_runtime(&schedule_id)
        .expect("runtime projection should load")
        .expect("runtime projection should exist");
    assert_eq!(runtime_projection.schedule_id, schedule_id);
    assert!(runtime_projection.enabled);

    let (get_status, get_body) = get_json(
        &client,
        &server,
        &format!("/api/v1/schedules/{schedule_id}"),
    )
    .await;
    assert_eq!(get_status, reqwest::StatusCode::OK);
    assert_eq!(get_body["id"], json!(schedule_id));
    assert!(get_body["runtime_status"].is_object());

    let (update_status, update_body) = put_json(
        &client,
        &server,
        &format!("/api/v1/schedules/{schedule_id}"),
        workflow_run_schedule_definition(
            "schedule-writer",
            "repo-review",
            true,
            "Updated Persisted Schedule",
        ),
    )
    .await;
    assert_eq!(update_status, reqwest::StatusCode::OK);
    assert_eq!(update_body["name"], json!("Updated Persisted Schedule"));

    let (runtime_status, runtime_body) = get_json(
        &client,
        &server,
        &format!("/api/v1/schedules/{schedule_id}/runtime"),
    )
    .await;
    assert_eq!(runtime_status, reqwest::StatusCode::OK);
    assert_eq!(runtime_body["schedule_id"], get_body["id"]);
    assert_eq!(runtime_body["enabled"], json!(true));
    assert!(runtime_body["one_shot"].is_boolean());
}

#[tokio::test]
async fn list_schedules_should_paginate_items_and_next_cursor() {
    let server = start_schedule_v1_test_server().await;
    let client = reqwest::Client::new();
    create_schedule_prerequisites(&client, &server).await;

    for index in 0..4 {
        let (status, _body) = post_json(
            &client,
            &server,
            "/api/v1/schedules",
            workflow_run_schedule_definition(
                "schedule-writer",
                "repo-review",
                true,
                &format!("Schedule {index}"),
            ),
        )
        .await;
        assert_eq!(status, reqwest::StatusCode::CREATED);
    }

    let (first_status, first_body) = get_json(&client, &server, "/api/v1/schedules?limit=2").await;
    assert_eq!(first_status, reqwest::StatusCode::OK);
    assert_eq!(first_body["items"].as_array().map(Vec::len), Some(2));
    let next_cursor = first_body["next_cursor"]
        .as_str()
        .expect("first page should return a cursor")
        .to_string();

    let (second_status, second_body) = get_json(
        &client,
        &server,
        &format!("/api/v1/schedules?limit=2&cursor={next_cursor}"),
    )
    .await;
    assert_eq!(second_status, reqwest::StatusCode::OK);
    assert_eq!(second_body["items"].as_array().map(Vec::len), Some(2));
    let total = first_body["items"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default()
        + second_body["items"]
            .as_array()
            .map(Vec::len)
            .unwrap_or_default();
    assert_eq!(total, 4);
}

#[tokio::test]
async fn disabled_schedule_should_clear_active_queue_and_run_now_should_still_succeed() {
    let server = start_schedule_v1_test_server().await;
    let client = reqwest::Client::new();
    create_schedule_prerequisites(&client, &server).await;

    let (create_status, create_body) = post_json(
        &client,
        &server,
        "/api/v1/schedules",
        workflow_run_schedule_definition(
            "schedule-writer",
            "repo-review",
            false,
            "Disabled Schedule",
        ),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);
    let schedule_id = create_body["id"]
        .as_str()
        .expect("schedule ID should be present")
        .to_string();

    let meta = server
        .state
        .kernel
        .cron_scheduler
        .get_meta_by_definition_id(&schedule_id)
        .expect("schedule should be tracked");
    assert!(!meta.job.enabled);
    assert!(meta.job.next_run.is_none());

    let (run_status, run_body) = post_json(
        &client,
        &server,
        &format!("/api/v1/schedules/{schedule_id}/run-now"),
        json!({
            "metadata": {
                "source": "integration-tests"
            }
        }),
    )
    .await;
    assert_eq!(run_status, reqwest::StatusCode::ACCEPTED);
    assert_eq!(run_body["accepted"], json!(true));
    assert!(run_body["run_id"].is_string());
}

#[tokio::test]
async fn schedule_run_now_dry_run_should_return_resolved_action_without_side_effects() {
    let server = start_schedule_v1_test_server().await;
    let client = reqwest::Client::new();
    create_schedule_prerequisites(&client, &server).await;

    let (create_status, create_body) = post_json(
        &client,
        &server,
        "/api/v1/schedules",
        workflow_run_schedule_definition(
            "schedule-writer",
            "repo-review",
            true,
            "Dry Run Schedule",
        ),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);
    let schedule_id = create_body["id"]
        .as_str()
        .expect("schedule ID should be present")
        .to_string();

    let before_runs = server
        .state
        .kernel
        .workflow_stores
        .workflow_run
        .list_for_workflow("repo-review")
        .expect("workflow runs should load")
        .len();

    let (dry_status, dry_body) = post_json(
        &client,
        &server,
        &format!("/api/v1/schedules/{schedule_id}/run-now/dry-run"),
        json!({
            "metadata": {
                "source": "integration-tests"
            }
        }),
    )
    .await;
    assert_eq!(dry_status, reqwest::StatusCode::OK);
    assert_eq!(dry_body["would_execute"], json!(true));
    assert_eq!(dry_body["resolved"]["schedule_id"], json!(schedule_id));
    assert_eq!(
        dry_body["resolved"]["action"]["kind"],
        json!("workflow_run")
    );

    let after_runs = server
        .state
        .kernel
        .workflow_stores
        .workflow_run
        .list_for_workflow("repo-review")
        .expect("workflow runs should load")
        .len();
    assert_eq!(before_runs, after_runs);
}

#[tokio::test]
async fn schedule_fork_should_return_user_owned_copy_with_provenance() {
    let server = start_schedule_v1_test_server().await;
    let client = reqwest::Client::new();
    create_schedule_prerequisites(&client, &server).await;

    let (create_status, create_body) = post_json(
        &client,
        &server,
        "/api/v1/schedules",
        workflow_run_schedule_definition(
            "schedule-writer",
            "repo-review",
            true,
            "Forkable Schedule",
        ),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);
    let schedule_id = create_body["id"]
        .as_str()
        .expect("schedule ID should be present")
        .to_string();

    let (fork_status, fork_body) = post_json(
        &client,
        &server,
        &format!("/api/v1/schedules/{schedule_id}/fork"),
        json!({
            "mode": "shadow"
        }),
    )
    .await;
    assert_eq!(fork_status, reqwest::StatusCode::OK);
    assert_eq!(fork_body["origin"]["kind"], json!("user"));
    assert_eq!(fork_body["forked_from"]["resource_type"], json!("schedule"));
    assert_eq!(fork_body["forked_from"]["resource_id"], json!(schedule_id));
}

#[tokio::test]
async fn delete_missing_schedule_should_return_stable_error_envelope_and_legacy_route_should_be_absent(
) {
    let server = start_schedule_v1_test_server().await;
    let client = reqwest::Client::new();

    let (delete_status, delete_body) =
        delete_json(&client, &server, "/api/v1/schedules/sched_missing").await;
    assert_eq!(delete_status, reqwest::StatusCode::NOT_FOUND);
    assert_eq!(delete_body["error"]["code"], json!("not_found"));
    assert_eq!(
        delete_body["error"]["message"],
        json!("Schedule definition not found")
    );

    let legacy_status = client
        .get(format!("{}/api/schedules", server.base_url))
        .send()
        .await
        .expect("request should succeed")
        .status();
    assert_eq!(legacy_status, reqwest::StatusCode::NOT_FOUND);
}
