use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use openfang_api::server::run_daemon;
use openfang_kernel::OpenFangKernel;
use openfang_types::config::{DefaultModelConfig, KernelConfig};
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
        let daemon_info_path = home.path().join("daemon.json");
        let base_url = format!("http://{listen_addr}");
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

        tokio::spawn(async move {
            run_daemon(
                OpenFangKernel::boot_with_config(config)
                    .expect("kernel should boot for CLI event tests"),
                &listen_addr,
                Some(daemon_info_path.as_path()),
            )
            .await
            .expect("CLI event test daemon should stay available");
        });

        wait_for_health(&base_url).await;

        Self { home, base_url }
    }

    fn home(&self) -> &Path {
        self.home.path()
    }

    fn base_url(&self) -> &str {
        &self.base_url
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

fn write_text_file(home: &Path, name: &str, contents: &str) -> String {
    let path = home.join(name);
    fs::write(&path, contents).expect("text fixture should be written");
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
        "description": "Event CLI integration workflow",
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

fn workflow_start_trigger(id: &str, workflow_id: &str) -> Value {
    json!({
        "id": id,
        "name": format!("Trigger {id}"),
        "description": "Starts a workflow from event ingress",
        "enabled": true,
        "max_fires": 0,
        "cooldown_secs": 0,
        "match": {
            "event": "issue.created",
            "source": "api"
        },
        "target": {
            "kind": "workflow_start",
            "workflow": workflow_id,
            "input": {
                "issue_id": "{{ event.payload.issue_id }}"
            }
        }
    })
}

fn event_request() -> Value {
    json!({
        "event": "issue.created",
        "source": "api",
        "payload": {
            "issue_id": "ISSUE-123",
            "issue": {
                "id": "ISSUE-123"
            }
        },
        "idempotency_key": "event-cli-test-key",
        "occurred_at": "2026-03-27T12:00:00Z",
        "metadata": {
            "actor": "cli-test"
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
    let body = read_json_response(response, "workflow create response").await;
    assert_eq!(
        status,
        reqwest::StatusCode::CREATED,
        "workflow create request should succeed: {body}"
    );
}

async fn create_trigger_definition(client: &reqwest::Client, base_url: &str, trigger: Value) {
    let response = client
        .post(format!("{base_url}/api/v1/triggers"))
        .json(&trigger)
        .send()
        .await
        .expect("trigger create request should succeed");
    let status = response.status();
    let body = read_json_response(response, "trigger create response").await;
    assert_eq!(
        status,
        reqwest::StatusCode::CREATED,
        "trigger create request should succeed: {body}"
    );
}

async fn workflow_run_count(client: &reqwest::Client, base_url: &str, workflow_id: &str) -> usize {
    let response = client
        .get(format!("{base_url}/api/v1/runs"))
        .query(&[("workflow_id", workflow_id)])
        .send()
        .await
        .expect("workflow run list request should succeed");
    let body = read_json_response(response, "workflow run list response").await;
    body["items"]
        .as_array()
        .expect("workflow run list should return an items array")
        .len()
}

async fn wait_for_workflow_run_count(
    client: &reqwest::Client,
    base_url: &str,
    workflow_id: &str,
    expected: usize,
) {
    for _ in 0..50 {
        if workflow_run_count(client, base_url, workflow_id).await == expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let actual = workflow_run_count(client, base_url, workflow_id).await;
    panic!("workflow {workflow_id} should have {expected} run(s), found {actual}");
}

#[test]
fn event_help_should_list_all_subcommands() {
    let home = TestHome::new("event-help");
    let output = run_openfang(home.path(), &["event", "--help"]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "event help should exit successfully\n{text}"
    );

    for subcommand in ["send", "dry-run"] {
        assert!(
            help_contains_subcommand(&text, subcommand),
            "event help should include '{subcommand}'\n{text}"
        );
    }
}

#[test]
fn event_send_should_report_missing_file() {
    let home = TestHome::new("event-send-missing-file");
    let missing_arg = home.path().join("missing-event.json");
    let missing_arg = missing_arg.to_string_lossy().to_string();
    let output = run_openfang(home.path(), &["event", "send", &missing_arg]);
    let text = output_text(&output);

    assert!(
        output.status.code() == Some(1),
        "event send should fail for a missing file\n{text}"
    );
    assert!(
        text.contains("Event file not found"),
        "event send should report the missing file before daemon lookup\n{text}"
    );
}

#[test]
fn event_send_without_required_file_should_show_usage_help() {
    let home = TestHome::new("event-send-missing-args");
    let output = run_openfang(home.path(), &["event", "send"]);
    let text = output_text(&output);

    assert!(
        !output.status.success(),
        "event send without args should fail\n{text}"
    );
    assert!(
        text.contains("Usage:") && text.contains("event send"),
        "event send without args should print usage help\n{text}"
    );
}

#[test]
fn event_send_should_fail_fast_for_invalid_json_file() {
    let home = TestHome::new("event-send-invalid-json");
    let invalid_file = write_text_file(home.path(), "invalid-event.json", "{ not-valid-json");
    let output = run_openfang(home.path(), &["event", "send", &invalid_file]);
    let text = output_text(&output);

    assert!(
        output.status.code() == Some(1),
        "event send should fail for malformed JSON\n{text}"
    );
    assert!(
        text.contains("Invalid JSON in"),
        "event send should validate the file before sending it to the daemon\n{text}"
    );
    assert!(
        !text.contains("requires a running daemon"),
        "event send should fail before daemon lookup when the file is invalid\n{text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn event_dry_run_should_report_summary_without_creating_workflow_run() {
    let daemon = TestDaemon::start("event-dry-run").await;
    let client = reqwest::Client::new();
    let workflow_id = "event_dry_run_workflow";
    let trigger_id = "event_dry_run_trigger";

    create_workflow_definition(
        &client,
        daemon.base_url(),
        noop_workflow_definition(workflow_id),
    )
    .await;
    create_trigger_definition(
        &client,
        daemon.base_url(),
        workflow_start_trigger(trigger_id, workflow_id),
    )
    .await;

    let event_file = write_json_file(daemon.home(), "event-dry-run.json", &event_request());
    let output = output_text(&run_openfang_success(
        daemon.home(),
        &["event", "dry-run", &event_file],
    ));

    assert!(
        output.contains("Event dry-run completed."),
        "event dry-run should confirm completion\n{output}"
    );
    assert!(
        output.contains("Would execute: yes"),
        "event dry-run should report that it would execute\n{output}"
    );
    assert!(
        output.contains("Resolved triggers: 1"),
        "event dry-run should report the resolved trigger count\n{output}"
    );
    assert!(
        output.contains("Effects: 1"),
        "event dry-run should report the effect count\n{output}"
    );
    assert!(
        output.contains("Explanation: trigger_engine"),
        "event dry-run should include the explanation block\n{output}"
    );

    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        workflow_run_count(&client, daemon.base_url(), workflow_id).await,
        0,
        "event dry-run must not create a workflow run"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn event_send_should_report_summary_and_create_workflow_run() {
    let daemon = TestDaemon::start("event-send").await;
    let client = reqwest::Client::new();
    let workflow_id = "event_send_workflow";
    let trigger_id = "event_send_trigger";

    create_workflow_definition(
        &client,
        daemon.base_url(),
        noop_workflow_definition(workflow_id),
    )
    .await;
    create_trigger_definition(
        &client,
        daemon.base_url(),
        workflow_start_trigger(trigger_id, workflow_id),
    )
    .await;

    let event_file = write_json_file(daemon.home(), "event-send.json", &event_request());
    let output = output_text(&run_openfang_success(
        daemon.home(),
        &["event", "send", &event_file],
    ));

    assert!(
        output.contains("Event accepted:"),
        "event send should print the event id summary\n{output}"
    );
    assert!(
        output.contains("Matched triggers: 1"),
        "event send should report the matched trigger count\n{output}"
    );
    assert!(
        output.contains("Effects: 1"),
        "event send should report the effect count\n{output}"
    );
    assert!(
        output.contains("Failures: 0"),
        "event send should report the failure count\n{output}"
    );

    wait_for_workflow_run_count(&client, daemon.base_url(), workflow_id, 1).await;

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn event_send_should_surface_api_validation_errors() {
    let daemon = TestDaemon::start("event-send-api-error").await;
    let invalid_request = write_json_file(
        daemon.home(),
        "event-invalid-request.json",
        &json!({
            "event": 42,
            "source": "api",
            "payload": {}
        }),
    );

    let output = run_openfang(daemon.home(), &["event", "send", &invalid_request]);
    let text = output_text(&output);

    assert!(
        output.status.code() == Some(1),
        "event send should fail for an API validation error\n{text}"
    );
    assert!(
        text.contains("Failed to send event:"),
        "event send should surface the API error prefix\n{text}"
    );
    assert!(
        text.contains("Invalid JSON body:"),
        "event send should surface the structured API error message\n{text}"
    );

    daemon.shutdown().await;
}
