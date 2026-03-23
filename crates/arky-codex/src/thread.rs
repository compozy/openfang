//! Thread and turn lifecycle helpers for the Codex app-server.

use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use arky_provider::ProviderError;
use async_trait::async_trait;
use futures::Stream;
use serde_json::{Map, Value};
use tokio::time::{Duration, timeout};
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::{
    notification::{CodexNotification, NotificationRouter},
    rpc::RpcTransport,
};

/// RPC abstraction used by the thread manager.
#[async_trait]
pub trait RpcClient: Send + Sync {
    /// Sends a JSON-RPC request and returns the raw JSON result.
    async fn request_value(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, ProviderError>;
}

#[async_trait]
impl RpcClient for RpcTransport {
    async fn request_value(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, ProviderError> {
        Self::request_value(self, method, params).await
    }
}

/// Parameters for `thread/start` and `thread/resume`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ThreadOpenParams {
    /// Optional model override.
    pub model: Option<String>,
    /// Provider-specific config overrides.
    pub config_overrides: Option<Map<String, Value>>,
}

/// Extracted result shape for `thread/start` / `thread/resume`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadStartResult {
    /// Normalized thread identifier.
    pub thread_id: String,
}

/// Parameters for `turn/start`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TurnStartParams {
    /// Scope identifier used by the notification router.
    pub scope_id: Option<String>,
    /// Rendered prompt text.
    pub prompt: Option<String>,
    /// Structured input items.
    pub input: Option<Vec<Value>>,
    /// Optional model override.
    pub model: Option<String>,
    /// Provider-specific config overrides.
    pub config_overrides: Option<Map<String, Value>>,
    /// Optional JSON schema for structured output.
    pub output_schema: Option<Value>,
    /// Optional approval policy override.
    pub approval_policy: Option<String>,
}

/// Parameters for `thread/compact/start`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompactThreadParams {
    /// Optional routing scope identifier.
    pub scope_id: Option<String>,
    /// Additional compaction payload fields.
    pub payload: Map<String, Value>,
}

/// Stream of notifications for one active turn.
pub struct TurnNotificationStream {
    thread_id: String,
    registration_id: u64,
    router: NotificationRouter,
    receiver: UnboundedReceiverStream<Result<CodexNotification, ProviderError>>,
}

impl std::fmt::Debug for TurnNotificationStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TurnNotificationStream")
            .field("thread_id", &self.thread_id)
            .finish_non_exhaustive()
    }
}

impl Stream for TurnNotificationStream {
    type Item = Result<CodexNotification, ProviderError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.receiver).poll_next(cx)
    }
}

impl Drop for TurnNotificationStream {
    fn drop(&mut self) {
        let router = self.router.clone();
        let thread_id = self.thread_id.clone();
        let registration_id = self.registration_id;
        tokio::spawn(async move {
            router
                .unregister_if_matches(&thread_id, registration_id)
                .await;
        });
    }
}

/// High-level helper for Codex thread and turn RPCs.
#[derive(Debug, Clone)]
pub struct ThreadManager<C> {
    rpc: Arc<C>,
    router: NotificationRouter,
}

