//! Real HTTP integration tests for the Agent definition v2 API surface.

use std::sync::Arc;

use openfang_api::routes::AppState;
use openfang_api::server::build_router;
use openfang_kernel::OpenFangKernel;
use openfang_types::agent::SessionId;
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use openfang_types::message::{Message, MessageContent, Role};
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

fn codex_agent_definition_value(id: &str, name: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "version": "1.0.0",
        "description": "Executes simple prompts through Codex",
        "enabled": true,
        "group": "tests",
        "tags": ["tests", "messages"],
        "provider": {
            "driver": "codex",
            "model": "gpt-4.1",
            "defaults": {
                "max_tokens": 512
            },
            "config": {
                "web_search": false
            }
        },
        "prompt": {
            "system": "You are a concise test assistant.",
            "instructions": "Answer briefly and directly.",
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

async fn create_agent_definition(
    client: &reqwest::Client,
    server: &TestServer,
    id: &str,
    name: &str,
) -> Value {
    let response = client
        .post(format!("{}/api/v1/agents", server.base_url))
        .json(&agent_definition_value(id, name))
        .send()
        .await
        .expect("create request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    response
        .json()
        .await
        .expect("create response should be JSON")
}

async fn create_codex_agent_definition(
    client: &reqwest::Client,
    server: &TestServer,
    id: &str,
    name: &str,
) -> Value {
    let response = client
        .post(format!("{}/api/v1/agents", server.base_url))
        .json(&codex_agent_definition_value(id, name))
        .send()
        .await
        .expect("create request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    response
        .json()
        .await
        .expect("create response should be JSON")
}

async fn get_runtime(client: &reqwest::Client, server: &TestServer, definition_id: &str) -> Value {
    let response = client
        .get(format!(
            "{}/api/v1/agents/{definition_id}/runtime",
            server.base_url
        ))
        .send()
        .await
        .expect("runtime request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    response
        .json()
        .await
        .expect("runtime response should be JSON")
}

async fn list_sessions(
    client: &reqwest::Client,
    server: &TestServer,
    definition_id: &str,
) -> Value {
    let response = client
        .get(format!(
            "{}/api/v1/agents/{definition_id}/sessions",
            server.base_url
        ))
        .send()
        .await
        .expect("sessions list request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    response
        .json()
        .await
        .expect("sessions list response should be JSON")
}

fn runtime_agent_id(server: &TestServer, definition_id: &str) -> openfang_types::agent::AgentId {
    server
        .state
        .kernel
        .registry
        .list()
        .into_iter()
        .find(|entry| {
            entry
                .manifest
                .metadata
                .get("compozy")
                .and_then(|value| value.get("definition_id"))
                .and_then(Value::as_str)
                == Some(definition_id)
        })
        .map(|entry| entry.id)
        .expect("runtime agent should exist")
}

fn codex_live_available() -> bool {
    std::env::var("OPENAI_API_KEY").is_ok()
        || std::env::var("CODEX_HOME")
            .map(std::path::PathBuf::from)
            .ok()
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|home| std::path::PathBuf::from(home).join(".codex"))
            })
            .map(|path| path.join("auth.json").is_file())
            .unwrap_or(false)
}

fn message_request(session_id: &str, text: &str) -> Value {
    json!({
        "session_id": session_id,
        "input": {
            "items": [{
                "type": "text",
                "text": text
            }]
        },
        "metadata": {
            "source": "tests"
        }
    })
}

fn seed_session_message(server: &TestServer, definition_id: &str, session_id: &str) {
    let session_id = SessionId(
        session_id
            .parse::<uuid::Uuid>()
            .expect("session_id should parse as UUID"),
    );
    let mut session = server
        .state
        .kernel
        .memory
        .get_session(session_id)
        .expect("session load should succeed")
        .expect("session should exist");
    session.messages.push(Message {
        role: Role::User,
        content: MessageContent::Text("Reset this session".to_string()),
    });
    server
        .state
        .kernel
        .memory
        .save_session(&session)
        .expect("session should save");
    server
        .state
        .kernel
        .refresh_agent_runtime_projection(runtime_agent_id(server, definition_id))
        .expect("runtime projection should refresh");
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

#[tokio::test]
async fn runtime_lifecycle_sequence_should_return_consistent_runtime_states() {
    let server = start_agent_v2_test_server().await;
    let client = reqwest::Client::new();
    create_agent_definition(&client, &server, "runtime-sequence", "Runtime Sequence").await;

    let start = client
        .post(format!(
            "{}/api/v1/agents/runtime-sequence/runtime/start",
            server.base_url
        ))
        .send()
        .await
        .expect("start request should succeed");
    assert_eq!(start.status(), reqwest::StatusCode::OK);
    let started: Value = start.json().await.expect("start response should be JSON");
    assert_eq!(started["accepted"], json!(true));
    assert_eq!(started["resource_id"], json!("runtime-sequence"));

    let runtime_after_start = get_runtime(&client, &server, "runtime-sequence").await;
    assert_eq!(runtime_after_start["loaded"], json!(true));
    assert_eq!(runtime_after_start["state"], json!("running"));

    let stop = client
        .post(format!(
            "{}/api/v1/agents/runtime-sequence/runtime/stop",
            server.base_url
        ))
        .send()
        .await
        .expect("stop request should succeed");
    assert_eq!(stop.status(), reqwest::StatusCode::OK);
    let stopped: Value = stop.json().await.expect("stop response should be JSON");
    assert_eq!(stopped["accepted"], json!(true));
    assert_eq!(stopped["resource_id"], json!("runtime-sequence"));

    let runtime_after_stop = get_runtime(&client, &server, "runtime-sequence").await;
    assert_eq!(runtime_after_stop["loaded"], json!(true));
    assert_eq!(runtime_after_stop["state"], json!("suspended"));
}

#[tokio::test]
async fn session_lifecycle_should_create_list_activate_and_reset_sessions() {
    let server = start_agent_v2_test_server().await;
    let client = reqwest::Client::new();
    create_agent_definition(&client, &server, "session-flow", "Session Flow").await;

    let start = client
        .post(format!(
            "{}/api/v1/agents/session-flow/runtime/start",
            server.base_url
        ))
        .send()
        .await
        .expect("start request should succeed");
    assert_eq!(start.status(), reqwest::StatusCode::OK);

    let initial_sessions = list_sessions(&client, &server, "session-flow").await;
    let initial_session_id = initial_sessions["items"]
        .as_array()
        .expect("items should be an array")
        .first()
        .and_then(|item| item["session_id"].as_str())
        .expect("default session should exist")
        .to_string();

    let create_session = client
        .post(format!(
            "{}/api/v1/agents/session-flow/sessions",
            server.base_url
        ))
        .json(&json!({"label": "Review"}))
        .send()
        .await
        .expect("create session request should succeed");
    assert_eq!(create_session.status(), reqwest::StatusCode::CREATED);
    let created_session: Value = create_session
        .json()
        .await
        .expect("create session response should be JSON");
    let created_session_id = created_session["session_id"]
        .as_str()
        .expect("created session should include session_id")
        .to_string();

    let after_create = list_sessions(&client, &server, "session-flow").await;
    assert!(after_create["items"]
        .as_array()
        .expect("items should be an array")
        .iter()
        .any(|item| item["session_id"] == json!(created_session_id)));

    let activate = client
        .post(format!(
            "{}/api/v1/agents/session-flow/sessions/{}/activate",
            server.base_url, initial_session_id
        ))
        .send()
        .await
        .expect("activate session request should succeed");
    assert_eq!(activate.status(), reqwest::StatusCode::OK);
    let activated: Value = activate
        .json()
        .await
        .expect("activate session response should be JSON");
    assert_eq!(activated["accepted"], json!(true));
    assert_eq!(activated["session_id"], json!(initial_session_id));

    let after_activate = list_sessions(&client, &server, "session-flow").await;
    let items = after_activate["items"]
        .as_array()
        .expect("items should be an array");
    let initial_item = items
        .iter()
        .find(|item| item["session_id"] == json!(initial_session_id))
        .expect("initial session should still exist");
    let created_item = items
        .iter()
        .find(|item| item["session_id"] == json!(created_session_id))
        .expect("created session should still exist");
    assert_eq!(initial_item["active"], json!(true));
    assert_eq!(created_item["active"], json!(false));

    seed_session_message(&server, "session-flow", &initial_session_id);

    let reset = client
        .post(format!(
            "{}/api/v1/agents/session-flow/sessions/{}/reset",
            server.base_url, initial_session_id
        ))
        .send()
        .await
        .expect("reset session request should succeed");
    assert_eq!(reset.status(), reqwest::StatusCode::OK);
    let reset_body: Value = reset.json().await.expect("reset response should be JSON");
    let reset_session_id = reset_body["session_id"]
        .as_str()
        .expect("reset response should include replacement session_id")
        .to_string();

    let session_detail = client
        .get(format!(
            "{}/api/v1/agents/session-flow/sessions/{}",
            server.base_url, reset_session_id
        ))
        .send()
        .await
        .expect("session detail request should succeed");
    assert_eq!(session_detail.status(), reqwest::StatusCode::OK);
    let session_detail: Value = session_detail
        .json()
        .await
        .expect("session detail response should be JSON");
    assert_eq!(session_detail["session_id"], json!(reset_session_id));
    assert_eq!(session_detail["message_count"], json!(0));

    let final_sessions = list_sessions(&client, &server, "session-flow").await;
    let final_items = final_sessions["items"]
        .as_array()
        .expect("items should be an array");
    assert!(final_items
        .iter()
        .any(|item| item["session_id"] == json!(reset_session_id)));
    assert!(!final_items
        .iter()
        .any(|item| item["session_id"] == json!(initial_session_id)));
}

#[tokio::test]
async fn put_agent_should_not_change_runtime_state_observable_via_runtime_endpoint() {
    let server = start_agent_v2_test_server().await;
    let client = reqwest::Client::new();
    create_agent_definition(&client, &server, "put-runtime", "Put Runtime").await;

    let start = client
        .post(format!(
            "{}/api/v1/agents/put-runtime/runtime/start",
            server.base_url
        ))
        .send()
        .await
        .expect("start request should succeed");
    assert_eq!(start.status(), reqwest::StatusCode::OK);

    let runtime_before = get_runtime(&client, &server, "put-runtime").await;

    let mut updated_definition = agent_definition_value("put-runtime", "Put Runtime Updated");
    updated_definition["description"] = json!("Updated description");
    let update = client
        .put(format!("{}/api/v1/agents/put-runtime", server.base_url))
        .json(&updated_definition)
        .send()
        .await
        .expect("update request should succeed");
    assert_eq!(update.status(), reqwest::StatusCode::OK);

    let runtime_after = get_runtime(&client, &server, "put-runtime").await;
    assert_eq!(runtime_after["state"], runtime_before["state"]);
    assert_eq!(runtime_after["loaded"], runtime_before["loaded"]);
    assert_eq!(runtime_after["mode"], runtime_before["mode"]);
}

#[tokio::test]
async fn message_stream_endpoint_should_return_sse_content_type_and_keepalive_event() {
    if !codex_live_available() {
        eprintln!("Codex credentials not available, skipping live SSE integration test");
        return;
    }

    let server = start_agent_v2_test_server().await;
    let client = reqwest::Client::new();
    create_codex_agent_definition(&client, &server, "stream-keepalive", "Stream Keepalive").await;

    let start = client
        .post(format!(
            "{}/api/v1/agents/stream-keepalive/runtime/start",
            server.base_url
        ))
        .send()
        .await
        .expect("start request should succeed");
    assert_eq!(start.status(), reqwest::StatusCode::OK);

    let session_id = get_runtime(&client, &server, "stream-keepalive")
        .await
        .get("active_session_id")
        .and_then(Value::as_str)
        .expect("runtime should expose active_session_id")
        .to_string();

    let response = client
        .post(format!(
            "{}/api/v1/agents/stream-keepalive/messages/stream",
            server.base_url
        ))
        .json(&message_request(
            &session_id,
            "Say hello in exactly four words.",
        ))
        .send()
        .await
        .expect("stream request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        content_type.starts_with("text/event-stream"),
        "expected SSE content type, got {content_type}"
    );

    let body = response
        .text()
        .await
        .expect("stream body should be readable");
    assert!(body.contains("event: keepalive"));
    assert!(!body.contains("\"accepted\":true"));
}

#[tokio::test]
async fn message_submit_should_return_message_id_and_increase_session_message_count() {
    if !codex_live_available() {
        eprintln!("Codex credentials not available, skipping live message submit test");
        return;
    }

    let server = start_agent_v2_test_server().await;
    let client = reqwest::Client::new();
    create_codex_agent_definition(&client, &server, "message-flow", "Message Flow").await;

    let start = client
        .post(format!(
            "{}/api/v1/agents/message-flow/runtime/start",
            server.base_url
        ))
        .send()
        .await
        .expect("start request should succeed");
    assert_eq!(start.status(), reqwest::StatusCode::OK);

    let runtime = get_runtime(&client, &server, "message-flow").await;
    let session_id = runtime["active_session_id"]
        .as_str()
        .expect("runtime should expose active_session_id")
        .to_string();

    let before = client
        .get(format!(
            "{}/api/v1/agents/message-flow/sessions/{}",
            server.base_url, session_id
        ))
        .send()
        .await
        .expect("session detail request should succeed");
    assert_eq!(before.status(), reqwest::StatusCode::OK);
    let before_body: Value = before.json().await.expect("session detail should be JSON");
    let before_count = before_body["message_count"]
        .as_u64()
        .expect("message_count should be numeric");

    let submit = client
        .post(format!(
            "{}/api/v1/agents/message-flow/messages",
            server.base_url
        ))
        .json(&message_request(
            &session_id,
            "Reply with exactly two words.",
        ))
        .send()
        .await
        .expect("submit request should succeed");
    assert_eq!(submit.status(), reqwest::StatusCode::ACCEPTED);
    let submit_body: Value = submit.json().await.expect("submit response should be JSON");
    assert_eq!(submit_body["accepted"], json!(true));
    assert_eq!(submit_body["session_id"], json!(session_id));
    assert!(submit_body["message_id"].is_string());

    let mut after_count = before_count;
    for _ in 0..40 {
        let after = client
            .get(format!(
                "{}/api/v1/agents/message-flow/sessions/{}",
                server.base_url, session_id
            ))
            .send()
            .await
            .expect("session detail request should succeed");
        assert_eq!(after.status(), reqwest::StatusCode::OK);
        let after_body: Value = after.json().await.expect("session detail should be JSON");
        after_count = after_body["message_count"]
            .as_u64()
            .expect("message_count should be numeric");
        if after_count > before_count {
            break;
        }

        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }

    assert!(
        after_count > before_count,
        "message count should increase after submit"
    );
}

#[tokio::test]
async fn message_stream_should_emit_delta_and_completed_events_for_live_dispatch() {
    if !codex_live_available() {
        eprintln!("Codex credentials not available, skipping live message stream test");
        return;
    }

    let server = start_agent_v2_test_server().await;
    let client = reqwest::Client::new();
    create_codex_agent_definition(&client, &server, "message-stream", "Message Stream").await;

    let start = client
        .post(format!(
            "{}/api/v1/agents/message-stream/runtime/start",
            server.base_url
        ))
        .send()
        .await
        .expect("start request should succeed");
    assert_eq!(start.status(), reqwest::StatusCode::OK);

    let runtime = get_runtime(&client, &server, "message-stream").await;
    let session_id = runtime["active_session_id"]
        .as_str()
        .expect("runtime should expose active_session_id")
        .to_string();

    let response = client
        .post(format!(
            "{}/api/v1/agents/message-stream/messages/stream",
            server.base_url
        ))
        .json(&message_request(
            &session_id,
            "Say hello in exactly three words.",
        ))
        .send()
        .await
        .expect("stream request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let body = response
        .text()
        .await
        .expect("stream body should be readable");
    assert!(body.contains("event: keepalive"));
    if body.contains("event: error") {
        if body.contains("error sending request for url") {
            eprintln!(
                "Codex live stream transport unavailable, skipping stream event assertions:\n{body}"
            );
            return;
        }
        panic!("expected delta/completed events, got error stream:\n{body}");
    }
    assert!(body.contains("event: message.delta"));
    assert!(body.contains("event: message.completed"));
}

#[tokio::test]
async fn dry_run_should_return_provider_and_model_without_dispatching() {
    let server = start_agent_v2_test_server().await;
    let client = reqwest::Client::new();
    create_agent_definition(&client, &server, "dry-run-live", "Dry Run Live").await;

    let start = client
        .post(format!(
            "{}/api/v1/agents/dry-run-live/runtime/start",
            server.base_url
        ))
        .send()
        .await
        .expect("start request should succeed");
    assert_eq!(start.status(), reqwest::StatusCode::OK);

    let runtime = get_runtime(&client, &server, "dry-run-live").await;
    let session_id = runtime["active_session_id"]
        .as_str()
        .expect("runtime should expose active_session_id")
        .to_string();

    let stop = client
        .post(format!(
            "{}/api/v1/agents/dry-run-live/runtime/stop",
            server.base_url
        ))
        .send()
        .await
        .expect("stop request should succeed");
    assert_eq!(stop.status(), reqwest::StatusCode::OK);

    let response = client
        .post(format!(
            "{}/api/v1/agents/dry-run-live/messages/dry-run",
            server.base_url
        ))
        .json(&message_request(
            &session_id,
            "Describe the dry-run resolution.",
        ))
        .send()
        .await
        .expect("dry-run request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response
        .json()
        .await
        .expect("dry-run response should be JSON");
    assert_eq!(body["would_execute"], json!(true));
    assert_eq!(body["resolved"]["provider"]["driver"], json!("claude_code"));
    assert_eq!(body["resolved"]["provider"]["model"], json!("sonnet"));
    assert_eq!(body["resolved"]["session_id"], json!(session_id));
    assert_eq!(body["effects"]["message_submit"], json!(true));
}
