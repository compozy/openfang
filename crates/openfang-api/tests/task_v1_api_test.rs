//! Real HTTP integration tests for the task/subtask control-plane API.

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

async fn start_task_v1_test_server() -> TestServer {
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
        .expect("listener should expose a local address");
    let (app, state) = build_router(kernel, address).await;

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("task v1 test server should stay available");
    });

    TestServer {
        base_url: format!("http://{address}"),
        state,
        _tmp: tmp,
    }
}

fn task_request(id: &str, slug: &str, title: &str, status: &str, description: &str) -> Value {
    json!({
        "id": id,
        "slug": slug,
        "title": title,
        "description": description,
        "status": status,
        "priority": "high",
        "complexity": "medium",
        "position": 1,
        "source": { "kind": "manual" },
        "owner": { "kind": "agent_group", "ref": "sdlc" },
        "created_by": { "kind": "agent", "ref": "planner" },
        "repository_refs": [{ "repository_id": "repo_main", "role": "primary" }],
        "label_refs": ["planning", "prd"],
        "artifact_refs": [{ "artifact_id": "artifact_001", "type": "prd", "current_version_id": "artifact_v3" }],
        "doc_refs": [{ "doc_id": "doc_001", "type": "brief", "current_version_id": "doc_v2" }],
        "file_refs": [{ "path": "docs/prd.md", "kind": "workspace", "description": "Current PRD draft" }],
        "metadata": { "area": "product" }
    })
}

fn subtask_request(
    id: &str,
    title: &str,
    position: i64,
    assignee_ref: &str,
    depends_on: Vec<&str>,
) -> Value {
    json!({
        "id": id,
        "title": title,
        "description": format!("Description for {title}"),
        "kind": "doc_change",
        "status": "planned",
        "complexity": "medium",
        "position": position,
        "assignee": { "kind": "agent", "ref": assignee_ref },
        "depends_on": depends_on,
        "parallelizable": false,
        "input": { "artifact_id": "artifact_001" },
        "metadata": {}
    })
}

struct NewSubtask<'a> {
    task_id: &'a str,
    id: &'a str,
    title: &'a str,
    position: i64,
    assignee_ref: &'a str,
    depends_on: Vec<&'a str>,
}

