//! Real HTTP integration tests for the Pack v1 API surface.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use openfang_agent_definition::{
    stage4_normalize, AgentDefinition, CapabilitiesBlock, PromptBlock, ProviderBlock, RuntimeBlock,
};
use openfang_api::routes::AppState;
use openfang_api::server::build_router;
use openfang_api::types::{
    AgentOrigin, AgentResponse, PackManifest, PackObjectRef, PackRecord, PackResourceType,
    PackSource, PackSourceKind, ScheduleDefinition, WorkflowOrigin, WorkflowOriginKind,
    WorkflowResponse,
};
use openfang_kernel::OpenFangKernel;
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use openfang_types::trigger::TriggerV2Definition;
use openfang_types::workflow::WorkflowV2Definition;
use serde::Serialize;
use serde_json::{json, Value};
use tempfile::TempDir;

struct TestServer {
    base_url: String,
    state: Arc<AppState>,
    _tmp: TempDir,
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

fn pack_agent_definition(id: &str, name: &str) -> AgentDefinition {
    AgentDefinition {
        id: id.to_string(),
        name: name.to_string(),
        version: "1.0.0".to_string(),
        description: format!("Pack fixture agent {name}"),
        enabled: Some(true),
        group: Some("packs".to_string()),
        tags: vec!["packs".to_string()],
        provider: ProviderBlock {
            driver: "claude_code".to_string(),
            model: "sonnet".to_string(),
            ..ProviderBlock::default()
        },
        prompt: PromptBlock::default(),
        capabilities: CapabilitiesBlock::default(),
        runtime: RuntimeBlock::default(),
        input: None,
        output: None,
    }
}

fn noop_workflow_definition(id: &str, name: &str) -> WorkflowV2Definition {
    serde_json::from_value(json!({
        "id": id,
        "name": name,
        "version": "1.0.0",
        "description": format!("Pack workflow {name}"),
        "enabled": true,
        "tags": ["packs"],
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
    }))
    .expect("workflow fixture should deserialize")
}

fn trigger_definition(id: &str, workflow_id: &str) -> TriggerV2Definition {
    serde_json::from_value(json!({
        "id": id,
        "name": format!("Trigger {id}"),
        "description": "Pack trigger fixture",
        "enabled": true,
        "max_fires": 0,
        "cooldown_secs": 0,
        "match": {
            "event": "issue.created",
            "source": "tests",
            "filters": {}
        },
        "target": {
            "kind": "workflow_start",
            "workflow": workflow_id,
            "input": {
                "issue_id": "{{ event.issue_id }}"
            }
        }
    }))
    .expect("trigger fixture should deserialize")
}

fn schedule_definition(agent: &str, workflow_id: &str) -> ScheduleDefinition {
    serde_json::from_value(json!({
        "agent": agent,
        "name": "Analytics Daily",
        "enabled": true,
        "schedule": {
            "kind": "cron",
            "expr": "0 9 * * *",
            "tz": "UTC"
        },
        "action": {
            "kind": "workflow_run",
            "workflow_id": workflow_id,
            "input": {
                "scope": "packs"
            },
            "timeout_secs": 120
        },
        "delivery": {
            "kind": "none"
        }
    }))
    .expect("schedule fixture should deserialize")
}

fn user_agent_resource(id: &str, name: &str) -> AgentResponse {
    AgentResponse {
        definition: stage4_normalize(pack_agent_definition(id, name)),
        origin: AgentOrigin::user(),
        forked_from: None,
        created_at: "2026-03-25T12:00:00Z".to_string(),
        updated_at: "2026-03-25T12:00:00Z".to_string(),
    }
}

fn user_workflow_resource(id: &str, name: &str) -> WorkflowResponse {
    WorkflowResponse {
        definition: noop_workflow_definition(id, name),
        origin: WorkflowOrigin::user(),
        forked_from: None,
        created_at: "2026-03-25T12:00:00Z".to_string(),
        updated_at: "2026-03-25T12:00:00Z".to_string(),
    }
}

fn seed_pack_fixtures(home_dir: &Path) {
    write_toml_file(
        &home_dir.join("agents/schedule-runner.toml"),
        &user_agent_resource("schedule-runner", "Schedule Runner"),
    );

    let analytics_manifest = PackManifest {
        id: "analytics".to_string(),
        name: "Analytics".to_string(),
        version: "1.2.0".to_string(),
        description: "Analytics pack fixture".to_string(),
        source: PackSource {
            kind: PackSourceKind::Bundled,
        },
        objects: vec![
            PackObjectRef {
                resource_type: PackResourceType::Agent,
                resource_id: "analytics-agent".to_string(),
            },
            PackObjectRef {
                resource_type: PackResourceType::Workflow,
                resource_id: "analytics-main".to_string(),
            },
            PackObjectRef {
                resource_type: PackResourceType::Trigger,
                resource_id: "analytics-trigger".to_string(),
            },
            PackObjectRef {
                resource_type: PackResourceType::Schedule,
                resource_id: "analytics-daily".to_string(),
            },
            PackObjectRef {
                resource_type: PackResourceType::Template,
                resource_id: "analytics-template".to_string(),
            },
        ],
    };
    write_toml_file(
        &home_dir.join("packs/analytics/pack.toml"),
        &analytics_manifest,
    );
    write_toml_file(
        &home_dir.join("packs/analytics/agents/analytics-agent.toml"),
        &pack_agent_definition("analytics-agent", "Analytics Agent"),
    );
    write_toml_file(
        &home_dir.join("packs/analytics/workflows/analytics-main.toml"),
        &noop_workflow_definition("analytics-main", "Analytics Main"),
    );
    write_toml_file(
        &home_dir.join("packs/analytics/triggers/analytics-trigger.toml"),
        &trigger_definition("analytics-trigger", "analytics-main"),
    );
    write_toml_file(
        &home_dir.join("packs/analytics/schedules/analytics-daily.toml"),
        &schedule_definition("schedule-runner", "analytics-main"),
    );
    write_file(
        &home_dir.join("packs/analytics/templates/analytics-template.toml"),
        "title = \"Analytics Template\"\nbody = \"analytics\"\n",
    );
}

async fn start_pack_v1_test_server() -> TestServer {
    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    seed_pack_fixtures(tmp.path());

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

fn load_pack_record(server: &TestServer, pack_id: &str) -> Option<PackRecord> {
    server
        .state
        .kernel
        .workflow_stores
        .pack
        .find_by_id(pack_id)
        .expect("pack lookup should succeed")
}

#[tokio::test]
async fn get_packs_v1_should_return_items_and_next_cursor_shape() {
    let server = start_pack_v1_test_server().await;
    let client = reqwest::Client::new();

    let (status, body) = get_json(&client, &server, "/api/v1/packs?limit=1").await;

    assert_eq!(status, reqwest::StatusCode::OK);
    let items = body["items"]
        .as_array()
        .expect("pack list should return an items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], json!("analytics"));
    assert_eq!(items[0]["source"]["kind"], json!("bundled"));
    assert_eq!(items[0]["installed"], json!(true));
    assert_eq!(items[0]["managed"], json!(true));
    assert_eq!(items[0]["objects"]["agents"], json!(1));
    assert_eq!(items[0]["objects"]["workflows"], json!(1));
    assert_eq!(items[0]["objects"]["triggers"], json!(1));
    assert_eq!(items[0]["objects"]["schedules"], json!(1));
    assert_eq!(items[0]["objects"]["templates"], json!(1));
    assert_eq!(body["next_cursor"], json!("1"));
}

#[tokio::test]
async fn get_pack_v1_should_return_detail_for_known_pack() {
    let server = start_pack_v1_test_server().await;
    let client = reqwest::Client::new();

    let (status, body) = get_json(&client, &server, "/api/v1/packs/analytics").await;

    assert_eq!(status, reqwest::StatusCode::OK);
    assert_eq!(body["id"], json!("analytics"));
    assert_eq!(body["name"], json!("Analytics"));
    assert_eq!(body["version"], json!("1.2.0"));
    assert_eq!(body["source"]["kind"], json!("bundled"));
    assert_eq!(body["installed"], json!(true));
    assert_eq!(body["managed"], json!(true));
    assert_eq!(
        body["objects"]
            .as_array()
            .expect("detail should include objects")
            .len(),
        5
    );
}

#[tokio::test]
async fn get_pack_v1_should_return_404_for_unknown_pack() {
    let server = start_pack_v1_test_server().await;
    let client = reqwest::Client::new();

    let (status, body) = get_json(&client, &server, "/api/v1/packs/missing-pack").await;

    assert_eq!(status, reqwest::StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], json!("not_found"));
}

#[tokio::test]
async fn kernel_boot_should_bootstrap_bundled_sdlc_pack() {
    let server = start_pack_v1_test_server().await;
    let client = reqwest::Client::new();

    let record = load_pack_record(&server, "sdlc").expect("sdlc pack record should exist");
    assert_eq!(record.version, "1.2.0");
    assert_eq!(record.source_kind, PackSourceKind::Bundled);
    assert_eq!(record.installed, 4);
    assert!(server
        .state
        .kernel
        .config
        .home_dir
        .join("packs/sdlc/pack.toml")
        .exists());

    let (status, body) = get_json(&client, &server, "/api/v1/packs/sdlc").await;
    assert_eq!(status, reqwest::StatusCode::OK);
    assert_eq!(body["version"], json!("1.2.0"));
    assert_eq!(body["source"]["kind"], json!("bundled"));
}

#[tokio::test]
async fn post_pack_install_should_create_pack_record_for_bundled_sdlc() {
    let server = start_pack_v1_test_server().await;
    let client = reqwest::Client::new();

    let (uninstall_status, _) = post_json(
        &client,
        &server,
        "/api/v1/packs/sdlc/uninstall",
        json!({ "force": false }),
    )
    .await;
    assert_eq!(uninstall_status, reqwest::StatusCode::ACCEPTED);
    assert!(load_pack_record(&server, "sdlc").is_none());

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/packs/install",
        json!({
            "source": {
                "kind": "bundled",
                "pack_id": "sdlc",
                "version": "1.2.0"
            }
        }),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
    assert_eq!(body["accepted"], json!(true));
    assert_eq!(body["resource_id"], json!("sdlc"));

    let record = load_pack_record(&server, "sdlc").expect("sdlc pack record should exist");
    assert_eq!(record.version, "1.2.0");
    assert_eq!(record.source_kind, PackSourceKind::Bundled);
    assert!(server
        .state
        .kernel
        .config
        .home_dir
        .join("packs/sdlc/workflows/sdlc-main.toml")
        .exists());
}

#[tokio::test]
async fn post_pack_upgrade_dry_run_should_not_mutate_pack_version() {
    let server = start_pack_v1_test_server().await;
    let client = reqwest::Client::new();

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/packs/sdlc/upgrade/dry-run",
        json!({
            "target_version": "1.3.0"
        }),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::OK);
    assert_eq!(body["would_execute"], json!(true));
    assert_eq!(body["resolved"]["pack_id"], json!("sdlc"));
    assert_eq!(body["resolved"]["from_version"], json!("1.2.0"));
    assert_eq!(body["resolved"]["to_version"], json!("1.3.0"));
    assert_eq!(body["effects"]["managed_objects_added"], json!(1));
    assert_eq!(body["effects"]["managed_objects_updated"], json!(3));
    assert_eq!(body["effects"]["managed_objects_removed"], json!(0));
    assert_eq!(body["effects"]["forks_untouched"], json!(0));
    assert_eq!(body["explanation"]["managed_objects_only"], json!(true));
    assert_eq!(body["explanation"]["forks_remain_detached"], json!(true));

    let record = load_pack_record(&server, "sdlc").expect("sdlc pack record should exist");
    assert_eq!(record.version, "1.2.0");
}

#[tokio::test]
async fn post_pack_upgrade_should_update_managed_objects_without_touching_user_forks() {
    let server = start_pack_v1_test_server().await;
    let client = reqwest::Client::new();

    let shadow_before = toml::to_string_pretty(&WorkflowResponse {
        definition: noop_workflow_definition("sdlc-main", "Forked SDLC Main"),
        origin: WorkflowOrigin::user(),
        forked_from: Some(openfang_api::types::WorkflowForkedFrom {
            kind: WorkflowOriginKind::Pack,
            pack_id: Some("sdlc".to_string()),
            pack_version: Some("1.2.0".to_string()),
            resource_type: "workflow".to_string(),
            resource_id: "sdlc-main".to_string(),
        }),
        created_at: "2026-03-25T12:00:00Z".to_string(),
        updated_at: "2026-03-25T12:00:00Z".to_string(),
    })
    .expect("workflow fork should serialize");
    let shadow_path = server
        .state
        .kernel
        .config
        .home_dir
        .join("workflows/sdlc-main.toml");
    write_file(&shadow_path, &shadow_before);
    server
        .state
        .kernel
        .workflow_stores
        .pack
        .find_by_id("sdlc")
        .expect("pack lookup should succeed");

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/packs/sdlc/upgrade",
        json!({
            "target_version": "1.3.0"
        }),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
    assert_eq!(body["accepted"], json!(true));
    assert_eq!(body["resource_id"], json!("sdlc"));

    let record = load_pack_record(&server, "sdlc").expect("sdlc pack record should exist");
    assert_eq!(record.version, "1.3.0");
    assert_eq!(record.installed, 5);
    assert_eq!(record.managed, 4);

    let shadow_after = fs::read_to_string(&shadow_path).expect("workflow shadow should remain");
    assert_eq!(shadow_after, shadow_before);
    let shadow_resource: WorkflowResponse =
        toml::from_str(&shadow_after).expect("workflow shadow should deserialize");
    let forked_from = shadow_resource
        .forked_from
        .expect("fork provenance should remain present");
    assert_eq!(forked_from.pack_id, Some("sdlc".to_string()));
    assert_eq!(forked_from.pack_version, Some("1.2.0".to_string()));

    assert!(server
        .state
        .kernel
        .config
        .home_dir
        .join("packs/sdlc/agents/sdlc-reviewer.toml")
        .exists());
}

#[tokio::test]
async fn post_pack_uninstall_should_require_force_when_user_forks_exist() {
    let server = start_pack_v1_test_server().await;
    let client = reqwest::Client::new();

    let shadow_path = server
        .state
        .kernel
        .config
        .home_dir
        .join("workflows/sdlc-main.toml");
    write_file(
        &shadow_path,
        &toml::to_string_pretty(&WorkflowResponse {
            definition: noop_workflow_definition("sdlc-main", "Forked SDLC Main"),
            origin: WorkflowOrigin::user(),
            forked_from: Some(openfang_api::types::WorkflowForkedFrom {
                kind: WorkflowOriginKind::Pack,
                pack_id: Some("sdlc".to_string()),
                pack_version: Some("1.2.0".to_string()),
                resource_type: "workflow".to_string(),
                resource_id: "sdlc-main".to_string(),
            }),
            created_at: "2026-03-25T12:00:00Z".to_string(),
            updated_at: "2026-03-25T12:00:00Z".to_string(),
        })
        .expect("workflow fork should serialize"),
    );

    let (conflict_status, conflict_body) = post_json(
        &client,
        &server,
        "/api/v1/packs/sdlc/uninstall",
        json!({ "force": false }),
    )
    .await;
    assert_eq!(conflict_status, reqwest::StatusCode::CONFLICT);
    assert_eq!(
        conflict_body["error"]["details"][0]["forked_ids"],
        json!(["sdlc-main"])
    );

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/packs/sdlc/uninstall",
        json!({ "force": true }),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
    assert_eq!(body["accepted"], json!(true));
    assert!(load_pack_record(&server, "sdlc").is_none());
    assert!(!server
        .state
        .kernel
        .config
        .home_dir
        .join("packs/sdlc")
        .exists());
    assert!(shadow_path.exists());
}

#[tokio::test]
async fn get_pack_objects_v1_should_return_forked_flags() {
    let server = start_pack_v1_test_server().await;
    let client = reqwest::Client::new();

    write_toml_file(
        &server
            .state
            .kernel
            .config
            .home_dir
            .join("workflows/analytics-main.toml"),
        &user_workflow_resource("analytics-main", "Analytics Main"),
    );

    let (status, body) = get_json(&client, &server, "/api/v1/packs/analytics/objects").await;

    assert_eq!(status, reqwest::StatusCode::OK);
    let items = body["items"]
        .as_array()
        .expect("pack object list should return items");
    let workflow_item = items
        .iter()
        .find(|item| item["resource_type"] == json!("workflow"))
        .expect("workflow object should be present");
    let trigger_item = items
        .iter()
        .find(|item| item["resource_type"] == json!("trigger"))
        .expect("trigger object should be present");
    assert_eq!(workflow_item["resource_id"], json!("analytics-main"));
    assert_eq!(workflow_item["forked"], json!(true));
    assert_eq!(trigger_item["forked"], json!(false));
}

#[tokio::test]
async fn post_pack_fork_should_create_user_shadow_and_return_provenance() {
    let server = start_pack_v1_test_server().await;
    let client = reqwest::Client::new();
    let managed_path = server
        .state
        .kernel
        .config
        .home_dir
        .join("packs/analytics/workflows/analytics-main.toml");
    let managed_before =
        fs::read_to_string(&managed_path).expect("managed pack workflow should exist");

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/packs/analytics/fork",
        json!({
            "resource_type": "workflow",
            "resource_id": "analytics-main",
            "mode": "shadow"
        }),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::OK);
    assert_eq!(body["id"], json!("analytics-main"));
    assert_eq!(body["origin"]["kind"], json!("user"));
    assert_eq!(body["forked_from"]["kind"], json!("pack"));
    assert_eq!(body["forked_from"]["pack_id"], json!("analytics"));
    assert_eq!(body["forked_from"]["pack_version"], json!("1.2.0"));
    assert_eq!(body["forked_from"]["resource_type"], json!("workflow"));
    assert_eq!(body["forked_from"]["resource_id"], json!("analytics-main"));

