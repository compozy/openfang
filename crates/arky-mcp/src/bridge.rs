//! Schema and result translation plus bridge orchestration.

use std::sync::Arc;

use arky_error::ClassifiedError;
use arky_protocol::{ToolContent, ToolResult};
use arky_tools::{ToolDescriptor, ToolOrigin, ToolRegistrationHandle, ToolRegistry};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use rmcp::model::{CallToolResult, Content, RawContent, ResourceContents, Tool};
use serde_json::{json, Map, Value};

use crate::{
    build_import_canonical_name, encode_export_tool_name, McpClient, McpError, McpServer,
    McpServerHandle, McpServerTransport,
};

/// Translates an MCP tool definition into an Arky `ToolDescriptor`.
pub fn tool_descriptor_from_mcp(
    server_name: &str,
    tool: &Tool,
) -> Result<ToolDescriptor, McpError> {
    let canonical_name = build_import_canonical_name(server_name, tool.name.as_ref())?;
    let description = tool.description.as_deref().unwrap_or_default().to_owned();
    let display_name = tool.title.clone().unwrap_or_else(|| tool.name.to_string());

    ToolDescriptor::new(
        canonical_name,
        display_name,
        description,
        Value::Object(tool.input_schema.as_ref().clone()),
        ToolOrigin::Mcp {
            server_name: server_name.to_owned(),
        },
    )
    .map_err(|error| {
        McpError::schema_mismatch(
            "failed to translate MCP tool descriptor",
            error.correction_context(),
        )
    })
}

/// Translates an Arky `ToolDescriptor` into an MCP tool definition.
pub fn mcp_tool_from_descriptor(descriptor: &ToolDescriptor) -> Result<Tool, McpError> {
    let input_schema = schema_object(&descriptor.input_schema, "descriptor.input_schema")?;
    let export_name = encode_export_tool_name(&descriptor.canonical_name)?;

    Ok(Tool::new_with_raw(
        export_name,
        Some(descriptor.description.clone().into()),
        Arc::new(input_schema),
    )
    .with_title(descriptor.display_name.clone()))
}

/// Translates an MCP `CallToolResult` into an Arky `ToolResult`.
pub fn tool_result_from_mcp(
    id: impl Into<String>,
    canonical_name: impl Into<String>,
    result: CallToolResult,
) -> Result<ToolResult, McpError> {
    let id = id.into();
    let canonical_name = canonical_name.into();
    let structured_text = result
        .structured_content
        .as_ref()
        .map(serde_json::Value::to_string);

    let mut content = Vec::new();
    if let Some(structured) = result.structured_content.clone() {
        content.push(ToolContent::json(structured));
    }

    for block in result.content {
        match block.raw {
            RawContent::Text(text) => {
                if structured_text
                    .as_ref()
                    .is_some_and(|structured| structured == &text.text)
                {
                    continue;
                }
                content.push(ToolContent::text(text.text));
            }
            RawContent::Image(image) => {
                let data = BASE64_STANDARD.decode(image.data).map_err(|error| {
                    McpError::protocol_error(
                        "failed to decode MCP image content",
                        Some(json!({
                            "canonical_name": canonical_name,
                            "reason": error.to_string(),
                        })),
                    )
                })?;
                content.push(ToolContent::image(data, image.mime_type));
            }
            RawContent::Resource(resource) => match resource.resource {
                ResourceContents::TextResourceContents { text, .. } => {
                    content.push(ToolContent::text(text));
                }
                ResourceContents::BlobResourceContents { .. } => {
                    return Err(McpError::schema_mismatch(
                        "embedded binary MCP resources are unsupported by Arky tool results",
                        Some(json!({
                            "canonical_name": canonical_name,
                        })),
                    ));
                }
            },
            RawContent::Audio(_) | RawContent::ResourceLink(_) => {
                return Err(McpError::schema_mismatch(
                    "MCP content type is unsupported by Arky tool results",
                    Some(json!({
                        "canonical_name": canonical_name,
                        "content_type": format!("{:?}", block.raw),
                    })),
                ));
            }
        }
    }

    Ok(ToolResult::new(
        id,
        canonical_name,
        content,
        result.is_error.unwrap_or(false),
    ))
}

