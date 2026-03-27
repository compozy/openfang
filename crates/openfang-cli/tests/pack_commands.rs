use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use openfang_api::server::run_daemon;
use openfang_kernel::OpenFangKernel;
use openfang_types::config::{DefaultModelConfig, KernelConfig};
use pretty_assertions::assert_eq;
use serde_json::Value;

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
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("test daemon did not become healthy: {health_url}");
}

fn write_local_pack_fixture(home: &Path) -> String {
    let pack_root = home.join("local-pack-source");
    fs::create_dir_all(pack_root.join("templates")).expect("template dir should be created");

    fs::write(
        pack_root.join("pack.toml"),
        r#"
id = "local-fixture"
name = "Local Fixture"
version = "0.1.0"
description = "Local fixture pack"

[source]
kind = "external"

[[objects]]
resource_type = "template"
resource_id = "bug-report"
"#,
    )
    .expect("pack manifest should be written");
    fs::write(
        pack_root.join("templates/bug-report.toml"),
        "title = \"Bug report\"\nbody = \"Describe the bug\"\n",
    )
    .expect("template fixture should be written");

    pack_root.to_string_lossy().to_string()
}

#[test]
fn pack_help_should_list_all_subcommands() {
    let home = TestHome::new("pack-help");
    let output = run_openfang(home.path(), &["pack", "--help"]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "pack help should exit successfully\n{text}"
    );

    for subcommand in [
        "list",
        "get",
        "objects",
        "install",
        "upgrade",
        "uninstall",
        "fork",
    ] {
        assert!(
            help_contains_subcommand(&text, subcommand),
            "pack help should include '{subcommand}'\n{text}"
        );
    }
}

#[test]
fn pack_list_without_daemon_should_explain_daemon_requirement() {
    let home = TestHome::new("pack-list-no-daemon");
    let output = run_openfang(home.path(), &["pack", "list"]);
    let text = output_text(&output);

    assert!(
        !output.status.success(),
        "pack list should fail without a daemon\n{text}"
    );
    assert!(
        text.contains("requires a running daemon"),
        "pack list should explain the daemon requirement\n{text}"
    );
}

#[test]
fn pack_install_without_required_source_should_show_usage_help() {
    let home = TestHome::new("pack-install-missing-args");
    let output = run_openfang(home.path(), &["pack", "install"]);
    let text = output_text(&output);

    assert!(
        !output.status.success(),
        "pack install without args should fail\n{text}"
    );
    assert!(
        text.contains("Usage:") && text.contains("pack install"),
        "pack install without args should print usage help\n{text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn pack_list_json_should_return_valid_json() {
    let daemon = TestDaemon::start("pack-list-json").await;

    let stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["pack", "list", "--json"],
    ));
    let body: Value = serde_json::from_str(&stdout).expect("pack list JSON should parse");
    let items = body.as_array().expect("pack list JSON should be an array");
    assert!(
        items
            .iter()
            .any(|item| item.get("id").and_then(Value::as_str) == Some("sdlc")),
        "pack list JSON should include the bootstrapped sdlc pack\n{stdout}"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pack_upgrade_dry_run_should_report_effects_without_mutating_installed_pack() {
    let daemon = TestDaemon::start("pack-upgrade-dry-run").await;

    let output = output_text(&run_openfang_success(
        daemon.home(),
        &["pack", "upgrade", "sdlc", "--dry-run"],
    ));
    assert!(
        output.contains("Pack sdlc upgrade dry-run completed."),
        "pack upgrade dry-run should report completion\n{output}"
    );
    assert!(
        output.contains("Would execute:") && output.contains("yes"),
        "pack upgrade dry-run should report that work would execute\n{output}"
    );
    assert!(
        output.contains("Added:") && output.contains("Updated:") && output.contains("Removed:"),
        "pack upgrade dry-run should report the managed object effect counts\n{output}"
    );

    let pack_get = stdout_text(&run_openfang_success(
        daemon.home(),
        &["pack", "get", "sdlc", "--json"],
    ));
    let body: Value = serde_json::from_str(&pack_get).expect("pack get JSON should parse");
    assert_eq!(
        body.get("version").and_then(Value::as_str),
        Some("1.2.0"),
        "dry-run should not mutate the installed pack version"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pack_uninstall_missing_pack_should_surface_daemon_error() {
    let daemon = TestDaemon::start("pack-uninstall-missing").await;

    let output = run_openfang(daemon.home(), &["pack", "uninstall", "missing-pack"]);
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "pack uninstall should fail for an unknown pack\n{text}"
    );
    assert!(
        text.contains("Failed to uninstall pack missing-pack"),
        "pack uninstall should include the pack ID in the error message\n{text}"
    );
    assert!(
        text.to_ascii_lowercase().contains("not found"),
        "pack uninstall should surface the not-found daemon error\n{text}"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pack_install_from_local_path_should_stage_and_install_external_pack() {
    let daemon = TestDaemon::start("pack-install-local").await;
    let pack_source = write_local_pack_fixture(daemon.home());

    let install_output = output_text(&run_openfang_success(
        daemon.home(),
        &["pack", "install", &pack_source],
    ));
    assert!(
        install_output.contains("Pack local-fixture install accepted."),
        "pack install should confirm the installed pack\n{install_output}"
    );

    let pack_get = stdout_text(&run_openfang_success(
        daemon.home(),
        &["pack", "get", "local-fixture", "--json"],
    ));
    let body: Value = serde_json::from_str(&pack_get).expect("pack get JSON should parse");
    assert_eq!(
        body.get("id").and_then(Value::as_str),
        Some("local-fixture")
    );
    assert_eq!(
        body.get("source")
            .and_then(|source| source.get("kind"))
            .and_then(Value::as_str),
        Some("external")
    );

    assert!(
        daemon
            .home()
            .join(".pack-staging/local-fixture/0.1.0/pack.toml")
            .is_file(),
        "pack install should stage the local source under .pack-staging"
    );
    assert!(
        daemon
            .home()
            .join("packs/local-fixture/templates/bug-report.toml")
            .is_file(),
        "pack install should materialize the managed pack files"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pack_objects_should_list_managed_objects_for_bootstrapped_pack() {
    let daemon = TestDaemon::start("pack-objects").await;

    let stdout = stdout_text(&run_openfang_success(
        daemon.home(),
        &["pack", "objects", "sdlc", "--json"],
    ));
    let body: Value = serde_json::from_str(&stdout).expect("pack objects JSON should parse");
    let items = body
        .as_array()
        .expect("pack objects JSON should be an array");
    assert!(
        !items.is_empty(),
        "pack objects JSON should include the bootstrapped pack objects\n{stdout}"
    );

    let human_output = output_text(&run_openfang_success(
        daemon.home(),
        &["pack", "objects", "sdlc"],
    ));
    assert!(
        human_output.contains("TYPE") && human_output.contains("STATUS"),
        "pack objects should render the object table headers\n{human_output}"
    );

    daemon.shutdown().await;
}
