use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use openfang_api::server::run_daemon;
use openfang_kernel::OpenFangKernel;
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

const BUDGET_TEST_MANIFEST: &str = r#"
name = "budget-cli-tester"
version = "0.1.0"
description = "Budget CLI integration test agent"
author = "test"
module = "builtin:chat"

[model]
provider = "ollama"
model = "test-model"
system_prompt = "You are a budget CLI integration test agent."

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

        tokio::spawn(async move {
            run_daemon(
                OpenFangKernel::boot_with_config(config).expect("kernel should boot for CLI tests"),
                &listen_addr,
                Some(daemon_info_path.as_path()),
            )
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

#[derive(Clone, Default)]
struct A2aCaptureState {
    send_request: Arc<Mutex<Option<Value>>>,
    get_request: Arc<Mutex<Option<Value>>>,
}

struct A2aMockServer {
    base_url: String,
    rpc_url: String,
    state: A2aCaptureState,
}

impl A2aMockServer {
    fn start() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("A2A mock server should bind to a local port");
        let address = listener
            .local_addr()
            .expect("A2A mock server should expose a local address")
            .to_string();
        let base_url = format!("http://{address}");
        let rpc_url = format!("{base_url}/rpc");
        let state = A2aCaptureState::default();
        let thread_state = state.clone();
        let thread_base_url = base_url.clone();
        let thread_rpc_url = rpc_url.clone();

        std::thread::spawn(move || {
            for _ in 0..8 {
                let (stream, _) = listener
                    .accept()
                    .expect("A2A mock server should accept a request");
                handle_a2a_connection(stream, &thread_state, &thread_base_url, &thread_rpc_url);
            }
        });

        Self {
            base_url,
            rpc_url,
            state,
        }
    }
}

fn handle_a2a_connection(
    stream: std::net::TcpStream,
    state: &A2aCaptureState,
    base_url: &str,
    rpc_url: &str,
) {
    let mut reader = BufReader::new(stream);

    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .expect("A2A mock server should read the request line");
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or("GET");
    let path = request_parts.next().unwrap_or("/").to_string();

    let mut content_length = 0usize;
    loop {
        let mut header_line = String::new();
        reader
            .read_line(&mut header_line)
            .expect("A2A mock server should read the next header");
        if header_line == "\r\n" {
            break;
        }

        if let Some((name, value)) = header_line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().unwrap_or(0);
            }
        }
    }

    let mut body = vec![0; content_length];
    if content_length > 0 {
        reader
            .read_exact(&mut body)
            .expect("A2A mock server should read the request body");
    }

    match (method, path.as_str()) {
        ("GET", "/.well-known/agent.json") => write_http_json_response(
            reader.into_inner(),
            "200 OK",
            &json!({
                "name": "cli-mock-agent",
                "description": "A2A mock server for CLI tests",
                "url": rpc_url,
                "version": "1.0.0",
                "capabilities": {
                    "streaming": false,
                    "pushNotifications": false,
                    "stateTransitionHistory": true,
                },
                "skills": [{
                    "id": "review",
                    "name": "Review",
                    "description": "Reviews tasks",
                    "tags": ["cli"],
                    "examples": ["Review this change"],
                }],
                "defaultInputModes": ["text/plain"],
                "defaultOutputModes": ["text/plain"],
                "metadata": {
                    "baseUrl": base_url,
                }
            })
            .to_string(),
        ),
        ("POST", "/rpc") => {
            let body_json: Value =
                serde_json::from_slice(&body).expect("A2A mock server should receive valid JSON");
            match body_json.get("method").and_then(Value::as_str) {
                Some("tasks/send") => {
                    *state
                        .send_request
                        .lock()
                        .expect("A2A mock server should capture send request") =
                        Some(body_json.clone());
                    let session_id = body_json
                        .pointer("/params/sessionId")
                        .cloned()
                        .unwrap_or(Value::Null);
                    write_http_json_response(
                        reader.into_inner(),
                        "200 OK",
                        &json!({
                            "jsonrpc": "2.0",
                            "id": 1,
                            "result": {
                                "id": "task_cli_123",
                                "sessionId": session_id,
                                "status": "working",
                                "messages": [{
                                    "role": "user",
                                    "parts": [{
                                        "type": "text",
                                        "text": body_json
                                            .pointer("/params/message/parts/0/text")
                                            .and_then(Value::as_str)
                                            .unwrap_or(""),
                                    }],
                                }],
                                "artifacts": [],
                            }
                        })
                        .to_string(),
                    );
                }
                Some("tasks/get") => {
                    *state
                        .get_request
                        .lock()
                        .expect("A2A mock server should capture get request") =
                        Some(body_json.clone());
                    let task_id = body_json
                        .pointer("/params/id")
                        .and_then(Value::as_str)
                        .unwrap_or("task_cli_123");
                    write_http_json_response(
                        reader.into_inner(),
                        "200 OK",
                        &json!({
                            "jsonrpc": "2.0",
                            "id": 1,
                            "result": {
                                "id": task_id,
                                "sessionId": "session-123",
                                "status": {
                                    "state": "completed",
                                    "message": null,
                                },
                                "messages": [{
                                    "role": "agent",
                                    "parts": [{
                                        "type": "text",
                                        "text": "Task completed",
                                    }],
                                }],
                                "artifacts": [],
                            }
                        })
                        .to_string(),
                    );
                }
                _ => write_http_json_response(
                    reader.into_inner(),
                    "400 Bad Request",
                    &json!({
                        "error": "unsupported method",
                    })
                    .to_string(),
                ),
            }
        }
        _ => write_http_json_response(
            reader.into_inner(),
            "404 Not Found",
            &json!({
                "error": "not found",
            })
            .to_string(),
        ),
    }
}

