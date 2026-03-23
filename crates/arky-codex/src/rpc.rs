//! Newline-delimited JSON-RPC transport for the Codex app-server.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use arky_provider::ProviderError;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, BufWriter},
    sync::{mpsc, oneshot, watch, Mutex as AsyncMutex},
    task::JoinHandle,
    time::timeout,
};
use tracing::debug;

use crate::notification::CodexNotification;

/// JSON-RPC identifier accepted by the Codex transport.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcId {
    /// Numeric identifier.
    Number(i64),
    /// String identifier.
    String(String),
    /// Null identifier.
    Null,
}

impl std::fmt::Display for JsonRpcId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(value) => write!(f, "{value}"),
            Self::String(value) => f.write_str(value),
            Self::Null => f.write_str("null"),
        }
    }
}

/// JSON-RPC error payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcErrorObject {
    /// Stable error code.
    pub code: i64,
    /// Human-readable message.
    pub message: String,
    /// Optional structured details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// `initialize.params.clientInfo`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeClientInfo {
    /// Client identifier.
    pub name: String,
    /// Optional user-facing title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Client version string.
    pub version: String,
}

/// `initialize.params.capabilities`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeCapabilities {
    /// Whether experimental app-server APIs should be enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental_api: Option<bool>,
    /// Exact method names to suppress for this connection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub opt_out_notification_methods: Vec<String>,
}

/// Parameters for the initial JSON-RPC handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    /// Protocol version requested by the client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<u32>,
    /// Client metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_info: Option<InitializeClientInfo>,
    /// Capability negotiation data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<InitializeCapabilities>,
}

impl Default for InitializeParams {
    fn default() -> Self {
        Self {
            protocol_version: Some(1),
            client_info: Some(InitializeClientInfo {
                name: "arky-codex".to_owned(),
                title: Some("Arky Codex Provider".to_owned()),
                version: env!("CARGO_PKG_VERSION").to_owned(),
            }),
            capabilities: Some(InitializeCapabilities {
                experimental_api: Some(false),
                opt_out_notification_methods: Vec::new(),
            }),
        }
    }
}

/// Shape returned by the `initialize` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    /// Negotiated protocol version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<u32>,
    /// Server metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_info: Option<Value>,
    /// Client-facing upstream user agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    /// Runtime platform family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_family: Option<String>,
    /// Runtime platform OS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_os: Option<String>,
    /// Additional capabilities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Value>,
}

/// Server-initiated JSON-RPC request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexServerRequest {
    /// Request identifier.
    pub id: JsonRpcId,
    /// Request method.
    pub method: String,
    /// Request parameters.
    pub params: Value,
}

/// Runtime configuration for [`RpcTransport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RpcTransportConfig {
    /// Maximum time to wait for one correlated response.
    pub request_timeout: Duration,
    /// Maximum buffered outbound frames.
    pub write_capacity: usize,
}

impl Default for RpcTransportConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            write_capacity: 64,
        }
    }
}

#[derive(Debug)]
struct PendingRequest {
    method: String,
    sender: oneshot::Sender<Result<Value, JsonRpcErrorObject>>,
}

#[derive(Debug, Serialize)]
struct OutgoingRequest<'a> {
    jsonrpc: &'static str,
    id: &'a JsonRpcId,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<&'a Value>,
}

#[derive(Debug, Serialize)]
struct OutgoingNotification<'a> {
    jsonrpc: &'static str,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<&'a Value>,
}

#[derive(Debug, Serialize)]
struct OutgoingResponse<'a> {
    jsonrpc: &'static str,
    id: &'a JsonRpcId,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a JsonRpcErrorObject>,
}

#[derive(Debug)]
enum IncomingMessage {
    Response {
        id: JsonRpcId,
        result: Result<Value, JsonRpcErrorObject>,
    },
    Notification(CodexNotification),
    Request(CodexServerRequest),
}

