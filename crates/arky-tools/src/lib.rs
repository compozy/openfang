//! Tool traits, registry primitives, and provider naming codecs for Arky.
//!
//! Use `arky-tools` to define tool descriptors, register concrete handlers, and
//! translate canonical tool identity into the provider-specific naming schemes
//! used by Claude, Codex, and MCP bridges.

mod codec;
mod descriptor;
mod error;
mod registry;
mod truncation;

pub use crate::{
    codec::{
        create_claude_code_tool_id_codec, create_claude_compatible_tool_id_codec,
        create_codex_tool_id_codec, create_opencode_tool_id_codec, ParsedProviderToolName,
        StaticToolIdCodec, ToolIdCodec,
    },
    descriptor::{
        build_canonical_tool_name, parse_canonical_tool_name, validate_canonical_segment,
        validate_canonical_tool_name, ParsedCanonicalToolName, ToolDescriptor, ToolOrigin,
    },
    error::ToolError,
    registry::{Tool, ToolRegistrationHandle, ToolRegistry},
    truncation::{truncate_tool_output, TruncationConfig, TruncationResult},
};
pub use arky_protocol::{ToolCall, ToolContent, ToolResult};

#[doc(hidden)]
pub mod __private {
    pub use async_trait::async_trait;
    pub use schemars;
    pub use serde;
    pub use serde_json;
    pub use tokio_util;
}
