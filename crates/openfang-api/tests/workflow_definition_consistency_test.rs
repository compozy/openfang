//! Workflow definition consistency regressions for the v1 control-plane.

use openfang_api::routes::AppState;
use openfang_api::server::build_router;
use openfang_api::types::{WorkflowOrigin, WorkflowResponse};
use openfang_kernel::OpenFangKernel;
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use openfang_types::workflow::WorkflowV2Definition;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct WorkflowTestServer {
    base_url: String,
    state: Arc<AppState>,
    _temp_dir: tempfile::TempDir,
    home_dir: PathBuf,
}

impl Drop for WorkflowTestServer {
    fn drop(&mut self) {
        self.state.kernel.shutdown();
    }
}

impl WorkflowTestServer {
    fn workflow_path(&self, workflow_id: &str) -> PathBuf {
        self.home_dir
            .join("workflows")
            .join(format!("{workflow_id}.toml"))
    }
}

async fn start_workflow_test_server(home_dir: PathBuf) -> WorkflowTestServer {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let config = KernelConfig {
        home_dir: home_dir.clone(),
        data_dir: temp_dir.path().join("data"),
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

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let address = listener.local_addr().expect("address should resolve");
    let (app, state) = build_router(kernel, address).await;

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server should run");
    });

    WorkflowTestServer {
        base_url: format!("http://{address}"),
        state,
        _temp_dir: temp_dir,
        home_dir,
    }
}