/// JSON-RPC client transport over newline-delimited stdio.
pub struct RpcTransport {
    write_tx: mpsc::Sender<String>,
    pending: Arc<AsyncMutex<HashMap<String, PendingRequest>>>,
    next_id: AtomicU64,
    request_timeout: Duration,
    fatal_rx: watch::Receiver<Option<ProviderError>>,
    notifications_rx:
        Mutex<Option<mpsc::UnboundedReceiver<Result<CodexNotification, ProviderError>>>>,
    server_requests_rx:
        Mutex<Option<mpsc::UnboundedReceiver<Result<CodexServerRequest, ProviderError>>>>,
    read_task: JoinHandle<()>,
    write_task: JoinHandle<()>,
}

impl RpcTransport {
    /// Creates a transport from an async reader/writer pair.
    #[must_use]
    pub fn new<R, W>(reader: R, writer: W, config: RpcTransportConfig) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (write_tx, write_rx) = mpsc::channel(config.write_capacity);
        let (notifications_tx, notifications_rx) = mpsc::unbounded_channel();
        let (server_requests_tx, server_requests_rx) = mpsc::unbounded_channel();
        let (fatal_tx, fatal_rx) = watch::channel(None);
        let pending = Arc::new(AsyncMutex::new(HashMap::new()));

        let read_task = tokio::spawn(read_loop(
            reader,
            pending.clone(),
            fatal_tx.clone(),
            notifications_tx,
            server_requests_tx,
        ));
        let write_task = tokio::spawn(write_loop(writer, write_rx, fatal_tx));

