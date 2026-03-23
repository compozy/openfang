//! Provider-specific tool-name codecs.

use arky_protocol::ProviderId;
use serde::{Deserialize, Serialize};

use crate::{ToolError, build_canonical_tool_name, validate_canonical_tool_name};

const PROVIDER_SEPARATOR: &str = "__";
const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Parsed provider-specific tool naming information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedProviderToolName {
    /// Provider-specific tool name.
    pub provider_name: String,
    /// Canonical provider-agnostic tool name.
    pub canonical_name: String,
    /// Server segment extracted from the canonical name.
    pub server_name: String,
    /// Tool segment extracted from the canonical name.
    pub tool_name: String,
    /// Provider identifier when known.
    pub provider_id: Option<ProviderId>,
}

/// Canonical <-> provider-specific naming round-trip contract.
pub trait ToolIdCodec: Send + Sync {
    /// Provider identifier owned by this codec, when applicable.
    fn provider_id(&self) -> Option<&ProviderId> {
        None
    }

    /// Encodes a canonical tool name into a provider-specific one.
    fn encode(&self, canonical_name: &str) -> Result<String, ToolError>;

    /// Decodes a provider-specific tool name back into canonical metadata.
    fn decode(&self, provider_name: &str) -> Result<ParsedProviderToolName, ToolError>;
}

/// A reusable prefix-based codec used by current providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticToolIdCodec {
    provider_id: ProviderId,
    prefix: &'static str,
}

impl StaticToolIdCodec {
    /// Creates a prefix-based codec.
    #[must_use]
    pub const fn new(provider_id: ProviderId, prefix: &'static str) -> Self {
        Self {
            provider_id,
            prefix,
        }
    }
}

impl ToolIdCodec for StaticToolIdCodec {
    fn provider_id(&self) -> Option<&ProviderId> {
        Some(&self.provider_id)
    }

    fn encode(&self, canonical_name: &str) -> Result<String, ToolError> {
        let parsed = validate_canonical_tool_name(canonical_name)?;

        Ok(format!(
            "{}{}{}{}",
            self.prefix,
            encode_segment(&parsed.server_name),
            PROVIDER_SEPARATOR,
            encode_segment(&parsed.tool_name),
        ))
    }

    fn decode(&self, provider_name: &str) -> Result<ParsedProviderToolName, ToolError> {
        if !provider_name.starts_with(self.prefix) {
            return Err(ToolError::invalid_args(
                "provider tool name prefix does not match codec",
                Some(serde_json::json!({
                    "provider_name": provider_name,
                    "expected_prefix": self.prefix,
                    "provider_id": self.provider_id.as_str(),
                })),
            ));
        }

        let remainder = &provider_name[self.prefix.len()..];
        let (encoded_server_name, encoded_tool_name) =
            remainder.split_once(PROVIDER_SEPARATOR).ok_or_else(|| {
                ToolError::invalid_args(
                    "provider tool name must contain exactly one separator",
                    Some(serde_json::json!({
                        "provider_name": provider_name,
                        "separator": PROVIDER_SEPARATOR,
                    })),
                )
            })?;

        let server_name = decode_segment(encoded_server_name)?;
        let tool_name = decode_segment(encoded_tool_name)?;
        let canonical_name = build_canonical_tool_name(&server_name, &tool_name)?;

        Ok(ParsedProviderToolName {
            provider_name: provider_name.to_owned(),
            canonical_name,
            server_name,
            tool_name,
            provider_id: Some(self.provider_id.clone()),
        })
    }
}

/// Creates the Claude Code tool-name codec.
#[must_use]
pub fn create_claude_code_tool_id_codec() -> StaticToolIdCodec {
    create_claude_compatible_tool_id_codec(ProviderId::new("claude-code"))
}

/// Creates a Claude-compatible tool-name codec for one concrete provider ID.
#[must_use]
pub const fn create_claude_compatible_tool_id_codec(provider_id: ProviderId) -> StaticToolIdCodec {
    StaticToolIdCodec::new(provider_id, "mcp__compozy__")
}

/// Creates the Codex tool-name codec.
#[must_use]
pub fn create_codex_tool_id_codec() -> StaticToolIdCodec {
    StaticToolIdCodec::new(ProviderId::new("codex"), "codex__compozy__")
}

/// Creates the `OpenCode` tool-name codec.
#[must_use]
pub fn create_opencode_tool_id_codec() -> StaticToolIdCodec {
    StaticToolIdCodec::new(ProviderId::new("opencode"), "compozy_")
}

fn encode_segment(segment: &str) -> String {
    let mut encoded = String::with_capacity(segment.len());

    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[(byte >> 4) as usize]));
            encoded.push(char::from(HEX[(byte & 0x0F) as usize]));
        }
    }

    encoded
}

fn decode_segment(encoded: &str) -> Result<String, ToolError> {
    let mut index = 0;
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());

    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(ToolError::invalid_args(
                    "provider tool name contains an incomplete escape sequence",
                    Some(serde_json::json!({
                        "encoded_segment": encoded,
                    })),
                ));
            }

            let escape = std::str::from_utf8(&bytes[index + 1..index + 3]).map_err(|_| {
                ToolError::invalid_args(
                    "provider tool name contains a non-utf8 escape sequence",
                    Some(serde_json::json!({
                        "encoded_segment": encoded,
                    })),
                )
            })?;
            let value = u8::from_str_radix(escape, 16).map_err(|_| {
                ToolError::invalid_args(
                    "provider tool name contains an invalid escape sequence",
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
        ToolError::invalid_args(
            "provider tool name decoded to invalid utf-8",
            Some(serde_json::json!({
                "encoded_segment": encoded,
            })),
        )
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::{
        ToolIdCodec, create_claude_code_tool_id_codec, create_codex_tool_id_codec,
        create_opencode_tool_id_codec,
    };

    #[test]
    fn tool_id_codecs_should_round_trip_canonical_names() {
        let canonical_name = "mcp/server/read_file";
        let codecs = vec![
            (
                create_claude_code_tool_id_codec(),
                "mcp__compozy__server__read_file",
            ),
            (
                create_codex_tool_id_codec(),
                "codex__compozy__server__read_file",
            ),
            (create_opencode_tool_id_codec(), "compozy_server__read_file"),
        ];

        for (codec, expected_provider_name) in codecs {
            let encoded = codec
                .encode(canonical_name)
                .expect("canonical name should encode");
            let decoded = codec.decode(&encoded).expect("provider name should decode");

            assert_eq!(encoded, expected_provider_name);
            assert_eq!(decoded.canonical_name, canonical_name);
        }
    }

    #[test]
    fn tool_id_codec_should_escape_segments_when_needed() {
        let codec = create_codex_tool_id_codec();
        let canonical_name = "mcp/server name/tool with spaces";
        let encoded = codec
            .encode(canonical_name)
            .expect("canonical name should encode");
        let decoded = codec.decode(&encoded).expect("provider name should decode");

        assert_eq!(decoded.canonical_name, canonical_name);
    }
}
