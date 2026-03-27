use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use openfang_api::server::run_daemon;
use openfang_kernel::OpenFangKernel;
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

struct TestHome {
    path: PathBuf,
}

impl TestHome {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "openfang-cli-{name}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("test home should be created");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct TestDaemon {
    home: TestHome,
    base_url: String,
}

impl TestDaemon {
    async fn start(name: &str) -> Self {
        let home = TestHome::new(name);
        let listen_addr = reserve_listen_addr();
        let config = KernelConfig {
            home_dir: home.path().to_path_buf(),
            data_dir: home.path().join("data"),
            api_listen: listen_addr.clone(),
            default_model: DefaultModelConfig {
                provider: "ollama".to_string(),
                model: "test-model".to_string(),
                api_key_env: "OLLAMA_API_KEY".to_string(),
                base_url: None,
            },
            ..KernelConfig::default()
        };
        let daemon_info_path = home.path().join("daemon.json");
        let base_url = format!("http://{listen_addr}");

        tokio::spawn(async move {
            run_daemon(
                OpenFangKernel::boot_with_config(config)
                    .expect("kernel should boot for CLI trigger tests"),
                &listen_addr,
                Some(daemon_info_path.as_path()),
            )
            .await
            .expect("CLI trigger test daemon should stay available");
        });

        wait_for_health(&base_url).await;

        Self { home, base_url }
    }

    fn home(&self) -> &Path {
        self.home.path()
    }

    async fn shutdown(self) {
        let _ = reqwest::Client::new()
            .post(format!("{}/api/shutdown", self.base_url))
            .send()
            .await;
    }
}

fn run_openfang(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_openfang"))
        .env("OPENFANG_HOME", home)
        .args(args)
        .output()
        .expect("openfang command should run")
}

fn run_openfang_success(home: &Path, args: &[&str]) -> Output {
    let output = run_openfang(home, args);
    let text = output_text(&output);
    assert!(
        output.status.success(),
        "command should succeed: openfang {}\n{text}",
        args.join(" ")
    );
    output
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn help_contains_subcommand(text: &str, subcommand: &str) -> bool {
    text.lines()
        .map(str::trim_start)
        .any(|line| line.starts_with(subcommand) && line[subcommand.len()..].starts_with(' '))
}

fn reserve_listen_addr() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("test daemon should reserve a local port");
    let address = listener
        .local_addr()
        .expect("test daemon should expose a local address");
    address.to_string()
}

