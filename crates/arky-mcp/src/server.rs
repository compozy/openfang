//! MCP server implementation for exposing an Arky `ToolRegistry`.

use std::{
    future::Future,
    net::{IpAddr, SocketAddr},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use arky_error::ClassifiedError;
use arky_tools::ToolRegistry;
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResult, Implementation, InitializeResult, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerInfo,
    },
    service::{RequestContext, RoleServer},
    transport::{
        stdio,
        streamable_http_server::{
            session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
        },
    },
    ServerHandler, ServiceExt,
};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{decode_export_tool_name, mcp_tool_from_descriptor, tool_result_to_mcp, McpError};

static NEXT_CALL_ID: AtomicU64 = AtomicU64::new(1);

/// Transport selection for an MCP server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpServerTransport {
    /// Run a server over stdin/stdout in the current process.
    Stdio,
    /// Run a streamable-HTTP server bound to the supplied address and path.
    StreamableHttp {
        /// Socket address to bind.
        bind_addr: SocketAddr,
        /// Route prefix to expose the MCP service under.
        path: String,
    },
}

/// MCP server exposing a local `ToolRegistry`.
#[derive(Clone)]
pub struct McpServer {
    name: String,
    registry: Arc<ToolRegistry>,
    instructions: Option<String>,
}

impl std::fmt::Debug for McpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServer")
            .field("name", &self.name)
            .field("instructions", &self.instructions)
            .finish_non_exhaustive()
    }
}

impl McpServer {
    /// Creates an MCP server from a tool registry.
    #[must_use]
    pub fn from_registry(name: impl Into<String>, registry: Arc<ToolRegistry>) -> Self {
        Self {
            name: name.into(),
            registry,
            instructions: None,
        }
    }

    /// Adds optional human-readable server instructions.
    #[must_use]
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Starts serving using the requested transport.
    pub async fn serve(&self, transport: McpServerTransport) -> Result<McpServerHandle, McpError> {
        match transport {
            McpServerTransport::Stdio => self.serve_stdio().await.map(McpServerHandle::Stdio),
            McpServerTransport::StreamableHttp { bind_addr, path } => self
                .serve_http(bind_addr, path)
                .await
                .map(McpServerHandle::StreamableHttp),
        }
    }

    async fn serve_stdio(&self) -> Result<McpStdioServerHandle, McpError> {
        let handler = RegistryMcpServerHandler::new(
            self.name.clone(),
            self.registry.clone(),
            self.instructions.clone(),
        );
        let running = handler
            .serve(stdio())
            .await
            .map_err(|error| map_server_init_error(&error))?;

        Ok(McpStdioServerHandle {
            running: Mutex::new(Some(running)),
        })
    }

    async fn serve_http(
        &self,
        bind_addr: SocketAddr,
        path: String,
    ) -> Result<McpHttpServerHandle, McpError> {
        let cancellation = CancellationToken::new();
        let handler = RegistryMcpServerHandler::new(
            self.name.clone(),
            self.registry.clone(),
            self.instructions.clone(),
        );
        let path = normalize_path(&path);
        let session_manager = Arc::new(LocalSessionManager::default());
        let service = StreamableHttpService::new(
            move || Ok(handler.clone()),
            session_manager,
            StreamableHttpServerConfig {
                stateful_mode: true,
                cancellation_token: cancellation.child_token(),
                ..Default::default()
            },
        );

        let router = axum::Router::new().nest_service(&path, service);
        let listener = tokio::net::TcpListener::bind(bind_addr)
            .await
            .map_err(|error| {
                McpError::connection_failed(
                    "failed to bind MCP HTTP listener",
                    Some(self.name.clone()),
                    Some(json!({
                        "bind_addr": bind_addr,
                        "reason": error.to_string(),
                    })),
                )
            })?;
        let local_addr = listener.local_addr().map_err(|error| {
            McpError::connection_failed(
                "failed to inspect MCP HTTP listener address",
                Some(self.name.clone()),
                Some(json!({
                    "reason": error.to_string(),
                })),
            )
        })?;
        let join = tokio::spawn({
            let cancellation = cancellation.clone();
            async move {
                axum::serve(listener, router)
                    .with_graceful_shutdown(async move {
                        cancellation.cancelled_owned().await;
                    })
                    .await
                    .map_err(|error| std::io::Error::other(error.to_string()))
            }
        });

        Ok(McpHttpServerHandle {
            url: format!("http://{}{}", socket_addr_host(local_addr), path),
            bind_addr: local_addr,
            cancellation,
            join: Mutex::new(Some(join)),
        })
    }
}

/// Running stdio MCP server handle.
#[derive(Debug)]
pub struct McpStdioServerHandle {
    running: Mutex<Option<rmcp::service::RunningService<RoleServer, RegistryMcpServerHandler>>>,
}

impl McpStdioServerHandle {
    /// Waits for the stdio transport to close.
    pub async fn wait(self) -> Result<(), McpError> {
        let running = self.running.lock().await.take();
        if let Some(running) = running {
            running.waiting().await.map_err(|error| {
                McpError::server_crashed(
                    "MCP stdio server task crashed while waiting for shutdown",
                    None,
                    Some(json!({
                        "reason": error.to_string(),
                    })),
                )
            })?;
        }

        Ok(())
    }

    /// Closes the stdio server and waits for cleanup.
    pub async fn close(&mut self) -> Result<(), McpError> {
        let running = self.running.lock().await.take();
        if let Some(mut running) = running {
            running.close().await.map_err(|error| {
                McpError::server_crashed(
                    "failed to close MCP stdio server",
                    None,
                    Some(json!({
                        "reason": error.to_string(),
                    })),
                )
            })?;
        }

        Ok(())
    }
}

