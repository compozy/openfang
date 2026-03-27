use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use openfang_api::server::run_daemon;
use openfang_kernel::OpenFangKernel;
use openfang_types::artifact::{
    ArtifactId, ArtifactType, ArtifactVersionId, NewArtifact, NewArtifactVersion, ProvenanceKind,
    ProvenanceRef,
};
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use openfang_types::doc::{DocId, DocType, DocVersionId, NewDoc, NewDocVersion};
use openfang_types::task::{
    ActorKind, ArtifactRef, Complexity, DocRef, OwnerRef, Priority, TaskId, TaskRecord, TaskSource,
    TaskStatus,
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

struct TestDaemon {
    home: TestHome,
    base_url: String,
}

impl TestDaemon {
    async fn start_seeded(name: &str) -> Self {
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

        let kernel = OpenFangKernel::boot_with_config(config)
            .expect("kernel should boot for artifact/doc CLI tests");
        seed_artifact_and_doc_fixture(&kernel);

        tokio::spawn(async move {
            run_daemon(kernel, &listen_addr, Some(daemon_info_path.as_path()))
                .await
                .expect("artifact/doc CLI test daemon should stay available");
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

fn insert_task(kernel: &OpenFangKernel, task: TaskRecord) {
    kernel
        .workflow_stores
        .task
        .create(&task)
        .expect("task should be created");
}

fn insert_artifact(
    kernel: &OpenFangKernel,
    artifact_id: &str,
    artifact_type: &str,
    initial_version_id: &str,
    created_at: &str,
    appended_versions: &[(&str, &str, &str)],
) -> String {
    let repo = &kernel.workflow_stores.artifact;
    let created = repo
        .create(&NewArtifact {
            artifact_id: ArtifactId::new(artifact_id),
            artifact_version_id: ArtifactVersionId::new(initial_version_id),
            type_name: ArtifactType::new(artifact_type),
            metadata: json!({ "origin": "workflow", "title": format!("Artifact {artifact_id}") }),
            content: json!({
                "title": format!("Artifact {artifact_id} initial"),
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
                        "title": format!("Artifact {artifact_id} {section}"),
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
    kernel: &OpenFangKernel,
    doc_id: &str,
    doc_type: &str,
    initial_version_id: &str,
    created_at: &str,
    appended_versions: &[(&str, &str, &str)],
) -> String {
    let repo = &kernel.workflow_stores.doc;
    let created = repo
        .create(&NewDoc {
            doc_id: DocId::new(doc_id),
            doc_version_id: DocVersionId::new(initial_version_id),
            type_name: DocType::new(doc_type),
            metadata: json!({ "origin": "workflow", "title": format!("Doc {doc_id}") }),
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

fn seed_artifact_and_doc_fixture(kernel: &OpenFangKernel) {
    let artifact_one_current = insert_artifact(
        kernel,
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
        kernel,
        "artifact_002",
        "brief",
        "artifact_002_v1",
        "2026-03-25T11:00:00Z",
        &[],
    );
    let doc_one_current = insert_doc(
        kernel,
        "doc_001",
        "brief",
        "doc_001_v1",
        "2026-03-25T10:00:00Z",
        &[("doc_001_v2", "updated brief", "2026-03-25T10:03:00Z")],
    );
    let doc_two_current = insert_doc(
        kernel,
        "doc_002",
        "research",
        "doc_002_v1",
        "2026-03-25T11:00:00Z",
        &[],
    );

    insert_task(
        kernel,
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
        kernel,
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
    insert_task(
        kernel,
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
        kernel,
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

#[test]
fn artifact_help_should_list_all_subcommands() {
    let home = TestHome::new("artifact-help");
    let output = run_openfang(home.path(), &["artifact", "--help"]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "artifact help should exit successfully\n{text}"
    );

    for subcommand in ["list", "get", "versions"] {
        assert!(
            help_contains_subcommand(&text, subcommand),
            "artifact help should include '{subcommand}'\n{text}"
        );
    }
}

#[test]
fn doc_help_should_list_all_subcommands() {
    let home = TestHome::new("doc-help");
    let output = run_openfang(home.path(), &["doc", "--help"]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "doc help should exit successfully\n{text}"
    );

    for subcommand in ["list", "get", "versions"] {
        assert!(
            help_contains_subcommand(&text, subcommand),
            "doc help should include '{subcommand}'\n{text}"
        );
    }
}

#[test]
fn artifact_and_doc_list_help_should_expose_filters() {
    let home = TestHome::new("artifact-doc-list-help");
    let artifact_help = output_text(&run_openfang(home.path(), &["artifact", "list", "--help"]));
    assert!(
        artifact_help.contains("--type") && artifact_help.contains("--task_id"),
        "artifact list help should expose --type and --task_id\n{artifact_help}"
    );

    let doc_help = output_text(&run_openfang(home.path(), &["doc", "list", "--help"]));
    assert!(
        doc_help.contains("--type") && doc_help.contains("--task_id"),
        "doc list help should expose --type and --task_id\n{doc_help}"
    );
}

#[test]
fn artifact_list_should_require_running_daemon() {
    let home = TestHome::new("artifact-list-no-daemon");
    let output = run_openfang(home.path(), &["artifact", "list"]);
    let text = output_text(&output);

    assert!(
        output.status.code() == Some(1),
        "artifact list should fail without a daemon\n{text}"
    );
    assert!(
        text.contains("requires a running daemon"),
        "artifact list should explain the daemon requirement\n{text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn artifact_list_json_should_return_valid_json_array_with_running_daemon() {
    let daemon = TestDaemon::start_seeded("artifact-list-json").await;

    let stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["artifact", "list", "--json"],
    ));
    let body: Value = serde_json::from_str(&stdout).expect("artifact list JSON should parse");
    let items = body
        .as_array()
        .expect("artifact list JSON should be an array");
    assert!(
        items.iter().any(|item| item["id"] == json!("artifact_001")),
        "artifact list JSON should include the seeded artifact\n{stdout}"
    );

    let human_output = output_text(&run_openfang_success(daemon.home(), &["artifact", "list"]));
    assert!(
        human_output.contains("ID")
            && human_output.contains("TYPE")
            && human_output.contains("TITLE")
            && human_output.contains("VERSION")
            && human_output.contains("TASK_ID"),
        "artifact list should render the artifact table headers\n{human_output}"
    );
    assert!(
        human_output.contains("artifact_001"),
        "artifact list should include the seeded artifact in table output\n{human_output}"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn doc_list_should_filter_by_task_id_with_running_daemon() {
    let daemon = TestDaemon::start_seeded("doc-list-filter").await;

    let stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["doc", "list", "--task_id", "task_doc_001", "--json"],
    ));
    let body: Value = serde_json::from_str(&stdout).expect("doc list JSON should parse");
    let items = body.as_array().expect("doc list JSON should be an array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], json!("doc_001"));

    let human_output = output_text(&run_openfang_success(
        daemon.home(),
        &["doc", "list", "--task_id", "task_doc_001"],
    ));
    assert!(
        human_output.contains("doc_001") && human_output.contains("TASK_ID"),
        "doc list should render the filtered row in table output\n{human_output}"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn artifact_and_doc_get_should_render_provenance_fields() {
    let daemon = TestDaemon::start_seeded("artifact-doc-get").await;

    let artifact_output = output_text(&run_openfang_success(
        daemon.home(),
        &["artifact", "get", "artifact_001"],
    ));
    assert!(
        artifact_output.contains("Created by kind:")
            && artifact_output.contains("agent")
            && artifact_output.contains("Created by ref:")
            && artifact_output.contains("artifact-writer")
            && artifact_output.contains("Current content:"),
        "artifact get should render provenance fields and current content section\n{artifact_output}"
    );

    let doc_output = output_text(&run_openfang_success(
        daemon.home(),
        &["doc", "get", "doc_001"],
    ));
    assert!(
        doc_output.contains("Created by kind:")
            && doc_output.contains("dispatch")
            && doc_output.contains("Created by ref:")
            && doc_output.contains("dispatch_001")
            && doc_output.contains("Current content:"),
        "doc get should render provenance fields and current content section\n{doc_output}"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn artifact_versions_should_show_full_hash_and_error_for_unknown_id() {
    let daemon = TestDaemon::start_seeded("artifact-versions").await;

    let json_output = stdout_text(&run_openfang_success(
        daemon.home(),
        &["artifact", "versions", "artifact_001", "--json"],
    ));
    let body: Value =
        serde_json::from_str(&json_output).expect("artifact versions JSON should parse");
    let items = body
        .as_array()
        .expect("artifact versions JSON should be an array");
    let full_hash = items[0]["content_hash"]
        .as_str()
        .expect("content hash should be present")
        .to_string();

    let human_output = output_text(&run_openfang_success(
        daemon.home(),
        &["artifact", "versions", "artifact_001"],
    ));
    assert!(
        human_output.contains("VERSION")
            && human_output.contains("HASH")
            && human_output.contains("CREATED_BY")
            && human_output.contains("CREATED_AT"),
        "artifact versions should render the version table headers\n{human_output}"
    );
    assert!(
        human_output.contains(&full_hash),
        "artifact versions should print the full SHA-256 hash without truncation\n{human_output}"
    );

    let missing = run_openfang(daemon.home(), &["artifact", "versions", "missing"]);
    let missing_text = output_text(&missing);
    assert!(
        missing.status.code() == Some(1),
        "artifact versions should fail for an unknown artifact\n{missing_text}"
    );
    assert!(
        missing_text.contains("Failed to list versions for artifact missing"),
        "artifact versions should surface the artifact-specific error\n{missing_text}"
    );

    daemon.shutdown().await;
}