/// Translates an Arky `ToolResult` into an MCP `CallToolResult`.
pub fn tool_result_to_mcp(result: &ToolResult) -> Result<CallToolResult, McpError> {
    let mut content = Vec::with_capacity(result.content.len());
    let mut structured_content = None;

    for item in &result.content {
        match item {
            ToolContent::Text { text } => {
                content.push(Content::text(text.clone()));
            }
            ToolContent::Image { data, media_type } => {
                content.push(Content::image(
                    BASE64_STANDARD.encode(data),
                    media_type.clone(),
                ));
            }
            ToolContent::Json { value } => {
                if structured_content.is_some() {
                    return Err(McpError::schema_mismatch(
                        "cannot translate multiple JSON fragments into one MCP structured result",
                        Some(json!({
                            "tool_call_id": result.id,
                            "canonical_name": result.name,
                        })),
                    ));
                }
                structured_content = Some(value.clone());
                content.push(Content::text(value.to_string()));
            }
        }
    }

    let mut translated = if let Some(structured) = structured_content.clone() {
        if result.is_error {
            CallToolResult::structured_error(structured)
        } else {
            CallToolResult::structured(structured)
        }
    } else if result.is_error {
        CallToolResult::error(content.clone())
    } else {
        CallToolResult::success(content.clone())
    };
    translated.content = content;

    Ok(translated)
}

/// Builder for `McpToolBridge`.
#[derive(Default)]
pub struct McpToolBridgeBuilder {
    registry: Option<Arc<ToolRegistry>>,
    server_name: Option<String>,
    instructions: Option<String>,
    clients: Vec<McpClient>,
}

impl std::fmt::Debug for McpToolBridgeBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpToolBridgeBuilder")
            .field("server_name", &self.server_name)
            .field("instructions", &self.instructions)
            .field("clients", &self.clients)
            .finish_non_exhaustive()
    }
}

impl McpToolBridgeBuilder {
    /// Creates an empty bridge builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Uses the supplied registry for imported and exported tools.
    #[must_use]
    pub fn registry(mut self, registry: Arc<ToolRegistry>) -> Self {
        self.registry = Some(registry);
        self
    }

    /// Configures the logical MCP server name for exported tools.
    #[must_use]
    pub fn server_name(mut self, server_name: impl Into<String>) -> Self {
        self.server_name = Some(server_name.into());
        self
    }

    /// Configures optional instructions on the exported server.
    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Adds a remote client whose imported tools should be bridged.
    #[must_use]
    pub fn import_client(mut self, client: McpClient) -> Self {
        self.clients.push(client);
        self
    }

    /// Builds the bridge.
    pub fn build(self) -> Result<McpToolBridge, McpError> {
        let registry = self
            .registry
            .unwrap_or_else(|| Arc::new(ToolRegistry::new()));
        let server = {
            let base = McpServer::from_registry(
                self.server_name
                    .unwrap_or_else(|| "arky-mcp-bridge".to_owned()),
                registry.clone(),
            );
            if let Some(instructions) = self.instructions {
                base.with_instructions(instructions)
            } else {
                base
            }
        };

        Ok(McpToolBridge {
            registry,
            server,
            clients: self.clients,
            imported_handle: None,
        })
    }
}

/// Bidirectional MCP bridge for importing remote tools and exporting local ones.
pub struct McpToolBridge {
    registry: Arc<ToolRegistry>,
    server: McpServer,
    clients: Vec<McpClient>,
    imported_handle: Option<ToolRegistrationHandle>,
}

impl std::fmt::Debug for McpToolBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let imported_registration_count = self
            .imported_handle
            .iter()
            .map(|handle| handle.canonical_names().len())
            .sum::<usize>();

        f.debug_struct("McpToolBridge")
            .field("server", &self.server)
            .field("clients", &self.clients)
            .field("imported_registration_count", &imported_registration_count)
            .finish_non_exhaustive()
    }
}

impl McpToolBridge {
    /// Creates a bridge builder.
    #[must_use]
    pub fn builder() -> McpToolBridgeBuilder {
        McpToolBridgeBuilder::new()
    }

    /// Returns the backing tool registry.
    #[must_use]
    pub const fn registry(&self) -> &Arc<ToolRegistry> {
        &self.registry
    }

    /// Returns the exported MCP server facade.
    #[must_use]
    pub const fn server(&self) -> &McpServer {
        &self.server
    }

    /// Returns the imported clients currently managed by the bridge.
    #[must_use]
    pub fn clients(&self) -> &[McpClient] {
        &self.clients
    }