/// Running streamable-HTTP MCP server handle.
#[derive(Debug)]
pub struct McpHttpServerHandle {
    url: String,
    bind_addr: SocketAddr,
    cancellation: CancellationToken,
    join: Mutex<Option<tokio::task::JoinHandle<std::io::Result<()>>>>,
}

impl McpHttpServerHandle {
    /// Returns the bound URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the actual bound address.
    #[must_use]
    pub const fn bind_addr(&self) -> SocketAddr {
        self.bind_addr
    }

    /// Closes the HTTP server and waits for the background task to exit.
    pub async fn close(&mut self) -> Result<(), McpError> {
        self.cancellation.cancel();

        let join = self.join.lock().await.take();
        if let Some(join) = join {
            let result = join.await.map_err(|error| {
                McpError::server_crashed(
                    "MCP HTTP server task crashed",
                    None,
                    Some(json!({
                        "reason": error.to_string(),
                    })),
                )
            })?;
            result.map_err(|error| {
                McpError::server_crashed(
                    "MCP HTTP server exited with an error",
                    None,
                    Some(json!({
                        "reason": error.to_string(),
                    })),
                )
            })?;
        }

        Ok(())
    }
}

/// Unified handle returned by `McpServer::serve`.
#[derive(Debug)]
pub enum McpServerHandle {
    /// Stdio server handle.
    Stdio(McpStdioServerHandle),
    /// Streamable HTTP server handle.
    StreamableHttp(McpHttpServerHandle),
}

impl McpServerHandle {
    /// Returns the HTTP URL when the handle represents an HTTP server.
    #[must_use]
    pub fn url(&self) -> Option<&str> {
        match self {
            Self::StreamableHttp(handle) => Some(handle.url()),
            Self::Stdio(_) => None,
        }
    }

    /// Closes the running server.
    pub async fn close(&mut self) -> Result<(), McpError> {
        match self {
            Self::Stdio(handle) => handle.close().await,
            Self::StreamableHttp(handle) => handle.close().await,
        }
    }
}

#[derive(Clone)]
struct RegistryMcpServerHandler {
    name: String,
    registry: Arc<ToolRegistry>,
    instructions: Option<String>,
}

impl std::fmt::Debug for RegistryMcpServerHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistryMcpServerHandler")
            .field("name", &self.name)
            .field("instructions", &self.instructions)
            .finish_non_exhaustive()
    }
}

impl RegistryMcpServerHandler {
    const fn new(name: String, registry: Arc<ToolRegistry>, instructions: Option<String>) -> Self {
        Self {
            name,
            registry,
            instructions,
        }
    }
}

impl ServerHandler for RegistryMcpServerHandler {
    fn get_info(&self) -> ServerInfo {
        InitializeResult {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: self.name.clone(),
                title: None,
                version: env!("CARGO_PKG_VERSION").to_owned(),
                description: None,
                icons: None,
                website_url: None,
            },
            instructions: self.instructions.clone(),
            ..Default::default()
        }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, rmcp::ErrorData>> + Send + '_ {
        let registry = self.registry.clone();

        async move {
            let mut tools = Vec::new();
            for descriptor in registry.list() {
                let tool = mcp_tool_from_descriptor(&descriptor).map_err(|error: McpError| {
                    rmcp::ErrorData::internal_error(error.to_string(), error.correction_context())
                })?;
                tools.push(tool);
            }

            Ok(ListToolsResult::with_all_items(tools))
        }
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResult, rmcp::ErrorData>> + Send + '_ {
        let registry = self.registry.clone();

        async move {
            let canonical_name =
                decode_export_tool_name(request.name.as_ref()).map_err(|error| {
                    rmcp::ErrorData::invalid_params(error.to_string(), error.correction_context())
                })?;

            let call = arky_tools::ToolCall::new(
                format!("mcp-call-{}", NEXT_CALL_ID.fetch_add(1, Ordering::Relaxed)),
                canonical_name.clone(),
                request
                    .arguments
                    .into_iter()
                    .fold(Value::Null, |_, arguments| Value::Object(arguments)),
            );

            let result = registry.execute(call, context.ct.clone()).await;
            match result {
                Ok(result) => tool_result_to_mcp(&result).map_err(|error: McpError| {
                    rmcp::ErrorData::internal_error(error.to_string(), error.correction_context())
                }),
                Err(error) => Ok(tool_error_to_mcp_result(&error, &canonical_name)),
            }
        }
    }
}

fn tool_error_to_mcp_result(error: &arky_tools::ToolError, canonical_name: &str) -> CallToolResult {
    CallToolResult::structured_error(json!({
        "canonical_name": canonical_name,
        "error_code": error.error_code(),
        "message": error.to_string(),
        "details": error.correction_context(),
    }))
}

fn map_server_init_error(error: &rmcp::service::ServerInitializeError) -> McpError {
    McpError::connection_failed(
        "failed to initialize the MCP server transport",
        None,
        Some(json!({
            "reason": error.to_string(),
        })),
    )
}

fn normalize_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    }
}

fn socket_addr_host(addr: SocketAddr) -> String {
    match addr.ip() {
        IpAddr::V4(ip) => format!("{ip}:{}", addr.port()),
        IpAddr::V6(ip) => format!("[{ip}]:{}", addr.port()),
    }
}
