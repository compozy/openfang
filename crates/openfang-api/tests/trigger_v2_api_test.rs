//! Real HTTP integration tests for the Trigger v2 API surface.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use openfang_api::routes::AppState;
use openfang_api::server::build_router;
use openfang_api::types::{
    PackManifest, PackObjectRef, PackResourceType, PackSource, PackSourceKind,
};
use openfang_kernel::OpenFangKernel;
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use openfang_types::trigger::TriggerV2Definition;
use openfang_types::workflow::WorkflowV2Definition;
use serde::Serialize;
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

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent directory should exist");
    }
    fs::write(path, content).expect("fixture file should be written");
}

fn write_toml_file<T: Serialize>(path: &Path, value: &T) {
    let payload = toml::to_string_pretty(value).expect("fixture should serialize to TOML");
    write_file(path, &payload);
}

fn pack_workflow_definition(id: &str) -> WorkflowV2Definition {
    serde_json::from_value(noop_workflow_definition(id))
        .expect("workflow fixture should deserialize")
}

fn pack_trigger_definition(id: &str, workflow_id: &str) -> TriggerV2Definition {
    serde_json::from_value(workflow_start_trigger(
        id,
        workflow_id,
        true,
        "issue.created",
    ))
    .expect("trigger fixture should deserialize")
}

fn seed_pack_trigger_fixture(home_dir: &Path) {
    let manifest = PackManifest {
        id: "analytics".to_string(),
        name: "Analytics".to_string(),
        version: "1.2.0".to_string(),
        description: "Analytics pack fixture".to_string(),
        source: PackSource {
            kind: PackSourceKind::Bundled,
        },
        objects: vec![
            PackObjectRef {
                resource_type: PackResourceType::Workflow,
                resource_id: "analytics-main".to_string(),
            },
            PackObjectRef {
                resource_type: PackResourceType::Trigger,
                resource_id: "analytics-trigger".to_string(),
            },
        ],
    };
    write_toml_file(&home_dir.join("packs/analytics/pack.toml"), &manifest);
    write_toml_file(
        &home_dir.join("packs/analytics/workflows/analytics-main.toml"),
        &pack_workflow_definition("analytics-main"),
    );
    write_toml_file(
        &home_dir.join("packs/analytics/triggers/analytics-trigger.toml"),
        &pack_trigger_definition("analytics-trigger", "analytics-main"),
    );
}

