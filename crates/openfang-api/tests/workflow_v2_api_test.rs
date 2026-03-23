//! Real HTTP integration tests for the Workflow v2 API surface.

use std::collections::BTreeSet;
use std::sync::Arc;

use openfang_api::routes::AppState;
use openfang_api::server::build_router;
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
    register_nested_workflow(&server).await;
    let definition = valid_workflow_definition();
    server
        .state
        .kernel
        .workflows
        .register_workflow_v2_definition(
            definition.clone(),
            available_agent_refs(&server.state.kernel),
        )
        .await
        .expect("workflow should register");
    server
        .state
        .kernel
        .workflows
        .set_known_primitives(std::iter::empty::<String>())
        .await;

    let response = reqwest::Client::new()
        .get(format!(
            "{}/api/v1/workflows/{}/compiled",
            server.base_url, definition.id
        ))
        .send()
        .await
        .expect("compiled workflow request should succeed");
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .expect("compiled workflow body should deserialize");

    assert!(status == reqwest::StatusCode::OK);
    assert!(body["definition_id"] == Value::String(definition.id));
    assert!(
        body["compiled"]["workflow_ir"]["symbol_table"]["final_result"]
            == Value::String("noop-step".to_string())
    );
}

#[tokio::test]
async fn get_compiled_returns_404_for_unknown_id() {
    let server = start_workflow_v2_test_server().await;

    let response = reqwest::Client::new()
        .get(format!(
            "{}/api/v1/workflows/unknown-workflow/compiled",
            server.base_url
        ))
        .send()
        .await
        .expect("unknown workflow request should succeed");
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .expect("unknown workflow body should deserialize");

    assert!(status == reqwest::StatusCode::NOT_FOUND);
    assert!(body["error"]["code"] == Value::String("not_found".to_string()));
    assert!(body["error"]["message"] == Value::String("Workflow not found".to_string()));
    assert!(body["error"]["details"].is_null());
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