        Self {
            write_tx,
            pending,
            next_id: AtomicU64::new(0),
            request_timeout: config.request_timeout,
            fatal_rx,
            notifications_rx: Mutex::new(Some(notifications_rx)),
            server_requests_rx: Mutex::new(Some(server_requests_rx)),
            read_task,
            write_task,
        }
    }

    /// Sends a request and waits for its correlated response.
    pub async fn request<T>(&self, method: &str, params: Option<Value>) -> Result<T, ProviderError>
    where
        T: DeserializeOwned,
    {
        let value = self.request_value(method, params).await?;
        serde_json::from_value(value).map_err(|error| {
            ProviderError::protocol_violation(
                format!("failed to decode response for `{method}`"),
                Some(json!({
                    "reason": error.to_string(),
                })),
            )
        })
    }

    /// Sends a request and returns the raw JSON result.
    pub async fn request_value(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, ProviderError> {
        if let Some(error) = self.fatal_error() {
            return Err(error);
        }

        let id = JsonRpcId::Number(
            i64::try_from(self.next_id.fetch_add(1, Ordering::Relaxed)).unwrap_or(i64::MAX),
        );
        let key = request_key(&id).ok_or_else(|| {
            ProviderError::protocol_violation("outgoing requests require a non-null id", None)
        })?;
        let (sender, receiver) = oneshot::channel();

        self.pending.lock().await.insert(
            key.clone(),
            PendingRequest {
                method: method.to_owned(),
                sender,
            },
        );

        let outbound = serde_json::to_string(&OutgoingRequest {
            jsonrpc: "2.0",
            id: &id,
            method,
            params: params.as_ref(),
        })
        .map_err(|error| {
            ProviderError::protocol_violation(
                format!("failed to serialize request `{method}`"),
                Some(json!({
                    "reason": error.to_string(),
                })),
            )
        })?;
        self.write_tx
            .send(outbound)
            .await
            .map_err(|_| ProviderError::stream_interrupted("rpc writer task has shut down"))?;

        let mut fatal_rx = self.fatal_rx.clone();
        let wait = timeout(self.request_timeout, receiver);
        let result = tokio::select! {
            biased;
            result = wait => {
                match result {
                    Ok(Ok(Ok(value))) => {
                        self.pending.lock().await.remove(&key);
                        return Ok(value);
                    }
                    Ok(Ok(Err(error))) => ProviderError::protocol_violation(
                        format!("rpc request `{method}` failed: {}", error.message),
                        Some(json!({
                            "code": error.code,
                            "data": error.data,
                        })),
                    ),
                    Ok(Err(_)) => ProviderError::stream_interrupted(
                        format!("rpc response channel closed for `{method}`")
                    ),
                    Err(_) => ProviderError::stream_interrupted(
                        format!("rpc request `{method}` timed out")
                    ),
                }
            }
            changed = fatal_rx.changed() => {
                changed.map_err(|_| ProviderError::stream_interrupted("rpc fatal state watcher closed"))?;
                self.fatal_error().ok_or_else(|| {
                    ProviderError::stream_interrupted("rpc transport closed while waiting for fatal state")
                })?
            }
        };

        self.pending.lock().await.remove(&key);
        Err(result)
    }

    /// Sends a JSON-RPC notification.
    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<(), ProviderError> {
        if let Some(error) = self.fatal_error() {
            return Err(error);
        }

        let outbound = serde_json::to_string(&OutgoingNotification {
            jsonrpc: "2.0",
            method,
            params: params.as_ref(),
        })
        .map_err(|error| {
            ProviderError::protocol_violation(
                format!("failed to serialize notification `{method}`"),
                Some(json!({
                    "reason": error.to_string(),
                })),
            )
        })?;

        self.write_tx
            .send(outbound)
            .await
            .map_err(|_| ProviderError::stream_interrupted("rpc writer task has shut down"))
    }

    /// Sends a successful JSON-RPC response.
    pub async fn respond(&self, id: JsonRpcId, result: Value) -> Result<(), ProviderError> {
        self.send_response(id, Some(result), None).await
    }

    /// Sends an error JSON-RPC response.
    pub async fn respond_error(
        &self,
        id: JsonRpcId,
        error: JsonRpcErrorObject,
    ) -> Result<(), ProviderError> {
        self.send_response(id, None, Some(error)).await
    }

    /// Performs the `initialize`/`initialized` handshake.
    pub async fn initialize(
        &self,
        params: InitializeParams,
    ) -> Result<InitializeResponse, ProviderError> {
        let response = self
            .request::<InitializeResponse>(
                "initialize",
                Some(serde_json::to_value(params).map_err(|error| {
                    ProviderError::protocol_violation(
                        "failed to serialize initialize params",
                        Some(json!({
                            "reason": error.to_string(),
                        })),
                    )
                })?),
            )
            .await?;
        self.notify("initialized", None).await?;
        Ok(response)
    }

    /// Takes the notification receiver. This may only be called once.
    #[must_use]
    pub fn take_notifications(
        &self,
    ) -> Option<mpsc::UnboundedReceiver<Result<CodexNotification, ProviderError>>> {
        self.notifications_rx.lock().ok()?.take()
    }

    /// Takes the server-request receiver. This may only be called once.
    #[must_use]
    pub fn take_server_requests(
        &self,
    ) -> Option<mpsc::UnboundedReceiver<Result<CodexServerRequest, ProviderError>>> {
        self.server_requests_rx.lock().ok()?.take()
    }

    /// Returns the terminal transport error once one has been recorded.
    #[must_use]
    pub fn fatal_error(&self) -> Option<ProviderError> {
        self.fatal_rx.borrow().clone()
    }

    async fn send_response(
        &self,
        id: JsonRpcId,
        result: Option<Value>,
        error: Option<JsonRpcErrorObject>,
    ) -> Result<(), ProviderError> {
        let outbound = serde_json::to_string(&OutgoingResponse {
            jsonrpc: "2.0",
            id: &id,
            result: result.as_ref(),
            error: error.as_ref(),
        })
        .map_err(|serialize_error| {
            ProviderError::protocol_violation(
                "failed to serialize json-rpc response",
                Some(json!({
                    "reason": serialize_error.to_string(),
                })),
            )
        })?;

        self.write_tx
            .send(outbound)
            .await
            .map_err(|_| ProviderError::stream_interrupted("rpc writer task has shut down"))
    }
}