async fn wait_for_health(base_url: &str) {
    let client = reqwest::Client::new();
    let health_url = format!("{base_url}/api/health");
    for _ in 0..50 {
        if let Ok(response) = client.get(&health_url).send().await {
            if response.status().is_success() {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("test daemon did not become healthy: {health_url}");
}

fn write_json_file(home: &Path, name: &str, value: &Value) -> String {
    let path = home.join(name);
    fs::write(
        &path,
        serde_json::to_vec_pretty(value).expect("JSON fixture should serialize"),
    )
    .expect("JSON fixture should be written");
    path.to_string_lossy().to_string()
}

async fn read_json_response(response: reqwest::Response, context: &str) -> Value {
    let status = response.status();
    let body = response
        .text()
        .await
        .expect("HTTP response body should be readable");
    serde_json::from_str(&body).unwrap_or_else(|error| {
        panic!("{context} should return JSON ({status}): {error}\n{body}");
    })
}

fn noop_workflow_definition(id: &str) -> Value {
    json!({
        "id": id,
        "name": format!("Workflow {id}"),
        "version": "1.0.0",
        "description": "Trigger CLI integration workflow",
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
        "description": "Trigger CLI integration definition",
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

fn invalid_trigger_definition() -> Value {
    json!({
        "id": "invalid-trigger",
        "name": "Invalid Trigger",
        "description": "Missing match block",
        "enabled": true,
        "target": {
            "kind": "workflow_start",
            "workflow": "missing-workflow",
            "input": {}
        }
    })
}

async fn create_workflow_definition(client: &reqwest::Client, base_url: &str, workflow: Value) {
    let response = client
        .post(format!("{base_url}/api/v1/workflows"))
        .json(&workflow)
        .send()
        .await
        .expect("workflow create request should succeed");
    let status = response.status();
    let body = read_json_response(response, "workflow create").await;

    assert_eq!(status, reqwest::StatusCode::CREATED);
    assert_eq!(body["id"], workflow["id"]);
}

async fn create_trigger_definition(client: &reqwest::Client, base_url: &str, trigger: Value) {
    let response = client
        .post(format!("{base_url}/api/v1/triggers"))
        .json(&trigger)
        .send()
        .await
        .expect("trigger create request should succeed");
    let status = response.status();
    let body = read_json_response(response, "trigger create").await;

    assert_eq!(status, reqwest::StatusCode::CREATED);
    assert_eq!(body["id"], trigger["id"]);
}

#[test]
fn trigger_help_should_list_all_subcommands() {
    let home = TestHome::new("trigger-help");
    let output = run_openfang(home.path(), &["trigger", "--help"]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "trigger help should exit successfully\n{text}"
    );

    for subcommand in [
        "list", "create", "delete", "get", "update", "enable", "disable", "test", "fork",
        "validate", "compile", "runtime",
    ] {
        assert!(
            help_contains_subcommand(&text, subcommand),
            "trigger help should include '{subcommand}'\n{text}"
        );
    }
}

#[test]
fn trigger_list_should_require_running_daemon() {
    let home = TestHome::new("trigger-list-no-daemon");
    let output = run_openfang(home.path(), &["trigger", "list"]);
    let text = output_text(&output);

    assert!(
        output.status.code() == Some(1),
        "trigger list should fail without a daemon\n{text}"
    );
    assert!(
        text.contains("requires a running daemon"),
        "trigger list should explain the daemon requirement\n{text}"
    );
}

#[test]
fn trigger_test_missing_arguments_should_show_usage_help() {
    let home = TestHome::new("trigger-test-missing-args");
    let output = run_openfang(home.path(), &["trigger", "test"]);
    let text = output_text(&output);

    assert!(
        !output.status.success(),
        "trigger test without args should fail\n{text}"
    );
    assert!(
        text.contains("Usage:") && text.contains("openfang trigger test"),
        "trigger test without args should print usage help\n{text}"
    );
}

#[test]
fn trigger_validate_missing_file_should_report_not_found() {
    let home = TestHome::new("trigger-validate-missing-file");
    let output = run_openfang(home.path(), &["trigger", "validate", "nonexistent.json"]);
    let text = output_text(&output);

    assert!(
        output.status.code() == Some(1),
        "trigger validate should fail for a missing file\n{text}"
    );
    assert!(
        text.contains("Trigger definition file not found"),
        "trigger validate should report a file-not-found error\n{text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn trigger_get_json_should_return_valid_json() {
    let daemon = TestDaemon::start("trigger-get-json").await;
    let client = reqwest::Client::new();
    create_workflow_definition(
        &client,
        &daemon.base_url,
        noop_workflow_definition("trigger-get-workflow"),
    )
    .await;
    create_trigger_definition(
        &client,
        &daemon.base_url,
        workflow_start_trigger(
            "trigger-get-json",
            "trigger-get-workflow",
            true,
            "issue.created",
        ),
    )
    .await;

    let output = run_openfang_success(
        daemon.home(),
        &["trigger", "get", "trigger-get-json", "--json"],
    );
    let json_text = stdout_text(&output);
    let body: Value =
        serde_json::from_str(&json_text).expect("trigger get --json should print valid JSON");

    assert_eq!(body["id"], json!("trigger-get-json"));
    assert_eq!(body["target"]["kind"], json!("workflow_start"));

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn trigger_test_should_print_match_result_with_explanation() {
    let daemon = TestDaemon::start("trigger-test-cli").await;
    let client = reqwest::Client::new();
    create_workflow_definition(
        &client,
        &daemon.base_url,
        noop_workflow_definition("trigger-test-workflow"),
    )
    .await;
    create_trigger_definition(
        &client,
        &daemon.base_url,
        workflow_start_trigger(
            "trigger-test-cli",
            "trigger-test-workflow",
            true,
            "issue.created",
        ),
    )
    .await;

    let output = run_openfang_success(
        daemon.home(),
        &[
            "trigger",
            "test",
            "trigger-test-cli",
            r#"{"event":"issue.created","source":"api","payload":{"issue_id":"ISSUE-123"}}"#,
        ],
    );
    let text = output_text(&output);

    assert!(
        text.contains("Matched:") && text.contains("true"),
        "trigger test should print the match result\n{text}"
    );
    assert!(
        text.contains("Resolved target: workflow_start(trigger-test-workflow)"),
        "trigger test should print the resolved target\n{text}"
    );
    assert!(
        text.contains("Would dispatch:") && text.contains("true"),
        "trigger test should print the dispatch result\n{text}"
    );
    assert!(
        text.contains("Explanation:"),
        "trigger test should print the explanation\n{text}"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn trigger_enable_disable_should_toggle_trigger_state_and_runtime_json() {
    let daemon = TestDaemon::start("trigger-enable-disable").await;
    let client = reqwest::Client::new();
    create_workflow_definition(
        &client,
        &daemon.base_url,
        noop_workflow_definition("trigger-toggle-workflow"),
    )
    .await;
    create_trigger_definition(
        &client,
        &daemon.base_url,
        workflow_start_trigger(
            "trigger-toggle",
            "trigger-toggle-workflow",
            true,
            "issue.created",
        ),
    )
    .await;

    let disable_output =
        run_openfang_success(daemon.home(), &["trigger", "disable", "trigger-toggle"]);
    let disable_text = output_text(&disable_output);
    assert!(
        disable_text.contains("disable accepted"),
        "trigger disable should report success\n{disable_text}"
    );

    let runtime_disabled = run_openfang_success(
        daemon.home(),
        &["trigger", "runtime", "trigger-toggle", "--json"],
    );
    let disabled_body: Value = serde_json::from_str(&stdout_text(&runtime_disabled))
        .expect("trigger runtime --json should print valid JSON");
    assert_eq!(disabled_body["trigger_id"], json!("trigger-toggle"));
    assert_eq!(disabled_body["enabled"], json!(false));

    let enable_output =
        run_openfang_success(daemon.home(), &["trigger", "enable", "trigger-toggle"]);
    let enable_text = output_text(&enable_output);
    assert!(
        enable_text.contains("enable accepted"),
        "trigger enable should report success\n{enable_text}"
    );

    let runtime_enabled = run_openfang_success(
        daemon.home(),
        &["trigger", "runtime", "trigger-toggle", "--json"],
    );
    let enabled_body: Value = serde_json::from_str(&stdout_text(&runtime_enabled))
        .expect("trigger runtime --json should print valid JSON");
    assert_eq!(enabled_body["enabled"], json!(true));

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn trigger_validate_should_report_validity_and_issues() {
    let daemon = TestDaemon::start("trigger-validate").await;
    let client = reqwest::Client::new();
    create_workflow_definition(
        &client,
        &daemon.base_url,
        noop_workflow_definition("trigger-validate-workflow"),
    )
    .await;

    let valid_file = write_json_file(
        daemon.home(),
        "trigger-valid.json",
        &workflow_start_trigger(
            "trigger-validate-valid",
            "trigger-validate-workflow",
            true,
            "issue.created",
        ),
    );
    let valid_output = run_openfang_success(daemon.home(), &["trigger", "validate", &valid_file]);
    let valid_text = output_text(&valid_output);
    assert!(
        valid_text.contains("Validation result: VALID"),
        "trigger validate should report a valid definition\n{valid_text}"
    );
    assert!(
        valid_text.contains("Issues: none"),
        "trigger validate should report no issues for a valid definition\n{valid_text}"
    );
    assert!(
        valid_text.contains("Normalized definition:"),
        "trigger validate should print the normalized definition\n{valid_text}"
    );

    let invalid_file = write_json_file(
        daemon.home(),
        "trigger-invalid.json",
        &invalid_trigger_definition(),
    );
    let invalid_output =
        run_openfang_success(daemon.home(), &["trigger", "validate", &invalid_file]);
    let invalid_text = output_text(&invalid_output);
    assert!(
        invalid_text.contains("Validation result: INVALID"),
        "trigger validate should report an invalid definition\n{invalid_text}"
    );
    assert!(
        invalid_text.contains("Issues:") && invalid_text.contains("[error]"),
        "trigger validate should print the issue list\n{invalid_text}"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn trigger_update_and_compile_should_use_file_definitions() {
    let daemon = TestDaemon::start("trigger-update-compile").await;
    let client = reqwest::Client::new();
    create_workflow_definition(
        &client,
        &daemon.base_url,
        noop_workflow_definition("trigger-update-workflow"),
    )
    .await;
    create_trigger_definition(
        &client,
        &daemon.base_url,
        workflow_start_trigger(
            "trigger-update",
            "trigger-update-workflow",
            true,
            "issue.created",
        ),
    )
    .await;

    let mut updated_definition = workflow_start_trigger(
        "trigger-update",
        "trigger-update-workflow",
        true,
        "issue.created",
    );
    updated_definition["max_fires"] = json!(8);
    updated_definition["cooldown_secs"] = json!(45);
    let update_file = write_json_file(daemon.home(), "trigger-update.json", &updated_definition);

    let update_output = run_openfang_success(
        daemon.home(),
        &["trigger", "update", "trigger-update", &update_file],
    );
    let update_text = output_text(&update_output);
    assert!(
        update_text.contains("Trigger trigger-update updated."),
        "trigger update should report success\n{update_text}"
    );

    let get_output = run_openfang_success(
        daemon.home(),
        &["trigger", "get", "trigger-update", "--json"],
    );
    let get_body: Value =
        serde_json::from_str(&stdout_text(&get_output)).expect("trigger get should print JSON");
    assert_eq!(get_body["max_fires"], json!(8));
    assert_eq!(get_body["cooldown_secs"], json!(45));

    let compile_output = run_openfang_success(daemon.home(), &["trigger", "compile", &update_file]);
    let compile_text = output_text(&compile_output);
    assert!(
        compile_text.contains("Definition ID:   trigger-update"),
        "trigger compile should print the definition id\n{compile_text}"
    );
    assert!(
        compile_text.contains("Dispatch action: workflow_run_create"),
        "trigger compile should print the compiled payload summary\n{compile_text}"
    );
    assert!(
        compile_text.contains("Target:          workflow_start(trigger-update-workflow)"),
        "trigger compile should print the resolved target summary\n{compile_text}"
    );

    daemon.shutdown().await;
}
