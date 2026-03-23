//! Integration test covering stdio MCP round-trips against a subprocess fixture.

use arky_mcp::{McpClient, McpStdioClientConfig};
use arky_protocol::ToolContent;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test]
async fn stdio_client_server_round_trip_should_list_and_call_tool()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = std::env::var("CARGO_BIN_EXE_mcp_stdio_fixture")?;
    let client = McpClient::stdio(McpStdioClientConfig::new(
        "fixture",
        fixture,
        std::iter::empty::<String>(),
    ));

    client.connect().await?;

    let descriptors = client.descriptors();
    assert_eq!(descriptors.len(), 1);
    assert_eq!(descriptors[0].canonical_name, "mcp/fixture/fixture__echo");

    let result = client
        .call_tool("fixture__echo", json!({ "value": "hello" }))
        .await?;
    assert_eq!(result.content, vec![ToolContent::text("echo: hello")],);
    assert!(!result.is_error);

    client.disconnect().await?;
    Ok(())
}
