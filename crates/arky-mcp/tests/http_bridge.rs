//! Integration tests for MCP import/export bridging over streamable HTTP.

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};

use arky_mcp::{
    McpClient, McpHttpClientConfig, McpServer, McpServerHandle, McpServerTransport, McpToolBridge,
};
use arky_protocol::{ToolCall, ToolContent, ToolResult};
use arky_tools::{Tool, ToolDescriptor, ToolError, ToolOrigin, ToolRegistry};
use async_trait::async_trait;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
struct PrefixTool {
    descriptor: ToolDescriptor,
    prefix: &'static str,
}

impl PrefixTool {
    fn new(canonical_name: &str, display_name: &str, prefix: &'static str) -> Self {
        Self {
            descriptor: ToolDescriptor::new(
                canonical_name,
                display_name,
                format!("{display_name} tool"),
                json!({
                    "type": "object",
                    "properties": {
                        "value": {
                            "type": "string"
                        }
                    },
                    "required": ["value"]
                }),
                ToolOrigin::Local,
            )
            .expect("descriptor should be valid"),
            prefix,
        }
    }
}

#[async_trait]
impl Tool for PrefixTool {
    fn descriptor(&self) -> ToolDescriptor {
        self.descriptor.clone()
    }

    async fn execute(
        &self,
        call: ToolCall,
        _cancel: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let value = call
            .input
            .get("value")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                ToolError::invalid_args(
                    "value must be a string",
                    Some(json!({
                        "input": call.input,
                    })),
                )
            })?;

        Ok(ToolResult::success(
            call.id,
            call.name,
            vec![ToolContent::text(format!("{}: {value}", self.prefix))],
        ))
    }
}

fn http_transport() -> McpServerTransport {
    McpServerTransport::StreamableHttp {
        bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        path: "/mcp".to_owned(),
    }
}

fn http_url(handle: &McpServerHandle) -> String {
    handle
        .url()
        .expect("HTTP server should expose a URL")
        .to_owned()
}

#[tokio::test]
async fn bridge_should_import_remote_tools_into_registry() -> Result<(), Box<dyn std::error::Error>>
{
    let origin_registry = Arc::new(ToolRegistry::new());
    origin_registry.register(PrefixTool::new("mcp/origin/echo", "Origin Echo", "origin"))?;

    let origin_server = McpServer::from_registry("origin", origin_registry.clone());
    let mut origin_handle = origin_server.serve(http_transport()).await?;
    let origin_url = http_url(&origin_handle);

    let origin_client = McpClient::http(McpHttpClientConfig::new("origin", origin_url));
    let bridge_registry = Arc::new(ToolRegistry::new());
    let mut bridge = McpToolBridge::builder()
        .registry(bridge_registry.clone())
        .server_name("bridge")
        .import_client(origin_client)
        .build()?;

    let descriptors = bridge.import_tools().await?;
    assert_eq!(descriptors.len(), 1);
    assert_eq!(descriptors[0].canonical_name, "mcp/origin/origin__echo");
    assert!(bridge_registry.contains("mcp/origin/origin__echo"));

    let result = bridge_registry
        .execute(
            ToolCall::new(
                "call-1",
                "mcp/origin/origin__echo",
                json!({ "value": "hello" }),
            ),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(result.content, vec![ToolContent::text("origin: hello")]);

    bridge.disconnect_all().await?;
    origin_handle.close().await?;
    Ok(())
}

#[tokio::test]
async fn bridge_server_should_proxy_imported_and_local_tools(
) -> Result<(), Box<dyn std::error::Error>> {
    let origin_registry = Arc::new(ToolRegistry::new());
    origin_registry.register(PrefixTool::new("mcp/origin/echo", "Origin Echo", "origin"))?;

    let origin_server = McpServer::from_registry("origin", origin_registry.clone());
    let mut origin_handle = origin_server.serve(http_transport()).await?;
    let origin_url = http_url(&origin_handle);

    let origin_client = McpClient::http(McpHttpClientConfig::new("origin", origin_url));
    let bridge_registry = Arc::new(ToolRegistry::new());
    bridge_registry.register(PrefixTool::new(
        "mcp/local/reverse",
        "Local Reverse",
        "local",
    ))?;

    let mut bridge = McpToolBridge::builder()
        .registry(bridge_registry.clone())
        .server_name("bridge")
        .import_client(origin_client)
        .build()?;
    let _ = bridge.import_tools().await?;

    let mut bridge_handle = bridge.serve(http_transport()).await?;
    let bridge_url = http_url(&bridge_handle);
    let external_client = McpClient::http(McpHttpClientConfig::new("bridge", bridge_url));
    external_client.connect().await?;

    let descriptors = external_client.descriptors();
    let names = descriptors
        .iter()
        .map(|descriptor| descriptor.canonical_name.as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"mcp/bridge/local__reverse"));
    assert!(names.contains(&"mcp/bridge/origin__origin__echo"));

    let local_result = external_client
        .call_tool("local__reverse", json!({ "value": "abc" }))
        .await?;
    assert_eq!(local_result.content, vec![ToolContent::text("local: abc")]);

    let bridged_result = external_client
        .call_tool("origin__origin__echo", json!({ "value": "proxy" }))
        .await?;
    assert_eq!(
        bridged_result.content,
        vec![ToolContent::text("origin: proxy")],
    );

    external_client.disconnect().await?;
    bridge.disconnect_all().await?;
    bridge_handle.close().await?;
    origin_handle.close().await?;
    Ok(())
}

#[tokio::test]
async fn bridge_drop_should_cleanup_imported_registrations(
) -> Result<(), Box<dyn std::error::Error>> {
    let origin_registry = Arc::new(ToolRegistry::new());
    origin_registry.register(PrefixTool::new("mcp/origin/echo", "Origin Echo", "origin"))?;

    let origin_server = McpServer::from_registry("origin", origin_registry.clone());
    let mut origin_handle = origin_server.serve(http_transport()).await?;
    let origin_url = http_url(&origin_handle);

    let bridge_registry = Arc::new(ToolRegistry::new());
    {
        let origin_client = McpClient::http(McpHttpClientConfig::new("origin", origin_url));
        let mut bridge = McpToolBridge::builder()
            .registry(bridge_registry.clone())
            .server_name("bridge")
            .import_client(origin_client)
            .build()?;
        let _ = bridge.import_tools().await?;

        assert!(bridge_registry.contains("mcp/origin/origin__echo"));
    }

    assert!(!bridge_registry.contains("mcp/origin/origin__echo"));

    origin_handle.close().await?;
    Ok(())
}

#[tokio::test]
async fn keepalive_should_mark_http_client_disconnected_after_server_shutdown(
) -> Result<(), Box<dyn std::error::Error>> {
    let origin_registry = Arc::new(ToolRegistry::new());
    origin_registry.register(PrefixTool::new("mcp/origin/echo", "Origin Echo", "origin"))?;

    let origin_server = McpServer::from_registry("origin", origin_registry.clone());
    let mut origin_handle = origin_server.serve(http_transport()).await?;
    let origin_url = http_url(&origin_handle);

    let mut client_config = McpHttpClientConfig::new("origin", origin_url);
    client_config.keepalive_interval = Some(std::time::Duration::from_millis(50));
    let client = McpClient::http(client_config);
    client.connect().await?;

    origin_handle.close().await?;
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    assert_eq!(
        client.connection_state(),
        arky_mcp::ConnectionState::Disconnected,
    );

    Ok(())
}