impl Drop for RpcTransport {
    fn drop(&mut self) {
        self.read_task.abort();
        self.write_task.abort();
    }
}

async fn read_loop<R>(
    reader: R,
    pending: Arc<AsyncMutex<HashMap<String, PendingRequest>>>,
    fatal_tx: watch::Sender<Option<ProviderError>>,
    notifications_tx: mpsc::UnboundedSender<Result<CodexNotification, ProviderError>>,
    server_requests_tx: mpsc::UnboundedSender<Result<CodexServerRequest, ProviderError>>,
) where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                let _ = break_on_fatal(
                    ProviderError::stream_interrupted("rpc input closed unexpectedly"),
                    &pending,
                    &fatal_tx,
                    &notifications_tx,
                    &server_requests_tx,
                )
                .await;
                break;
            }
            Ok(_) => {
                trim_line_endings(&mut line);
                if line.is_empty() {
                    continue;
                }

                if handle_incoming_line(
                    &line,
                    &pending,
                    &fatal_tx,
                    &notifications_tx,
                    &server_requests_tx,
                )
                .await
                {
                    break;
                }
            }
            Err(error) => {
                let _ = break_on_fatal(
                    ProviderError::stream_interrupted(format!("failed to read rpc frame: {error}")),
                    &pending,
                    &fatal_tx,
                    &notifications_tx,
                    &server_requests_tx,
                )
                .await;
                break;
            }
        }
    }
}

async fn handle_incoming_line(
    line: &str,
    pending: &Arc<AsyncMutex<HashMap<String, PendingRequest>>>,
    fatal_tx: &watch::Sender<Option<ProviderError>>,
    notifications_tx: &mpsc::UnboundedSender<Result<CodexNotification, ProviderError>>,
    server_requests_tx: &mpsc::UnboundedSender<Result<CodexServerRequest, ProviderError>>,
) -> bool {
    match parse_incoming(line) {
        Ok(message) => {
            handle_incoming_message(
                message,
                pending,
                fatal_tx,
                notifications_tx,
                server_requests_tx,
            )
            .await
        }
        Err(error) => {
            break_on_fatal(
                error,
                pending,
                fatal_tx,
                notifications_tx,
                server_requests_tx,
            )
            .await
        }
    }
}

async fn handle_incoming_message(
    message: IncomingMessage,
    pending: &Arc<AsyncMutex<HashMap<String, PendingRequest>>>,
    fatal_tx: &watch::Sender<Option<ProviderError>>,
    notifications_tx: &mpsc::UnboundedSender<Result<CodexNotification, ProviderError>>,
    server_requests_tx: &mpsc::UnboundedSender<Result<CodexServerRequest, ProviderError>>,
) -> bool {
    match message {
        IncomingMessage::Notification(notification) => {
            if notifications_tx.send(Ok(notification)).is_ok() {
                return false;
            }

            break_on_fatal(
                ProviderError::stream_interrupted("notification stream dropped"),
                pending,
                fatal_tx,
                notifications_tx,
                server_requests_tx,
            )
            .await
        }
        IncomingMessage::Request(request) => {
            if server_requests_tx.send(Ok(request)).is_ok() {
                return false;
            }

            break_on_fatal(
                ProviderError::stream_interrupted("server request stream dropped"),
                pending,
                fatal_tx,
                notifications_tx,
                server_requests_tx,
            )
            .await
        }
        IncomingMessage::Response { id, result } => {
            let Some(key) = request_key(&id) else {
                return break_on_fatal(
                    ProviderError::protocol_violation("received a response with a null id", None),
                    pending,
                    fatal_tx,
                    notifications_tx,
                    server_requests_tx,
                )
                .await;
            };

            let pending_request = pending.lock().await.remove(&key);
            let Some(pending_request) = pending_request else {
                return break_on_fatal(
                    ProviderError::protocol_violation(
                        format!("json-rpc transport desync: unknown response id `{key}`"),
                        None,
                    ),
                    pending,
                    fatal_tx,
                    notifications_tx,
                    server_requests_tx,
                )
                .await;
            };
            debug!(
                method = %pending_request.method,
                response_id = %key,
                "resolved rpc response"
            );
            let _ = pending_request.sender.send(result);
            false
        }
    }
}

