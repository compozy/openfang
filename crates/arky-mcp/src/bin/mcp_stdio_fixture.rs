//! Stdio MCP fixture binary used by `arky-mcp` integration tests.

use std::sync::Arc;

use arky_mcp::{McpServer, McpServerHandle, McpServerTransport};
use arky_protocol::{ToolCall, ToolContent, ToolResult};
use arky_tools::{Tool, ToolDescriptor, ToolError, ToolOrigin, ToolRegistry};
use async_trait::async_trait;
use serde_json::json;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
struct FixtureEchoTool {
    descriptor: ToolDescriptor,
}

impl FixtureEchoTool {
    fn new() -> Self {
        Self {
            descriptor: ToolDescriptor::new(
                "mcp/fixture/echo",
                "Fixture Echo",
                "Echoes the provided string for MCP integration tests.",
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
            .expect("fixture descriptor should be valid"),
        }
    }
}

#[async_trait]
impl Tool for FixtureEchoTool {
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
            vec![ToolContent::text(format!("echo: {value}"))],
        ))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = Arc::new(ToolRegistry::new());
    registry.register(FixtureEchoTool::new())?;

    let server = McpServer::from_registry("fixture", registry);
    let handle = server.serve(McpServerTransport::Stdio).await?;

    match handle {
        McpServerHandle::Stdio(handle) => handle.wait().await?,
        McpServerHandle::StreamableHttp(_) => unreachable!("fixture only uses stdio"),
    }

    Ok(())
}
