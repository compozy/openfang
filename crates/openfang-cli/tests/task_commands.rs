use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

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
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
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

async fn create_task_via_http(base_url: &str, task_id: &str) {
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{base_url}/api/v1/tasks"))
        .json(&task_payload(task_id, "Seed task from HTTP"))
        .send()
        .await
        .expect("seed task request should succeed");
    assert!(
        response.status().is_success(),
        "seed task request should succeed: {}",
        response.status()
    );
}

fn task_payload(task_id: &str, title: &str) -> Value {
    json!({
        "id": task_id,
        "slug": format!("{task_id}-slug"),
        "title": title,
        "description": format!("Description for {title}"),
        "status": "planned",
        "priority": "high",
        "complexity": "medium",
        "position": 1,
        "source": { "kind": "manual" },
        "owner": { "kind": "agent_group", "ref": "sdlc" },
        "created_by": { "kind": "agent", "ref": "planner" },
        "repository_refs": [{ "repository_id": "repo_main", "role": "primary" }],
        "label_refs": ["planning", "cli"],
        "artifact_refs": [{ "artifact_id": "artifact_001", "type": "prd", "current_version_id": "artifact_v3" }],
        "doc_refs": [{ "doc_id": "doc_001", "type": "brief", "current_version_id": "doc_v2" }],
        "file_refs": [{ "path": "docs/prd.md", "kind": "workspace", "description": "Current PRD draft" }],
        "metadata": { "area": "product" }
    })
}

fn subtask_payload(subtask_id: &str, title: &str) -> Value {
    json!({
        "id": subtask_id,
        "title": title,
        "description": format!("Description for {title}"),
        "kind": "doc_change",
        "status": "planned",
        "complexity": "medium",
        "position": 1,
        "assignee": { "kind": "agent", "ref": "writer" },
        "depends_on": [],
        "parallelizable": false,
        "input": { "artifact_id": "artifact_001" },
        "metadata": {}
    })
}

#[test]
fn task_help_should_list_all_subcommands() {
    let home = TestHome::new("task-help");
    let output = run_openfang(home.path(), &["task", "--help"]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "task help should exit successfully\n{text}"
    );

    for subcommand in [
        "list",
        "get",
        "create",
        "update",
        "delete",
        "replan",
        "subtasks",
        "artifacts",
        "docs",
    ] {
        assert!(
            help_contains_subcommand(&text, subcommand),
            "task help should include '{subcommand}'\n{text}"
        );
    }
}

#[test]
fn subtask_help_should_list_all_subcommands() {
    let home = TestHome::new("subtask-help");
    let output = run_openfang(home.path(), &["subtask", "--help"]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "subtask help should exit successfully\n{text}"
    );

    for subcommand in ["list", "get", "create", "update", "delete"] {
        assert!(
            help_contains_subcommand(&text, subcommand),
            "subtask help should include '{subcommand}'\n{text}"
        );
    }
}

#[test]
fn subtask_list_help_should_expose_task_id_flag() {
    let home = TestHome::new("subtask-list-help");
    let output = run_openfang(home.path(), &["subtask", "list", "--help"]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "subtask list help should exit successfully\n{text}"
    );
    assert!(
        text.contains("--task_id"),
        "subtask list help should expose the --task_id filter flag\n{text}"
    );
}

#[test]
fn task_list_should_require_running_daemon() {
    let home = TestHome::new("task-list-no-daemon");
    let output = run_openfang(home.path(), &["task", "list"]);
    let text = output_text(&output);

    assert!(
        output.status.code() == Some(1),
        "task list should fail without a daemon\n{text}"
    );
    assert!(
        text.contains("requires a running daemon"),
        "task list should explain the daemon requirement\n{text}"
    );
}

