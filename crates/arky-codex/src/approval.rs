//! Approval-flow handling for server-initiated Codex requests.

use std::{collections::HashMap, sync::Arc, time::Duration};

use arky_provider::ProviderError;
use serde_json::{json, Value};
use tokio::{
    sync::{oneshot, Mutex},
    time::timeout,
};

use crate::rpc::{CodexServerRequest, JsonRpcErrorObject, JsonRpcId, RpcTransport};

/// One approval decision supported by the Codex request flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalDecision {
    /// Accept the request for this turn only.
    Accept,
    /// Accept the request for the session.
    AcceptForSession,
    /// Decline the request.
    Decline,
    /// Cancel the request entirely.
    Cancel,
    /// Return a subset of granted permissions.
    GrantPermissions {
        /// Whether the grant should persist for the session.
        scope: Option<String>,
        /// Granted permissions payload.
        permissions: Value,
    },
}

/// Approval behavior used by the provider runtime.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ApprovalMode {
    /// Resolve every request automatically with an accept decision.
    #[default]
    AutoApprove,
    /// Resolve every request automatically with a decline decision.
    AutoDeny,
    /// Wait for an explicit decision from the host with a bounded timeout.
    Manual {
        /// Maximum wait time before failing the request.
        timeout: Duration,
    },
}

/// Normalized approval request surfaced to the handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    /// JSON-RPC identifier of the server request.
    pub id: JsonRpcId,
    /// Request method name.
    pub method: String,
    /// Original parameters.
    pub params: Value,
}

/// Handles server-initiated approval requests.
#[derive(Debug, Clone)]
pub struct ApprovalHandler {
    mode: ApprovalMode,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<ApprovalDecision>>>>,
}

