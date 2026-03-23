//! MCP client, server, and tool-bridge primitives for Arky.
//!
//! This crate contains the canonical naming rules plus the client/server bridge
//! pieces needed to import remote MCP tools and expose local Arky tools over
//! MCP transports.

mod auth;
mod bridge;
mod client;
mod error;
mod naming;
mod server;

pub use crate::{
    auth::{McpAuth, McpOAuthAuth, McpOAuthFlow, McpOAuthOptions},
    bridge::{
        McpToolBridge, McpToolBridgeBuilder, mcp_tool_from_descriptor, tool_descriptor_from_mcp,
        tool_result_from_mcp, tool_result_to_mcp,
    },
    client::{
        ConnectionState, McpClient, McpClientConfig, McpHttpClientConfig, McpStdioClientConfig,
    },
    error::McpError,
    naming::{build_import_canonical_name, decode_export_tool_name, encode_export_tool_name},
    server::{
        McpHttpServerHandle, McpServer, McpServerHandle, McpServerTransport, McpStdioServerHandle,
    },
};
