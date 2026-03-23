//! MCP client implementation for stdio and streamable HTTP transports.

use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

use arky_error::ClassifiedError;
use arky_tools::{Tool, ToolDescriptor, ToolError};
use async_trait::async_trait;
use axum::http::{HeaderName, HeaderValue};
use rmcp::{
    model::{CallToolRequestParams, ClientRequest, PingRequest},
    service::{ClientInitializeError, Peer, RoleClient, ServiceError},
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
        TokioChildProcess,
    },
    ServiceExt,
};
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::{tool_descriptor_from_mcp, tool_result_from_mcp, McpAuth, McpError};

#[derive(Debug, Clone)]
struct ImportedToolDefinition {
    remote_name: String,
    descriptor: ToolDescriptor,
}

/// Connection lifecycle state for an `McpClient`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// The client is not connected.
    Disconnected,
    /// An initial connection attempt is in progress.
    Connecting,
    /// The transport is healthy and initialized.
    Connected,
    /// A reconnect attempt is in progress.
    Reconnecting,
}

#[derive(Debug)]
struct ConnectionLifecycle {
    state: ConnectionState,
}

impl Default for ConnectionLifecycle {
    fn default() -> Self {
        Self {
            state: ConnectionState::Disconnected,
        }
    }
}

impl ConnectionLifecycle {
    const fn begin_connect(&mut self) {
        self.state = ConnectionState::Connecting;
    }

    const fn connected(&mut self) {
        self.state = ConnectionState::Connected;
    }

    const fn begin_reconnect(&mut self) {
        self.state = ConnectionState::Reconnecting;
    }

    const fn disconnected(&mut self) {
        self.state = ConnectionState::Disconnected;
    }

    const fn state(&self) -> ConnectionState {
        self.state
    }
}

/// Stdio transport configuration for an MCP client.
#[derive(Debug, Clone)]
pub struct McpStdioClientConfig {
    /// Canonical server name used for imported tool IDs.
    pub server_name: String,
    /// Executable to spawn.
    pub command: String,
    /// Process arguments.
    pub args: Vec<String>,
    /// Optional child environment overrides.
    pub env: BTreeMap<String, String>,
    /// Optional working directory.
    pub cwd: Option<PathBuf>,
    /// Optional keepalive ping interval.
    pub keepalive_interval: Option<Duration>,
}

impl McpStdioClientConfig {
    /// Creates a stdio client configuration.
    #[must_use]
    pub fn new(
        server_name: impl Into<String>,
        command: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            server_name: server_name.into(),
            command: command.into(),
            args: args.into_iter().map(Into::into).collect(),
            env: BTreeMap::new(),
            cwd: None,
            keepalive_interval: None,
        }
    }
}

/// Streamable HTTP transport configuration for an MCP client.
#[derive(Debug, Clone)]
pub struct McpHttpClientConfig {
    /// Canonical server name used for imported tool IDs.
    pub server_name: String,
    /// Base MCP endpoint URI.
    pub url: String,
    /// Optional bearer-token or OAuth auth.
    pub auth: Option<McpAuth>,
    /// Optional custom headers.
    pub headers: HashMap<String, String>,
    /// Whether stateless responses are acceptable.
    pub allow_stateless: bool,
    /// Optional keepalive ping interval.
    pub keepalive_interval: Option<Duration>,
}

impl McpHttpClientConfig {
    /// Creates a streamable-HTTP client configuration.
    #[must_use]
    pub fn new(server_name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            server_name: server_name.into(),
            url: url.into(),
            auth: None,
            headers: HashMap::new(),
            allow_stateless: true,
            keepalive_interval: None,
        }
    }
}

/// Supported transport configuration for an `McpClient`.
#[derive(Debug, Clone)]
pub enum McpClientConfig {
    /// Local subprocess transport over stdio.
    Stdio(McpStdioClientConfig),
    /// Remote streamable HTTP transport.
    StreamableHttp(McpHttpClientConfig),
}

impl McpClientConfig {
    fn server_name(&self) -> &str {
        match self {
            Self::Stdio(config) => &config.server_name,
            Self::StreamableHttp(config) => &config.server_name,
        }
    }

    const fn keepalive_interval(&self) -> Option<Duration> {
        match self {
            Self::Stdio(config) => config.keepalive_interval,
            Self::StreamableHttp(config) => config.keepalive_interval,
        }
    }
}

