//! Tool descriptors and canonical naming helpers.

use arky_protocol::ProviderId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ToolError;

const MCP_PREFIX: &str = "mcp";

/// Provider-agnostic canonical tool-name parts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedCanonicalToolName {
    /// The server segment.
    pub server_name: String,
    /// The tool segment.
    pub tool_name: String,
}

impl ParsedCanonicalToolName {
    /// Creates parsed canonical parts after validating each segment.
    pub fn new(
        server_name: impl Into<String>,
        tool_name: impl Into<String>,
    ) -> Result<Self, ToolError> {
        let server_name = server_name.into();
        let tool_name = tool_name.into();

        validate_canonical_segment(&server_name, "server_name")?;
        validate_canonical_segment(&tool_name, "tool_name")?;

        Ok(Self {
            server_name,
            tool_name,
        })
    }

    /// Returns the canonical string representation.
    #[must_use]
    pub fn canonical_name(&self) -> String {
        format!("{MCP_PREFIX}/{}/{}", self.server_name, self.tool_name)
    }
}

/// Origin metadata for a tool descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolOrigin {
    /// Tool implemented directly inside the process.
    Local,
    /// Tool imported from an MCP server.
    Mcp {
        /// Originating MCP server name.
        server_name: String,
    },
    /// Tool visible only to a specific provider implementation.
    ProviderScoped {
        /// Provider identifier that owns the tool exposure.
        provider_id: ProviderId,
    },
}

/// Tool metadata exposed to registries and providers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDescriptor {
    /// Canonical provider-agnostic tool identifier.
    pub canonical_name: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON schema describing accepted input.
    pub input_schema: Value,
    /// Source/origin of the tool.
    pub origin: ToolOrigin,
}

impl ToolDescriptor {
    /// Creates a descriptor after validating canonical naming invariants.
    pub fn new(
        canonical_name: impl Into<String>,
        display_name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        origin: ToolOrigin,
    ) -> Result<Self, ToolError> {
        let canonical_name = canonical_name.into();
        let parsed = validate_canonical_tool_name(&canonical_name)?;

        if let ToolOrigin::Mcp { server_name } = &origin {
            validate_canonical_segment(server_name, "server_name")?;
            if server_name != &parsed.server_name {
                return Err(ToolError::invalid_args(
                    "descriptor origin server does not match canonical name",
                    Some(serde_json::json!({
                        "canonical_name": canonical_name,
                        "origin_server_name": server_name,
                    })),
                ));
            }
        }

        Ok(Self {
            canonical_name,
            display_name: display_name.into(),
            description: description.into(),
            input_schema,
            origin,
        })
    }

    /// Returns the validated canonical-name parts.
    pub fn canonical_parts(&self) -> Result<ParsedCanonicalToolName, ToolError> {
        validate_canonical_tool_name(&self.canonical_name)
    }
}

/// Validates a single canonical segment.
pub fn validate_canonical_segment(value: &str, label: &str) -> Result<(), ToolError> {
    if value.is_empty() {
        return Err(ToolError::invalid_args(
            format!("{label} cannot be empty"),
            Some(serde_json::json!({
                "label": label,
                "value": value,
            })),
        ));
    }

    if value.contains('/') {
        return Err(ToolError::invalid_args(
            format!("{label} cannot contain '/'"),
            Some(serde_json::json!({
                "label": label,
                "value": value,
            })),
        ));
    }

    Ok(())
}

/// Builds a canonical tool name from validated segments.
pub fn build_canonical_tool_name(server_name: &str, tool_name: &str) -> Result<String, ToolError> {
    ParsedCanonicalToolName::new(server_name, tool_name).map(|parsed| parsed.canonical_name())
}

/// Parses a canonical tool name if it matches the expected `mcp/<server>/<tool>` shape.
#[must_use]
pub fn parse_canonical_tool_name(canonical_name: &str) -> Option<ParsedCanonicalToolName> {
    validate_canonical_tool_name(canonical_name).ok()
}

/// Validates and parses a canonical tool name.
pub fn validate_canonical_tool_name(
    canonical_name: &str,
) -> Result<ParsedCanonicalToolName, ToolError> {
    let segments: Vec<_> = canonical_name.split('/').collect();
    if segments.len() != 3 || segments[0] != MCP_PREFIX {
        return Err(ToolError::invalid_args(
            "canonical tool name must match mcp/<server>/<tool>",
            Some(serde_json::json!({
                "canonical_name": canonical_name,
            })),
        ));
    }

    ParsedCanonicalToolName::new(segments[1], segments[2])
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use crate::{
        ParsedCanonicalToolName, ToolDescriptor, ToolOrigin, parse_canonical_tool_name,
        validate_canonical_tool_name,
    };
    use arky_protocol::ProviderId;

    #[test]
    fn tool_descriptor_should_support_all_origin_variants() {
        let local = ToolDescriptor::new(
            "mcp/local/read_file",
            "Read File",
            "Reads a file from disk.",
            json!({ "type": "object" }),
            ToolOrigin::Local,
        )
        .expect("local descriptor should be valid");
        let mcp = ToolDescriptor::new(
            "mcp/fs/read_file",
            "Read File",
            "Reads a file from disk.",
            json!({ "type": "object" }),
            ToolOrigin::Mcp {
                server_name: "fs".to_owned(),
            },
        )
        .expect("mcp descriptor should be valid");
        let provider_scoped = ToolDescriptor::new(
            "mcp/codex/run_task",
            "Run Task",
            "Runs a provider-scoped task.",
            json!({ "type": "object" }),
            ToolOrigin::ProviderScoped {
                provider_id: ProviderId::new("codex"),
            },
        )
        .expect("provider-scoped descriptor should be valid");

        let actual = vec![local.origin, mcp.origin, provider_scoped.origin];
        let expected = vec![
            ToolOrigin::Local,
            ToolOrigin::Mcp {
                server_name: "fs".to_owned(),
            },
            ToolOrigin::ProviderScoped {
                provider_id: ProviderId::new("codex"),
            },
        ];

        assert_eq!(actual, expected);
    }

    #[test]
    fn canonical_name_validation_should_accept_and_reject_expected_shapes() {
        let valid = validate_canonical_tool_name("mcp/server/tool")
            .expect("valid canonical name should parse");
        let invalid = validate_canonical_tool_name("server/tool");

        assert_eq!(
            valid,
            ParsedCanonicalToolName::new("server", "tool").expect("segments should be valid")
        );
        assert!(invalid.is_err());
    }

    #[test]
    fn parse_canonical_tool_name_should_return_none_for_invalid_input() {
        let actual = parse_canonical_tool_name("mcp/server/tool/extra");

        assert_eq!(actual, None);
    }
}
