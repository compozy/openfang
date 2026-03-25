//! Real HTTP integration tests for standalone artifact/doc read endpoints.

use std::sync::Arc;

use openfang_api::routes::AppState;
use openfang_api::server::build_router;
use openfang_kernel::OpenFangKernel;
use openfang_types::artifact::{
    ArtifactType, ArtifactVersionId, NewArtifact, NewArtifactVersion, ProvenanceKind, ProvenanceRef,
};
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use openfang_types::doc::{DocType, DocVersionId, NewDoc, NewDocVersion};
use openfang_types::task::{
    ActorKind, ArtifactRef, Complexity, DocRef, OwnerRef, Priority, TaskId, TaskRecord, TaskSource,
    TaskStatus,
};
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

async fn start_artifact_doc_v1_test_server() -> TestServer {
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
            .expect("artifact/doc v1 test server should stay available");
    });

    TestServer {
        base_url: format!("http://{address}"),
        state,
        _tmp: tmp,
    }
}

fn sample_task(
    task_id: &str,
    artifact_refs: Vec<ArtifactRef>,
    doc_refs: Vec<DocRef>,
) -> TaskRecord {
    TaskRecord {
        task_id: TaskId::new(task_id),
        slug: task_id.to_string(),
        title: format!("Task {task_id}"),
        description: format!("Task for {task_id}"),
        status: TaskStatus::Planned,
        priority: Priority::Medium,
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
        artifact_refs,
        doc_refs,
        file_refs: vec![],
        metadata: json!({}),
        created_at: "2026-03-25T09:59:00Z".to_string(),
        updated_at: "2026-03-25T09:59:00Z".to_string(),
        completed_at: None,
    }
}

fn insert_task(server: &TestServer, task: TaskRecord) {
    server
        .state
        .kernel
        .workflow_stores
        .task
        .create(&task)
        .expect("task should be created");
}

fn insert_artifact(
    server: &TestServer,
    artifact_id: &str,
    artifact_type: &str,
    initial_version_id: &str,
    created_at: &str,
    appended_versions: &[(&str, &str, &str)],
) -> String {
    let repo = &server.state.kernel.workflow_stores.artifact;
    let created = repo
        .create(&NewArtifact {
            artifact_id: openfang_types::artifact::ArtifactId::new(artifact_id),
            artifact_version_id: ArtifactVersionId::new(initial_version_id),
            type_name: ArtifactType::new(artifact_type),
            metadata: json!({ "origin": "workflow" }),
            content: json!({
                "title": format!("Artifact {artifact_id}"),
                "sections": ["initial"],
            }),
            provenance: Some(ProvenanceRef {
                kind: ProvenanceKind::Run,
                ref_id: "run_001".to_string(),
            }),
            created_at: created_at.to_string(),
        })
        .expect("artifact should be created");
    let artifact_id = created.artifact_id.clone();
    let mut current_version_id = created.current_version_id.to_string();

    for (version_id, section, timestamp) in appended_versions {
        let updated = repo
            .append_version(
                &artifact_id,
                &NewArtifactVersion {
                    artifact_version_id: ArtifactVersionId::new(*version_id),
                    content: json!({
                        "title": format!("Artifact {artifact_id}"),
                        "sections": [section],
                    }),
                    provenance: Some(ProvenanceRef {
                        kind: ProvenanceKind::Agent,
                        ref_id: "artifact-writer".to_string(),
                    }),
                    created_at: (*timestamp).to_string(),
                },
            )
            .expect("artifact version should be appended");
        current_version_id = updated.current_version_id.to_string();
    }

    current_version_id
}