async fn break_on_fatal(
    error: ProviderError,
    pending: &Arc<AsyncMutex<HashMap<String, PendingRequest>>>,
    fatal_tx: &watch::Sender<Option<ProviderError>>,
    notifications_tx: &mpsc::UnboundedSender<Result<CodexNotification, ProviderError>>,
    server_requests_tx: &mpsc::UnboundedSender<Result<CodexServerRequest, ProviderError>>,
) -> bool {
    signal_fatal(
        error,
        pending,
        fatal_tx,
        notifications_tx,
        server_requests_tx,
    )
    .await;
    true
}

async fn write_loop<W>(
    writer: W,
    mut write_rx: mpsc::Receiver<String>,
    fatal_tx: watch::Sender<Option<ProviderError>>,
) where
    W: AsyncWrite + Unpin,
{
    let mut writer = BufWriter::new(writer);

    while let Some(frame) = write_rx.recv().await {
        if writer.write_all(frame.as_bytes()).await.is_err()
            || writer.write_all(b"\n").await.is_err()
            || writer.flush().await.is_err()
        {
            let _ = fatal_tx.send(Some(ProviderError::stream_interrupted(
                "failed to write rpc frame",
            )));
            break;
        }
    }

    let _ = writer.flush().await;
}

async fn signal_fatal(
    error: ProviderError,
    pending: &Arc<AsyncMutex<HashMap<String, PendingRequest>>>,
    fatal_tx: &watch::Sender<Option<ProviderError>>,
    notifications_tx: &mpsc::UnboundedSender<Result<CodexNotification, ProviderError>>,
    server_requests_tx: &mpsc::UnboundedSender<Result<CodexServerRequest, ProviderError>>,
) {
    let _ = fatal_tx.send(Some(error.clone()));

    let mut pending = pending.lock().await;
    pending.clear();
    drop(pending);

    let _ = notifications_tx.send(Err(error.clone()));
    let _ = server_requests_tx.send(Err(error));
}

fn request_key(id: &JsonRpcId) -> Option<String> {
    match id {
        JsonRpcId::Number(value) => Some(value.to_string()),
        JsonRpcId::String(value) => Some(value.clone()),
        JsonRpcId::Null => None,
    }
}

fn trim_line_endings(line: &mut String) {
    while matches!(line.chars().last(), Some('\n' | '\r')) {
        line.pop();
    }
}

fn parse_incoming(line: &str) -> Result<IncomingMessage, ProviderError> {
    let value: Value = serde_json::from_str(line).map_err(|error| {
        ProviderError::protocol_violation(
            "failed to parse json-rpc frame",
            Some(json!({
                "reason": error.to_string(),
                "line": line,
            })),
        )
    })?;
    let object = value.as_object().ok_or_else(|| {
        ProviderError::protocol_violation("json-rpc frame must be an object", None)
    })?;

    if let Some(method) = object.get("method").and_then(Value::as_str) {
        let params = object.get("params").cloned().unwrap_or(Value::Null);
        if let Some(id) = parse_id(object.get("id")) {
            return Ok(IncomingMessage::Request(CodexServerRequest {
                id,
                method: method.to_owned(),
                params,
            }));
        }

        return Ok(IncomingMessage::Notification(CodexNotification {
            method: method.to_owned(),
            params,
        }));
    }

    let id = parse_id(object.get("id")).ok_or_else(|| {
        ProviderError::protocol_violation(
            "json-rpc response is missing an id",
            Some(json!({
                "line": line,
            })),
        )
    })?;

    if let Some(error) = object.get("error") {
        return Ok(IncomingMessage::Response {
            id,
            result: Err(parse_error_object(error)?),
        });
    }

    let result = object.get("result").cloned().ok_or_else(|| {
        ProviderError::protocol_violation(
            "json-rpc response must include `result` or `error`",
            Some(json!({
                "line": line,
            })),
        )
    })?;

    Ok(IncomingMessage::Response {
        id,
        result: Ok(result),
    })
}

