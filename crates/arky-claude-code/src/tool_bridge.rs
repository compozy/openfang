//! Local-tool bridge helpers for exposing Arky tools to Claude over MCP.

use std::{collections::BTreeMap, sync::Arc};

use arky_mcp::{McpError, McpToolBridge};
use arky_provider::ProviderError;
use arky_tools::ToolRegistry;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Serializable tool metadata exposed to Claude's MCP bridge configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeToolBridgeTool {
    /// Canonical Arky tool name.
    pub name: String,
    /// Human-readable tool description.
    pub description: String,
    /// JSON schema describing the tool input object.
    pub input_schema: Value,
}

/// Serializable configuration for the local Claude MCP bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeToolBridgeConfig {
    /// Logical MCP server name presented to Claude.
    pub server_name: String,
    /// Tool descriptors exported through the bridge.
    pub tools: Vec<ClaudeToolBridgeTool>,
}

/// Size limits applied when serializing tool input for Claude.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInputLimits {
    /// Hard maximum input size in bytes.
    pub max_size: usize,
    /// Warning threshold in bytes.
    pub warn_size: usize,
}

impl ToolInputLimits {
    /// Creates explicit tool-input limits.
    #[must_use]
    pub const fn new(max_size: usize, warn_size: usize) -> Self {
        Self {
            max_size,
            warn_size,
        }
    }
}

/// Default tool-input size limits matching Claude parity expectations.
pub const DEFAULT_TOOL_INPUT_LIMITS: ToolInputLimits = ToolInputLimits::new(100_000, 50_000);

/// Combined MCP configuration for local tools plus external server definitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeCombinedToolBridgeConfig {
    /// Embedded local custom-tool server.
    pub custom_server: ClaudeToolBridgeConfig,
    /// Additional external MCP server definitions.
    pub external_servers: BTreeMap<String, Value>,
}

/// Structured result returned after Claude tool-input serialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializedToolInput {
    /// Final serialized input payload.
    pub serialized: String,
    /// Serialized size in bytes.
    pub size_bytes: usize,
    /// Optional warning emitted for large-but-allowed payloads.
    pub warning: Option<String>,
}

impl ClaudeToolBridgeConfig {
    /// Builds a bridge configuration snapshot from a tool registry.
    #[must_use]
    pub fn from_registry(registry: &ToolRegistry, server_name: impl Into<String>) -> Self {
        let tools = registry
            .list()
            .into_iter()
            .map(|descriptor| ClaudeToolBridgeTool {
                name: descriptor.canonical_name,
                description: descriptor.description,
                input_schema: descriptor.input_schema,
            })
            .collect();

        Self {
            server_name: server_name.into(),
            tools,
        }
    }

    /// Encodes the bridge configuration as a JSON string suitable for CLI flags.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Converts the local tool bridge into a Claude MCP SDK-style config payload.
    #[must_use]
    pub fn to_mcp_server_value(&self) -> Value {
        json!({
            "type": "sdk",
            "name": self.server_name,
            "tools": self.tools,
        })
    }
}

impl ClaudeCombinedToolBridgeConfig {
    /// Builds a combined configuration from local tools plus external servers.
    #[must_use]
    pub fn from_registry(
        registry: &ToolRegistry,
        server_name: impl Into<String>,
        external_servers: BTreeMap<String, Value>,
    ) -> Self {
        Self {
            custom_server: ClaudeToolBridgeConfig::from_registry(registry, server_name),
            external_servers,
        }
    }

    /// Returns a merged MCP server map containing custom and external servers.
    #[must_use]
    pub fn merged_servers(&self) -> BTreeMap<String, Value> {
        let mut servers = self.external_servers.clone();
        servers.insert(
            self.custom_server.server_name.clone(),
            self.custom_server.to_mcp_server_value(),
        );
        servers
    }
}

/// Serializes one tool input with Claude-compatible size checks.
pub fn serialize_tool_input(
    input: &Value,
    limits: ToolInputLimits,
) -> Result<String, ProviderError> {
    serialize_tool_input_with_metadata(input, limits).map(|serialized| serialized.serialized)
}