fn insert_doc(
    server: &TestServer,
    doc_id: &str,
    doc_type: &str,
    initial_version_id: &str,
    created_at: &str,
    appended_versions: &[(&str, &str, &str)],
) -> String {
    let repo = &server.state.kernel.workflow_stores.doc;
    let created = repo
        .create(&NewDoc {
            doc_id: openfang_types::doc::DocId::new(doc_id),
            doc_version_id: DocVersionId::new(initial_version_id),
            type_name: DocType::new(doc_type),
            metadata: json!({ "origin": "workflow" }),
            content: json!({
                "summary": format!("Doc {doc_id} initial"),
            }),
            provenance: Some(ProvenanceRef {
                kind: ProvenanceKind::Agent,
                ref_id: "doc-writer".to_string(),
            }),
            created_at: created_at.to_string(),
        })
        .expect("doc should be created");
    let doc_id = created.doc_id.clone();
    let mut current_version_id = created.current_version_id.to_string();

    for (version_id, summary, timestamp) in appended_versions {
        let updated = repo
            .append_version(
                &doc_id,
                &NewDocVersion {
                    doc_version_id: DocVersionId::new(*version_id),
                    content: json!({
                        "summary": summary,
                    }),
                    provenance: Some(ProvenanceRef {
                        kind: ProvenanceKind::Dispatch,
                        ref_id: "dispatch_001".to_string(),
                    }),
                    created_at: (*timestamp).to_string(),
                },
            )
            .expect("doc version should be appended");
        current_version_id = updated.current_version_id.to_string();
    }

    current_version_id
}

fn seed_artifact_fixture(server: &TestServer) {
    let artifact_one_current = insert_artifact(
        server,
        "artifact_001",
        "prd",
        "artifact_001_v1",
        "2026-03-25T10:00:00Z",
        &[
            ("artifact_001_v2", "draft", "2026-03-25T10:01:00Z"),
            ("artifact_001_v3", "final", "2026-03-25T10:02:00Z"),
        ],
    );
    let artifact_two_current = insert_artifact(
        server,
        "artifact_002",
        "brief",
        "artifact_002_v1",
        "2026-03-25T11:00:00Z",
        &[],
    );

    insert_task(
        server,
        sample_task(
            "task_001",
            vec![ArtifactRef {
                artifact_id: "artifact_001".to_string(),
                type_name: "prd".to_string(),
                current_version_id: Some(artifact_one_current),
            }],
            vec![],
        ),
    );
    insert_task(
        server,
        sample_task(
            "task_002",
            vec![ArtifactRef {
                artifact_id: "artifact_002".to_string(),
                type_name: "brief".to_string(),
                current_version_id: Some(artifact_two_current),
            }],
            vec![],
        ),
    );
}

fn seed_doc_fixture(server: &TestServer) {
    let doc_one_current = insert_doc(
        server,
        "doc_001",
        "brief",
        "doc_001_v1",
        "2026-03-25T10:00:00Z",
        &[("doc_001_v2", "updated brief", "2026-03-25T10:03:00Z")],
    );
    let doc_two_current = insert_doc(
        server,
        "doc_002",
        "research",
        "doc_002_v1",
        "2026-03-25T11:00:00Z",
        &[],
    );

    insert_task(
        server,
        sample_task(
            "task_doc_001",
            vec![],
            vec![DocRef {
                doc_id: "doc_001".to_string(),
                type_name: "brief".to_string(),
                current_version_id: Some(doc_one_current),
            }],
        ),
    );
    insert_task(
        server,
        sample_task(
            "task_doc_002",
            vec![],
            vec![DocRef {
                doc_id: "doc_002".to_string(),
                type_name: "research".to_string(),
                current_version_id: Some(doc_two_current),
            }],
        ),
    );
}