#[derive(Debug)]
struct McpClientInner {
    config: McpClientConfig,
    lifecycle: RwLock<ConnectionLifecycle>,
    running: Mutex<Option<rmcp::service::RunningService<RoleClient, ()>>>,
    imported_tools: RwLock<BTreeMap<String, ImportedToolDefinition>>,
    keepalive: Mutex<Option<tokio::task::JoinHandle<()>>>,
    keepalive_cancel: Mutex<Option<CancellationToken>>,
}

/// Client wrapper for importing tools from an external MCP server.
#[derive(Clone, Debug)]
pub struct McpClient {
    inner: Arc<McpClientInner>,
}

impl McpClient {
    /// Creates a new MCP client from explicit transport configuration.
    #[must_use]
    pub fn new(config: McpClientConfig) -> Self {
        Self {
            inner: Arc::new(McpClientInner {
                config,
                lifecycle: RwLock::new(ConnectionLifecycle::default()),
                running: Mutex::new(None),
                imported_tools: RwLock::new(BTreeMap::new()),
                keepalive: Mutex::new(None),
                keepalive_cancel: Mutex::new(None),
            }),
        }
    }

    /// Creates a stdio-backed MCP client.
    #[must_use]
    pub fn stdio(config: McpStdioClientConfig) -> Self {
        Self::new(McpClientConfig::Stdio(config))
    }

    /// Creates a streamable-HTTP-backed MCP client.
    #[must_use]
    pub fn http(config: McpHttpClientConfig) -> Self {
        Self::new(McpClientConfig::StreamableHttp(config))
    }

    /// Returns the current connection state.
    #[must_use]
    pub fn connection_state(&self) -> ConnectionState {
        self.inner
            .lifecycle
            .read()
            .expect("mcp client lifecycle lock should not be poisoned")
            .state()
    }

    /// Connects to the configured MCP server and discovers its tools.
    pub async fn connect(&self) -> Result<(), McpError> {
        if self.connection_state() == ConnectionState::Connected {
            return Ok(());
        }

        self.inner
            .lifecycle
            .write()
            .expect("mcp client lifecycle lock should not be poisoned")
            .begin_connect();

        match self.connect_inner().await {
            Ok(()) => Ok(()),
            Err(error) => {
                self.inner
                    .lifecycle
                    .write()
                    .expect("mcp client lifecycle lock should not be poisoned")
                    .disconnected();
                Err(error)
            }
        }
    }

    /// Disconnects from the MCP server and clears discovered tools.
    pub async fn disconnect(&self) -> Result<(), McpError> {
        self.stop_keepalive().await;

        let running = self.inner.running.lock().await.take();
        if let Some(mut running) = running {
            running.close().await.map_err(|error| {
                McpError::server_crashed(
                    "failed to close MCP connection cleanly",
                    Some(self.inner.config.server_name().to_owned()),
                    Some(serde_json::json!({
                        "reason": error.to_string(),
                    })),
                )
            })?;
        }

        self.inner
            .imported_tools
            .write()
            .expect("mcp imported-tools lock should not be poisoned")
            .clear();
        self.inner
            .lifecycle
            .write()
            .expect("mcp client lifecycle lock should not be poisoned")
            .disconnected();

        Ok(())
    }

    /// Disconnects and reconnects using the stored configuration.
    pub async fn reconnect(&self) -> Result<(), McpError> {
        self.inner
            .lifecycle
            .write()
            .expect("mcp client lifecycle lock should not be poisoned")
            .begin_reconnect();
        self.disconnect().await?;
        self.connect().await
    }

    /// Refreshes the remote tool list without reconnecting.
    pub async fn refresh_tools(&self) -> Result<Vec<ToolDescriptor>, McpError> {
        let peer = self.connected_peer().await?;
        let definitions = self.load_remote_tools(&peer).await?;
        let descriptors = definitions
            .values()
            .map(|tool| tool.descriptor.clone())
            .collect::<Vec<_>>();

        *self
            .inner
            .imported_tools
            .write()
            .expect("mcp imported-tools lock should not be poisoned") = definitions;

        Ok(descriptors)
    }

    /// Returns the imported tool descriptors currently known to the client.
    #[must_use]
    pub fn descriptors(&self) -> Vec<ToolDescriptor> {
        self.inner
            .imported_tools
            .read()
            .expect("mcp imported-tools lock should not be poisoned")
            .values()
            .map(|tool| tool.descriptor.clone())
            .collect()
    }