fn write_http_json_response(mut stream: std::net::TcpStream, status_line: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .expect("mock server should write a response");
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

async fn create_agent(client: &reqwest::Client, base_url: &str, manifest_toml: &str) -> Value {
    let response = client
        .post(format!("{base_url}/api/agents"))
        .json(&json!({ "manifest_toml": manifest_toml }))
        .send()
        .await
        .expect("agent create request should succeed");
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .expect("agent create response should be JSON");
    assert_eq!(
        status,
        reqwest::StatusCode::CREATED,
        "agent create request should succeed: {body}"
    );
    body
}

#[test]
fn top_level_help_should_list_a2a_peers_and_budget_commands() {
    let home = TestHome::new("top-help");
    let output = run_openfang(home.path(), &["--help"]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "top-level help should exit successfully\n{text}"
    );

    for subcommand in ["a2a", "peers", "budget"] {
        assert!(
            help_contains_subcommand(&text, subcommand),
            "top-level help should include '{subcommand}'\n{text}"
        );
    }
}

#[test]
fn a2a_help_should_list_all_subcommands() {
    let home = TestHome::new("a2a-help");
    let output = run_openfang(home.path(), &["a2a", "--help"]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "a2a help should exit successfully\n{text}"
    );

    for subcommand in ["list", "discover", "send", "status"] {
        assert!(
            help_contains_subcommand(&text, subcommand),
            "a2a help should include '{subcommand}'\n{text}"
        );
    }
}

#[test]
fn peers_help_should_list_all_subcommands() {
    let home = TestHome::new("peers-help");
    let output = run_openfang(home.path(), &["peers", "--help"]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "peers help should exit successfully\n{text}"
    );

    for subcommand in ["list", "status"] {
        assert!(
            help_contains_subcommand(&text, subcommand),
            "peers help should include '{subcommand}'\n{text}"
        );
    }
}

#[test]
fn budget_help_should_list_all_subcommands() {
    let home = TestHome::new("budget-help");
    let output = run_openfang(home.path(), &["budget", "--help"]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "budget help should exit successfully\n{text}"
    );

    for subcommand in ["status", "update", "agents", "agent"] {
        assert!(
            help_contains_subcommand(&text, subcommand),
            "budget help should include '{subcommand}'\n{text}"
        );
    }
}

#[test]
fn a2a_list_without_daemon_should_explain_daemon_requirement() {
    let home = TestHome::new("a2a-list-no-daemon");
    let output = run_openfang(home.path(), &["a2a", "list"]);
    let text = output_text(&output);

    assert!(
        output.status.code() == Some(1),
        "a2a list should fail without a daemon\n{text}"
    );
    assert!(
        text.contains("requires a running daemon"),
        "a2a list should explain the daemon requirement\n{text}"
    );
}

#[test]
fn peers_list_without_daemon_should_explain_daemon_requirement() {
    let home = TestHome::new("peers-list-no-daemon");
    let output = run_openfang(home.path(), &["peers", "list"]);
    let text = output_text(&output);

    assert!(
        output.status.code() == Some(1),
        "peers list should fail without a daemon\n{text}"
    );
    assert!(
        text.contains("requires a running daemon"),
        "peers list should explain the daemon requirement\n{text}"
    );
}

#[test]
fn budget_status_without_daemon_should_explain_daemon_requirement() {
    let home = TestHome::new("budget-status-no-daemon");
    let output = run_openfang(home.path(), &["budget", "status"]);
    let text = output_text(&output);

    assert!(
        output.status.code() == Some(1),
        "budget status should fail without a daemon\n{text}"
    );
    assert!(
        text.contains("requires a running daemon"),
        "budget status should explain the daemon requirement\n{text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a2a_commands_should_discover_send_and_check_status() {
    let daemon = TestDaemon::start("a2a-runtime").await;
    let mock = A2aMockServer::start();

    let discover_stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["a2a", "discover", &mock.base_url, "--json"],
    ));
    let discover_body: Value =
        serde_json::from_str(&discover_stdout).expect("a2a discover JSON should parse");
    assert_eq!(discover_body["agent"]["name"], json!("cli-mock-agent"));
    assert_eq!(discover_body["agent"]["url"], json!(mock.rpc_url.clone()));

    let list_stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["a2a", "list", "--json"],
    ));
    let list_body: Value = serde_json::from_str(&list_stdout).expect("a2a list JSON should parse");
    let items = list_body
        .as_array()
        .expect("a2a list JSON should be an array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"], json!("cli-mock-agent"));
    assert_eq!(items[0]["url"], json!(mock.rpc_url.clone()));

    let human_list = output_text(&run_openfang_success(daemon.home(), &["a2a", "list"]));
    assert!(
        human_list.contains("cli-mock-agent"),
        "human a2a list should include the discovered agent\n{human_list}"
    );

    let send_stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &[
            "a2a",
            "send",
            &mock.rpc_url,
            "Ship the release",
            "--session-id",
            "session-123",
            "--json",
        ],
    ));
    let send_body: Value = serde_json::from_str(&send_stdout).expect("a2a send JSON should parse");
    assert_eq!(send_body["id"], json!("task_cli_123"));
    assert_eq!(send_body["status"], json!("working"));

    let status_stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["a2a", "status", "task_cli_123", "--json"],
    ));
    let status_body: Value =
        serde_json::from_str(&status_stdout).expect("a2a status JSON should parse");
    assert_eq!(status_body["id"], json!("task_cli_123"));
    assert_eq!(status_body["status"]["state"], json!("completed"));

    let captured_send = mock
        .state
        .send_request
        .lock()
        .expect("send request should be captured")
        .clone()
        .expect("send request should exist");
    assert_eq!(captured_send["method"], json!("tasks/send"));
    assert_eq!(
        captured_send["params"]["message"]["parts"][0]["text"],
        json!("Ship the release")
    );
    assert_eq!(captured_send["params"]["sessionId"], json!("session-123"));

    let captured_get = mock
        .state
        .get_request
        .lock()
        .expect("get request should be captured")
        .clone()
        .expect("get request should exist");
    assert_eq!(captured_get["method"], json!("tasks/get"));
    assert_eq!(captured_get["params"]["id"], json!("task_cli_123"));

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn peers_and_budget_commands_should_return_valid_json_with_running_daemon() {
    let daemon = TestDaemon::start("peers-budget-runtime").await;
    let client = reqwest::Client::new();
    let agent = create_agent(&client, daemon.base_url(), BUDGET_TEST_MANIFEST).await;
    let agent_id = agent["agent_id"]
        .as_str()
        .expect("agent create response should include an agent_id")
        .to_string();

    let peers_stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["peers", "list", "--json"],
    ));
    let peers_body: Value =
        serde_json::from_str(&peers_stdout).expect("peers list JSON should parse");
    let peers = peers_body
        .as_array()
        .expect("peers list JSON should be an array");
    assert!(
        peers.is_empty(),
        "fresh test daemon should not report peers by default\n{peers_stdout}"
    );

    let status_stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["peers", "status", "--json"],
    ));
    let status_body: Value =
        serde_json::from_str(&status_stdout).expect("peers status JSON should parse");
    assert!(
        status_body
            .get("enabled")
            .and_then(Value::as_bool)
            .is_some(),
        "peers status should include the enabled flag\n{status_stdout}"
    );

    let budget_stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["budget", "status", "--json"],
    ));
    let budget_body: Value =
        serde_json::from_str(&budget_stdout).expect("budget status JSON should parse");
    assert!(
        budget_body
            .get("hourly_limit")
            .and_then(Value::as_f64)
            .is_some(),
        "budget status should include hourly_limit\n{budget_stdout}"
    );

    let update_stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &[
            "budget",
            "update",
            "--hourly",
            "1.25",
            "--daily",
            "4.50",
            "--monthly",
            "10.00",
            "--json",
        ],
    ));
    let update_body: Value =
        serde_json::from_str(&update_stdout).expect("budget update JSON should parse");
    assert_eq!(update_body["hourly_limit"], json!(1.25));
    assert_eq!(update_body["daily_limit"], json!(4.5));
    assert_eq!(update_body["monthly_limit"], json!(10.0));

    let agents_stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["budget", "agents", "--json"],
    ));
    let agents_body: Value =
        serde_json::from_str(&agents_stdout).expect("budget agents JSON should parse");
    let ranking = agents_body
        .as_array()
        .expect("budget agents JSON should be an array");
    assert!(
        ranking.is_empty(),
        "budget agents should be empty before any spend is recorded\n{agents_stdout}"
    );

    let agent_stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["budget", "agent", &agent_id, "--json"],
    ));
    let agent_body: Value =
        serde_json::from_str(&agent_stdout).expect("budget agent JSON should parse");
    assert_eq!(agent_body["agent_id"], json!(agent_id));
    assert!(
        agent_body.get("hourly").is_some() && agent_body.get("tokens").is_some(),
        "budget agent response should include hourly and tokens sections\n{agent_stdout}"
    );

    daemon.shutdown().await;
}