/// Serializes one tool input and returns size metadata/warnings.
pub fn serialize_tool_input_with_metadata(
    input: &Value,
    limits: ToolInputLimits,
) -> Result<SerializedToolInput, ProviderError> {
    let serialized = if input.is_string() {
        input.as_str().unwrap_or_default().to_owned()
    } else {
        serde_json::to_string(input).map_err(|error| {
            ProviderError::protocol_violation(
                "failed to serialize Claude tool input to JSON",
                Some(json!({
                    "reason": error.to_string(),
                })),
            )
        })?
    };

    let input_bytes = serialized.len();
    if input_bytes > limits.max_size {
        return Err(ProviderError::protocol_violation(
            format!(
                "tool input exceeds maximum size of {} bytes (got {} bytes)",
                limits.max_size, input_bytes
            ),
            Some(json!({
                "max_bytes": limits.max_size,
                "actual_bytes": input_bytes,
            })),
        ));
    }

    Ok(SerializedToolInput {
        warning: (input_bytes > limits.warn_size).then(|| {
            format!(
                "Large tool input detected ({input_bytes} bytes). Consider reducing payload size."
            )
        }),
        serialized,
        size_bytes: input_bytes,
    })
}

/// Builds an MCP bridge that exports the supplied registry to Claude.
pub fn build_tool_bridge(
    registry: Arc<ToolRegistry>,
    server_name: impl Into<String>,
) -> Result<McpToolBridge, McpError> {
    McpToolBridge::builder()
        .registry(registry)
        .server_name(server_name)
        .build()
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use async_trait::async_trait;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use tokio_util::sync::CancellationToken;

    use super::{
        ClaudeCombinedToolBridgeConfig, ClaudeToolBridgeConfig, DEFAULT_TOOL_INPUT_LIMITS,
        build_tool_bridge, serialize_tool_input, serialize_tool_input_with_metadata,
    };
    use arky_tools::{Tool, ToolDescriptor, ToolOrigin, ToolRegistry, ToolResult};

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor::new(
                "mcp/test/echo",
                "Echo",
                "Echo test tool",
                json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" }
                    },
                }),
                ToolOrigin::Local,
            )
            .expect("tool descriptor should be valid")
        }

        async fn execute(
            &self,
            call: arky_tools::ToolCall,
            _cancel: CancellationToken,
        ) -> Result<ToolResult, arky_tools::ToolError> {
            Ok(ToolResult::success(call.id, call.name, Vec::new()))
        }
    }

    #[test]
    fn tool_bridge_should_serialize_registry_configuration() {
        let registry = ToolRegistry::new();
        registry
            .register_arc(Arc::new(EchoTool))
            .expect("tool should register");

        let config = ClaudeToolBridgeConfig::from_registry(&registry, "claude-code-tools");
        let encoded = config.to_json().expect("config should encode");

        assert_eq!(config.server_name, "claude-code-tools");
        assert_eq!(config.tools.len(), 1);
        assert_eq!(encoded.contains("mcp/test/echo"), true);
    }

    #[test]
    fn tool_bridge_should_build_mcp_bridge() {
        let registry = Arc::new(ToolRegistry::new());
        let bridge = build_tool_bridge(registry, "claude-code-tools").expect("bridge should build");

        assert_eq!(
            format!("{:?}", bridge.server()).contains("claude-code-tools"),
            true
        );
    }

    #[test]
    fn serialize_tool_input_should_enforce_size_limits() {
        let serialized = serialize_tool_input(&json!({ "text": "ok" }), DEFAULT_TOOL_INPUT_LIMITS)
            .expect("tool input should serialize");
        let warning = serialize_tool_input_with_metadata(
            &json!({ "text": "x".repeat(60_000) }),
            DEFAULT_TOOL_INPUT_LIMITS,
        )
        .expect("large tool input should still serialize");
        let error = serialize_tool_input(
            &json!({ "text": "x".repeat(100_001) }),
            DEFAULT_TOOL_INPUT_LIMITS,
        )
        .expect_err("oversized tool input should fail");

        assert_eq!(serialized.contains("\"text\":\"ok\""), true);
        assert_eq!(warning.warning.is_some(), true);
        assert_eq!(
            error
                .to_string()
                .contains("tool input exceeds maximum size"),
            true
        );
    }

    #[test]
    fn combined_bridge_config_should_merge_custom_and_external_servers() {
        let registry = ToolRegistry::new();
        let combined = ClaudeCombinedToolBridgeConfig::from_registry(
            &registry,
            "claude-code-tools",
            BTreeMap::from([(
                "filesystem".to_owned(),
                json!({ "type": "http", "url": "http://127.0.0.1:7777/mcp" }),
            )]),
        );

        assert_eq!(combined.custom_server.server_name, "claude-code-tools");
        assert_eq!(combined.external_servers.contains_key("filesystem"), true);
        assert_eq!(
            combined
                .merged_servers()
                .get("claude-code-tools")
                .and_then(|value| value.get("type"))
                .and_then(Value::as_str),
            Some("sdk")
        );
    }
}