    /// Returns the imported tools as Arky tool adapters.
    #[must_use]
    pub fn tools(&self) -> Vec<Box<dyn Tool>> {
        self.tool_arcs()
            .into_iter()
            .map(|tool| Box::new(ArcToolAdapter(tool)) as Box<dyn Tool>)
            .collect()
    }

    pub(crate) fn tool_arcs(&self) -> Vec<Arc<dyn Tool>> {
        self.inner
            .imported_tools
            .read()
            .expect("mcp imported-tools lock should not be poisoned")
            .values()
            .cloned()
            .map(|tool| {
                Arc::new(McpImportedToolAdapter {
                    client: self.clone(),
                    remote_name: tool.remote_name,
                    descriptor: tool.descriptor,
                }) as Arc<dyn Tool>
            })
            .collect()
    }

    /// Calls a remote MCP tool by its MCP name.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<arky_tools::ToolResult, McpError> {
        let peer = self.connected_peer().await?;
        let imported_tool = self
            .inner
            .imported_tools
            .read()
            .expect("mcp imported-tools lock should not be poisoned")
            .values()
            .find(|tool| tool.remote_name == name)
            .cloned()
            .ok_or_else(|| {
                McpError::schema_mismatch(
                    "unknown remote MCP tool",
                    Some(serde_json::json!({
                        "tool_name": name,
                    })),
                )
            })?;

        let arguments = match arguments {
            Value::Object(arguments) => Some(arguments),
            Value::Null => None,
            other => {
                return Err(McpError::schema_mismatch(
                    "MCP tool arguments must be a JSON object",
                    Some(serde_json::json!({
                        "tool_name": name,
                        "arguments": other,
                    })),
                ));
            }
        };

        let result = peer
            .call_tool(CallToolRequestParams {
                meta: None,
                name: imported_tool.remote_name.clone().into(),
                arguments,
                task: None,
            })
            .await
            .map_err(|error| self.map_service_error(error))?;