fn parse_id(value: Option<&Value>) -> Option<JsonRpcId> {
    match value? {
        Value::Number(number) => number.as_i64().map(JsonRpcId::Number),
        Value::String(value) => Some(JsonRpcId::String(value.clone())),
        Value::Null => Some(JsonRpcId::Null),
        _ => None,
    }
}

fn parse_error_object(value: &Value) -> Result<JsonRpcErrorObject, ProviderError> {
    let object = value.as_object().ok_or_else(|| {
        ProviderError::protocol_violation("json-rpc error payload must be an object", None)
    })?;
    let code = object.get("code").and_then(Value::as_i64).ok_or_else(|| {
        ProviderError::protocol_violation("json-rpc error is missing `code`", None)
    })?;
    let message = object
        .get("message")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProviderError::protocol_violation("json-rpc error is missing `message`", None)
        })?;

    Ok(JsonRpcErrorObject {
        code,
        message: message.to_owned(),
        data: object.get("data").cloned(),
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use arky_provider::ProviderError;
    use pretty_assertions::assert_eq;
    use serde_json::{json, Value};
    use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};

    use super::{
        parse_incoming, IncomingMessage, JsonRpcErrorObject, JsonRpcId, RpcTransport,
        RpcTransportConfig,
    };

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
    }

    fn fixture_names() -> Vec<String> {
        let mut names = fs::read_dir(fixture_dir())
            .expect("fixture directory should read")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
            .filter_map(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn parse_fixture(name: &str) -> Vec<IncomingMessage> {
        let raw = fs::read_to_string(fixture_dir().join(name)).expect("fixture should read");
        raw.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| parse_incoming(line).expect("fixture frame should parse"))
            .collect()
    }

    #[test]
    fn rpc_fixture_corpus_should_parse_without_errors() {
        let frame_count = fixture_names()
            .into_iter()
            .map(|name| parse_fixture(&name).len())
            .sum::<usize>();

        assert!(frame_count > 0);
    }

    #[test]
    fn rpc_fixture_corpus_should_cover_requests_notifications_and_responses() {
        let mut saw_request = false;
        let mut saw_notification = false;
        let mut saw_response = false;

        for fixture_name in fixture_names() {
            for frame in parse_fixture(&fixture_name) {
                match frame {
                    IncomingMessage::Request(_) => saw_request = true,
                    IncomingMessage::Notification(_) => saw_notification = true,
                    IncomingMessage::Response { .. } => saw_response = true,
                }
            }
        }

        assert!(saw_request);
        assert!(saw_notification);
        assert!(saw_response);
    }

    #[tokio::test]
    async fn rpc_transport_should_correlate_request_responses_by_id() {
        let (client_stream, server_stream) = duplex(8_192);
        let (client_read, client_write) = tokio::io::split(client_stream);
        let (server_read, mut server_write) = tokio::io::split(server_stream);
        let transport = RpcTransport::new(client_read, client_write, RpcTransportConfig::default());

        let server = tokio::spawn(async move {
            let mut reader = BufReader::new(server_read);
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .await
                .expect("request should read");
            let request: Value = serde_json::from_str(line.trim()).expect("request should parse");
            let id = request.get("id").cloned().expect("id should exist");
            server_write
                .write_all(
                    format!(
                        "{}\n",
                        json!({
                            "id": id,
                            "result": {
                                "ok": "done",
                            },
                        })
                    )
                    .as_bytes(),
                )
                .await
                .expect("response should write");
            server_write.flush().await.expect("response should flush");
        });

        let response: Value = transport
            .request_value("thread/start", Some(json!({"model": "gpt-5"})))
            .await
            .expect("request should succeed");

        server.await.expect("server task should finish");
        assert_eq!(response, json!({"ok": "done"}));
    }

    #[tokio::test]
    async fn rpc_transport_should_parse_multiple_newline_delimited_messages() {
        let (client_stream, mut server_stream) = duplex(8_192);
        let (client_read, client_write) = tokio::io::split(client_stream);
        let transport = RpcTransport::new(client_read, client_write, RpcTransportConfig::default());
        let mut notifications = transport
            .take_notifications()
            .expect("notifications receiver should be available");

        server_stream
            .write_all(
                format!(
                    "{}\n{}\n",
                    json!({"method": "turn/started", "params": {"threadId": "thread-1"}}),
                    json!({"method": "turn/completed", "params": {"threadId": "thread-1"}}),
                )
                .as_bytes(),
            )
            .await
            .expect("frames should write");

        let first = notifications
            .recv()
            .await
            .expect("first notification should arrive")
            .expect("first notification should be valid");
        let second = notifications
            .recv()
            .await
            .expect("second notification should arrive")
            .expect("second notification should be valid");

        assert_eq!(first.method, "turn/started");
        assert_eq!(second.method, "turn/completed");
    }

    #[tokio::test]
    async fn rpc_transport_should_surface_transport_desync_for_unknown_response_id() {
        let (client_stream, mut server_stream) = duplex(8_192);
        let (client_read, client_write) = tokio::io::split(client_stream);
        let transport = RpcTransport::new(client_read, client_write, RpcTransportConfig::default());
        let mut notifications = transport
            .take_notifications()
            .expect("notifications receiver should be available");

        server_stream
            .write_all(
                format!(
                    "{}\n",
                    json!({
                        "id": 999,
                        "result": {
                            "ok": true,
                        },
                    }),
                )
                .as_bytes(),
            )
            .await
            .expect("desync frame should write");

        let error = notifications
            .recv()
            .await
            .expect("fatal error should be forwarded")
            .expect_err("desync should surface as an error");

        assert!(matches!(error, ProviderError::ProtocolViolation { .. }));
    }

    #[tokio::test]
    async fn rpc_transport_should_send_success_and_error_responses() {
        let (client_stream, server_stream) = duplex(8_192);
        let (client_read, client_write) = tokio::io::split(client_stream);
        let (server_read, _server_write) = tokio::io::split(server_stream);
        let transport = RpcTransport::new(client_read, client_write, RpcTransportConfig::default());

        transport
            .respond(JsonRpcId::Number(11), json!({"ok": true}))
            .await
            .expect("success response should send");
        transport
            .respond_error(
                JsonRpcId::Number(12),
                JsonRpcErrorObject {
                    code: -32001,
                    message: "approval disabled".to_owned(),
                    data: None,
                },
            )
            .await
            .expect("error response should send");

        let mut reader = BufReader::new(server_read);
        let mut first = String::new();
        let mut second = String::new();
        reader
            .read_line(&mut first)
            .await
            .expect("first line should read");
        reader
            .read_line(&mut second)
            .await
            .expect("second line should read");
        let first: Value = serde_json::from_str(first.trim()).expect("first response should parse");
        let second: Value =
            serde_json::from_str(second.trim()).expect("second response should parse");

        assert_eq!(first, json!({"jsonrpc":"2.0","id":11,"result":{"ok":true}}));
        assert_eq!(
            second,
            json!({"jsonrpc":"2.0","id":12,"error":{"code":-32001,"message":"approval disabled"}})
        );
    }
}
