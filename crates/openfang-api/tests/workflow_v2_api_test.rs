//! Real HTTP integration tests for the Workflow v2 API surface.

use std::collections::BTreeSet;
use std::sync::Arc;

use openfang_api::routes::AppState;
use openfang_api::server::build_router;
use openfang_api::types::{WorkflowOrigin, WorkflowOriginKind, WorkflowResponse};
use openfang_kernel::OpenFangKernel;
use openfang_types::agent::AgentManifest;
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use openfang_types::workflow::WorkflowV2Definition;
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

const TEST_MANIFEST: &str = r#"
name = "workflow-tester"
version = "0.1.0"
description = "Workflow v2 integration test agent"
author = "test"
module = "builtin:chat"

[model]
provider = "ollama"
model = "test-model"
system_prompt = "You are a workflow test agent."

[capabilities]
tools = []
memory_read = ["*"]
memory_write = ["self.*"]
"#;

async fn start_workflow_v2_test_server() -> TestServer {
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
    kernel
        .workflows
        .set_known_primitives(["artifact.write"])
        .await;

    let manifest: AgentManifest =
        toml::from_str(TEST_MANIFEST).expect("test manifest should deserialize");
    kernel
        .spawn_agent(manifest)
        .expect("workflow test agent should spawn");

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

fn available_agent_refs(kernel: &OpenFangKernel) -> Vec<String> {
    let mut refs = BTreeSet::new();
    for entry in kernel.registry.list() {
        refs.insert(entry.id.to_string());
        refs.insert(entry.name);
    }
    refs.into_iter().collect()
}

fn nested_workflow_definition() -> WorkflowV2Definition {
    serde_json::from_value(json!({
        "id": "nested-review",
        "name": "Nested Review",
        "version": "1.0.0",
        "description": "Nested workflow used by Workflow v2 API tests",
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
                "id": "nested-noop",
                "name": "Nested Noop",
                "kind": "noop",
                "save_as": "result",
                "flow": { "mode": "sequential" }
            }
        ],
        "outputs": {
            "result": "{{ vars.result }}"
        }
    }))
    .expect("nested workflow definition should deserialize")
}

async fn register_nested_workflow(server: &TestServer) {
    server
        .state
        .kernel
        .workflows
        .register_workflow_v2_definition(
            nested_workflow_definition(),
            available_agent_refs(&server.state.kernel),
        )
        .await
        .expect("nested workflow should register");
}

fn valid_workflow_definition() -> WorkflowV2Definition {
    serde_json::from_value(json!({
        "id": "workflow-v2-api",
        "name": "Workflow V2 API",
        "version": "1.0.0",
        "description": "Workflow v2 API integration test definition",
        "input": {
            "kind": "object",
            "required": ["topic"],
            "open": false,
            "fields": {
                "topic": { "kind": "text" }
            }
        },
        "output": {
            "kind": "object",
            "required": ["final_result"],
            "open": false,
            "fields": {
                "final_result": { "kind": "string" }
            }
        },
        "steps": [
            {
                "id": "agent-step",
                "name": "Agent Step",
                "kind": "agent",
                "uses": { "agent": "workflow-tester" },
                "with": {
                    "message": "Analyze {{ input.topic }}"
                },
                "save_as": "analysis",
                "flow": { "mode": "sequential" }
            },
            {
                "id": "primitive-step",
                "name": "Primitive Step",
                "kind": "primitive",
                "uses": { "primitive": "artifact.write" },
                "with": {
                    "source": "{{ vars.analysis }}"
                },
                "save_as": "artifact",
                "flow": { "mode": "fan_out" }
            },
            {
                "id": "collect-step",
                "name": "Collect Step",
                "kind": "collect",
                "save_as": "collected",
                "flow": { "mode": "sequential" }
            },
            {
                "id": "workflow-step",
                "name": "Workflow Step",
                "kind": "workflow",
                "uses": { "workflow": "nested-review" },
                "with": {
                    "topic": "{{ input.topic }}"
                },
                "save_as": "nested",
                "flow": {
                    "mode": "conditional",
                    "when": "true"
                }
            },
            {
                "id": "wait-step",
                "name": "Wait Step",
                "kind": "wait_signal",
                "uses": { "signal_name": "approval_ready" },
                "save_as": "approval",
                "flow": { "mode": "sequential" }
            },
            {
                "id": "looper-step",
                "name": "Looper Step",
                "kind": "start_looper",
                "uses": { "task_ref": "task-1" },
                "save_as": "looper",
                "flow": { "mode": "sequential" }
            },
            {
                "id": "emit-step",
                "name": "Emit Step",
                "kind": "emit_event",
                "uses": {
                    "event": "artifact.created",
                    "payload_template": "{{ vars.artifact }}"
                },
                "save_as": "event_receipt",
                "flow": { "mode": "sequential" }
            },
            {
                "id": "noop-step",
                "name": "Noop Step",
                "kind": "noop",
                "save_as": "final_result",
                "flow": {
                    "mode": "loop",
                    "until": "done",
                    "max_iterations": 2
                }
            }
        ],
        "outputs": {
            "final_result": "{{ vars.final_result }}"
        }
    }))
    .expect("valid workflow definition should deserialize")
}

