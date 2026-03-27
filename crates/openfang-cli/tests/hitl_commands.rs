use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::Utc;
use openfang_api::server::{run_daemon, DaemonInfo};
use openfang_kernel::OpenFangKernel;
use openfang_memory::{
    HitlKind, HitlRepository, NewHitlRequest, WorkflowRunRecord, WorkflowRunStatus,
};
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

struct SeedHitlRequest {
    hitl_id: String,
    run_id: String,
    kind: HitlKind,
    question: String,
    answered: bool,
}

struct TestDaemon {
    home: TestHome,
    base_url: String,
}

impl TestDaemon {
    async fn start(name: &str) -> Self {
        Self::start_seeded(name, &[]).await
    }

    async fn start_seeded(name: &str, requests: &[SeedHitlRequest]) -> Self {
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
            .expect("kernel should boot for CLI hitl tests");

        let mut seeded_run_ids = BTreeSet::new();
        for request in requests {
            if seeded_run_ids.insert(request.run_id.clone()) {
                seed_run(&kernel, &request.run_id);
            }
        }

        for request in requests {
            seed_hitl_request(&kernel, request).await;
        }

        tokio::spawn(async move {
            run_daemon(kernel, &listen_addr, Some(daemon_info_path.as_path()))
                .await
                .expect("CLI hitl test daemon should stay available");
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

#[derive(Clone, Default)]
struct CaptureState {
    request_uri: Arc<Mutex<Option<String>>>,
    request_body: Arc<Mutex<Option<Value>>>,
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

fn start_capture_server(state: CaptureState) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("capture server should bind to a local port");
    let address = listener
        .local_addr()
        .expect("capture server should expose a local address")
        .to_string();

    std::thread::spawn(move || {
        for _ in 0..4 {
            let (stream, _) = listener
                .accept()
                .expect("capture server should accept a request");
            handle_capture_connection(stream, &state);
        }
    });

    address
}

fn handle_capture_connection(stream: std::net::TcpStream, state: &CaptureState) {
    let mut reader = BufReader::new(stream);

    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .expect("capture server should read the request line");
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();

    let mut content_length = 0usize;
    loop {
        let mut header_line = String::new();
        reader
            .read_line(&mut header_line)
            .expect("capture server should read the next header");
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
            .expect("capture server should read the request body");
    }

    if path == "/api/health" {
        write_http_json_response(reader.into_inner(), &json!({ "status": "ok" }).to_string());
        return;
    }

    *state
        .request_uri
        .lock()
        .expect("capture server should lock the request uri") = Some(path.clone());
    *state
        .request_body
        .lock()
        .expect("capture server should lock the request body") =
        Some(serde_json::from_slice(&body).expect("capture server should receive valid JSON"));

    let hitl_id = path
        .trim_start_matches("/api/v1/hitl-requests/")
        .trim_end_matches("/answer")
        .to_string();
    write_http_json_response(
        reader.into_inner(),
        &json!({
            "accepted": true,
            "resource_id": hitl_id,
            "status": "accepted"
        })
        .to_string(),
    );
}

fn write_http_json_response(mut stream: std::net::TcpStream, body: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .expect("capture server should write a response");
}

async fn seed_hitl_request(kernel: &OpenFangKernel, request: &SeedHitlRequest) {
    let created = kernel
        .workflow_stores
        .hitl
        .create(NewHitlRequest {
            hitl_request_id: request.hitl_id.clone(),
            run_id: request.run_id.clone(),
            step_id: "review_step".to_string(),
            dispatch_id: None,
            kind: request.kind,
            question: request.question.clone(),
            context_json: json!({ "source": "cli-test" }),
            created_at: Utc::now(),
            timeout_at: None,
        })
        .await
        .expect("seed HITL request should persist");

    if request.answered {
        kernel
            .workflow_stores
            .hitl
            .answer(&created.hitl_request_id, &json!("Approved"), Utc::now())
            .await
            .expect("seed HITL request should answer");
    }
}

fn seed_run(kernel: &OpenFangKernel, run_id: &str) {
    kernel
        .workflow_stores
        .workflow_run
        .insert_run(&sample_run(run_id))
        .expect("seed workflow run should persist");
}

fn sample_run(run_id: &str) -> WorkflowRunRecord {
    WorkflowRunRecord {
        run_id: run_id.to_string(),
        workflow_id: "workflow-cli-hitl".to_string(),
        workflow_version: "1.0.0".to_string(),
        status: WorkflowRunStatus::Running,
        input_json: json!({ "artifact": "prd" }).to_string(),
        vars_json: json!({}).to_string(),
        current_step_id: Some("review_step".to_string()),
        waiting_kind: None,
        waiting_ref: None,
        active_dispatch_id: None,
        active_hitl_request_id: None,
        labels_json: json!(["cli", "hitl"]).to_string(),
        metadata_json: json!({ "source": "cli-test" }).to_string(),
        error_json: None,
        started_at: "2026-03-27T10:00:00Z".to_string(),
        updated_at: "2026-03-27T10:00:00Z".to_string(),
        completed_at: None,
    }
}

fn truncate_text(text: &str, max_width: usize) -> String {
    if text.chars().count() <= max_width {
        return text.to_string();
    }

    let keep = max_width.saturating_sub(3);
    format!("{}...", text.chars().take(keep).collect::<String>())
}

fn write_daemon_info(home: &Path, listen_addr: &str) {
    let daemon_info = DaemonInfo {
        pid: std::process::id(),
        listen_addr: listen_addr.to_string(),
        started_at: "2026-03-27T00:00:00Z".to_string(),
        version: "test".to_string(),
        platform: "test".to_string(),
    };
    fs::write(
        home.join("daemon.json"),
        serde_json::to_vec(&daemon_info).expect("daemon info should serialize"),
    )
    .expect("daemon info should be written");
}

#[test]
fn hitl_help_should_list_all_subcommands() {
    let home = TestHome::new("hitl-help");
    let output = run_openfang(home.path(), &["hitl", "--help"]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "hitl help should exit successfully\n{text}"
    );

    for subcommand in ["list", "get", "answer", "cancel", "watch"] {
        assert!(
            help_contains_subcommand(&text, subcommand),
            "hitl help should include '{subcommand}'\n{text}"
        );
    }
}

#[test]
fn hitl_list_should_require_running_daemon() {
    let home = TestHome::new("hitl-list-no-daemon");
    let output = run_openfang(home.path(), &["hitl", "list"]);
    let text = output_text(&output);

    assert!(
        output.status.code() == Some(1),
        "hitl list should fail without a daemon\n{text}"
    );
    assert!(
        text.contains("requires a running daemon"),
        "hitl list should explain the daemon requirement\n{text}"
    );
}

#[test]
fn hitl_answer_without_required_arguments_should_show_usage_help() {
    let home = TestHome::new("hitl-answer-missing-args");
    let output = run_openfang(home.path(), &["hitl", "answer"]);
    let text = output_text(&output);

    assert!(
        !output.status.success(),
        "hitl answer without args should fail\n{text}"
    );
    assert!(
        text.contains("Usage:") && text.contains("hitl answer"),
        "hitl answer without args should print usage help\n{text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn hitl_list_json_should_return_valid_json_array_with_running_daemon() {
    let long_question = "This is a deliberately verbose HITL question that should be truncated in the CLI table output.";
    let daemon = TestDaemon::start_seeded(
        "hitl-list-json",
        &[
            SeedHitlRequest {
                hitl_id: "hitl_pending_cli".to_string(),
                run_id: "run_hitl_cli".to_string(),
                kind: HitlKind::Clarification,
                question: long_question.to_string(),
                answered: false,
            },
            SeedHitlRequest {
                hitl_id: "hitl_answered_cli".to_string(),
                run_id: "run_hitl_cli".to_string(),
                kind: HitlKind::Approval,
                question: "Has this request already been handled?".to_string(),
                answered: true,
            },
        ],
    )
    .await;

    let stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["hitl", "list", "--json"],
    ));
    let body: Value = serde_json::from_str(&stdout).expect("hitl list JSON should parse");
    let items = body.as_array().expect("hitl list JSON should be an array");
    assert!(
        items
            .iter()
            .any(|item| item["id"] == json!("hitl_pending_cli")),
        "hitl list JSON should include the pending request\n{stdout}"
    );
    assert!(
        items
            .iter()
            .any(|item| item["id"] == json!("hitl_answered_cli")),
        "hitl list JSON should include the answered request\n{stdout}"
    );

    let human_output = output_text(&run_openfang_success(daemon.home(), &["hitl", "list"]));
    assert!(
        human_output.contains("RUN_ID") && human_output.contains("QUESTION"),
        "hitl list should render the HITL table headers\n{human_output}"
    );
    assert!(
        human_output.contains("run_hitl_cli"),
        "hitl list should include the run_id column in table output\n{human_output}"
    );

    let truncated_question = truncate_text(long_question, 40);
    assert!(
        human_output.contains(&truncated_question),
        "hitl list should truncate the QUESTION column to 40 characters\n{human_output}"
    );
    assert!(
        !human_output.contains(long_question),
        "hitl list should not print the full long question in table output\n{human_output}"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn hitl_list_json_should_filter_pending_requests_with_running_daemon() {
    let daemon = TestDaemon::start_seeded(
        "hitl-list-filter",
        &[
            SeedHitlRequest {
                hitl_id: "hitl_pending_filter".to_string(),
                run_id: "run_pending_filter".to_string(),
                kind: HitlKind::Approval,
                question: "Approve deployment?".to_string(),
                answered: false,
            },
            SeedHitlRequest {
                hitl_id: "hitl_answered_filter".to_string(),
                run_id: "run_answered_filter".to_string(),
                kind: HitlKind::Clarification,
                question: "Need more context?".to_string(),
                answered: true,
            },
        ],
    )
    .await;

    let stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["hitl", "list", "--status", "pending", "--json"],
    ));
    let body: Value = serde_json::from_str(&stdout).expect("filtered hitl list JSON should parse");
    let items = body
        .as_array()
        .expect("filtered hitl list JSON should be an array");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], json!("hitl_pending_filter"));
    assert_eq!(items[0]["status"], json!("pending"));

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn hitl_cancel_nonexistent_should_return_error_without_panicking() {
    let daemon = TestDaemon::start("hitl-cancel-missing").await;
    let hitl_id = "hitl_missing_request";
    let output = run_openfang(daemon.home(), &["hitl", "cancel", hitl_id]);
    let text = output_text(&output);

    assert!(
        output.status.code() == Some(1),
        "hitl cancel should fail for a missing request\n{text}"
    );
    assert!(
        text.contains("HITL request not found"),
        "hitl cancel should report a not-found error\n{text}"
    );
    assert!(
        !text.to_lowercase().contains("panic"),
        "hitl cancel should not panic\n{text}"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn hitl_answer_should_send_plain_text_response_in_json_body() {
    let home = TestHome::new("hitl-answer-body");
    let state = CaptureState::default();
    let listen_addr = start_capture_server(state.clone());
    write_daemon_info(home.path(), &listen_addr);

    wait_for_health(&format!("http://{listen_addr}")).await;

    let output = run_openfang_success(
        home.path(),
        &["hitl", "answer", "req_123", "Yes, approve the deployment"],
    );
    let text = output_text(&output);

    assert!(
        text.contains("Answer submitted for HITL request req_123."),
        "hitl answer should print confirmation output\n{text}"
    );

    let captured_uri = state
        .request_uri
        .lock()
        .expect("test should lock the captured request uri")
        .clone();
    let captured_body = state
        .request_body
        .lock()
        .expect("test should lock the captured request body")
        .clone();

    assert_eq!(
        captured_uri.as_deref(),
        Some("/api/v1/hitl-requests/req_123/answer")
    );
    assert_eq!(
        captured_body,
        Some(json!({
            "response": "Yes, approve the deployment"
        }))
    );
}