async fn create_task(
    client: &reqwest::Client,
    server: &TestServer,
    id: &str,
    slug: &str,
    title: &str,
    status: &str,
    description: &str,
) -> Value {
    let response = client
        .post(format!("{}/api/v1/tasks", server.base_url))
        .json(&task_request(id, slug, title, status, description))
        .send()
        .await
        .expect("task create request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    response
        .json::<Value>()
        .await
        .expect("task create response should deserialize")
}

async fn create_subtask(
    client: &reqwest::Client,
    server: &TestServer,
    request: NewSubtask<'_>,
) -> Value {
    let response = client
        .post(format!(
            "{}/api/v1/tasks/{}/subtasks",
            server.base_url, request.task_id
        ))
        .json(&subtask_request(
            request.id,
            request.title,
            request.position,
            request.assignee_ref,
            request.depends_on,
        ))
        .send()
        .await
        .expect("subtask create request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    response
        .json::<Value>()
        .await
        .expect("subtask create response should deserialize")
}

#[tokio::test]
async fn task_crud_round_trip_should_work() {
    let server = start_task_v1_test_server().await;
    let client = reqwest::Client::new();

    let created = create_task(
        &client,
        &server,
        "task_001",
        "onboarding-revamp-prd",
        "Prepare PRD for onboarding revamp",
        "planned",
        "Define the new onboarding flow and acceptance criteria",
    )
    .await;
    assert_eq!(created["id"], json!("task_001"));
    assert_eq!(
        created["artifact_refs"][0]["artifact_id"],
        json!("artifact_001")
    );

    let fetched = client
        .get(format!("{}/api/v1/tasks/task_001", server.base_url))
        .send()
        .await
        .expect("task get request should succeed");
    assert_eq!(fetched.status(), reqwest::StatusCode::OK);
    let fetched = fetched
        .json::<Value>()
        .await
        .expect("task get response should deserialize");
    assert_eq!(fetched["slug"], json!("onboarding-revamp-prd"));
    assert_eq!(fetched["doc_refs"][0]["doc_id"], json!("doc_001"));

    let updated = client
        .put(format!("{}/api/v1/tasks/task_001", server.base_url))
        .json(&json!({
            "priority": "critical"
        }))
        .send()
        .await
        .expect("task update request should succeed");
    assert_eq!(updated.status(), reqwest::StatusCode::OK);
    let updated = updated
        .json::<Value>()
        .await
        .expect("task update response should deserialize");
    assert_eq!(updated["priority"], json!("critical"));

    let deleted = client
        .delete(format!("{}/api/v1/tasks/task_001", server.base_url))
        .send()
        .await
        .expect("task delete request should succeed");
    assert_eq!(deleted.status(), reqwest::StatusCode::NO_CONTENT);

    let missing = client
        .get(format!("{}/api/v1/tasks/task_001", server.base_url))
        .send()
        .await
        .expect("task missing request should succeed");
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn replan_should_update_subtask_plan_and_store_agent_metadata() {
    let server = start_task_v1_test_server().await;
    let client = reqwest::Client::new();

    let created = create_task(
        &client,
        &server,
        "task_replan",
        "task-replan",
        "Task Replan",
        "in_progress",
        "Exercise the replan endpoint",
    )
    .await;
    let original_slug = created["slug"].clone();

    create_subtask(
        &client,
        &server,
        NewSubtask {
            task_id: "task_replan",
            id: "subtask_001",
            title: "Draft problem statement",
            position: 1,
            assignee_ref: "prd-writer",
            depends_on: vec![],
        },
    )
    .await;
    create_subtask(
        &client,
        &server,
        NewSubtask {
            task_id: "task_replan",
            id: "subtask_002",
            title: "Review the draft",
            position: 2,
            assignee_ref: "reviewer",
            depends_on: vec!["subtask_001"],
        },
    )
    .await;

    let response = client
        .post(format!(
            "{}/api/v1/tasks/task_replan/replan",
            server.base_url
        ))
        .json(&json!({
            "reason": "Split the work into smaller review-driven subtasks",
            "operations": [
                { "op": "cancel_subtasks", "subtask_ids": ["subtask_002"] },
                {
                    "op": "create_subtasks",
                    "items": [{
                        "id": "subtask_003",
                        "title": "Address review comments",
                        "description": "Resolve review feedback",
                        "kind": "review_item",
                        "status": "ready",
                        "complexity": "medium",
                        "position": 3,
                        "assignee": { "kind": "agent", "ref": "prd-writer" },
                        "depends_on": ["subtask_001"],
                        "parallelizable": true,
                        "input": {},
                        "metadata": {}
                    }]
                }
            ],
            "metadata": {
                "source": "agent"
            }
        }))
        .send()
        .await
        .expect("replan request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body = response
        .json::<Value>()
        .await
        .expect("replan response should deserialize");
    assert_eq!(body["accepted"], json!(true));
    assert_eq!(body["effects"]["created_subtasks"], json!(1));
    assert_eq!(body["effects"]["cancelled_subtasks"], json!(1));

    let task = client
        .get(format!("{}/api/v1/tasks/task_replan", server.base_url))
        .send()
        .await
        .expect("task get after replan should succeed");
    assert_eq!(task.status(), reqwest::StatusCode::OK);
    let task = task
        .json::<Value>()
        .await
        .expect("task response should deserialize");
    assert_eq!(task["id"], json!("task_replan"));
    assert_eq!(task["slug"], original_slug);
    assert_eq!(
        task["metadata"]["last_replan"]["metadata"]["source"],
        json!("agent")
    );

    let subtasks = client
        .get(format!(
            "{}/api/v1/tasks/task_replan/subtasks",
            server.base_url
        ))
        .send()
        .await
        .expect("subtask list should succeed");
    assert_eq!(subtasks.status(), reqwest::StatusCode::OK);
    let subtasks = subtasks
        .json::<Value>()
        .await
        .expect("subtask list should deserialize");
    let items = subtasks["items"]
        .as_array()
        .expect("subtask items should be an array");
    assert_eq!(items.len(), 3);
    assert!(items.iter().any(|item| item["id"] == json!("subtask_003")));
}

#[tokio::test]
async fn scoped_subtask_listing_and_global_assignee_filter_should_work() {
    let server = start_task_v1_test_server().await;
    let client = reqwest::Client::new();

    create_task(
        &client,
        &server,
        "task_alpha",
        "task-alpha",
        "Task Alpha",
        "planned",
        "First task",
    )
    .await;
    create_task(
        &client,
        &server,
        "task_beta",
        "task-beta",
        "Task Beta",
        "planned",
        "Second task",
    )
    .await;

    create_subtask(
        &client,
        &server,
        NewSubtask {
            task_id: "task_alpha",
            id: "subtask_alpha",
            title: "Alpha work",
            position: 1,
            assignee_ref: "prd-writer",
            depends_on: vec![],
        },
    )
    .await;
    create_subtask(
        &client,
        &server,
        NewSubtask {
            task_id: "task_beta",
            id: "subtask_beta",
            title: "Beta work",
            position: 1,
            assignee_ref: "other-agent",
            depends_on: vec![],
        },
    )
    .await;

    let scoped = client
        .get(format!(
            "{}/api/v1/tasks/task_alpha/subtasks",
            server.base_url
        ))
        .send()
        .await
        .expect("scoped subtask list should succeed");
    assert_eq!(scoped.status(), reqwest::StatusCode::OK);
    let scoped = scoped
        .json::<Value>()
        .await
        .expect("scoped subtask list should deserialize");
    let items = scoped["items"]
        .as_array()
        .expect("scoped items should be an array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["task_id"], json!("task_alpha"));

    let filtered = client
        .get(format!(
            "{}/api/v1/subtasks?assignee_ref=prd-writer",
            server.base_url
        ))
        .send()
        .await
        .expect("global subtask filter should succeed");
    assert_eq!(filtered.status(), reqwest::StatusCode::OK);
    let filtered = filtered
        .json::<Value>()
        .await
        .expect("filtered subtask list should deserialize");
    let items = filtered["items"]
        .as_array()
        .expect("filtered items should be an array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], json!("subtask_alpha"));
}

#[tokio::test]
async fn linked_context_and_task_filters_should_work() {
    let server = start_task_v1_test_server().await;
    let client = reqwest::Client::new();

    create_task(
        &client,
        &server,
        "task_search_1",
        "onboarding-prd",
        "Prepare onboarding PRD",
        "in_progress",
        "Onboarding redesign planning work",
    )
    .await;
    create_task(
        &client,
        &server,
        "task_search_2",
        "release-note",
        "Release Note",
        "planned",
        "Summarize the release",
    )
    .await;

    let artifacts = client
        .get(format!(
            "{}/api/v1/tasks/task_search_1/artifacts",
            server.base_url
        ))
        .send()
        .await
        .expect("artifact projection should succeed");
    assert_eq!(artifacts.status(), reqwest::StatusCode::OK);
    let artifacts = artifacts
        .json::<Value>()
        .await
        .expect("artifact response should deserialize");
    assert_eq!(artifacts["items"][0]["artifact_id"], json!("artifact_001"));
    assert_eq!(artifacts["next_cursor"], Value::Null);

    let docs = client
        .get(format!(
            "{}/api/v1/tasks/task_search_1/docs",
            server.base_url
        ))
        .send()
        .await
        .expect("doc projection should succeed");
    assert_eq!(docs.status(), reqwest::StatusCode::OK);
    let docs = docs
        .json::<Value>()
        .await
        .expect("doc response should deserialize");
    assert_eq!(docs["items"][0]["doc_id"], json!("doc_001"));

    let files = client
        .get(format!(
            "{}/api/v1/tasks/task_search_1/files",
            server.base_url
        ))
        .send()
        .await
        .expect("file projection should succeed");
    assert_eq!(files.status(), reqwest::StatusCode::OK);
    let files = files
        .json::<Value>()
        .await
        .expect("file response should deserialize");
    assert_eq!(files["items"][0]["path"], json!("docs/prd.md"));

    let status_filtered = client
        .get(format!(
            "{}/api/v1/tasks?status=in_progress",
            server.base_url
        ))
        .send()
        .await
        .expect("status filter should succeed");
    assert_eq!(status_filtered.status(), reqwest::StatusCode::OK);
    let status_filtered = status_filtered
        .json::<Value>()
        .await
        .expect("status filtered response should deserialize");
    let items = status_filtered["items"]
        .as_array()
        .expect("status filtered items should be an array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], json!("task_search_1"));

    let search_filtered = client
        .get(format!("{}/api/v1/tasks?q=onboarding", server.base_url))
        .send()
        .await
        .expect("search filter should succeed");
    assert_eq!(search_filtered.status(), reqwest::StatusCode::OK);
    let search_filtered = search_filtered
        .json::<Value>()
        .await
        .expect("search filtered response should deserialize");
    let items = search_filtered["items"]
        .as_array()
        .expect("search filtered items should be an array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["slug"], json!("onboarding-prd"));
}
