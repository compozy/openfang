//! Integration tests for subprocess lifecycle management.

use std::time::Duration;

use arky_provider::{ProcessConfig, ProcessManager};
use pretty_assertions::assert_eq;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    time::{sleep, timeout},
};

#[tokio::test]
async fn process_manager_should_capture_real_subprocess_output_and_shutdown() {
    let manager = ProcessManager::new(
        ProcessConfig::new("sh")
            .with_args(["-c", "printf 'hello from process\\n'; cat >/dev/null"]),
    );
    let mut process = manager.spawn().expect("process should spawn");
    let stdout = process.take_stdout().expect("stdout should be available");
    let mut stdout = BufReader::new(stdout);
    let mut line = String::new();

    stdout.read_line(&mut line).await.expect("line should read");

    assert_eq!(line, "hello from process\n");

    process
        .graceful_shutdown()
        .await
        .expect("shutdown should succeed");
}

#[tokio::test]
async fn process_manager_should_kill_processes_on_drop() {
    let manager = ProcessManager::new(
        ProcessConfig::new("sh").with_args(["-c", "trap '' TERM; while :; do sleep 1; done"]),
    );
    let process = manager.spawn().expect("process should spawn");
    let pid = process.id().expect("pid should be available");

    drop(process);

    timeout(Duration::from_secs(2), async move {
        loop {
            let status = Command::new("sh")
                .args(["-c", &format!("kill -0 {pid} >/dev/null 2>&1")])
                .status()
                .await
                .expect("kill probe should run");
            if !status.success() {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("process should be gone after drop");
}
