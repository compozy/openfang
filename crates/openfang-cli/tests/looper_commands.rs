use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use openfang_api::server::{run_daemon, DaemonInfo};
use openfang_kernel::OpenFangKernel;
use openfang_memory::{LooperRunRepository, NewLooperRun, TaskRepository};
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use openfang_types::looper::{LooperExecutionMode, LooperRunId, LooperRunStatus};
use openfang_types::task::{
    ActorKind, Complexity, OwnerRef, Priority, TaskId, TaskRecord, TaskSource, TaskStatus,
};
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

struct SeedLooperRun {
    looper_run_id: String,
    task_id: String,
    status: LooperRunStatus,
    mode: LooperExecutionMode,
    completed: u64,
    total: u64,
    failed: u64,
}

struct TestDaemon {
    home: TestHome,
    base_url: String,
}

impl TestDaemon {
    async fn start(name: &str) -> Self {
        Self::start_seeded(name, &[]).await
    }

    async fn start_seeded(name: &str, runs: &[SeedLooperRun]) -> Self {
        let home = TestHome::new(name);
        let listen_addr = reserve_listen_addr();
        let daemon_info_path = home.path().join("daemon.json");
        let base_url = format!("http://{listen_addr}");

        let mut provider_urls = std::collections::HashMap::new();
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
            .expect("kernel should boot for CLI looper tests");
        let task_repo = kernel.workflow_stores.task.clone();
        let looper_run_repo = kernel.workflow_stores.looper_run.clone();

        tokio::spawn(async move {
            run_daemon(kernel, &listen_addr, Some(daemon_info_path.as_path()))
                .await
                .expect("CLI looper test daemon should stay available");
        });

        wait_for_health(&base_url).await;

        let mut seeded_task_ids = BTreeSet::new();
        for run in runs {
            if seeded_task_ids.insert(run.task_id.clone()) {
                seed_task(&task_repo, &run.task_id);
            }
        }

        for run in runs {
            seed_looper_run(&looper_run_repo, run);
        }

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

fn wait_for_health_blocking(base_url: &str) {
    let client = reqwest::blocking::Client::new();
    let health_url = format!("{base_url}/api/health");
    for _ in 0..50 {
        if let Ok(response) = client.get(&health_url).send() {
            if response.status().is_success() {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("test daemon did not become healthy: {health_url}");
}

fn sample_task(task_id: &str) -> TaskRecord {
    TaskRecord {
        task_id: TaskId::new(task_id),
        slug: format!("{task_id}-slug"),
        source: TaskSource::Manual,
        title: format!("Task {task_id}"),
        description: "Looper CLI test task".to_string(),
        status: TaskStatus::Planned,
        priority: Priority::High,
        complexity: Complexity::Medium,
        position: 1,
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
        artifact_refs: vec![],
        doc_refs: vec![],
        file_refs: vec![],
        metadata: json!({}),
        created_at: "2026-03-27T12:00:00Z".to_string(),
        updated_at: "2026-03-27T12:00:00Z".to_string(),
        completed_at: None,
    }
}

fn seed_task(task_repo: &TaskRepository, task_id: &str) {
    task_repo
        .create(&sample_task(task_id))
        .expect("seed task should persist");
}

fn seed_looper_run(looper_run_repo: &LooperRunRepository, run: &SeedLooperRun) {
    looper_run_repo
        .create(&NewLooperRun {
            looper_run_id: LooperRunId::new(&run.looper_run_id),
            task_id: TaskId::new(&run.task_id),
            source_run_id: Some("run_source_cli".to_string()),
            status: run.status,
            execution_policy_json: json!({
                "mode": run.mode.as_str(),
                "max_parallelism": 4,
                "selection": "priority"
            }),
            current_subtask_id: None,
            progress_json: json!({
                "total": run.total,
                "completed": run.completed,
                "failed": run.failed
            }),
            error_json: None,
            started_at: "2026-03-27T12:00:00Z".to_string(),
            updated_at: "2026-03-27T12:01:00Z".to_string(),
            completed_at: run
                .status
                .is_terminal()
                .then(|| "2026-03-27T12:02:00Z".to_string()),
        })
        .expect("seed looper run should persist");
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

    write_http_json_response(
        reader.into_inner(),
        &json!({
            "accepted": true,
            "resource_id": "loop_created_capture",
            "status": "accepted",
            "looper_run_id": "loop_created_capture"
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
fn looper_help_should_list_all_subcommands() {
    let home = TestHome::new("looper-help");
    let output = run_openfang(home.path(), &["looper", "--help"]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "looper help should exit successfully\n{text}"
    );

    for subcommand in [
        "list", "get", "create", "subtasks", "pause", "resume", "cancel", "watch",
    ] {
        assert!(
            help_contains_subcommand(&text, subcommand),
            "looper help should include '{subcommand}'\n{text}"
        );
    }
}

#[test]
fn looper_list_should_require_running_daemon() {
    let home = TestHome::new("looper-list-no-daemon");
    let output = run_openfang(home.path(), &["looper", "list"]);
    let text = output_text(&output);

    assert!(
        output.status.code() == Some(1),
        "looper list should fail without a daemon\n{text}"
    );
    assert!(
        text.contains("requires a running daemon"),
        "looper list should explain the daemon requirement\n{text}"
    );
}

#[test]
fn looper_create_missing_file_should_report_not_found() {
    let home = TestHome::new("looper-create-missing-file");
    let output = run_openfang(home.path(), &["looper", "create", "nonexistent.json"]);
    let text = output_text(&output);

    assert!(
        output.status.code() == Some(1),
        "looper create should fail for a missing file\n{text}"
    );
    assert!(
        text.contains("Looper request file not found"),
        "looper create should report a file-not-found error\n{text}"
    );
}

#[test]
fn looper_create_should_send_json_file_body_and_print_looper_run_id() {
    let home = TestHome::new("looper-create-body");
    let state = CaptureState::default();
    let listen_addr = start_capture_server(state.clone());
    write_daemon_info(home.path(), &listen_addr);
    wait_for_health_blocking(&format!("http://{listen_addr}"));

    let request_path = home.path().join("looper-request.json");
    fs::write(
        &request_path,
        serde_json::to_vec_pretty(&json!({
            "task_id": "task_create_cli",
            "subtask_ids": null,
            "execution_policy": {
                "mode": "parallel",
                "max_parallelism": 4,
                "selection": "priority"
            }
        }))
        .expect("request should serialize"),
    )
    .expect("request file should be written");

    let output = run_openfang_success(
        home.path(),
        &[
            "looper",
            "create",
            request_path.to_str().unwrap_or_default(),
        ],
    );
    let text = output_text(&output);

    assert!(
        text.contains("Looper run creation accepted."),
        "looper create should print confirmation output\n{text}"
    );
    assert!(
        text.contains("loop_created_capture"),
        "looper create should print the returned looper run id\n{text}"
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

    assert_eq!(captured_uri.as_deref(), Some("/api/v1/looper-runs"));
    assert_eq!(
        captured_body,
        Some(json!({
            "task_id": "task_create_cli",
            "subtask_ids": null,
            "execution_policy": {
                "mode": "parallel",
                "max_parallelism": 4,
                "selection": "priority"
            }
        }))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn looper_list_json_should_return_valid_json_and_render_progress_column() {
    let daemon = TestDaemon::start_seeded(
        "looper-list-json",
        &[
            SeedLooperRun {
                looper_run_id: "loop_running_cli".to_string(),
                task_id: "task_looper_running".to_string(),
                status: LooperRunStatus::Running,
                mode: LooperExecutionMode::Parallel,
                completed: 3,
                total: 12,
                failed: 0,
            },
            SeedLooperRun {
                looper_run_id: "loop_completed_cli".to_string(),
                task_id: "task_looper_completed".to_string(),
                status: LooperRunStatus::Completed,
                mode: LooperExecutionMode::Sequential,
                completed: 2,
                total: 2,
                failed: 0,
            },
        ],
    )
    .await;

    let stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["looper", "list", "--json"],
    ));
    let body: Value = serde_json::from_str(&stdout).expect("looper list JSON should parse");
    let items = body
        .as_array()
        .expect("looper list JSON should be an array");

    assert!(
        items
            .iter()
            .any(|item| item["id"] == json!("loop_running_cli")),
        "looper list JSON should include the running looper run\n{stdout}"
    );
    assert!(
        items
            .iter()
            .any(|item| item["id"] == json!("loop_completed_cli")),
        "looper list JSON should include the completed looper run\n{stdout}"
    );

    let human_output = output_text(&run_openfang_success(daemon.home(), &["looper", "list"]));
    assert!(
        human_output.contains("TASK_ID") && human_output.contains("PROGRESS"),
        "looper list should render the looper table headers\n{human_output}"
    );
    assert!(
        human_output.contains("loop_running_cli") && human_output.contains("3/12"),
        "looper list should render the progress column in completed/total format\n{human_output}"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn looper_list_json_should_filter_running_status_and_execution_mode() {
    let daemon = TestDaemon::start_seeded(
        "looper-list-filter",
        &[
            SeedLooperRun {
                looper_run_id: "loop_running_parallel".to_string(),
                task_id: "task_filter_running".to_string(),
                status: LooperRunStatus::Running,
                mode: LooperExecutionMode::Parallel,
                completed: 1,
                total: 4,
                failed: 0,
            },
            SeedLooperRun {
                looper_run_id: "loop_completed_sequential".to_string(),
                task_id: "task_filter_completed".to_string(),
                status: LooperRunStatus::Completed,
                mode: LooperExecutionMode::Sequential,
                completed: 4,
                total: 4,
                failed: 0,
            },
        ],
    )
    .await;

    let running_stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["looper", "list", "--status", "running", "--json"],
    ));
    let running_body: Value =
        serde_json::from_str(&running_stdout).expect("filtered looper list JSON should parse");
    let running_items = running_body
        .as_array()
        .expect("filtered looper list JSON should be an array");

    assert_eq!(running_items.len(), 1);
    assert_eq!(running_items[0]["id"], json!("loop_running_parallel"));
    assert_eq!(running_items[0]["status"], json!("running"));

    let mode_stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["looper", "list", "--execution_mode", "sequential", "--json"],
    ));
    let mode_body: Value =
        serde_json::from_str(&mode_stdout).expect("execution-mode filtered JSON should parse");
    let mode_items = mode_body
        .as_array()
        .expect("execution-mode filtered JSON should be an array");

    assert_eq!(mode_items.len(), 1);
    assert_eq!(mode_items[0]["id"], json!("loop_completed_sequential"));
    assert_eq!(
        mode_items[0]["execution_policy"]["mode"],
        json!("sequential")
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn looper_pause_nonexistent_should_return_error_without_panicking() {
    let daemon = TestDaemon::start("looper-pause-missing").await;
    let output = run_openfang(daemon.home(), &["looper", "pause", "loop_missing"]);
    let text = output_text(&output);

    assert!(
        output.status.code() == Some(1),
        "looper pause should fail for a missing looper run\n{text}"
    );
    assert!(
        text.contains("Looper run not found"),
        "looper pause should report a not-found error\n{text}"
    );
    assert!(
        !text.to_lowercase().contains("panic"),
        "looper pause should not panic\n{text}"
    );

    daemon.shutdown().await;
}