impl ApprovalHandler {
    /// Creates a new approval handler.
    #[must_use]
    pub fn new(mode: ApprovalMode) -> Self {
        Self {
            mode,
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Waits for or synthesizes a decision for the given request.
    pub async fn decide(
        &self,
        request: &ApprovalRequest,
    ) -> Result<ApprovalDecision, ProviderError> {
        match &self.mode {
            ApprovalMode::AutoApprove => Ok(default_accept_decision(&request.method)),
            ApprovalMode::AutoDeny => Ok(default_deny_decision(&request.method)),
            ApprovalMode::Manual {
                timeout: timeout_ms,
            } => {
                let (tx, rx) = oneshot::channel();
                let key = request.id.to_string();
                self.pending.lock().await.insert(key.clone(), tx);

                let decision = timeout(*timeout_ms, rx).await;
                self.pending.lock().await.remove(&key);
                let decision = decision
                    .map_err(|_| {
                        ProviderError::stream_interrupted(format!(
                            "approval timed out for `{}`",
                            request.method
                        ))
                    })?
                    .map_err(|_| {
                        ProviderError::stream_interrupted(format!(
                            "approval channel closed for `{}`",
                            request.method
                        ))
                    })?;
                Ok(decision)
            }
        }
    }

    /// Resolves one pending manual approval.
    pub async fn resolve(
        &self,
        id: JsonRpcId,
        decision: ApprovalDecision,
    ) -> Result<(), ProviderError> {
        let key = id.to_string();
        let sender = self.pending.lock().await.remove(&key).ok_or_else(|| {
            ProviderError::protocol_violation(
                format!("no pending approval request with id `{key}`"),
                None,
            )
        })?;

        sender.send(decision).map_err(|_| {
            ProviderError::stream_interrupted("approval requester dropped before resolution")
        })
    }

    /// Handles one server request by responding over the JSON-RPC transport.
    pub async fn handle(
        &self,
        request: CodexServerRequest,
        transport: &RpcTransport,
    ) -> Result<(), ProviderError> {
        let request_id = request.id.clone();
        let approval_request = ApprovalRequest {
            id: request_id.clone(),
            method: request.method.clone(),
            params: request.params.clone(),
        };

        if !is_known_approval_method(&approval_request.method) {
            let error = JsonRpcErrorObject {
                code: -32601,
                message: format!(
                    "Unsupported server request method: {}",
                    approval_request.method
                ),
                data: None,
            };
            transport.respond_error(request_id, error.clone()).await?;
            return Err(ProviderError::protocol_violation(error.message, None));
        }

        let decision = match self.decide(&approval_request).await {
            Ok(decision) => decision,
            Err(error) => {
                let response = approval_rejection(format!("{error}"));
                transport
                    .respond_error(request_id, response.clone())
                    .await?;
                return Err(error);
            }
        };

        match decision_response(decision) {
            Ok(response) => {
                transport.respond(request_id, response).await?;
                Ok(())
            }
            Err(error) => {
                transport.respond_error(request_id, error.clone()).await?;
                Err(ProviderError::protocol_violation(error.message, None))
            }
        }
    }
}

fn is_known_approval_method(method: &str) -> bool {
    matches!(
        method,
        "item/commandExecution/requestApproval"
            | "item/command_execution/requestApproval"
            | "item/fileChange/requestApproval"
            | "item/file_change/requestApproval"
            | "item/permissions/requestApproval"
            | "item/permissions/request_approval"
    )
}

fn default_accept_decision(method: &str) -> ApprovalDecision {
    if method.starts_with("item/permissions/") {
        return ApprovalDecision::GrantPermissions {
            scope: Some("turn".to_owned()),
            permissions: json!({}),
        };
    }

    ApprovalDecision::Accept
}

const fn default_deny_decision(method: &str) -> ApprovalDecision {
    let _ = method;
    ApprovalDecision::Decline
}

fn decision_response(decision: ApprovalDecision) -> Result<Value, JsonRpcErrorObject> {
    match decision {
        ApprovalDecision::Accept => Ok(approval_response(false)),
        ApprovalDecision::AcceptForSession => Ok(approval_response(true)),
        ApprovalDecision::GrantPermissions { scope, .. } => Ok(approval_response(matches!(
            scope.as_deref(),
            Some("session")
        ))),
        ApprovalDecision::Decline => Err(approval_rejection(
            "Approval request requires explicit opt-in".to_owned(),
        )),
        ApprovalDecision::Cancel => Err(approval_rejection(
            "Approval request was cancelled".to_owned(),
        )),
    }
}

fn approval_response(for_session: bool) -> Value {
    json!({
        "outcome": "approved",
        "decision": "accept",
        "acceptSettings": {
            "forSession": for_session,
        },
    })
}

const fn approval_rejection(message: String) -> JsonRpcErrorObject {
    JsonRpcErrorObject {
        code: -32001,
        message,
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use arky_provider::ProviderError;
    use pretty_assertions::assert_eq;
    use serde_json::{json, Value};
    use tokio::{
        io::{duplex, AsyncBufReadExt, BufReader},
        time::Duration,
    };

    use super::{
        approval_response, ApprovalDecision, ApprovalHandler, ApprovalMode, ApprovalRequest,
    };
    use crate::{JsonRpcId, RpcTransport, RpcTransportConfig};

    #[tokio::test]
    async fn approval_handler_should_auto_approve_known_requests() {
        let handler = ApprovalHandler::new(ApprovalMode::AutoApprove);
        let decision = handler
            .decide(&ApprovalRequest {
                id: JsonRpcId::Number(1),
                method: "item/commandExecution/requestApproval".to_owned(),
                params: json!({}),
            })
            .await
            .expect("decision should succeed");

        assert_eq!(decision, ApprovalDecision::Accept);
    }

    #[tokio::test]
    async fn approval_handler_should_allow_manual_resolution() {
        let handler = ApprovalHandler::new(ApprovalMode::Manual {
            timeout: Duration::from_secs(1),
        });
        let request = ApprovalRequest {
            id: JsonRpcId::Number(2),
            method: "item/fileChange/requestApproval".to_owned(),
            params: json!({}),
        };

        let pending = {
            let handler = handler.clone();
            tokio::spawn(async move { handler.decide(&request).await })
        };

        tokio::task::yield_now().await;
        handler
            .resolve(JsonRpcId::Number(2), ApprovalDecision::Decline)
            .await
            .expect("manual resolution should succeed");
        let decision = pending
            .await
            .expect("task should finish")
            .expect("decision should resolve");

        assert_eq!(decision, ApprovalDecision::Decline);
    }

    #[tokio::test]
    async fn approval_handler_should_timeout_manual_requests() {
        let handler = ApprovalHandler::new(ApprovalMode::Manual {
            timeout: Duration::from_millis(20),
        });
        let request_id = JsonRpcId::Number(3);

        let error = handler
            .decide(&ApprovalRequest {
                id: request_id.clone(),
                method: "item/commandExecution/requestApproval".to_owned(),
                params: json!({}),
            })
            .await
            .expect_err("manual approval should time out");

        assert!(matches!(error, ProviderError::StreamInterrupted { .. }));
        assert!(matches!(
            handler.resolve(request_id, ApprovalDecision::Accept).await,
            Err(ProviderError::ProtocolViolation { .. })
        ));
    }

    #[tokio::test]
    async fn approval_handler_should_respond_with_codex_wire_shape() {
        let handler = ApprovalHandler::new(ApprovalMode::AutoApprove);
        let (client_stream, server_stream) = duplex(8_192);
        let (client_read, client_write) = tokio::io::split(client_stream);
        let (server_read, _server_write) = tokio::io::split(server_stream);
        let transport = RpcTransport::new(client_read, client_write, RpcTransportConfig::default());

        handler
            .handle(
                crate::CodexServerRequest {
                    id: JsonRpcId::Number(11),
                    method: "item/commandExecution/requestApproval".to_owned(),
                    params: json!({}),
                },
                &transport,
            )
            .await
            .expect("approval should succeed");

        let mut reader = BufReader::new(server_read);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .expect("response should be readable");
        let response: Value =
            serde_json::from_str(line.trim()).expect("response should be valid json");

        assert_eq!(
            response,
            json!({
                "jsonrpc": "2.0",
                "id": 11,
                "result": approval_response(false),
            })
        );
    }

    #[tokio::test]
    async fn approval_handler_should_reject_auto_deny_requests_with_rpc_error() {
        let handler = ApprovalHandler::new(ApprovalMode::AutoDeny);
        let (client_stream, server_stream) = duplex(8_192);
        let (client_read, client_write) = tokio::io::split(client_stream);
        let (server_read, _server_write) = tokio::io::split(server_stream);
        let transport = RpcTransport::new(client_read, client_write, RpcTransportConfig::default());

        let error = handler
            .handle(
                crate::CodexServerRequest {
                    id: JsonRpcId::Number(12),
                    method: "item/fileChange/requestApproval".to_owned(),
                    params: json!({}),
                },
                &transport,
            )
            .await
            .expect_err("auto deny should reject approval");
        assert!(matches!(error, ProviderError::ProtocolViolation { .. }));

        let mut reader = BufReader::new(server_read);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .expect("error response should be readable");
        let response: Value =
            serde_json::from_str(line.trim()).expect("error response should be valid json");

        assert_eq!(
            response,
            json!({
                "jsonrpc": "2.0",
                "id": 12,
                "error": {
                    "code": -32001,
                    "message": "Approval request requires explicit opt-in",
                },
            })
        );
    }
}