fn dangling_reference_definition() -> WorkflowV2Definition {
    let mut definition = valid_workflow_definition();
    definition.id = "workflow-v2-api-dangling".to_string();
    definition.outputs.insert(
        "final_result".to_string(),
        "{{ vars.missing_result }}".to_string(),
    );
    definition
}

fn simple_workflow_definition(id: &str, description: &str) -> WorkflowV2Definition {
    serde_json::from_value(json!({
        "id": id,
        "name": format!("Workflow {id}"),
        "version": "1.0.0",
        "description": description,
        "enabled": true,
        "tags": ["api"],
        "input": {
            "kind": "object",
            "required": ["topic"],
            "open": false,
            "fields": {
                "topic": { "kind": "text" }
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
    .expect("simple workflow definition should deserialize")
}

fn managed_pack_workflow_resource(id: &str) -> WorkflowResponse {
    WorkflowResponse {
        definition: simple_workflow_definition(id, "Managed pack workflow"),
        origin: WorkflowOrigin {
            kind: WorkflowOriginKind::Pack,
            pack_id: Some(id.to_string()),
            pack_version: Some("1.2.0".to_string()),
            source: Some("bundled".to_string()),
        },
        forked_from: None,
        created_at: "2026-03-24T00:00:00Z".to_string(),
        updated_at: "2026-03-24T00:00:00Z".to_string(),
    }
}

fn persist_workflow_resource(server: &TestServer, resource: &WorkflowResponse) {
    let workflows_dir = server.state.kernel.config.home_dir.join("workflows");
    std::fs::create_dir_all(&workflows_dir).expect("workflow directory should exist");
    std::fs::write(
        workflows_dir.join(format!("{}.toml", resource.definition.id)),
        toml::to_string_pretty(resource).expect("workflow resource should serialize"),
    )
    .expect("workflow resource should be written");
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

#[tokio::test]
async fn post_validate_returns_valid_true_for_correct_definition() {
    let server = start_workflow_v2_test_server().await;
    register_nested_workflow(&server).await;
    let client = reqwest::Client::new();

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/workflows/validate",
        json!({
            "definition": valid_workflow_definition(),
            "strict": false
        }),
    )
    .await;

    assert!(status == reqwest::StatusCode::OK);
    assert!(body["valid"] == Value::Bool(true));
    assert!(body["issues"] == Value::Array(Vec::new()));
    assert!(body["normalized"]["input"]["kind"] == Value::String("object".to_string()));
    assert!(
        body["normalized"]["input"]["fields"]["topic"]["kind"]
            == Value::String("string".to_string())
    );
}

#[tokio::test]
async fn post_validate_returns_issues_for_dangling_reference() {
    let server = start_workflow_v2_test_server().await;
    register_nested_workflow(&server).await;
    let client = reqwest::Client::new();

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/workflows/validate",
        json!({
            "definition": dangling_reference_definition()
        }),
    )
    .await;

    assert!(status == reqwest::StatusCode::OK);
    assert!(body["valid"] == Value::Bool(false));
    assert!(body["issues"]
        .as_array()
        .expect("issues should be an array")
        .iter()
        .any(|issue| {
            issue["severity"] == Value::String("error".to_string())
                && issue["code"] == Value::String("dangling_reference".to_string())
        }));
    assert!(body.get("normalized").is_none());
}

#[tokio::test]
async fn post_compile_returns_workflow_ir() {
    let server = start_workflow_v2_test_server().await;
    register_nested_workflow(&server).await;
    let client = reqwest::Client::new();

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/workflows/compile",
        json!({
            "definition": valid_workflow_definition()
        }),
    )
    .await;

    assert!(status == reqwest::StatusCode::OK);
    assert!(body["definition_id"] == Value::String("workflow-v2-api".to_string()));
    assert!(body["normalized"]["steps"].as_array().is_some());
    assert!(body["compiled"]["workflow_ir"]["workflow_id"] == body["definition_id"]);
    assert!(body["compiled"]["workflow_ir"]["steps"]
        .as_array()
        .map(|steps| !steps.is_empty())
        .unwrap_or(false));
}

#[tokio::test]
async fn get_compiled_returns_cached_ir_for_registered_workflow() {
    let server = start_workflow_v2_test_server().await;
    let client = reqwest::Client::new();
    let definition = simple_workflow_definition("compiled-after-create", "Persisted workflow");

    let (create_status, _created) = post_json(
        &client,
        &server,
        "/api/v1/workflows",
        serde_json::to_value(&definition).expect("workflow definition should serialize"),
    )
    .await;
    assert!(create_status == reqwest::StatusCode::CREATED);

    let (status, body) = get_json(
        &client,
        &server,
        "/api/v1/workflows/compiled-after-create/compiled",
    )
    .await;

    assert!(status == reqwest::StatusCode::OK);
    assert!(body["definition_id"] == Value::String(definition.id.clone()));
    assert!(
        body["compiled"]["workflow_ir"]["symbol_table"]["result"]
            == Value::String("noop-step".to_string())
    );
}

#[tokio::test]
async fn get_compiled_returns_404_for_unknown_id() {
    let server = start_workflow_v2_test_server().await;
    let client = reqwest::Client::new();
    let (status, body) = get_json(
        &client,
        &server,
        "/api/v1/workflows/unknown-workflow/compiled",
    )
    .await;

    assert!(status == reqwest::StatusCode::NOT_FOUND);
    assert!(body["error"]["code"] == Value::String("not_found".to_string()));
    assert!(body["error"]["message"] == Value::String("Workflow definition not found".to_string()));
    assert!(body["error"]["details"] == Value::Array(Vec::new()));
}

#[tokio::test]
async fn end_to_end_definition_to_ir_preserves_step_semantics() {
    let server = start_workflow_v2_test_server().await;
    register_nested_workflow(&server).await;
    let client = reqwest::Client::new();

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/workflows/compile",
        json!({
            "definition": valid_workflow_definition()
        }),
    )
    .await;

    let steps = body["compiled"]["workflow_ir"]["steps"]
        .as_array()
        .expect("compiled steps should be an array");

    assert!(status == reqwest::StatusCode::OK);
    assert!(steps.len() == 8);
    assert!(steps[0]["kind"]["kind"] == Value::String("agent".to_string()));
    assert!(steps[0]["kind"]["agent"] == Value::String("workflow-tester".to_string()));
    assert!(steps[0]["save_as"] == Value::String("analysis".to_string()));
    assert!(steps[0]["flow"]["mode"] == Value::String("sequential".to_string()));
    assert!(steps[1]["kind"]["kind"] == Value::String("primitive".to_string()));
    assert!(steps[1]["kind"]["primitive"] == Value::String("artifact.write".to_string()));
    assert!(steps[1]["save_as"] == Value::String("artifact".to_string()));
    assert!(steps[1]["flow"]["mode"] == Value::String("fan_out".to_string()));
    assert!(steps[2]["kind"]["kind"] == Value::String("collect".to_string()));
    assert!(steps[2]["save_as"] == Value::String("collected".to_string()));
    assert!(steps[2]["flow"]["mode"] == Value::String("sequential".to_string()));
    assert!(steps[3]["kind"]["kind"] == Value::String("workflow".to_string()));
    assert!(steps[3]["kind"]["workflow"] == Value::String("nested-review".to_string()));
    assert!(steps[3]["save_as"] == Value::String("nested".to_string()));
    assert!(steps[3]["flow"]["mode"] == Value::String("conditional".to_string()));
    assert!(steps[4]["kind"]["kind"] == Value::String("wait_signal".to_string()));
    assert!(steps[4]["kind"]["signal_name"] == Value::String("approval_ready".to_string()));
    assert!(steps[4]["save_as"] == Value::String("approval".to_string()));
    assert!(steps[5]["kind"]["kind"] == Value::String("start_looper".to_string()));
    assert!(steps[5]["kind"]["task_ref"] == Value::String("task-1".to_string()));
    assert!(steps[5]["save_as"] == Value::String("looper".to_string()));
    assert!(steps[6]["kind"]["kind"] == Value::String("emit_event".to_string()));
    assert!(steps[6]["kind"]["event"] == Value::String("artifact.created".to_string()));
    assert!(
        steps[6]["kind"]["payload_template"] == Value::String("{{ vars.artifact }}".to_string())
    );
    assert!(steps[6]["save_as"] == Value::String("event_receipt".to_string()));
    assert!(steps[7]["kind"]["kind"] == Value::String("noop".to_string()));
    assert!(steps[7]["save_as"] == Value::String("final_result".to_string()));
    assert!(steps[7]["flow"]["mode"] == Value::String("loop".to_string()));
    assert!(steps[7]["flow"]["until"] == Value::String("done".to_string()));
    assert!(steps[7]["flow"]["max_iterations"].as_u64() == Some(2));
}

#[tokio::test]
async fn post_compile_returns_error_envelope_when_validation_fails() {
    let server = start_workflow_v2_test_server().await;
    register_nested_workflow(&server).await;
    let client = reqwest::Client::new();

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/workflows/compile",
        json!({
            "definition": dangling_reference_definition()
        }),
    )
    .await;

    assert!(status == reqwest::StatusCode::BAD_REQUEST);
    assert!(body["error"]["code"] == Value::String("validation_error".to_string()));
    assert!(
        body["error"]["message"] == Value::String("workflow definition is invalid".to_string())
    );
    assert!(body["error"]["details"]
        .as_array()
        .expect("error details should be an array")
        .iter()
        .any(|issue| issue["code"] == Value::String("dangling_reference".to_string())));
    assert!(body["compiled"].is_null());
}

#[tokio::test]
async fn workflow_crud_round_trip_and_delete_returns_404_afterwards() {
    let server = start_workflow_v2_test_server().await;
    let client = reqwest::Client::new();
    let workflow_id = "crud-round-trip";

    let (create_status, create_body) = post_json(
        &client,
        &server,
        "/api/v1/workflows",
        serde_json::to_value(simple_workflow_definition(workflow_id, "Created workflow"))
            .expect("workflow definition should serialize"),
    )
    .await;
    assert!(create_status == reqwest::StatusCode::CREATED);
    assert!(create_body["id"] == workflow_id);

    let (get_status, get_body) = get_json(
        &client,
        &server,
        &format!("/api/v1/workflows/{workflow_id}"),
    )
    .await;
    assert!(get_status == reqwest::StatusCode::OK);
    assert!(get_body["description"] == "Created workflow");

    let (update_status, update_body) = put_json(
        &client,
        &server,
        &format!("/api/v1/workflows/{workflow_id}"),
        serde_json::to_value(simple_workflow_definition(workflow_id, "Updated workflow"))
            .expect("workflow definition should serialize"),
    )
    .await;
    assert!(update_status == reqwest::StatusCode::OK);
    assert!(update_body["description"] == "Updated workflow");

    let (delete_status, delete_body) = delete_json(
        &client,
        &server,
        &format!("/api/v1/workflows/{workflow_id}"),
    )
    .await;
    assert!(delete_status == reqwest::StatusCode::NO_CONTENT);
    assert!(delete_body.is_null());

    let (get_status, get_body) = get_json(
        &client,
        &server,
        &format!("/api/v1/workflows/{workflow_id}"),
    )
    .await;
    assert!(get_status == reqwest::StatusCode::NOT_FOUND);
    assert!(get_body["error"]["code"] == "not_found");
}

#[tokio::test]
async fn workflow_list_endpoint_supports_offset_pagination() {
    let server = start_workflow_v2_test_server().await;
    let client = reqwest::Client::new();

    for workflow_id in ["page-a", "page-b", "page-c"] {
        let (status, _) = post_json(
            &client,
            &server,
            "/api/v1/workflows",
            serde_json::to_value(simple_workflow_definition(
                workflow_id,
                "Pagination fixture",
            ))
            .expect("workflow definition should serialize"),
        )
        .await;
        assert!(status == reqwest::StatusCode::CREATED);
    }

    let (first_status, first_body) = get_json(
        &client,
        &server,
        "/api/v1/workflows?limit=2&sort=id&order=asc",
    )
    .await;
    assert!(first_status == reqwest::StatusCode::OK);
    assert!(first_body["items"].as_array().map(|items| items.len()) == Some(2));
    let next_cursor = first_body["next_cursor"]
        .as_str()
        .expect("next cursor should exist")
        .to_string();

    let mut ids = first_body["items"]
        .as_array()
        .expect("items should be an array")
        .iter()
        .filter_map(|item| item["id"].as_str())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut cursor = Some(next_cursor);

    while let Some(next_cursor) = cursor.take() {
        let (page_status, page_body) = get_json(
            &client,
            &server,
            &format!("/api/v1/workflows?limit=2&cursor={next_cursor}&sort=id&order=asc"),
        )
        .await;
        assert!(page_status == reqwest::StatusCode::OK);

        ids.extend(
            page_body["items"]
                .as_array()
                .expect("items should be an array")
                .iter()
                .filter_map(|item| item["id"].as_str())
                .map(str::to_string),
        );
        cursor = page_body["next_cursor"].as_str().map(ToOwned::to_owned);
    }

    let mut created_ids = ids
        .into_iter()
        .filter(|id| id.starts_with("page-"))
        .collect::<Vec<_>>();
    created_ids.sort();
    assert!(created_ids == vec!["page-a", "page-b", "page-c"]);
}

#[tokio::test]
async fn post_validate_returns_error_issue_for_missing_required_field() {
    let server = start_workflow_v2_test_server().await;
    let client = reqwest::Client::new();

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/workflows/validate",
        json!({
            "definition": {
                "id": "missing-name",
                "version": "1.0.0",
                "description": "Missing required field",
                "input": { "kind": "object", "fields": {}, "required": [], "open": false },
                "output": { "kind": "object", "fields": {}, "required": [], "open": false },
                "steps": []
            }
        }),
    )
    .await;

    assert!(status == reqwest::StatusCode::OK);
    assert!(body["valid"] == Value::Bool(false));
    assert!(body["issues"]
        .as_array()
        .expect("issues should be an array")
        .iter()
        .any(|issue| issue["severity"] == Value::String("error".to_string())));
}