fn workflow_definition(id: &str, description: &str) -> WorkflowV2Definition {
    serde_json::from_value(json!({
        "id": id,
        "name": format!("Workflow {id}"),
        "version": "1.0.0",
        "description": description,
        "enabled": true,
        "tags": ["consistency"],
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
    .expect("workflow definition should deserialize")
}

fn workflow_resource(id: &str, description: &str) -> WorkflowResponse {
    WorkflowResponse {
        definition: workflow_definition(id, description),
        origin: WorkflowOrigin::user(),
        forked_from: None,
        created_at: "2026-03-24T00:00:00Z".to_string(),
        updated_at: "2026-03-24T00:00:00Z".to_string(),
    }
}

fn persist_workflow_resource(home_dir: &Path, resource: &WorkflowResponse) {
    let workflows_dir = home_dir.join("workflows");
    std::fs::create_dir_all(&workflows_dir).expect("workflow dir should be created");
    std::fs::write(
        workflows_dir.join(format!("{}.toml", resource.definition.id)),
        toml::to_string_pretty(resource).expect("workflow resource should serialize"),
    )
    .expect("workflow resource should be written");
}

fn load_workflow_resource(path: &Path) -> WorkflowResponse {
    let content = std::fs::read_to_string(path).expect("workflow file should exist");
    toml::from_str(&content).expect("workflow resource should deserialize")
}

async fn post_workflow(
    client: &reqwest::Client,
    server: &WorkflowTestServer,
    body: Value,
) -> (reqwest::StatusCode, Value) {
    let response = client
        .post(format!("{}/api/v1/workflows", server.base_url))
        .json(&body)
        .send()
        .await
        .expect("create workflow request should succeed");
    let status = response.status();
    let body = response
        .json::<Value>()
        .await
        .expect("workflow response should deserialize");
    (status, body)
}

async fn put_workflow(
    client: &reqwest::Client,
    server: &WorkflowTestServer,
    workflow_id: &str,
    body: Value,
) -> (reqwest::StatusCode, Value) {
    let response = client
        .put(format!(
            "{}/api/v1/workflows/{workflow_id}",
            server.base_url
        ))
        .json(&body)
        .send()
        .await
        .expect("update workflow request should succeed");
    let status = response.status();
    let body = response
        .json::<Value>()
        .await
        .expect("workflow response should deserialize");
    (status, body)
}

async fn delete_workflow(
    client: &reqwest::Client,
    server: &WorkflowTestServer,
    workflow_id: &str,
) -> reqwest::StatusCode {
    client
        .delete(format!(
            "{}/api/v1/workflows/{workflow_id}",
            server.base_url
        ))
        .send()
        .await
        .expect("delete workflow request should succeed")
        .status()
}

#[tokio::test]
async fn preloaded_definitions_are_visible_before_first_request() {
    let home = tempfile::tempdir().expect("temp dir should be created");
    persist_workflow_resource(
        home.path(),
        &workflow_resource("preloaded", "on disk before boot"),
    );

    let server = start_workflow_test_server(home.path().to_path_buf()).await;
    let client = reqwest::Client::new();

    let list_response = client
        .get(format!("{}/api/v1/workflows", server.base_url))
        .send()
        .await
        .expect("list workflow request should succeed");
    assert_eq!(list_response.status(), reqwest::StatusCode::OK);
    let list_body = list_response
        .json::<Value>()
        .await
        .expect("workflow list should deserialize");
    assert_eq!(list_body["items"][0]["id"], "preloaded");

    let compiled_response = client
        .get(format!(
            "{}/api/v1/workflows/preloaded/compiled",
            server.base_url
        ))
        .send()
        .await
        .expect("compiled workflow request should succeed");
    assert_eq!(compiled_response.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn create_returns_internal_server_error_when_definition_persist_fails() {
    let home = tempfile::tempdir().expect("temp dir should be created");
    let server = start_workflow_test_server(home.path().to_path_buf()).await;
    let client = reqwest::Client::new();
    let blocked_workflows_path = home.path().join("workflows");
    std::fs::write(&blocked_workflows_path, "not a directory")
        .expect("workflow path blocker should exist");

    let (status, body) = post_workflow(
        &client,
        &server,
        serde_json::to_value(workflow_definition("broken-create", "should fail"))
            .expect("workflow definition should serialize"),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"]["code"], "definition_load_failed");
}

#[cfg(unix)]
#[tokio::test]
async fn update_failure_keeps_previous_definition_on_disk() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().expect("temp dir should be created");
    let server = start_workflow_test_server(home.path().to_path_buf()).await;
    let client = reqwest::Client::new();

    let workflow_id = "atomic-update";
    let (create_status, _) = post_workflow(
        &client,
        &server,
        serde_json::to_value(workflow_definition(workflow_id, "before failure"))
            .expect("workflow definition should serialize"),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);

    std::fs::set_permissions(
        server.home_dir.join("workflows"),
        std::fs::Permissions::from_mode(0o555),
    )
    .expect("workflow directory should become read-only");

    let (update_status, body) = put_workflow(
        &client,
        &server,
        workflow_id,
        serde_json::to_value(workflow_definition(workflow_id, "after failure"))
            .expect("workflow definition should serialize"),
    )
    .await;

    assert_eq!(update_status, reqwest::StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"]["code"], "definition_persist_failed");

    let persisted = load_workflow_resource(&server.workflow_path(workflow_id));
    assert_eq!(persisted.definition.description, "before failure");
}

#[tokio::test]
async fn delete_removes_definition_without_resurrection() {
    let home = tempfile::tempdir().expect("temp dir should be created");
    let server = start_workflow_test_server(home.path().to_path_buf()).await;
    let client = reqwest::Client::new();

    let workflow_id = "delete-me";
    let (create_status, _) = post_workflow(
        &client,
        &server,
        serde_json::to_value(workflow_definition(workflow_id, "delete test"))
            .expect("workflow definition should serialize"),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);

    let delete_status = delete_workflow(&client, &server, workflow_id).await;
    assert_eq!(delete_status, reqwest::StatusCode::NO_CONTENT);
    assert!(!server.workflow_path(workflow_id).exists());

    let get_response = client
        .get(format!(
            "{}/api/v1/workflows/{workflow_id}",
            server.base_url
        ))
        .send()
        .await
        .expect("get workflow request should succeed");
    assert_eq!(get_response.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn route_mutations_keep_compiled_and_runtime_views_aligned() {
    let home = tempfile::tempdir().expect("temp dir should be created");
    let server = start_workflow_test_server(home.path().to_path_buf()).await;
    let client = reqwest::Client::new();

    let workflow_id = "aligned-cache";
    let (create_status, _) = post_workflow(
        &client,
        &server,
        serde_json::to_value(workflow_definition(workflow_id, "initial description"))
            .expect("workflow definition should serialize"),
    )
    .await;
    assert_eq!(create_status, reqwest::StatusCode::CREATED);

    let runtime_response = client
        .get(format!(
            "{}/api/v1/workflows/{workflow_id}/runtime",
            server.base_url
        ))
        .send()
        .await
        .expect("workflow runtime request should succeed");
    assert_eq!(runtime_response.status(), reqwest::StatusCode::OK);
    let runtime_body = runtime_response
        .json::<Value>()
        .await
        .expect("runtime response should deserialize");
    assert_eq!(runtime_body["loaded"], true);
    assert_eq!(runtime_body["healthy"], true);

    let compiled_response = client
        .get(format!(
            "{}/api/v1/workflows/{workflow_id}/compiled",
            server.base_url
        ))
        .send()
        .await
        .expect("compiled workflow request should succeed");
    assert_eq!(compiled_response.status(), reqwest::StatusCode::OK);

    let (update_status, update_body) = put_workflow(
        &client,
        &server,
        workflow_id,
        serde_json::to_value(workflow_definition(workflow_id, "updated description"))
            .expect("workflow definition should serialize"),
    )
    .await;
    assert_eq!(update_status, reqwest::StatusCode::OK);
    assert_eq!(update_body["description"], "updated description");

    let persisted = load_workflow_resource(&server.workflow_path(workflow_id));
    assert_eq!(persisted.definition.description, "updated description");

    let delete_status = delete_workflow(&client, &server, workflow_id).await;
    assert_eq!(delete_status, reqwest::StatusCode::NO_CONTENT);

    let runtime_response = client
        .get(format!(
            "{}/api/v1/workflows/{workflow_id}/runtime",
            server.base_url
        ))
        .send()
        .await
        .expect("workflow runtime request should succeed");
    assert_eq!(runtime_response.status(), reqwest::StatusCode::NOT_FOUND);
}
