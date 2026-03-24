//! Real HTTP integration tests for the Agent definition v2 API surface.

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

async fn start_agent_v2_test_server() -> TestServer {
    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    let config = KernelConfig {
        home_dir: tmp.path().to_path_buf(),
        data_dir: tmp.path().join("data"),
        default_model: DefaultModelConfig {
            provider: "claude_code".to_string(),
            model: "sonnet".to_string(),
            api_key_env: "ANTHROPIC_API_KEY".to_string(),
            base_url: None,
        },
        ..KernelConfig::default()
    };

    let kernel = Arc::new(OpenFangKernel::boot_with_config(config).expect("kernel should boot"));
    kernel.set_self_handle();
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

fn agent_definition_value(id: &str, name: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "version": "1.0.0",
        "description": "Writes and iterates on product requirement documents",
        "enabled": true,
        "group": "sdlc",
        "tags": ["docs", "prd", "planning"],
        "provider": {
            "driver": "claude_code",
            "model": "sonnet",
            "profile": "default",
            "defaults": {
                "reasoning_effort": "high",
                "max_tokens": 8000
            },
            "config": {
                "continue_conversation": true,
                "fork_session": false,
                "allowed_tools": ["Read", "Write", "Bash"],
                "disallowed_tools": [],
                "additional_directories": ["./docs"],
                "max_budget_usd": 5.0,
                "fallback_model": "sonnet"
            }
        },
        "prompt": {
            "system": "You are a senior product writer.",
            "instructions": "Write clear, implementation-ready PRDs.",
            "skills": ["writing", "prd"]
        },
        "capabilities": {
            "tools": ["*"],
            "primitives": ["issue.read", "artifact.*", "doc.*", "hitl.*"],
            "delegation": ["call", "send"],
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
            "kind": "artifact_ref",
            "artifact_type": "prd"
        }
    })
}

#[tokio::test]
async fn create_validate_compile_and_get_compiled_flow_should_use_consistent_definition_id() {
    let server = start_agent_v2_test_server().await;
    let client = reqwest::Client::new();
    let definition = agent_definition_value("prd-writer", "PRD Writer");

    let create = client
        .post(format!("{}/api/v1/agents", server.base_url))
        .json(&definition)
        .send()
        .await
        .expect("create request should succeed");
    assert_eq!(create.status(), reqwest::StatusCode::CREATED);
    let created: Value = create.json().await.expect("create response should be JSON");
    assert_eq!(created["id"], json!("prd-writer"));
    assert_eq!(created["origin"]["kind"], json!("user"));
    assert!(created["forked_from"].is_null());

    let validate = client
        .post(format!("{}/api/v1/agents/validate", server.base_url))
        .json(&json!({
            "definition": definition,
            "strict": true,
            "context": {}
        }))
        .send()
        .await
        .expect("validate request should succeed");
    assert_eq!(validate.status(), reqwest::StatusCode::OK);
    let validated: Value = validate
        .json()
        .await
        .expect("validate response should be JSON");
    assert_eq!(validated["valid"], json!(true));
    assert_eq!(validated["issues"], json!([]));

    let compile = client
        .post(format!("{}/api/v1/agents/compile", server.base_url))
        .json(&json!({
            "definition": agent_definition_value("prd-writer", "PRD Writer"),
            "context": {}
        }))
        .send()
        .await
        .expect("compile request should succeed");
    assert_eq!(compile.status(), reqwest::StatusCode::OK);
    let compiled: Value = compile
        .json()
        .await
        .expect("compile response should be JSON");
    assert_eq!(compiled["definition_id"], json!("prd-writer"));
    assert!(compiled["compiled"]["agent_manifest"].is_object());
    assert!(compiled["compiled"]["provider_binding"].is_object());
    assert!(compiled["compiled"]["product_metadata"].is_object());

    let get_compiled = client
        .get(format!(
            "{}/api/v1/agents/prd-writer/compiled",
            server.base_url
        ))
        .send()
        .await
        .expect("get compiled request should succeed");
    assert_eq!(get_compiled.status(), reqwest::StatusCode::OK);
    let persisted_compiled: Value = get_compiled
        .json()
        .await
        .expect("compiled response should be JSON");
    assert_eq!(persisted_compiled["definition_id"], json!("prd-writer"));
    assert_eq!(persisted_compiled["normalized"]["id"], json!("prd-writer"));
    assert!(persisted_compiled["compiled"]["agent_manifest"].is_object());
    assert!(persisted_compiled["compiled"]["provider_binding"].is_object());
    assert!(persisted_compiled["compiled"]["product_metadata"].is_object());
}