impl<C> ThreadManager<C>
where
    C: RpcClient + 'static,
{
    /// Creates a thread manager backed by an RPC client and router.
    #[must_use]
    pub const fn new(rpc: Arc<C>, router: NotificationRouter) -> Self {
        Self { rpc, router }
    }

    /// Creates a new Codex thread.
    pub async fn start_thread(
        &self,
        params: ThreadOpenParams,
    ) -> Result<ThreadStartResult, ProviderError> {
        let response = self
            .rpc
            .request_value("thread/start", Some(thread_open_payload(None, params)))
            .await?;

        parse_thread_start_result(&response, "thread/start")
    }

    /// Resumes an existing Codex thread.
    pub async fn resume_thread(
        &self,
        thread_id: &str,
        params: ThreadOpenParams,
    ) -> Result<ThreadStartResult, ProviderError> {
        let response = self
            .rpc
            .request_value(
                "thread/resume",
                Some(thread_open_payload(Some(thread_id), params)),
            )
            .await?;

        let resumed = parse_thread_start_result(&response, "thread/resume")?;
        if resumed.thread_id != thread_id {
            return Err(ProviderError::protocol_violation(
                format!(
                    "stale thread routing: expected `{thread_id}` but resumed `{}`",
                    resumed.thread_id
                ),
                None,
            ));
        }

        Ok(resumed)
    }

    /// Starts a turn and returns a thread-scoped notification stream.
    pub async fn start_turn(
        &self,
        thread_id: &str,
        params: TurnStartParams,
    ) -> Result<TurnNotificationStream, ProviderError> {
        let scope_id = params
            .scope_id
            .clone()
            .unwrap_or_else(|| thread_id.to_owned());
        let (registration_id, receiver) =
            self.router.register(thread_id.to_owned(), scope_id).await;

        let payload = turn_start_payload(thread_id, params);
        if let Err(error) = self.rpc.request_value("turn/start", Some(payload)).await {
            self.router.unregister(thread_id).await;
            return Err(error);
        }

        Ok(TurnNotificationStream {
            thread_id: thread_id.to_owned(),
            registration_id,
            router: self.router.clone(),
            receiver: UnboundedReceiverStream::new(receiver),
        })
    }

    /// Requests thread compaction for an existing Codex thread.
    pub async fn compact_thread(
        &self,
        thread_id: &str,
        params: CompactThreadParams,
    ) -> Result<(), ProviderError> {
        let scope_id = params
            .scope_id
            .clone()
            .unwrap_or_else(|| thread_id.to_owned());
        let (registration_id, mut receiver) =
            self.router.register(thread_id.to_owned(), scope_id).await;

        let request_result = self
            .rpc
            .request_value(
                "thread/compact/start",
                Some(thread_compact_payload(thread_id, params)),
            )
            .await;
        if let Err(error) = request_result {
            self.router
                .unregister_if_matches(thread_id, registration_id)
                .await;
            return Err(error);
        }

        let completion = loop {
            let item = timeout(Duration::from_secs(60), receiver.recv())
                .await
                .map_err(|_| {
                    ProviderError::stream_interrupted(
                        "timed out waiting for Codex compaction to complete",
                    )
                })?
                .ok_or_else(|| {
                    ProviderError::stream_interrupted(
                        "compaction notification stream ended before completion",
                    )
                })?;
            let notification = item?;
            match canonical_notification_method(&notification.method).as_str() {
                "turn/completed" => break Ok(()),
                "turn/failed" | "error" => {
                    break Err(ProviderError::protocol_violation(
                        extract_error_message(&notification.params)
                            .unwrap_or_else(|| "Codex compaction failed".to_owned()),
                        None,
                    ));
                }
                _ => {}
            }
        };

        self.router
            .unregister_if_matches(thread_id, registration_id)
            .await;
        completion
    }
}

fn canonical_notification_method(method: &str) -> String {
    method.to_ascii_lowercase().replace('.', "/")
}

