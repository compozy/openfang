//! Real HTTP integration tests for the skills v1 API surface.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use openfang_api::routes::AppState;
use openfang_api::server::build_router;
use openfang_kernel::OpenFangKernel;
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use reqwest::StatusCode;
use serde_json::Value;

struct TestServer {
    base_url: String,
    state: Arc<AppState>,
    home_dir: PathBuf,
    _tmp: tempfile::TempDir,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.state.kernel.shutdown();
    }
}

fn create_test_skill(home_dir: &Path, name: &str, description: &str) {
    let skill_dir = home_dir.join("skills").join(name);
    std::fs::create_dir_all(&skill_dir).expect("skill directory should be created");
    std::fs::write(
        skill_dir.join("skill.toml"),
        format!(
            r#"
[skill]
name = "{name}"
version = "0.1.0"
description = "{description}"

[runtime]
type = "python"
entry = "main.py"
"#
        ),
    )
    .expect("skill manifest should be written");
}

async fn start_skill_v1_test_server(skills: &[(&str, &str)]) -> TestServer {
    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    for (name, description) in skills {
        create_test_skill(tmp.path(), name, description);
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

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener should bind");
    let address = listener
        .local_addr()
        .expect("listener should expose local address");
    let (app, state) = build_router(kernel, address).await;

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("test server should stay available");
    });

    TestServer {
        base_url: format!("http://{address}"),
        state,
        home_dir: tmp.path().to_path_buf(),
        _tmp: tmp,
    }
}

async fn get_json(
    client: &reqwest::Client,
    server: &TestServer,
    path: &str,
) -> (StatusCode, Value) {
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

#[tokio::test]
async fn list_skills_should_return_items_and_next_cursor_shape() {
    let server = start_skill_v1_test_server(&[
        ("writing", "Structured document authoring"),
        ("testing", "Regression and test planning"),
    ])
    .await;
    let client = reqwest::Client::new();

    let (status, body) = get_json(&client, &server, "/api/v1/skills").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().map(Vec::len), Some(2));
    assert!(body["next_cursor"].is_null());
}

#[tokio::test]
async fn get_skill_should_return_full_detail_for_valid_id() {
    let server = start_skill_v1_test_server(&[("writing", "Structured document authoring")]).await;
    let client = reqwest::Client::new();

    let (status, body) = get_json(&client, &server, "/api/v1/skills/writing").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], Value::String("writing".to_string()));
    assert_eq!(body["name"], Value::String("writing".to_string()));
    assert_eq!(
        body["source"],
        Value::String(
            server
                .home_dir
                .join("skills")
                .join("writing")
                .join("skill.toml")
                .display()
                .to_string()
        )
    );
    assert!(body["created_at"].is_string());
    assert!(body["updated_at"].is_string());
}

#[tokio::test]
async fn get_skill_should_return_not_found_envelope_for_unknown_id() {
    let server = start_skill_v1_test_server(&[("writing", "Structured document authoring")]).await;
    let client = reqwest::Client::new();

    let (status, body) = get_json(&client, &server, "/api/v1/skills/unknown-skill").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        body["error"]["code"],
        Value::String("not_found".to_string())
    );
    assert_eq!(
        body["error"]["message"],
        Value::String("skill 'unknown-skill' not found".to_string())
    );
}

#[tokio::test]
async fn list_skills_should_filter_case_insensitively_by_query() {
    let server = start_skill_v1_test_server(&[
        ("writing", "Structured document authoring"),
        ("reviewing", "Writing feedback and editing"),
        ("testing", "Regression and verification"),
    ])
    .await;
    let client = reqwest::Client::new();

    let (status, body) = get_json(&client, &server, "/api/v1/skills?q=WRITING").await;

    assert_eq!(status, StatusCode::OK);
    let items = body["items"]
        .as_array()
        .expect("items should be an array")
        .iter()
        .map(|item| item["id"].as_str().expect("skill id should be present"))
        .collect::<Vec<_>>();
    assert_eq!(items, vec!["reviewing", "writing"]);
}

#[tokio::test]
async fn list_skills_should_paginate_without_duplicates_or_omissions() {
    let server = start_skill_v1_test_server(&[
        ("alpha", "Skill alpha"),
        ("beta", "Skill beta"),
        ("gamma", "Skill gamma"),
        ("delta", "Skill delta"),
        ("epsilon", "Skill epsilon"),
    ])
    .await;
    let client = reqwest::Client::new();

    let (first_status, first_body) = get_json(&client, &server, "/api/v1/skills?limit=2").await;
    assert_eq!(first_status, StatusCode::OK);
    let first_cursor = first_body["next_cursor"]
        .as_str()
        .expect("first page should expose next cursor")
        .to_string();

    let (second_status, second_body) = get_json(
        &client,
        &server,
        &format!("/api/v1/skills?limit=2&cursor={first_cursor}"),
    )
    .await;
    assert_eq!(second_status, StatusCode::OK);
    let second_cursor = second_body["next_cursor"]
        .as_str()
        .expect("second page should expose next cursor")
        .to_string();

    let (final_status, final_body) = get_json(
        &client,
        &server,
        &format!("/api/v1/skills?limit=2&cursor={second_cursor}"),
    )
    .await;
    assert_eq!(final_status, StatusCode::OK);
    assert!(final_body["next_cursor"].is_null());

    let collected = [first_body, second_body, final_body]
        .into_iter()
        .flat_map(|body| {
            body["items"]
                .as_array()
                .expect("items should be an array")
                .iter()
                .map(|item| {
                    item["id"]
                        .as_str()
                        .expect("skill id should be present")
                        .to_string()
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(
        collected,
        vec![
            "alpha".to_string(),
            "beta".to_string(),
            "delta".to_string(),
            "epsilon".to_string(),
            "gamma".to_string(),
        ]
    );
}

#[tokio::test]
async fn skills_routes_should_be_read_only_and_serve_boot_loaded_registry_snapshots() {
    let server = start_skill_v1_test_server(&[("writing", "Structured document authoring")]).await;
    let client = reqwest::Client::new();

    assert!(server
        .state
        .kernel
        .skill_registry
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .get("writing")
        .is_some());

    std::fs::remove_file(
        server
            .home_dir
            .join("skills")
            .join("writing")
            .join("skill.toml"),
    )
    .expect("manifest should be removable");
    std::fs::remove_dir(server.home_dir.join("skills").join("writing"))
        .expect("skill directory should be removable");

    let (list_status, list_body) = get_json(&client, &server, "/api/v1/skills").await;
    assert_eq!(list_status, StatusCode::OK);
    assert_eq!(list_body["items"].as_array().map(Vec::len), Some(1));

    let (detail_status, detail_body) = get_json(&client, &server, "/api/v1/skills/writing").await;
    assert_eq!(detail_status, StatusCode::OK);
    assert_eq!(detail_body["id"], Value::String("writing".to_string()));

    let post_status = client
        .post(format!("{}/api/v1/skills", server.base_url))
        .send()
        .await
        .expect("request should succeed")
        .status();
    let put_status = client
        .put(format!("{}/api/v1/skills/writing", server.base_url))
        .send()
        .await
        .expect("request should succeed")
        .status();
    let delete_status = client
        .delete(format!("{}/api/v1/skills/writing", server.base_url))
        .send()
        .await
        .expect("request should succeed")
        .status();

    assert_eq!(post_status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(put_status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(delete_status, StatusCode::METHOD_NOT_ALLOWED);
}