#[tokio::test]
async fn list_agents_should_return_items_and_next_cursor_shape() {
    let server = start_agent_v2_test_server().await;
    let client = reqwest::Client::new();

    let create = client
        .post(format!("{}/api/v1/agents", server.base_url))
        .json(&agent_definition_value("list-writer", "List Writer"))
        .send()
        .await
        .expect("create request should succeed");
    assert_eq!(create.status(), reqwest::StatusCode::CREATED);

    let response = client
        .get(format!("{}/api/v1/agents", server.base_url))
        .send()
        .await
        .expect("list request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("list response should be JSON");

    assert!(body["items"].is_array());
    assert!(body["next_cursor"].is_null());
    let item = body["items"]
        .as_array()
        .expect("items should be an array")
        .iter()
        .find(|item| item["id"] == json!("list-writer"))
        .expect("created definition should appear in the list");
    assert_eq!(item["name"], json!("List Writer"));
    assert_eq!(item["enabled"], json!(true));
    assert_eq!(item["group"], json!("sdlc"));
    assert_eq!(item["provider"]["driver"], json!("claude_code"));
    assert_eq!(item["origin"]["kind"], json!("user"));
    assert!(item["forked_from"].is_null());
    assert_eq!(item["runtime_status"]["loaded"], json!(false));
    assert!(item["updated_at"].is_string());
}

#[tokio::test]
async fn delete_agent_then_get_should_return_not_found_error_envelope() {
    let server = start_agent_v2_test_server().await;
    let client = reqwest::Client::new();

    let create = client
        .post(format!("{}/api/v1/agents", server.base_url))
        .json(&agent_definition_value("delete-writer", "Delete Writer"))
        .send()
        .await
        .expect("create request should succeed");
    assert_eq!(create.status(), reqwest::StatusCode::CREATED);

    let delete = client
        .delete(format!("{}/api/v1/agents/delete-writer", server.base_url))
        .send()
        .await
        .expect("delete request should succeed");
    assert_eq!(delete.status(), reqwest::StatusCode::NO_CONTENT);

    let get = client
        .get(format!("{}/api/v1/agents/delete-writer", server.base_url))
        .send()
        .await
        .expect("get request should succeed");
    assert_eq!(get.status(), reqwest::StatusCode::NOT_FOUND);
    let body: Value = get.json().await.expect("error response should be JSON");
    assert_eq!(body["error"]["code"], json!("not_found"));
    assert_eq!(
        body["error"]["message"],
        json!("Agent definition not found")
    );
    assert!(body["error"]["details"].is_array());
}

#[tokio::test]
async fn put_agent_should_update_name_and_not_change_runtime_state() {
    let server = start_agent_v2_test_server().await;
    let client = reqwest::Client::new();

    let create = client
        .post(format!("{}/api/v1/agents", server.base_url))
        .json(&agent_definition_value("update-writer", "Update Writer"))
        .send()
        .await
        .expect("create request should succeed");
    assert_eq!(create.status(), reqwest::StatusCode::CREATED);

    let runtime_before = server
        .state
        .kernel
        .runtime_stores
        .agent_runtime
        .list_agent_runtimes()
        .expect("runtime projections should load");

    let mut updated_definition = agent_definition_value("update-writer", "Updated Writer");
    updated_definition["description"] = json!("Updated description");
    let update = client
        .put(format!("{}/api/v1/agents/update-writer", server.base_url))
        .json(&updated_definition)
        .send()
        .await
        .expect("update request should succeed");
    assert_eq!(update.status(), reqwest::StatusCode::OK);
    let updated: Value = update.json().await.expect("update response should be JSON");
    assert_eq!(updated["id"], json!("update-writer"));
    assert_eq!(updated["name"], json!("Updated Writer"));
    assert_eq!(updated["description"], json!("Updated description"));
    assert_eq!(updated["origin"]["kind"], json!("user"));

    let runtime_after = server
        .state
        .kernel
        .runtime_stores
        .agent_runtime
        .list_agent_runtimes()
        .expect("runtime projections should load");
    assert_eq!(runtime_after, runtime_before);
}

#[tokio::test]
async fn v1_agents_post_should_reject_legacy_manifest_payload_with_error_envelope() {
    let server = start_agent_v2_test_server().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/agents", server.base_url))
        .json(&json!({
            "manifest_toml": "name = 'legacy'"
        }))
        .send()
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: Value = response
        .json()
        .await
        .expect("error response should be JSON");
    assert_eq!(body["error"]["code"], json!("invalid_request"));
    assert!(body["error"]["message"].is_string());
    assert!(body["error"]["details"].is_array());
}