#[test]
fn task_create_should_report_missing_file() {
    let home = TestHome::new("task-create-missing-file");
    let missing = home.path().join("missing-task.json");
    let missing_arg = missing.to_string_lossy().to_string();
    let output = run_openfang(home.path(), &["task", "create", &missing_arg]);
    let text = output_text(&output);

    assert!(
        output.status.code() == Some(1),
        "task create should fail for a missing file\n{text}"
    );
    assert!(
        text.contains("Task file not found"),
        "task create should report the missing file before daemon lookup\n{text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn task_list_json_should_return_valid_json_array_with_running_daemon() {
    let daemon = TestDaemon::start("task-list-json").await;
    create_task_via_http(&daemon.base_url, "task_list_json").await;

    let stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["task", "list", "--json"],
    ));
    let body: Value = serde_json::from_str(&stdout).expect("task list JSON should parse");
    let items = body.as_array().expect("task list JSON should be an array");
    assert!(
        items
            .iter()
            .any(|item| item["id"] == json!("task_list_json")),
        "task list JSON should include the seeded task\n{stdout}"
    );

    let human_output = output_text(&run_openfang_success(daemon.home(), &["task", "list"]));
    assert!(
        human_output.contains("ID") && human_output.contains("TITLE"),
        "task list should render the task table headers\n{human_output}"
    );
    assert!(
        human_output.contains("task_list_json"),
        "task list should include the seeded task in table output\n{human_output}"
    );
    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn task_create_get_and_subtask_list_should_round_trip_with_running_daemon() {
    let daemon = TestDaemon::start("task-create-get").await;
    let task_file = write_json_file(
        daemon.home(),
        "task-create.json",
        &task_payload("task_cli_roundtrip", "CLI round trip task"),
    );
    let task_create = output_text(&run_openfang_success(
        daemon.home(),
        &["task", "create", &task_file],
    ));
    assert!(
        task_create.contains("task_cli_roundtrip"),
        "task create output should include the task id\n{task_create}"
    );

    let task_get = stdout_text(&run_openfang_success(
        daemon.home(),
        &["task", "get", "task_cli_roundtrip", "--json"],
    ));
    let task_body: Value = serde_json::from_str(&task_get).expect("task get JSON should parse");
    assert!(
        task_body["id"] == json!("task_cli_roundtrip"),
        "task get should return the created task id\n{task_get}"
    );
    assert!(
        task_body["title"] == json!("CLI round trip task"),
        "task get should return the created task title\n{task_get}"
    );

    let subtask_file = write_json_file(
        daemon.home(),
        "subtask-create.json",
        &subtask_payload("subtask_cli_roundtrip", "CLI round trip subtask"),
    );
    let subtask_create = output_text(&run_openfang_success(
        daemon.home(),
        &["subtask", "create", "task_cli_roundtrip", &subtask_file],
    ));
    assert!(
        subtask_create.contains("subtask_cli_roundtrip"),
        "subtask create output should include the subtask id\n{subtask_create}"
    );

    let subtask_list_human = output_text(&run_openfang_success(
        daemon.home(),
        &["subtask", "list", "--task_id", "task_cli_roundtrip"],
    ));
    assert!(
        subtask_list_human.contains("TASK_ID") && subtask_list_human.contains("ASSIGNEE"),
        "subtask list should render the subtask table headers\n{subtask_list_human}"
    );
    assert!(
        subtask_list_human.contains("subtask_cli_roundtrip"),
        "subtask list should include the created subtask in table output\n{subtask_list_human}"
    );

    let subtask_list = stdout_text(&run_openfang_success(
        daemon.home(),
        &[
            "subtask",
            "list",
            "--task_id",
            "task_cli_roundtrip",
            "--json",
        ],
    ));
    let subtask_body: Value =
        serde_json::from_str(&subtask_list).expect("subtask list JSON should parse");
    let items = subtask_body
        .as_array()
        .expect("subtask list JSON should be an array");
    assert!(
        items
            .iter()
            .any(|item| item["id"] == json!("subtask_cli_roundtrip")),
        "subtask list JSON should include the created subtask\n{subtask_list}"
    );

    let subtask_get = stdout_text(&run_openfang_success(
        daemon.home(),
        &["subtask", "get", "subtask_cli_roundtrip", "--json"],
    ));
    let subtask_get_body: Value =
        serde_json::from_str(&subtask_get).expect("subtask get JSON should parse");
    assert!(
        subtask_get_body["id"] == json!("subtask_cli_roundtrip"),
        "subtask get should return the created subtask id\n{subtask_get}"
    );

    let subtask_update_file = write_json_file(
        daemon.home(),
        "subtask-update.json",
        &json!({
            "status": "in_progress"
        }),
    );
    let subtask_update = output_text(&run_openfang_success(
        daemon.home(),
        &[
            "subtask",
            "update",
            "subtask_cli_roundtrip",
            &subtask_update_file,
        ],
    ));
    assert!(
        subtask_update.contains("Subtask subtask_cli_roundtrip updated."),
        "subtask update output should confirm the update\n{subtask_update}"
    );

    let subtask_get_after_update = stdout_text(&run_openfang_success(
        daemon.home(),
        &["subtask", "get", "subtask_cli_roundtrip", "--json"],
    ));
    let subtask_after_update_body: Value =
        serde_json::from_str(&subtask_get_after_update).expect("updated subtask JSON should parse");
    assert!(
        subtask_after_update_body["status"] == json!("in_progress"),
        "subtask update should persist the new status\n{subtask_get_after_update}"
    );

    let subtask_delete = output_text(&run_openfang_success(
        daemon.home(),
        &["subtask", "delete", "subtask_cli_roundtrip"],
    ));
    assert!(
        subtask_delete.contains("Subtask subtask_cli_roundtrip deleted."),
        "subtask delete output should confirm deletion\n{subtask_delete}"
    );

    let deleted_subtask_get = run_openfang(
        daemon.home(),
        &["subtask", "get", "subtask_cli_roundtrip", "--json"],
    );
    let deleted_subtask_text = output_text(&deleted_subtask_get);
    assert!(
        deleted_subtask_get.status.code() == Some(1),
        "subtask get should fail after deletion\n{deleted_subtask_text}"
    );
    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn task_update_replan_delete_and_linked_views_should_round_trip_with_running_daemon() {
    let daemon = TestDaemon::start("task-operations").await;
    let task_file = write_json_file(
        daemon.home(),
        "task-ops-create.json",
        &task_payload("task_cli_ops", "CLI operations task"),
    );
    run_openfang_success(daemon.home(), &["task", "create", &task_file]);

    let task_update_file = write_json_file(
        daemon.home(),
        "task-update.json",
        &json!({
            "priority": "critical"
        }),
    );
    let task_update = output_text(&run_openfang_success(
        daemon.home(),
        &["task", "update", "task_cli_ops", &task_update_file],
    ));
    assert!(
        task_update.contains("Task task_cli_ops updated."),
        "task update output should confirm the update\n{task_update}"
    );

    let task_get_after_update = stdout_text(&run_openfang_success(
        daemon.home(),
        &["task", "get", "task_cli_ops", "--json"],
    ));
    let updated_task_body: Value =
        serde_json::from_str(&task_get_after_update).expect("updated task JSON should parse");
    assert!(
        updated_task_body["priority"] == json!("critical"),
        "task update should persist the new priority\n{task_get_after_update}"
    );

    let task_replan_file = write_json_file(
        daemon.home(),
        "task-replan.json",
        &json!({
            "reason": "Split work for delivery",
            "operations": [{
                "op": "create_subtasks",
                "items": [{
                    "id": "subtask_replan_cli",
                    "title": "Prepare delivery notes",
                    "description": "Create the final delivery notes",
                    "kind": "doc_change",
                    "status": "ready",
                    "complexity": "medium",
                    "position": 2,
                    "assignee": { "kind": "agent", "ref": "writer" },
                    "depends_on": [],
                    "parallelizable": true,
                    "input": {},
                    "metadata": {}
                }]
            }],
            "metadata": {
                "source": "agent"
            }
        }),
    );
    let task_replan = output_text(&run_openfang_success(
        daemon.home(),
        &["task", "replan", "task_cli_ops", &task_replan_file],
    ));
    assert!(
        task_replan.contains("Task task_cli_ops replan accepted."),
        "task replan output should confirm acceptance\n{task_replan}"
    );

    let task_subtasks = stdout_text(&run_openfang_success(
        daemon.home(),
        &["task", "subtasks", "task_cli_ops", "--json"],
    ));
    let task_subtasks_body: Value =
        serde_json::from_str(&task_subtasks).expect("task subtasks JSON should parse");
    let subtasks = task_subtasks_body
        .as_array()
        .expect("task subtasks JSON should be an array");
    assert!(
        subtasks
            .iter()
            .any(|item| item["id"] == json!("subtask_replan_cli")),
        "task subtasks should include the replanned subtask\n{task_subtasks}"
    );

    let task_artifacts = stdout_text(&run_openfang_success(
        daemon.home(),
        &["task", "artifacts", "task_cli_ops", "--json"],
    ));
    let task_artifacts_body: Value =
        serde_json::from_str(&task_artifacts).expect("task artifacts JSON should parse");
    let artifacts = task_artifacts_body
        .as_array()
        .expect("task artifacts JSON should be an array");
    assert!(
        artifacts
            .iter()
            .any(|item| item["artifact_id"] == json!("artifact_001")),
        "task artifacts should expose linked artifact refs\n{task_artifacts}"
    );

    let task_docs = stdout_text(&run_openfang_success(
        daemon.home(),
        &["task", "docs", "task_cli_ops", "--json"],
    ));
    let task_docs_body: Value =
        serde_json::from_str(&task_docs).expect("task docs JSON should parse");
    let docs = task_docs_body
        .as_array()
        .expect("task docs JSON should be an array");
    assert!(
        docs.iter().any(|item| item["doc_id"] == json!("doc_001")),
        "task docs should expose linked doc refs\n{task_docs}"
    );

    let task_delete = output_text(&run_openfang_success(
        daemon.home(),
        &["task", "delete", "task_cli_ops"],
    ));
    assert!(
        task_delete.contains("Task task_cli_ops deleted."),
        "task delete output should confirm deletion\n{task_delete}"
    );

    let deleted_task_get = run_openfang(daemon.home(), &["task", "get", "task_cli_ops", "--json"]);
    let deleted_task_text = output_text(&deleted_task_get);
    assert!(
        deleted_task_get.status.code() == Some(1),
        "task get should fail after deletion\n{deleted_task_text}"
    );
    daemon.shutdown().await;
}
