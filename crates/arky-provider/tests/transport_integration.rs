//! Integration tests for stdio transport round-trips.

use arky_provider::{ProcessConfig, ProcessManager, StdioTransport, StdioTransportConfig};
use pretty_assertions::assert_eq;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn stdio_transport_should_round_trip_with_a_real_subprocess() {
    let manager = ProcessManager::new(ProcessConfig::new("sh").with_args([
        "-c",
        "while IFS= read -r line; do printf '%s\\n' \"$line\"; done",
    ]));
    let mut process = manager.spawn().expect("process should spawn");
    let stdin = process.take_stdin().expect("stdin should be available");
    let stdout = process.take_stdout().expect("stdout should be available");
    let mut transport = StdioTransport::new(stdout, stdin, StdioTransportConfig::default());

    transport
        .send_frame("ping", CancellationToken::new())
        .await
        .expect("send should succeed");

    let response = transport
        .recv_frame(CancellationToken::new())
        .await
        .expect("receive should succeed")
        .expect("frame should exist");

    assert_eq!(response, "ping");

    drop(transport);
    process
        .graceful_shutdown()
        .await
        .expect("shutdown should succeed");
}