async fn start_trigger_v2_test_server_with_fixtures(seed_pack_fixture: bool) -> TestServer {
    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    if seed_pack_fixture {
        seed_pack_trigger_fixture(tmp.path());
    }
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

async fn start_trigger_v2_test_server() -> TestServer {
    start_trigger_v2_test_server_with_fixtures(false).await
}

fn noop_workflow_definition(id: &str) -> Value {
    json!({
        "id": id,
        "name": format!("Workflow {id}"),
        "version": "1.0.0",
        "description": "Trigger integration workflow",
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

fn workflow_start_trigger(id: &str, workflow_id: &str, enabled: bool, event: &str) -> Value {
    json!({
        "id": id,
        "name": format!("Trigger {id}"),
        "description": "Trigger integration definition",
        "enabled": enabled,
        "max_fires": 5,
        "cooldown_secs": 30,
        "match": {
            "event": event,
            "source": "api"
        },
        "target": {
            "kind": "workflow_start",
            "workflow": workflow_id,
            "input": {
                "scope": "tests"
            }
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

#[tokio::test]
async fn trigger_crud_round_trip_should_persist_and_delete() {
    let server = start_trigger_v2_test_server().await;
    let client = reqwest::Client::new();
    create_workflow(&client, &server, "trigger-crud-workflow").await;

    let (create_status, create_body) = post_json(
        &client,
        &server,
        "/api/v1/triggers",
        workflow_start_trigger(
            "issue-created-start-sdlc",
            "trigger-crud-workflow",
            true,
            "issue.created",
        ),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);
    assert_eq!(create_body["id"], json!("issue-created-start-sdlc"));
    assert_eq!(create_body["max_fires"], json!(5));

    let (get_status, get_body) = get_json(
        &client,
        &server,
        "/api/v1/triggers/issue-created-start-sdlc",
    )
    .await;
    assert_eq!(get_status, reqwest::StatusCode::OK);
    assert_eq!(get_body["id"], json!("issue-created-start-sdlc"));
    assert_eq!(get_body["target"]["kind"], json!("workflow_start"));

    let mut updated = workflow_start_trigger(
        "issue-created-start-sdlc",
        "trigger-crud-workflow",
        true,
        "issue.created",
    );
    updated["max_fires"] = json!(8);

    let (update_status, update_body) = put_json(
        &client,
        &server,
        "/api/v1/triggers/issue-created-start-sdlc",
        updated,
    )
    .await;
    assert_eq!(update_status, reqwest::StatusCode::OK);
    assert_eq!(update_body["max_fires"], json!(8));

    let (runtime_status, runtime_body) = get_json(
        &client,
        &server,
        "/api/v1/triggers/issue-created-start-sdlc/runtime",
    )
    .await;
    assert_eq!(runtime_status, reqwest::StatusCode::OK);
    assert_eq!(runtime_body["max_fires"], json!(8));

    let (delete_status, delete_body) = delete_json(
        &client,
        &server,
        "/api/v1/triggers/issue-created-start-sdlc",
    )
    .await;
    assert_eq!(delete_status, reqwest::StatusCode::NO_CONTENT);
    assert_eq!(delete_body, Value::Null);

    let (missing_status, missing_body) = get_json(
        &client,
        &server,
        "/api/v1/triggers/issue-created-start-sdlc",
    )
    .await;
    assert_eq!(missing_status, reqwest::StatusCode::NOT_FOUND);
    assert_eq!(missing_body["error"]["code"], json!("not_found"));
    assert_eq!(
        missing_body["error"]["message"],
        json!("Trigger definition not found")
    );
}

#[tokio::test]
async fn trigger_test_should_match_without_dispatching() {
    let server = start_trigger_v2_test_server().await;
    let client = reqwest::Client::new();
    create_workflow(&client, &server, "trigger-test-workflow").await;

    let (create_status, _) = post_json(
        &client,
        &server,
        "/api/v1/triggers",
        workflow_start_trigger(
            "trigger-test-match",
            "trigger-test-workflow",
            true,
            "issue.created",
        ),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);

    let before_runs = server
        .state
        .kernel
        .workflow_stores
        .workflow_run
        .list_for_workflow("trigger-test-workflow")
        .expect("workflow runs should list");
    assert!(before_runs.is_empty());

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/triggers/trigger-test-match/test",
        json!({
            "event": {
                "event": "issue.created",
                "source": "api",
                "payload": {
                    "issue": {
                        "id": 42
                    }
                }
            }
        }),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::OK);
    assert_eq!(body["matched"], json!(true));
    assert_eq!(body["would_dispatch"], json!(true));
    assert_eq!(body["resolved_target"]["kind"], json!("workflow_start"));

    let after_runs = server
        .state
        .kernel
        .workflow_stores
        .workflow_run
        .list_for_workflow("trigger-test-workflow")
        .expect("workflow runs should list");
    assert!(
        after_runs.is_empty(),
        "dry-run trigger test must not create workflow runs"
    );
}

#[tokio::test]
async fn trigger_test_should_report_non_matching_event() {
    let server = start_trigger_v2_test_server().await;
    let client = reqwest::Client::new();
    create_workflow(&client, &server, "trigger-test-nomatch-workflow").await;

    let (create_status, _) = post_json(
        &client,
        &server,
        "/api/v1/triggers",
        workflow_start_trigger(
            "trigger-test-no-match",
            "trigger-test-nomatch-workflow",
            true,
            "issue.created",
        ),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/triggers/trigger-test-no-match/test",
        json!({
            "event": {
                "event": "issue.closed",
                "source": "api",
                "payload": {
                    "issue": {
                        "id": 42
                    }
                }
            }
        }),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::OK);
    assert_eq!(body["matched"], json!(false));
    assert_eq!(body["would_dispatch"], json!(false));
    assert!(body["resolved_target"].is_null());
}

#[tokio::test]
async fn trigger_fork_should_shadow_managed_pack_trigger() {
    let server = start_trigger_v2_test_server_with_fixtures(true).await;
    let client = reqwest::Client::new();

    assert!(server.state.pack_registry.get_pack("analytics").is_some());
    assert!(server
        .state
        .pack_registry
        .find_pack_for_object(PackResourceType::Trigger, "analytics-trigger")
        .is_some());

    let (fork_status, fork_body) = post_json(
        &client,
        &server,
        "/api/v1/triggers/analytics-trigger/fork",
        json!({ "mode": "shadow" }),
    )
    .await;

    assert_eq!(fork_status, reqwest::StatusCode::OK);
    assert_eq!(fork_body["id"], json!("analytics-trigger"));
    assert_eq!(fork_body["origin"]["kind"], json!("user"));
    assert_eq!(fork_body["forked_from"]["kind"], json!("pack"));
    assert_eq!(fork_body["forked_from"]["pack_id"], json!("analytics"));

    let (detail_status, detail_body) =
        get_json(&client, &server, "/api/v1/triggers/analytics-trigger").await;

    assert_eq!(detail_status, reqwest::StatusCode::OK);
    assert_eq!(detail_body["origin"]["kind"], json!("user"));
    assert_eq!(detail_body["forked_from"]["kind"], json!("pack"));
    assert_eq!(
        detail_body["forked_from"]["resource_id"],
        json!("analytics-trigger")
    );
}

#[tokio::test]
async fn trigger_list_should_include_origin_for_user_and_pack_definitions() {
    let server = start_trigger_v2_test_server_with_fixtures(true).await;
    let client = reqwest::Client::new();

    create_workflow(&client, &server, "trigger-origin-workflow").await;

    let (create_status, _) = post_json(
        &client,
        &server,
        "/api/v1/triggers",
        workflow_start_trigger(
            "trigger-origin-user",
            "trigger-origin-workflow",
            true,
            "issue.created",
        ),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);

    let (list_status, list_body) = get_json(&client, &server, "/api/v1/triggers").await;
    assert_eq!(list_status, reqwest::StatusCode::OK);

    let items = list_body["items"]
        .as_array()
        .expect("trigger list response should return items");

    let pack_item = items
        .iter()
        .find(|item| item["id"] == json!("analytics-trigger"))
        .expect("pack-managed trigger should be listed");
    assert_eq!(pack_item["origin"]["kind"], json!("pack"));
    assert_eq!(pack_item["origin"]["pack_id"], json!("analytics"));

    let user_item = items
        .iter()
        .find(|item| item["id"] == json!("trigger-origin-user"))
        .expect("user trigger should be listed");
    assert_eq!(user_item["origin"]["kind"], json!("user"));
    assert!(user_item["origin"]["pack_id"].is_null());
}

#[tokio::test]
async fn trigger_validate_should_report_missing_selector() {
    let server = start_trigger_v2_test_server().await;
    let client = reqwest::Client::new();

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/triggers/validate",
        json!({
            "definition": {
                "id": "broken-signal-trigger",
                "name": "Broken Signal Trigger",
                "description": "Should fail validation",
                "enabled": true,
                "max_fires": 0,
                "cooldown_secs": 0,
                "match": {
                    "event": "issue.updated"
                },
                "target": {
                    "kind": "workflow_signal",
                    "signal": "resume"
                }
            },
            "strict": true
        }),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::OK);
    assert_eq!(body["valid"], json!(false));
    assert!(body["issues"]
        .as_array()
        .expect("issues should be an array")
        .iter()
        .any(|issue| issue["path"] == json!("target.selector")));
}

#[tokio::test]
async fn trigger_list_should_support_pagination_and_target_kind_filter() {
    let server = start_trigger_v2_test_server().await;
    let client = reqwest::Client::new();
    create_workflow(&client, &server, "trigger-list-workflow").await;

    let first = workflow_start_trigger(
        "trigger-list-a",
        "trigger-list-workflow",
        true,
        "issue.created",
    );
    let second = workflow_start_trigger(
        "trigger-list-b",
        "trigger-list-workflow",
        false,
        "issue.updated",
    );

    let (first_status, _) = post_json(&client, &server, "/api/v1/triggers", first).await;
    let (second_status, _) = post_json(&client, &server, "/api/v1/triggers", second).await;
    assert_eq!(first_status, reqwest::StatusCode::CREATED);
    assert_eq!(second_status, reqwest::StatusCode::CREATED);

    let (status, body) = get_json(
        &client,
        &server,
        "/api/v1/triggers?target_kind=workflow_start&limit=1",
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::OK);
    let items = body["items"].as_array().expect("items should be an array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["target"]["kind"], json!("workflow_start"));
    assert!(body["next_cursor"].is_string());
}