    let persisted_path = server
        .state
        .kernel
        .config
        .home_dir
        .join("workflows/analytics-main.toml");
    let persisted = fs::read_to_string(&persisted_path).expect("forked workflow file should exist");
    let resource: WorkflowResponse =
        toml::from_str(&persisted).expect("forked workflow should deserialize");
    assert_eq!(resource.origin.kind, WorkflowOriginKind::User);
    assert_eq!(
        resource
            .forked_from
            .expect("fork provenance should be present")
            .pack_id,
        Some("analytics".to_string())
    );
    assert_eq!(
        load_pack_record(&server, "analytics")
            .expect("analytics pack record should exist")
            .managed,
        4
    );

    let managed_after =
        fs::read_to_string(&managed_path).expect("managed pack workflow should still exist");
    assert_eq!(managed_before, managed_after);
}

#[tokio::test]
async fn post_pack_fork_should_return_conflict_when_already_forked() {
    let server = start_pack_v1_test_server().await;
    let client = reqwest::Client::new();

    let (first_status, _) = post_json(
        &client,
        &server,
        "/api/v1/packs/analytics/fork",
        json!({
            "resource_type": "workflow",
            "resource_id": "analytics-main",
            "mode": "shadow"
        }),
    )
    .await;
    assert_eq!(first_status, reqwest::StatusCode::OK);

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/packs/analytics/fork",
        json!({
            "resource_type": "workflow",
            "resource_id": "analytics-main",
            "mode": "shadow"
        }),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], json!("already_forked"));
}