#[tokio::test]
async fn post_fork_creates_user_owned_shadow_with_provenance() {
    let server = start_workflow_v2_test_server().await;
    persist_workflow_resource(&server, &managed_pack_workflow_resource("pack-sdlc"));
    let client = reqwest::Client::new();

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/workflows/pack-sdlc/fork",
        json!({ "mode": "shadow" }),
    )
    .await;

    assert!(status == reqwest::StatusCode::OK);
    assert!(body["origin"]["kind"] == Value::String("user".to_string()));
    assert!(body["forked_from"]["kind"] == Value::String("pack".to_string()));
    assert!(body["forked_from"]["resource_type"] == Value::String("workflow".to_string()));
    assert!(body["forked_from"]["resource_id"] == Value::String("pack-sdlc".to_string()));

    let (get_status, get_body) = get_json(&client, &server, "/api/v1/workflows/pack-sdlc").await;
    assert!(get_status == reqwest::StatusCode::OK);
    assert!(get_body["origin"]["kind"] == Value::String("user".to_string()));
    assert!(get_body["forked_from"]["pack_id"] == Value::String("pack-sdlc".to_string()));
}

#[tokio::test]
async fn direct_create_with_managed_pack_collision_returns_conflict() {
    let server = start_workflow_v2_test_server().await;
    persist_workflow_resource(&server, &managed_pack_workflow_resource("pack-collision"));
    let client = reqwest::Client::new();

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/workflows",
        serde_json::to_value(simple_workflow_definition(
            "pack-collision",
            "Should conflict with managed pack",
        ))
        .expect("workflow definition should serialize"),
    )
    .await;

    assert!(status == reqwest::StatusCode::CONFLICT);
    assert!(body["error"]["code"] == Value::String("managed_definition_conflict".to_string()));
}