        tool_result_from_mcp(
            format!("remote-call-{name}"),
            imported_tool.descriptor.canonical_name,
            result,
        )
    }

    async fn connect_inner(&self) -> Result<(), McpError> {
        let running = match &self.inner.config {
            McpClientConfig::Stdio(config) => self.connect_stdio(config).await?,
            McpClientConfig::StreamableHttp(config) => self.connect_http(config).await?,
        };

        let peer = running.peer().clone();
        let discovered_tools = self.load_remote_tools(&peer).await?;

        *self.inner.running.lock().await = Some(running);
        *self
            .inner
            .imported_tools
            .write()
            .expect("mcp imported-tools lock should not be poisoned") = discovered_tools;
        self.inner
            .lifecycle
            .write()
            .expect("mcp client lifecycle lock should not be poisoned")
            .connected();

        if let Some(interval) = self.inner.config.keepalive_interval() {
            self.start_keepalive(peer, interval).await;
        }

        Ok(())
    }

    async fn connect_stdio(
        &self,
        config: &McpStdioClientConfig,
    ) -> Result<rmcp::service::RunningService<RoleClient, ()>, McpError> {
        let mut command = tokio::process::Command::new(&config.command);
        command.args(&config.args);
        if let Some(cwd) = &config.cwd {
            command.current_dir(cwd);
        }
        if !config.env.is_empty() {
            command.envs(&config.env);
        }

        let transport = TokioChildProcess::new(command).map_err(|error| {
            McpError::connection_failed(
                "failed to spawn MCP stdio server",
                Some(config.server_name.clone()),
                Some(serde_json::json!({
                    "command": config.command,
                    "reason": error.to_string(),
                })),
            )
        })?;

        ().serve(transport)
            .await
            .map_err(|error| self.map_initialize_error(&error))
    }

    async fn connect_http(
        &self,
        config: &McpHttpClientConfig,
    ) -> Result<rmcp::service::RunningService<RoleClient, ()>, McpError> {
        let http_config = Self::http_transport_config(config)?;

        let running = match &config.auth {
            Some(McpAuth::OAuth(auth)) => {
                let transport =
                    StreamableHttpClientTransport::with_client(auth.client(), http_config);
                ().serve(transport).await
            }
            Some(McpAuth::BearerToken(_)) | None => {
                let transport = StreamableHttpClientTransport::from_config(http_config);
                ().serve(transport).await
            }
        };

        running.map_err(|error| self.map_initialize_error(&error))
    }

    fn http_transport_config(
        config: &McpHttpClientConfig,
    ) -> Result<StreamableHttpClientTransportConfig, McpError> {
        let mut transport_config =
            StreamableHttpClientTransportConfig::with_uri(config.url.clone());
        transport_config.allow_stateless = config.allow_stateless;

        if let Some(McpAuth::BearerToken(token)) = &config.auth {
            transport_config = transport_config.auth_header(token.clone());
        }

        if !config.headers.is_empty() {
            let mut headers = HashMap::with_capacity(config.headers.len());
            for (name, value) in &config.headers {
                let header_name = HeaderName::try_from(name.as_str()).map_err(|error| {
                    McpError::protocol_error(
                        "invalid custom HTTP header name",
                        Some(serde_json::json!({
                            "name": name,
                            "reason": error.to_string(),
                        })),
                    )
                })?;
                let header_value = HeaderValue::try_from(value.as_str()).map_err(|error| {
                    McpError::protocol_error(
                        "invalid custom HTTP header value",
                        Some(serde_json::json!({
                            "name": name,
                            "reason": error.to_string(),
                        })),
                    )
                })?;
                headers.insert(header_name, header_value);
            }
            transport_config = transport_config.custom_headers(headers);
        }

        Ok(transport_config)
    }

    async fn load_remote_tools(
        &self,
        peer: &Peer<RoleClient>,
    ) -> Result<BTreeMap<String, ImportedToolDefinition>, McpError> {
        let server_name = self.inner.config.server_name().to_owned();
        let tools = peer.list_all_tools().await.map_err(|error| {
            McpError::protocol_error(
                "failed to list tools from the remote MCP server",
                Some(serde_json::json!({
                    "server_name": server_name,
                    "reason": error.to_string(),
                })),
            )
        })?;

        let mut imported = BTreeMap::new();
        for tool in tools {
            let descriptor = tool_descriptor_from_mcp(&server_name, &tool)?;
            let canonical_name = descriptor.canonical_name.clone();
            imported.insert(
                canonical_name,
                ImportedToolDefinition {
                    remote_name: tool.name.to_string(),
                    descriptor,
                },
            );
        }

        Ok(imported)
    }

    async fn connected_peer(&self) -> Result<Peer<RoleClient>, McpError> {
        self.inner
            .running
            .lock()
            .await
            .as_ref()
            .map(|running| running.peer().clone())
            .ok_or_else(|| {
                McpError::connection_failed(
                    "MCP client is not connected",
                    Some(self.inner.config.server_name().to_owned()),
                    None,
                )
            })
    }

    async fn start_keepalive(&self, peer: Peer<RoleClient>, interval: Duration) {
        self.stop_keepalive().await;

        let cancel = CancellationToken::new();
        let child_cancel = cancel.child_token();
        let client = self.clone();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = child_cancel.cancelled() => break,
                    () = tokio::time::sleep(interval) => {
                        let ping = peer.send_request(ClientRequest::PingRequest(PingRequest::default())).await;
                        if ping.is_err() {
                            warn!(
                                server_name = client.inner.config.server_name(),
                                "MCP keepalive ping failed; marking client disconnected",
                            );
                            client.inner
                                .lifecycle
                                .write()
                                .expect("mcp client lifecycle lock should not be poisoned")
                                .disconnected();
                            break;
                        }
                    }
                }
            }
        });

        *self.inner.keepalive.lock().await = Some(task);
        *self.inner.keepalive_cancel.lock().await = Some(cancel);
    }

    async fn stop_keepalive(&self) {
        let cancel = self.inner.keepalive_cancel.lock().await.take();
        if let Some(cancel) = cancel {
            cancel.cancel();
        }

        let task = self.inner.keepalive.lock().await.take();
        if let Some(task) = task {
            let _ = task.await;
        }
    }

    fn map_initialize_error(&self, error: &ClientInitializeError) -> McpError {
        let reason = error.to_string();
        if reason.contains("Auth error")
            || reason.contains("Auth required")
            || reason.contains("unauthorized")
        {
            return McpError::auth_failed(
                "failed to authenticate with the MCP server",
                Some(serde_json::json!({
                    "server_name": self.inner.config.server_name(),
                    "reason": reason,
                })),
            );
        }

        McpError::connection_failed(
            "failed to initialize the MCP client transport",
            Some(self.inner.config.server_name().to_owned()),
            Some(serde_json::json!({
                "reason": error.to_string(),
            })),
        )
    }

    fn map_service_error(&self, error: ServiceError) -> McpError {
        match error {
            ServiceError::TransportClosed | ServiceError::Cancelled { .. } => {
                McpError::server_crashed(
                    "MCP transport closed while a request was in flight",
                    Some(self.inner.config.server_name().to_owned()),
                    Some(serde_json::json!({
                        "reason": error.to_string(),
                    })),
                )
            }
            ServiceError::UnexpectedResponse => McpError::protocol_error(
                "remote MCP server returned an unexpected response",
                Some(serde_json::json!({
                    "server_name": self.inner.config.server_name(),
                    "reason": error.to_string(),
                })),
            ),
            ServiceError::Timeout { .. } => McpError::connection_failed(
                "MCP request timed out",
                Some(self.inner.config.server_name().to_owned()),
                Some(serde_json::json!({
                    "reason": error.to_string(),
                })),
            ),
            ServiceError::McpError(error) => McpError::protocol_error(
                "remote MCP server rejected the request",
                Some(serde_json::json!({
                    "server_name": self.inner.config.server_name(),
                    "reason": error.to_string(),
                })),
            ),
            ServiceError::TransportSend(_) => McpError::connection_failed(
                "failed to send a message to the MCP server",
                Some(self.inner.config.server_name().to_owned()),
                Some(serde_json::json!({
                    "reason": error.to_string(),
                })),
            ),
            _ => McpError::protocol_error(
                "unexpected MCP service error",
                Some(serde_json::json!({
                    "server_name": self.inner.config.server_name(),
                    "reason": error.to_string(),
                })),
            ),
        }
    }
}