#[tokio::test]
async fn artifacts_list_should_return_items_and_next_cursor_shape() {
    let server = start_artifact_doc_v1_test_server().await;
    seed_artifact_fixture(&server);
    let client = reqwest::Client::new();

    let response = client
        .get(format!("{}/api/v1/artifacts?limit=1", server.base_url))
        .send()
        .await
        .expect("artifact list request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body = response
        .json::<Value>()
        .await
        .expect("artifact list response should deserialize");

    assert_eq!(body["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(body["items"][0]["id"], json!("artifact_002"));
    assert_eq!(body["items"][0]["task_id"], json!("task_002"));
    assert!(body["next_cursor"].is_string());
}

#[tokio::test]
async fn artifact_detail_should_inline_current_version_and_unknown_should_404() {
    let server = start_artifact_doc_v1_test_server().await;
    seed_artifact_fixture(&server);
    let client = reqwest::Client::new();

    let detail = client
        .get(format!("{}/api/v1/artifacts/artifact_001", server.base_url))
        .send()
        .await
        .expect("artifact detail request should succeed");
    assert_eq!(detail.status(), reqwest::StatusCode::OK);
    let detail = detail
        .json::<Value>()
        .await
        .expect("artifact detail response should deserialize");
    assert_eq!(detail["id"], json!("artifact_001"));
    assert_eq!(detail["task_id"], json!("task_001"));
    assert_eq!(detail["current_version"]["id"], json!("artifact_001_v3"));
    assert_eq!(detail["current_version"]["version_number"], json!(3));

    let missing = client
        .get(format!("{}/api/v1/artifacts/missing", server.base_url))
        .send()
        .await
        .expect("missing artifact request should succeed");
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);
    let missing = missing
        .json::<Value>()
        .await
        .expect("missing artifact response should deserialize");
    assert_eq!(missing["error"]["code"], json!("not_found"));
    assert_eq!(
        missing["error"]["details"][0]["artifact_id"],
        json!("missing")
    );
}

#[tokio::test]
async fn artifact_versions_should_paginate_descend_and_allow_empty_page() {
    let server = start_artifact_doc_v1_test_server().await;
    seed_artifact_fixture(&server);
    let client = reqwest::Client::new();

    let versions = client
        .get(format!(
            "{}/api/v1/artifacts/artifact_001/versions?limit=2",
            server.base_url
        ))
        .send()
        .await
        .expect("artifact versions request should succeed");
    assert_eq!(versions.status(), reqwest::StatusCode::OK);
    let versions = versions
        .json::<Value>()
        .await
        .expect("artifact versions response should deserialize");
    assert_eq!(
        versions["items"]
            .as_array()
            .expect("artifact version items should be an array")
            .iter()
            .map(|item| item["id"]
                .as_str()
                .expect("artifact version id should be a string"))
            .collect::<Vec<_>>(),
        vec!["artifact_001_v3", "artifact_001_v2"]
    );
    assert!(versions["next_cursor"].is_string());

    let empty_page_cursor = serde_json::to_string(
        &json!({"version_number": 1, "artifact_version_id": "artifact_002_v1"}),
    )
    .expect("cursor json should serialize");
    let empty_page = client
        .get(format!(
            "{}/api/v1/artifacts/artifact_002/versions",
            server.base_url
        ))
        .query(&[("cursor", empty_page_cursor)])
        .send()
        .await
        .expect("artifact empty page request should succeed");
    assert_eq!(empty_page.status(), reqwest::StatusCode::OK);
    let empty_page = empty_page
        .json::<Value>()
        .await
        .expect("artifact empty page should deserialize");
    assert_eq!(empty_page["items"], json!([]));
    assert_eq!(empty_page["next_cursor"], Value::Null);
}

#[tokio::test]
async fn artifact_filters_should_apply_type_and_task_id() {
    let server = start_artifact_doc_v1_test_server().await;
    seed_artifact_fixture(&server);
    let client = reqwest::Client::new();

    let by_type = client
        .get(format!(
            "{}/api/v1/artifacts?artifact_type=prd",
            server.base_url
        ))
        .send()
        .await
        .expect("artifact type filter should succeed");
    assert_eq!(by_type.status(), reqwest::StatusCode::OK);
    let by_type = by_type
        .json::<Value>()
        .await
        .expect("artifact type filter response should deserialize");
    assert_eq!(by_type["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(by_type["items"][0]["id"], json!("artifact_001"));

    let by_task = client
        .get(format!(
            "{}/api/v1/artifacts?task_id=task_001",
            server.base_url
        ))
        .send()
        .await
        .expect("artifact task filter should succeed");
    assert_eq!(by_task.status(), reqwest::StatusCode::OK);
    let by_task = by_task
        .json::<Value>()
        .await
        .expect("artifact task filter response should deserialize");
    assert_eq!(by_task["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(by_task["items"][0]["id"], json!("artifact_001"));

    let by_query = client
        .get(format!("{}/api/v1/artifacts?q=task_001", server.base_url))
        .send()
        .await
        .expect("artifact search filter should succeed");
    assert_eq!(by_query.status(), reqwest::StatusCode::OK);
    let by_query = by_query
        .json::<Value>()
        .await
        .expect("artifact search response should deserialize");
    assert_eq!(by_query["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(by_query["items"][0]["id"], json!("artifact_001"));
}

#[tokio::test]
async fn docs_endpoints_should_list_detail_filter_and_404() {
    let server = start_artifact_doc_v1_test_server().await;
    seed_doc_fixture(&server);
    let client = reqwest::Client::new();

    let list = client
        .get(format!("{}/api/v1/docs", server.base_url))
        .send()
        .await
        .expect("doc list request should succeed");
    assert_eq!(list.status(), reqwest::StatusCode::OK);
    let list = list
        .json::<Value>()
        .await
        .expect("doc list response should deserialize");
    assert_eq!(list["items"].as_array().map(Vec::len), Some(2));
    assert_eq!(list["items"][0]["id"], json!("doc_002"));

    let detail = client
        .get(format!("{}/api/v1/docs/doc_001", server.base_url))
        .send()
        .await
        .expect("doc detail request should succeed");
    assert_eq!(detail.status(), reqwest::StatusCode::OK);
    let detail = detail
        .json::<Value>()
        .await
        .expect("doc detail response should deserialize");
    assert_eq!(detail["task_id"], json!("task_doc_001"));
    assert_eq!(detail["current_version"]["id"], json!("doc_001_v2"));

    let versions = client
        .get(format!("{}/api/v1/docs/doc_001/versions", server.base_url))
        .send()
        .await
        .expect("doc versions request should succeed");
    assert_eq!(versions.status(), reqwest::StatusCode::OK);
    let versions = versions
        .json::<Value>()
        .await
        .expect("doc versions response should deserialize");
    assert_eq!(versions["items"][0]["id"], json!("doc_001_v2"));

    let filtered = client
        .get(format!("{}/api/v1/docs?doc_type=brief", server.base_url))
        .send()
        .await
        .expect("doc filter request should succeed");
    assert_eq!(filtered.status(), reqwest::StatusCode::OK);
    let filtered = filtered
        .json::<Value>()
        .await
        .expect("doc filter response should deserialize");
    assert_eq!(filtered["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(filtered["items"][0]["id"], json!("doc_001"));

    let by_task = client
        .get(format!(
            "{}/api/v1/docs?task_id=task_doc_001",
            server.base_url
        ))
        .send()
        .await
        .expect("doc task filter request should succeed");
    assert_eq!(by_task.status(), reqwest::StatusCode::OK);
    let by_task = by_task
        .json::<Value>()
        .await
        .expect("doc task filter response should deserialize");
    assert_eq!(by_task["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(by_task["items"][0]["id"], json!("doc_001"));

    let by_query = client
        .get(format!("{}/api/v1/docs?q=task_doc_001", server.base_url))
        .send()
        .await
        .expect("doc search filter request should succeed");
    assert_eq!(by_query.status(), reqwest::StatusCode::OK);
    let by_query = by_query
        .json::<Value>()
        .await
        .expect("doc search response should deserialize");
    assert_eq!(by_query["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(by_query["items"][0]["id"], json!("doc_001"));

    let missing = client
        .get(format!("{}/api/v1/docs/missing", server.base_url))
        .send()
        .await
        .expect("missing doc request should succeed");
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn artifact_and_doc_routes_should_be_read_only() {
    let server = start_artifact_doc_v1_test_server().await;
    seed_artifact_fixture(&server);
    seed_doc_fixture(&server);
    let client = reqwest::Client::new();
    let endpoints = [
        format!("{}/api/v1/artifacts", server.base_url),
        format!("{}/api/v1/artifacts/artifact_001", server.base_url),
        format!("{}/api/v1/artifacts/artifact_001/versions", server.base_url),
        format!("{}/api/v1/docs", server.base_url),
        format!("{}/api/v1/docs/doc_001", server.base_url),
        format!("{}/api/v1/docs/doc_001/versions", server.base_url),
    ];

    for method in [
        reqwest::Method::POST,
        reqwest::Method::PUT,
        reqwest::Method::DELETE,
    ] {
        for endpoint in &endpoints {
            let response = client
                .request(method.clone(), endpoint.clone())
                .send()
                .await
                .expect("method guard request should succeed");
            assert_eq!(response.status(), reqwest::StatusCode::METHOD_NOT_ALLOWED);
        }
    }
}