fn extract_error_message(params: &Value) -> Option<String> {
    params
        .as_object()
        .and_then(|params| params.get("message"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            params
                .as_object()
                .and_then(|params| params.get("error"))
                .and_then(Value::as_object)
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            params
                .as_object()
                .and_then(|params| params.get("turn"))
                .and_then(Value::as_object)
                .and_then(|turn| turn.get("error"))
                .and_then(Value::as_object)
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn thread_open_payload(thread_id: Option<&str>, params: ThreadOpenParams) -> Value {
    let mut payload = Map::new();
    if let Some(thread_id) = thread_id {
        payload.insert("threadId".to_owned(), Value::String(thread_id.to_owned()));
    }
    if let Some(model) = params.model {
        payload.insert("model".to_owned(), Value::String(model));
    }
    if let Some(config) = params.config_overrides {
        payload.insert("config".to_owned(), Value::Object(config));
    }

    Value::Object(payload)
}

fn turn_start_payload(thread_id: &str, params: TurnStartParams) -> Value {
    let mut payload = Map::new();
    payload.insert("threadId".to_owned(), Value::String(thread_id.to_owned()));

    if let Some(input) = params.input {
        payload.insert("input".to_owned(), Value::Array(input));
    } else if let Some(prompt) = params.prompt {
        payload.insert(
            "input".to_owned(),
            Value::Array(vec![json_value_object([
                ("type", Value::String("text".to_owned())),
                ("text", Value::String(prompt)),
            ])]),
        );
    }

    if let Some(model) = params.model {
        payload.insert("model".to_owned(), Value::String(model));
    }
    if let Some(config) = params.config_overrides {
        if let Some(effort) = config.get("model_reasoning_effort").and_then(Value::as_str) {
            payload.insert("effort".to_owned(), Value::String(effort.to_owned()));
        }
        if let Some(summary) = config
            .get("model_reasoning_summary")
            .and_then(Value::as_str)
        {
            payload.insert("summary".to_owned(), Value::String(summary.to_owned()));
        }
    }
    if let Some(output_schema) = params.output_schema {
        payload.insert("outputSchema".to_owned(), output_schema);
    }
    if let Some(approval_policy) = params.approval_policy {
        payload.insert("approvalPolicy".to_owned(), Value::String(approval_policy));
    }

    Value::Object(payload)
}

fn json_value_object<const N: usize>(entries: [(&str, Value); N]) -> Value {
    let mut object = Map::with_capacity(N);
    for (key, value) in entries {
        object.insert(key.to_owned(), value);
    }
    Value::Object(object)
}

fn thread_compact_payload(thread_id: &str, params: CompactThreadParams) -> Value {
    let mut payload = params.payload;
    payload.insert("threadId".to_owned(), Value::String(thread_id.to_owned()));
    if let Some(scope_id) = params.scope_id {
        payload.insert("scopeId".to_owned(), Value::String(scope_id));
    }

    Value::Object(payload)
}

fn parse_thread_start_result(
    value: &Value,
    method: &str,
) -> Result<ThreadStartResult, ProviderError> {
    let object = value.as_object().ok_or_else(|| {
        ProviderError::protocol_violation(format!("{method} returned a non-object result"), None)
    })?;

    let nested_thread = object.get("thread").and_then(Value::as_object);
    let thread_id = nested_thread
        .and_then(|thread| thread.get("id"))
        .and_then(Value::as_str)
        .or_else(|| object.get("threadId").and_then(Value::as_str))
        .or_else(|| object.get("thread_id").and_then(Value::as_str))
        .or_else(|| object.get("id").and_then(Value::as_str))
        .ok_or_else(|| {
            ProviderError::protocol_violation(
                format!("{method} returned no thread identifier"),
                None,
            )
        })?;

    Ok(ThreadStartResult {
        thread_id: thread_id.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arky_provider::ProviderError;
    use async_trait::async_trait;
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};
    use tokio::sync::Mutex;

    use super::{CompactThreadParams, RpcClient, ThreadManager, ThreadOpenParams, TurnStartParams};
    use crate::{
        JsonRpcId,
        notification::{CodexNotification, NotificationRouter},
    };

    #[derive(Debug, Default)]
    struct MockRpcClient {
        calls: Mutex<Vec<(String, Value)>>,
        next_response: Mutex<Value>,
    }

    #[async_trait]
    impl RpcClient for MockRpcClient {
        async fn request_value(
            &self,
            method: &str,
            params: Option<Value>,
        ) -> Result<Value, ProviderError> {
            self.calls
                .lock()
                .await
                .push((method.to_owned(), params.clone().unwrap_or(Value::Null)));
            Ok(self.next_response.lock().await.clone())
        }
    }

    #[tokio::test]
    async fn thread_manager_should_forward_start_and_turn_payloads() {
        let rpc = Arc::new(MockRpcClient {
            calls: Mutex::new(Vec::new()),
            next_response: Mutex::new(json!({
                "thread": {
                    "id": "thread-1",
                },
            })),
        });
        let router = NotificationRouter::new();
        let manager = ThreadManager::new(rpc.clone(), router.clone());

        let started = manager
            .start_thread(ThreadOpenParams {
                model: Some("gpt-5".to_owned()),
                config_overrides: Some(Map::from_iter([(
                    "developer_instructions".to_owned(),
                    Value::String("Be terse".to_owned()),
                )])),
            })
            .await
            .expect("thread should start");
        assert_eq!(started.thread_id, "thread-1");

        let mut stream = manager
            .start_turn(
                "thread-1",
                TurnStartParams {
                    scope_id: Some("scope-1".to_owned()),
                    prompt: Some("hello".to_owned()),
                    model: Some("gpt-5".to_owned()),
                    config_overrides: Some(Map::from_iter([
                        (
                            "model_reasoning_effort".to_owned(),
                            Value::String("high".to_owned()),
                        ),
                        (
                            "model_reasoning_summary".to_owned(),
                            Value::String("none".to_owned()),
                        ),
                    ])),
                    ..TurnStartParams::default()
                },
            )
            .await
            .expect("turn should start");

        router
            .dispatch(CodexNotification {
                method: "turn/started".to_owned(),
                params: json!({
                    "threadId": "thread-1",
                }),
            })
            .await
            .expect("notification should route");
        let first = futures::StreamExt::next(&mut stream)
            .await
            .expect("notification should arrive")
            .expect("notification should be valid");
        assert_eq!(first.method, "turn/started");

        let calls = rpc.calls.lock().await.clone();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "thread/start");
        assert_eq!(calls[1].0, "turn/start");
        assert_eq!(calls[1].1["effort"], "high");
        assert_eq!(calls[1].1["summary"], "none");
        assert_eq!(calls[1].1["threadId"], "thread-1");
    }

    #[tokio::test]
    async fn thread_manager_should_reject_resume_with_wrong_thread_id() {
        let rpc = Arc::new(MockRpcClient {
            calls: Mutex::new(Vec::new()),
            next_response: Mutex::new(json!({
                "thread": {
                    "id": "thread-2",
                },
            })),
        });
        let manager = ThreadManager::new(rpc, NotificationRouter::new());

        let error = manager
            .resume_thread("thread-1", ThreadOpenParams::default())
            .await
            .expect_err("resume mismatch should fail");

        assert!(matches!(error, ProviderError::ProtocolViolation { .. }));
    }

    #[tokio::test]
    async fn thread_manager_should_forward_compaction_payloads() {
        let rpc = Arc::new(MockRpcClient {
            calls: Mutex::new(Vec::new()),
            next_response: Mutex::new(json!({ "ok": true })),
        });
        let router = NotificationRouter::new();
        let manager = ThreadManager::new(rpc.clone(), router.clone());

        let compaction = tokio::spawn(async move {
            manager
                .compact_thread(
                    "thread-1",
                    CompactThreadParams {
                        scope_id: Some("scope-1".to_owned()),
                        payload: Map::from_iter([
                            ("tokenThreshold".to_owned(), json!(8_192)),
                            ("prompt".to_owned(), json!("compact now")),
                        ]),
                    },
                )
                .await
        });
        tokio::task::yield_now().await;
        router
            .dispatch(CodexNotification {
                method: "turn/completed".to_owned(),
                params: json!({
                    "threadId": "thread-1",
                }),
            })
            .await
            .expect("compaction completion should route");
        compaction
            .await
            .expect("compaction task should finish")
            .expect("compaction should succeed");

        let calls = rpc.calls.lock().await.clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "thread/compact/start");
        assert_eq!(calls[0].1["threadId"], "thread-1");
        assert_eq!(calls[0].1["scopeId"], "scope-1");
        assert_eq!(calls[0].1["tokenThreshold"], 8_192);
        assert_eq!(calls[0].1["prompt"], "compact now");
    }

    #[test]
    fn json_rpc_id_display_should_be_stable() {
        assert_eq!(JsonRpcId::Number(7).to_string(), "7");
    }
}
