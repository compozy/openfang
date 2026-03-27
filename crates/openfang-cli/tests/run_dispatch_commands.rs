use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use openfang_api::server::run_daemon;
use openfang_kernel::OpenFangKernel;
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use serde_json::{json, Value};

const DISPATCH_TEST_MANIFEST: &str = r#"
name = "dispatch-cli-tester"
version = "0.1.0"
description = "Dispatch CLI integration test agent"
author = "test"
module = "builtin:chat"

[model]
provider = "ollama"
model = "test-model"
system_prompt = "You are a dispatch CLI integration test agent."

[capabilities]
tools = []
memory_read = ["*"]
memory_write = ["self.*"]
"#;

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

        let mut provider_urls = HashMap::new();
        provider_urls.insert(
            "ollama".to_string(),
            format!("http://{}/v1", reserve_listen_addr()),
        );

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
            provider_urls,
            ..KernelConfig::default()
        };

        let kernel = OpenFangKernel::boot_with_config(config)
            .expect("kernel should boot for CLI run/dispatch tests");

        tokio::spawn(async move {
            run_daemon(kernel, &listen_addr, Some(daemon_info_path.as_path()))
                .await
                .expect("CLI test daemon should stay available");
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
        "description": "CLI run list integration workflow",
        "enabled": true,
        "tags": ["cli"],
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

fn dispatch_workflow_definition(id: &str, agent_name: &str) -> Value {
    json!({
        "id": id,
        "name": format!("Workflow {id}"),
        "version": "1.0.0",
        "description": "CLI dispatch list integration workflow",
        "enabled": true,
        "tags": ["cli", "dispatch"],
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
        "steps": [{
            "id": "agent-step",
            "name": "Agent Step",
            "kind": "agent",
            "uses": { "agent": agent_name },
            "with": {
                "message": "Review {{ input.topic }}"
            },
            "runtime": {
                "dispatch": "call"
            },
            "save_as": "result",
            "flow": { "mode": "sequential" }
        }],
        "outputs": {
            "result": "{{ vars.result }}"
        }
    })
}

async fn create_agent(client: &reqwest::Client, base_url: &str, manifest_toml: &str) -> Value {
    let response = client
        .post(format!("{base_url}/api/agents"))
        .json(&json!({ "manifest_toml": manifest_toml }))
        .send()
        .await
        .expect("agent create request should succeed");
    let status = response.status();
    let body = read_json_response(response, "agent create response").await;
    assert_eq!(
        status,
        reqwest::StatusCode::CREATED,
        "agent create request should succeed: {body}"
    );
    body
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

async fn start_workflow_run(
    client: &reqwest::Client,
    base_url: &str,
    workflow_id: &str,
    topic: &str,
) -> String {
    let response = client
        .post(format!("{base_url}/api/v1/workflows/{workflow_id}/runs"))
        .json(&json!({
            "input": {
                "topic": topic,
            },
            "labels": ["cli"],
            "metadata": {
                "source": "cli-test",
            }
        }))
        .send()
        .await
        .expect("workflow run request should succeed");
    let status = response.status();
    let body = read_json_response(response, "workflow run response").await;
    assert_eq!(
        status,
        reqwest::StatusCode::ACCEPTED,
        "workflow run request should be accepted: {body}"
    );
    body["run_id"]
        .as_str()
        .expect("workflow run response should include run_id")
        .to_string()
}

async fn wait_for_run(client: &reqwest::Client, base_url: &str, run_id: &str) -> Value {
    for _ in 0..50 {
        let response = client
            .get(format!("{base_url}/api/v1/runs/{run_id}"))
            .send()
            .await
            .expect("run lookup request should succeed");
        if response.status().is_success() {
            return read_json_response(response, "run lookup response").await;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("run {run_id} did not become visible");
}

async fn wait_for_dispatches_for_run(
    client: &reqwest::Client,
    base_url: &str,
    run_id: &str,
) -> Vec<Value> {
    for _ in 0..80 {
        let response = client
            .get(format!("{base_url}/api/v1/dispatches"))
            .query(&[("run_id", run_id)])
            .send()
            .await
            .expect("dispatch list request should succeed");
        if response.status().is_success() {
            let body = read_json_response(response, "dispatch list response").await;
            if let Some(items) = body["items"].as_array() {
                if !items.is_empty() {
                    return items.clone();
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("dispatches for run {run_id} did not become visible");
}

#[test]
fn run_help_should_list_all_subcommands() {
    let home = TestHome::new("run-help");
    let output = run_openfang(home.path(), &["run", "--help"]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "run help should exit successfully\n{text}"
    );

    for subcommand in [
        "list",
        "get",
        "dispatches",
        "hitl",
        "signals",
        "signal",
        "checkpoints",
        "pause",
        "resume",
        "cancel",
        "watch",
    ] {
        assert!(
            help_contains_subcommand(&text, subcommand),
            "run help should include '{subcommand}'\n{text}"
        );
    }
}

#[test]
fn dispatch_help_should_list_all_subcommands() {
    let home = TestHome::new("dispatch-help");
    let output = run_openfang(home.path(), &["dispatch", "--help"]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "dispatch help should exit successfully\n{text}"
    );

    for subcommand in ["list", "get", "children", "retry", "cancel", "watch"] {
        assert!(
            help_contains_subcommand(&text, subcommand),
            "dispatch help should include '{subcommand}'\n{text}"
        );
    }
}

#[test]
fn run_list_should_require_running_daemon() {
    let home = TestHome::new("run-list-no-daemon");
    let output = run_openfang(home.path(), &["run", "list"]);
    let text = output_text(&output);

    assert!(
        output.status.code() == Some(1),
        "run list should fail without a daemon\n{text}"
    );
    assert!(
        text.contains("requires a running daemon"),
        "run list should explain the daemon requirement\n{text}"
    );
}

#[test]
fn run_signal_without_required_arguments_should_show_usage_help() {
    let home = TestHome::new("run-signal-missing-args");
    let output = run_openfang(home.path(), &["run", "signal"]);
    let text = output_text(&output);

    assert!(
        !output.status.success(),
        "run signal without args should fail\n{text}"
    );
    assert!(
        text.contains("Usage:") && text.contains("run signal"),
        "run signal without args should print usage help\n{text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn run_list_json_should_return_valid_json_array_with_running_daemon() {
    let daemon = TestDaemon::start("run-list-json").await;
    let client = reqwest::Client::new();
    let workflow_id = "cli-run-list";

    create_workflow_definition(
        &client,
        daemon.base_url(),
        noop_workflow_definition(workflow_id),
    )
    .await;

    let run_id = start_workflow_run(&client, daemon.base_url(), workflow_id, "CLI run list").await;
    let run = wait_for_run(&client, daemon.base_url(), &run_id).await;
    assert_eq!(run["id"], json!(run_id));

    let stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["run", "list", "--json"],
    ));
    let body: Value = serde_json::from_str(&stdout).expect("run list JSON should parse");
    let items = body.as_array().expect("run list JSON should be an array");
    assert!(
        items.iter().any(|item| item["id"] == json!(run_id)),
        "run list JSON should include the started run\n{stdout}"
    );

    let human_output = output_text(&run_openfang_success(daemon.home(), &["run", "list"]));
    assert!(
        human_output.contains("WORKFLOW") && human_output.contains("STEPS"),
        "run list should render the run table headers\n{human_output}"
    );
    assert!(
        human_output.contains(&run_id),
        "run list should include the started run in table output\n{human_output}"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn dispatch_list_json_should_return_valid_json_filtered_by_run() {
    let daemon = TestDaemon::start("dispatch-list-json").await;
    let client = reqwest::Client::new();
    let workflow_id = "cli-dispatch-filter";

    let agent = create_agent(&client, daemon.base_url(), DISPATCH_TEST_MANIFEST).await;
    let agent_name = agent["name"]
        .as_str()
        .expect("agent create response should include name")
        .to_string();

    create_workflow_definition(
        &client,
        daemon.base_url(),
        dispatch_workflow_definition(workflow_id, &agent_name),
    )
    .await;

    let run_id = start_workflow_run(
        &client,
        daemon.base_url(),
        workflow_id,
        "CLI dispatch filter",
    )
    .await;
    let dispatches = wait_for_dispatches_for_run(&client, daemon.base_url(), &run_id).await;
    assert!(
        !dispatches.is_empty(),
        "dispatch list should include at least one dispatch"
    );

    let stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["dispatch", "list", "--run_id", &run_id, "--json"],
    ));
    let body: Value = serde_json::from_str(&stdout).expect("dispatch list JSON should parse");
    let items = body
        .as_array()
        .expect("dispatch list JSON should be a filtered array");
    assert_eq!(items.len(), dispatches.len());
    assert!(
        items.iter().all(|item| item["run_id"] == json!(run_id)),
        "dispatch list JSON should only contain dispatches for the requested run\n{stdout}"
    );

    let human_output = output_text(&run_openfang_success(
        daemon.home(),
        &["dispatch", "list", "--run_id", &run_id],
    ));
    assert!(
        human_output.contains("RUN_ID") && human_output.contains("TARGET"),
        "dispatch list should render the dispatch table headers\n{human_output}"
    );
    assert!(
        human_output.contains(&run_id),
        "dispatch list should include the requested run in table output\n{human_output}"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn run_pause_nonexistent_should_return_error_without_panicking() {
    let daemon = TestDaemon::start("run-pause-missing").await;
    let run_id = "00000000-0000-0000-0000-000000000999";
    let output = run_openfang(daemon.home(), &["run", "pause", run_id]);
    let text = output_text(&output);

    assert!(
        output.status.code() == Some(1),
        "run pause should fail for a missing run\n{text}"
    );
    assert!(
        text.contains("Run not found"),
        "run pause should report a not-found error\n{text}"
    );
    assert!(
        !text.to_lowercase().contains("panic"),
        "run pause should not panic\n{text}"
    );

    daemon.shutdown().await;
}