#[tokio::test]
async fn post_pack_fork_should_return_not_found_for_unknown_object() {
    let server = start_pack_v1_test_server().await;
    let client = reqwest::Client::new();

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/packs/analytics/fork",
        json!({
            "resource_type": "workflow",
            "resource_id": "missing-workflow",
            "mode": "shadow"
        }),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], json!("not_found"));
}

#[tokio::test]
async fn post_pack_fork_should_reject_non_table_template_documents() {
    let server = start_pack_v1_test_server().await;
    let client = reqwest::Client::new();

    write_file(
        &server
            .state
            .kernel
            .config
            .home_dir
            .join("packs/analytics/templates/analytics-template.toml"),
        "123\n",
    );

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/packs/analytics/fork",
        json!({
            "resource_type": "template",
            "resource_id": "analytics-template",
            "mode": "shadow"
        }),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"]["code"], json!("definition_load_failed"));
}

#[tokio::test]
async fn get_pack_objects_v1_should_show_forked_after_successful_fork() {
    let server = start_pack_v1_test_server().await;
    let client = reqwest::Client::new();

    let (fork_status, _) = post_json(
        &client,
        &server,
        "/api/v1/packs/analytics/fork",
        json!({
            "resource_type": "workflow",
            "resource_id": "analytics-main",
            "mode": "shadow"
        }),
    )
    .await;
    assert_eq!(fork_status, reqwest::StatusCode::OK);

    let (status, body) = get_json(&client, &server, "/api/v1/packs/analytics/objects").await;

    assert_eq!(status, reqwest::StatusCode::OK);
    let items = body["items"]
        .as_array()
        .expect("pack object list should return items");
    let workflow_item = items
        .iter()
        .find(|item| item["resource_type"] == json!("workflow"))
        .expect("workflow object should be present");
    assert_eq!(workflow_item["resource_id"], json!("analytics-main"));
    assert_eq!(workflow_item["forked"], json!(true));
}