struct ArcToolAdapter(Arc<dyn Tool>);

#[async_trait]
impl Tool for ArcToolAdapter {
    fn descriptor(&self) -> ToolDescriptor {
        self.0.descriptor()
    }

    async fn execute(
        &self,
        call: arky_tools::ToolCall,
        cancel: CancellationToken,
    ) -> Result<arky_tools::ToolResult, ToolError> {
        self.0.execute(call, cancel).await
    }
}

#[derive(Clone)]
struct McpImportedToolAdapter {
    client: McpClient,
    remote_name: String,
    descriptor: ToolDescriptor,
}

impl fmt::Debug for McpImportedToolAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpImportedToolAdapter")
            .field("remote_name", &self.remote_name)
            .field("descriptor", &self.descriptor)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl Tool for McpImportedToolAdapter {
    fn descriptor(&self) -> ToolDescriptor {
        self.descriptor.clone()
    }

    async fn execute(
        &self,
        call: arky_tools::ToolCall,
        cancel: CancellationToken,
    ) -> Result<arky_tools::ToolResult, ToolError> {
        if cancel.is_cancelled() {
            return Err(ToolError::cancelled(
                "tool execution was cancelled before it started",
                Some(self.descriptor.canonical_name.clone()),
            ));
        }

        self.client
            .call_tool(&self.remote_name, call.input.clone())
            .await
            .map(|result| arky_tools::ToolResult {
                id: call.id,
                name: self.descriptor.canonical_name.clone(),
                content: result.content,
                is_error: result.is_error,
                parent_id: call.parent_id,
            })
            .map_err(|error| match error {
                McpError::SchemaMismatch { .. } => {
                    ToolError::invalid_args(error.to_string(), error.correction_context())
                }
                McpError::ConnectionFailed { .. } | McpError::ServerCrashed { .. } => {
                    ToolError::timeout(
                        error.to_string(),
                        Some(self.descriptor.canonical_name.clone()),
                        None,
                    )
                }
                McpError::ProtocolError { .. } | McpError::AuthFailed { .. } => {
                    ToolError::execution_failed(
                        error.to_string(),
                        Some(self.descriptor.canonical_name.clone()),
                    )
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::client::{ConnectionLifecycle, ConnectionState};

    #[test]
    fn connection_lifecycle_should_cover_expected_state_transitions() {
        let mut lifecycle = ConnectionLifecycle::default();
        assert_eq!(lifecycle.state(), ConnectionState::Disconnected);

        lifecycle.begin_connect();
        assert_eq!(lifecycle.state(), ConnectionState::Connecting);

        lifecycle.connected();
        assert_eq!(lifecycle.state(), ConnectionState::Connected);

        lifecycle.begin_reconnect();
        assert_eq!(lifecycle.state(), ConnectionState::Reconnecting);

        lifecycle.disconnected();
        assert_eq!(lifecycle.state(), ConnectionState::Disconnected);
    }
}