#[tokio::test]
async fn delete_nonexistent_workflow_returns_stable_error_envelope() {
    let server = start_workflow_v2_test_server().await;
    let client = reqwest::Client::new();

    let (status, body) = delete_json(&client, &server, "/api/v1/workflows/does-not-exist").await;

    assert!(status == reqwest::StatusCode::NOT_FOUND);
    assert!(body["error"]["code"] == Value::String("not_found".to_string()));
    assert!(body["error"]["message"] == Value::String("Workflow definition not found".to_string()));
    assert!(body["error"]["details"] == Value::Array(Vec::new()));
}

#[tokio::test]
async fn workflow_run_dry_run_returns_explanation_block() {
    let server = start_workflow_v2_test_server().await;
    let client = reqwest::Client::new();

    let (create_status, _) = post_json(
        &client,
        &server,
        "/api/v1/workflows",
        serde_json::to_value(simple_workflow_definition(
            "dry-run-flow",
            "Dry-run workflow",
        ))
        .expect("workflow definition should serialize"),
    )
    .await;
    assert!(create_status == reqwest::StatusCode::CREATED);

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/workflows/dry-run-flow/runs/dry-run",
        json!({
            "input": {
                "topic": "Ship it"
            },
            "labels": ["manual"],
            "metadata": {
                "source": "test"
            }
        }),
    )
    .await;

    assert!(status == reqwest::StatusCode::OK);
    assert!(body["would_execute"] == Value::Bool(true));
    assert!(body["explanation"]["input_contract"]["kind"] == Value::String("object".to_string()));
    assert!(body["explanation"]["output_contract"]["kind"] == Value::String("object".to_string()));
}