#[tokio::test]
async fn create_agent_definition_should_reject_managed_pack_id_collision() {
    let server = start_pack_v1_test_server().await;
    let client = reqwest::Client::new();

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/agents",
        serde_json::to_value(pack_agent_definition("analytics-agent", "Analytics Agent"))
            .expect("agent fixture should serialize"),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], json!("managed_definition_conflict"));
    assert_eq!(body["error"]["details"][0]["action"], json!("fork"));
    assert_eq!(body["error"]["details"][0]["pack_id"], json!("analytics"));
}

#[tokio::test]
async fn create_workflow_definition_should_reject_managed_pack_id_collision() {
    let server = start_pack_v1_test_server().await;
    let client = reqwest::Client::new();

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/workflows",
        json!({
            "id": "analytics-main",
            "name": "Analytics Main",
            "version": "1.0.0",
            "description": "Should conflict with managed pack workflow",
            "enabled": true,
            "tags": ["packs"],
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
        }),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], json!("managed_definition_conflict"));
    assert_eq!(body["error"]["details"][0]["action"], json!("fork"));
    assert_eq!(body["error"]["details"][0]["pack_id"], json!("analytics"));
}

#[tokio::test]
async fn create_trigger_definition_should_reject_managed_pack_id_collision() {
    let server = start_pack_v1_test_server().await;
    let client = reqwest::Client::new();

    write_toml_file(
        &server
            .state
            .kernel
            .config
            .home_dir
            .join("workflows/safe-trigger-target.toml"),
        &user_workflow_resource("safe-trigger-target", "Safe Trigger Target"),
    );

    let (status, body) = post_json(
        &client,
        &server,
        "/api/v1/triggers",
        serde_json::to_value(trigger_definition(
            "analytics-trigger",
            "safe-trigger-target",
        ))
        .expect("trigger fixture should serialize"),
    )
    .await;

    assert_eq!(status, reqwest::StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], json!("managed_definition_conflict"));
    assert_eq!(body["error"]["details"][0]["action"], json!("fork"));
    assert_eq!(body["error"]["details"][0]["pack_id"], json!("analytics"));
}