    /// Connects clients, imports their tools, and registers adapters in the bridge registry.
    pub async fn import_tools(&mut self) -> Result<Vec<ToolDescriptor>, McpError> {
        let mut imported_descriptors = Vec::new();
        let mut imported_tools = Vec::new();

        for client in &self.clients {
            if client.connection_state() == crate::ConnectionState::Disconnected {
                client.connect().await?;
            } else {
                let _ = client.refresh_tools().await?;
            }
            imported_descriptors.extend(client.descriptors());
            imported_tools.extend(client.tool_arcs());
        }

        if let Some(previous) = self.imported_handle.take() {
            let _ = previous.cleanup();
        }

        self.imported_handle = if imported_tools.is_empty() {
            None
        } else {
            Some(
                self.registry
                    .register_many_call_scoped(imported_tools)
                    .map_err(|error| {
                        McpError::schema_mismatch(
                            "failed to register imported MCP tools in the bridge registry",
                            error.correction_context(),
                        )
                    })?,
            )
        };

        Ok(imported_descriptors)
    }

    /// Refreshes all imported tools and updates the exported registry view.
    pub async fn refresh(&mut self) -> Result<Vec<ToolDescriptor>, McpError> {
        self.import_tools().await
    }

    /// Starts the exported MCP server on the selected transport.
    pub async fn serve(&self, transport: McpServerTransport) -> Result<McpServerHandle, McpError> {
        self.server.serve(transport).await
    }

    /// Disconnects all imported clients and drops call-scoped registrations.
    pub async fn disconnect_all(&mut self) -> Result<(), McpError> {
        if let Some(handle) = self.imported_handle.take() {
            let _ = handle.cleanup();
        }

        for client in &self.clients {
            client.disconnect().await?;
        }

        Ok(())
    }
}

impl Drop for McpToolBridge {
    fn drop(&mut self) {
        if let Some(handle) = self.imported_handle.take() {
            let _ = handle.cleanup();
        }
    }
}

fn schema_object(value: &Value, field_name: &str) -> Result<Map<String, Value>, McpError> {
    match value {
        Value::Object(map) => Ok(map.clone()),
        other => Err(McpError::schema_mismatch(
            "tool schema must be a JSON object",
            Some(json!({
                "field_name": field_name,
                "actual_type": type_name(other),
            })),
        )),
    }
}

const fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{
        mcp_tool_from_descriptor, tool_descriptor_from_mcp, tool_result_from_mcp,
        tool_result_to_mcp,
    };
    use arky_protocol::{ToolContent, ToolResult};
    use arky_tools::{ToolDescriptor, ToolOrigin};
    use rmcp::model::{CallToolResult, Content, Tool};
    use std::sync::Arc;

    #[test]
    fn schema_translation_should_round_trip_supported_subset() {
        let descriptor = ToolDescriptor::new(
            "mcp/filesystem/read_file",
            "Read File",
            "Reads a file from disk.",
            json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string"
                    }
                },
                "required": ["path"]
            }),
            ToolOrigin::Mcp {
                server_name: "filesystem".to_owned(),
            },
        )
        .expect("descriptor should be valid");

        let mcp_tool =
            mcp_tool_from_descriptor(&descriptor).expect("descriptor should translate to MCP");
        let remote_tool = Tool::new(
            "read_file",
            mcp_tool
                .description
                .clone()
                .expect("translated tool should preserve description"),
            Arc::new(mcp_tool.input_schema.as_ref().clone()),
        )
        .with_title(
            mcp_tool
                .title
                .clone()
                .expect("translated tool should preserve title"),
        );
        let translated = tool_descriptor_from_mcp("filesystem", &remote_tool)
            .expect("MCP tool should translate back");

        assert_eq!(translated, descriptor);
    }

    #[test]
    fn tool_result_translation_should_round_trip_supported_content() {
        let result = ToolResult::success(
            "call-1",
            "mcp/filesystem/read_file",
            vec![
                ToolContent::text("ok"),
                ToolContent::json(json!({
                    "path": "/tmp/demo.txt"
                })),
            ],
        );

        let mcp_result = tool_result_to_mcp(&result).expect("tool result should translate to MCP");
        let translated = tool_result_from_mcp("call-1", "mcp/filesystem/read_file", mcp_result)
            .expect("MCP result should translate back");

        assert_eq!(
            translated,
            ToolResult::success(
                "call-1",
                "mcp/filesystem/read_file",
                vec![
                    ToolContent::json(json!({
                        "path": "/tmp/demo.txt"
                    })),
                    ToolContent::text("ok"),
                ],
            ),
        );
    }

    #[test]
    fn tool_result_from_mcp_should_decode_images() {
        let translated = tool_result_from_mcp(
            "call-2",
            "mcp/images/thumbnail",
            CallToolResult::success(vec![Content::image("AQID", "image/png")]),
        )
        .expect("image content should translate");

        assert_eq!(
            translated,
            ToolResult::success(
                "call-2",
                "mcp/images/thumbnail",
                vec![ToolContent::image(vec![1, 2, 3], "image/png")],
            ),
        );
    }
}
