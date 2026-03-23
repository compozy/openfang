//! Canonical naming helpers for imported and exported MCP tools.

use arky_error::ClassifiedError;
use arky_tools::{
    build_canonical_tool_name, validate_canonical_segment, validate_canonical_tool_name,
};

use crate::McpError;

const EXPORT_SEPARATOR: &str = "__";
const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Builds the canonical Arky ID for an imported MCP tool.
pub fn build_import_canonical_name(server_name: &str, tool_name: &str) -> Result<String, McpError> {
    validate_canonical_segment(server_name, "server_name").map_err(|error| {
        McpError::schema_mismatch("invalid MCP server name", error.correction_context())
    })?;
    validate_canonical_segment(tool_name, "tool_name").map_err(|error| {
        McpError::schema_mismatch("invalid MCP tool name", error.correction_context())
    })?;

    build_canonical_tool_name(server_name, tool_name).map_err(|error| {
        McpError::schema_mismatch(
            "failed to build canonical MCP tool name",
            error.correction_context(),
        )
    })
}

/// Encodes a canonical Arky tool ID into an MCP-compatible tool name.
pub fn encode_export_tool_name(canonical_name: &str) -> Result<String, McpError> {
    let parsed = validate_canonical_tool_name(canonical_name).map_err(|error| {
        McpError::schema_mismatch(
            "cannot export an invalid canonical tool name",
            error.correction_context(),
        )
    })?;

    Ok(format!(
        "{}{EXPORT_SEPARATOR}{}",
        encode_segment(&parsed.server_name),
        encode_segment(&parsed.tool_name),
    ))
}

/// Decodes an exported MCP tool name back into its canonical Arky ID.
pub fn decode_export_tool_name(export_name: &str) -> Result<String, McpError> {
    let (encoded_server, encoded_tool) =
        export_name.split_once(EXPORT_SEPARATOR).ok_or_else(|| {
            McpError::schema_mismatch(
                "exported MCP tool names must match <server>__<tool>",
                Some(serde_json::json!({
                    "export_name": export_name,
                })),
            )
        })?;

    let server_name = decode_segment(encoded_server)?;
    let tool_name = decode_segment(encoded_tool)?;

    build_import_canonical_name(&server_name, &tool_name)
}

fn encode_segment(segment: &str) -> String {
    let mut encoded = String::with_capacity(segment.len());

    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('.');
            encoded.push(char::from(HEX[(byte >> 4) as usize]));
            encoded.push(char::from(HEX[(byte & 0x0F) as usize]));
        }
    }

    encoded
}

fn decode_segment(encoded: &str) -> Result<String, McpError> {
    let bytes = encoded.as_bytes();
    let mut index = 0usize;
    let mut decoded = Vec::with_capacity(bytes.len());

    while index < bytes.len() {
        if bytes[index] == b'.'
            && index + 2 < bytes.len()
            && bytes[index + 1].is_ascii_hexdigit()
            && bytes[index + 2].is_ascii_hexdigit()
        {
            let escape = std::str::from_utf8(&bytes[index + 1..index + 3]).map_err(|_| {
                McpError::schema_mismatch(
                    "exported MCP tool name contained non-utf8 escape bytes",
                    Some(serde_json::json!({
                        "encoded_segment": encoded,
                    })),
                )
            })?;
            let value = u8::from_str_radix(escape, 16).map_err(|_| {
                McpError::schema_mismatch(
                    "exported MCP tool name contained an invalid escape sequence",
                    Some(serde_json::json!({
                        "encoded_segment": encoded,
                        "escape": escape,
                    })),
                )
            })?;
            decoded.push(value);
            index += 3;
            continue;
        }

        decoded.push(bytes[index]);
        index += 1;
    }

    String::from_utf8(decoded).map_err(|_| {
        McpError::schema_mismatch(
            "exported MCP tool name decoded to invalid utf-8",
            Some(serde_json::json!({
                "encoded_segment": encoded,
            })),
        )
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::{build_import_canonical_name, decode_export_tool_name, encode_export_tool_name};

    #[test]
    fn import_canonical_name_should_match_expected_shape() {
        let actual = build_import_canonical_name("filesystem", "read_file")
            .expect("canonical name should be valid");

        assert_eq!(actual, "mcp/filesystem/read_file");
    }

    #[test]
    fn export_name_encoding_should_round_trip() {
        let canonical = "mcp/server name/read.file";
        let encoded = encode_export_tool_name(canonical).expect("canonical name should encode");
        let decoded = decode_export_tool_name(&encoded).expect("encoded name should decode");

        assert_eq!(decoded, canonical);
    }

    #[test]
    fn export_name_decoding_should_reject_invalid_shapes() {
        let actual = decode_export_tool_name("tool-only");

        assert!(actual.is_err());
    }
}
