//! Thread-scoped notification routing for Codex JSON-RPC streams.

use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use arky_provider::ProviderError;
use serde_json::{Map, Value};
use tokio::sync::{mpsc, Mutex};

/// Raw JSON-RPC notification emitted by the Codex app-server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexNotification {
    /// Notification method name.
    pub method: String,
    /// Notification parameters.
    pub params: Value,
}

type NotificationItem = Result<CodexNotification, ProviderError>;
type NotificationSender = mpsc::UnboundedSender<NotificationItem>;
type NotificationReceiver = mpsc::UnboundedReceiver<NotificationItem>;

#[derive(Debug)]
struct Registration {
    id: u64,
    scope_id: String,
    sender: NotificationSender,
}

#[derive(Debug, Default)]
struct RouterState {
    threads: HashMap<String, Registration>,
    scopes: HashMap<String, BTreeSet<String>>,
}

/// Routes Codex notifications to the correct active thread stream.
#[derive(Debug, Clone, Default)]
pub struct NotificationRouter {
    state: Arc<Mutex<RouterState>>,
    next_registration_id: Arc<AtomicU64>,
}

impl NotificationRouter {
    /// Creates an empty notification router.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a thread and returns a receiver for its notifications.
    pub async fn register(
        &self,
        thread_id: impl Into<String>,
        scope_id: impl Into<String>,
    ) -> (u64, NotificationReceiver) {
        let thread_id = thread_id.into();
        let scope_id = scope_id.into();
        let registration_id = self
            .next_registration_id
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut state = self.state.lock().await;

        if let Some(previous) = state.threads.insert(
            thread_id.clone(),
            Registration {
                id: registration_id,
                scope_id: scope_id.clone(),
                sender,
            },
        ) {
            remove_scope_thread(&mut state.scopes, &previous.scope_id, &thread_id);
        }

        state.scopes.entry(scope_id).or_default().insert(thread_id);
        drop(state);

        (registration_id, receiver)
    }

    /// Removes all routing state for a thread.
    pub async fn unregister(&self, thread_id: &str) {
        let mut state = self.state.lock().await;
        if let Some(previous) = state.threads.remove(thread_id) {
            remove_scope_thread(&mut state.scopes, &previous.scope_id, thread_id);
        }
    }

    /// Removes routing state only when the active registration still matches.
    pub async fn unregister_if_matches(&self, thread_id: &str, registration_id: u64) {
        let mut state = self.state.lock().await;
        let should_remove = state
            .threads
            .get(thread_id)
            .is_some_and(|registration| registration.id == registration_id);
        if should_remove {
            if let Some(previous) = state.threads.remove(thread_id) {
                remove_scope_thread(&mut state.scopes, &previous.scope_id, thread_id);
            }
        }
    }

    /// Dispatches one notification using thread, scope, or global fanout rules.
    pub async fn dispatch(&self, notification: CodexNotification) -> Result<(), ProviderError> {
        if notification.method.starts_with("account/") {
            return self.dispatch_global(notification).await;
        }

        if let Some(thread_id) = extract_thread_id_from_value(&notification.params) {
            return self.dispatch_to_thread(&thread_id, notification).await;
        }

        if let Some(scope_id) = extract_scope_id_from_value(&notification.params) {
            return self.dispatch_to_scope(&scope_id, notification).await;
        }

        Ok(())
    }

    /// Fanouts a notification to every thread registered under a scope.
    pub async fn dispatch_to_scope(
        &self,
        scope_id: &str,
        notification: CodexNotification,
    ) -> Result<(), ProviderError> {
        let (thread_ids, senders) = {
            let state = self.state.lock().await;
            let thread_ids = state.scopes.get(scope_id).cloned().ok_or_else(|| {
                ProviderError::protocol_violation(
                    format!("stale thread routing for scope `{scope_id}`"),
                    None,
                )
            })?;
            let mut senders = Vec::with_capacity(thread_ids.len());
            for thread_id in &thread_ids {
                if let Some(registration) = state.threads.get(thread_id) {
                    senders.push((thread_id.clone(), registration.sender.clone()));
                }
            }
            (thread_ids, senders)
        };

        if senders.is_empty() {
            return Err(ProviderError::protocol_violation(
                format!("stale thread routing for scope `{scope_id}`"),
                None,
            ));
        }

        let mut dropped = Vec::new();
        for (thread_id, sender) in senders {
            if sender.send(Ok(notification.clone())).is_err() {
                dropped.push(thread_id);
            }
        }

        if dropped.is_empty() && !thread_ids.is_empty() {
            return Ok(());
        }

        self.prune_dropped(&dropped).await;
        Err(ProviderError::stream_interrupted(
            "notification stream dropped while dispatching scoped notification",
        ))
    }

    /// Fanouts a notification to every registered thread.
    pub async fn dispatch_global(
        &self,
        notification: CodexNotification,
    ) -> Result<(), ProviderError> {
        let senders = {
            let state = self.state.lock().await;
            state
                .threads
                .iter()
                .map(|(thread_id, registration)| (thread_id.clone(), registration.sender.clone()))
                .collect::<Vec<_>>()
        };

        let mut dropped = Vec::new();
        for (thread_id, sender) in senders {
            if sender.send(Ok(notification.clone())).is_err() {
                dropped.push(thread_id);
            }
        }

        if dropped.is_empty() {
            return Ok(());
        }

        self.prune_dropped(&dropped).await;
        Err(ProviderError::stream_interrupted(
            "notification stream dropped while dispatching global notification",
        ))
    }

    /// Sends the same error to every active thread stream and clears routing.
    pub async fn error_all(&self, error: ProviderError) {
        let senders = {
            let mut state = self.state.lock().await;
            let senders = state
                .threads
                .drain()
                .map(|(_, registration)| registration.sender)
                .collect::<Vec<_>>();
            state.scopes.clear();
            senders
        };

        for sender in senders {
            let _ = sender.send(Err(error.clone()));
        }
    }

    async fn dispatch_to_thread(
        &self,
        thread_id: &str,
        notification: CodexNotification,
    ) -> Result<(), ProviderError> {
        let sender = {
            let state = self.state.lock().await;
            state
                .threads
                .get(thread_id)
                .map(|registration| registration.sender.clone())
        };

        let Some(sender) = sender else {
            return Ok(());
        };

        if sender.send(Ok(notification)).is_ok() {
            return Ok(());
        }

        self.unregister(thread_id).await;
        Err(ProviderError::stream_interrupted(
            "notification stream dropped while dispatching thread notification",
        ))
    }

    async fn prune_dropped(&self, dropped: &[String]) {
        if dropped.is_empty() {
            return;
        }

        let mut state = self.state.lock().await;
        for thread_id in dropped {
            if let Some(previous) = state.threads.remove(thread_id) {
                remove_scope_thread(&mut state.scopes, &previous.scope_id, thread_id);
            }
        }
    }
}

fn remove_scope_thread(
    scopes: &mut HashMap<String, BTreeSet<String>>,
    scope_id: &str,
    thread_id: &str,
) {
    if let Some(entries) = scopes.get_mut(scope_id) {
        entries.remove(thread_id);
        if entries.is_empty() {
            scopes.remove(scope_id);
        }
    }
}

fn object_field<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    object.get(key)
}

fn non_empty_string(value: &Value) -> Option<String> {
    value.as_str().and_then(|value| {
        if value.is_empty() {
            None
        } else {
            Some(value.to_owned())
        }
    })
}

fn extract_thread_id_from_object(object: &Map<String, Value>) -> Option<String> {
    for key in [
        "threadId",
        "thread_id",
        "sessionId",
        "session_id",
        "conversationId",
        "conversation_id",
        "id",
    ] {
        if let Some(value) = object_field(object, key).and_then(non_empty_string) {
            return Some(value);
        }
    }

    let nested_thread = object_field(object, "thread")?.as_object()?;
    for key in ["id", "threadId", "thread_id"] {
        if let Some(value) = object_field(nested_thread, key).and_then(non_empty_string) {
            return Some(value);
        }
    }

    None
}

fn extract_scope_id_from_object(object: &Map<String, Value>) -> Option<String> {
    for key in ["scopeId", "scope_id"] {
        if let Some(value) = object_field(object, key).and_then(non_empty_string) {
            return Some(value);
        }
    }

    None
}

/// Extracts a thread identifier from arbitrary notification JSON.
#[must_use]
pub fn extract_thread_id_from_value(value: &Value) -> Option<String> {
    value.as_object().and_then(extract_thread_id_from_object)
}

/// Extracts a scope identifier from arbitrary notification JSON.
#[must_use]
pub fn extract_scope_id_from_value(value: &Value) -> Option<String> {
    value.as_object().and_then(extract_scope_id_from_object)
}

#[cfg(test)]
mod tests {
    use arky_provider::ProviderError;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{CodexNotification, NotificationRouter};

    #[tokio::test]
    async fn notification_router_should_route_by_thread_id() {
        let router = NotificationRouter::new();
        let (_, mut receiver) = router.register("thread-1", "scope-1").await;

        router
            .dispatch(CodexNotification {
                method: "turn/started".to_owned(),
                params: json!({
                    "threadId": "thread-1",
                }),
            })
            .await
            .expect("dispatch should succeed");

        let delivered = receiver
            .recv()
            .await
            .expect("thread notification should arrive")
            .expect("thread notification should be valid");
        assert_eq!(delivered.method, "turn/started");
    }

    #[tokio::test]
    async fn notification_router_should_route_by_scope_id() {
        let router = NotificationRouter::new();
        let (_, mut receiver) = router.register("thread-1", "scope-1").await;

        router
            .dispatch(CodexNotification {
                method: "turn/updated".to_owned(),
                params: json!({
                    "scopeId": "scope-1",
                }),
            })
            .await
            .expect("scope dispatch should succeed");

        let delivered = receiver
            .recv()
            .await
            .expect("scope notification should arrive")
            .expect("scope notification should be valid");
        assert_eq!(delivered.method, "turn/updated");
    }

    #[tokio::test]
    async fn notification_router_should_treat_conversation_id_as_thread_id() {
        let router = NotificationRouter::new();
        let (_, mut receiver) = router.register("thread-1", "scope-1").await;

        router
            .dispatch(CodexNotification {
                method: "codex/event/mcp_startup_update".to_owned(),
                params: json!({
                    "conversationId": "thread-1",
                    "msg": {
                        "type": "mcp_startup_update",
                        "server": "pal",
                        "status": {
                            "state": "ready",
                        },
                    },
                }),
            })
            .await
            .expect("conversation-scoped notification should route to the thread");

        let delivered = receiver
            .recv()
            .await
            .expect("conversation notification should arrive")
            .expect("conversation notification should be valid");
        assert_eq!(delivered.method, "codex/event/mcp_startup_update");
    }

    #[tokio::test]
    async fn notification_router_should_report_stale_scope_routing() {
        let router = NotificationRouter::new();

        let error = router
            .dispatch_to_scope(
                "scope-missing",
                CodexNotification {
                    method: "turn/updated".to_owned(),
                    params: json!({
                        "scopeId": "scope-missing",
                    }),
                },
            )
            .await
            .expect_err("missing scope should be rejected");

        assert!(matches!(error, ProviderError::ProtocolViolation { .. }));
    }

    #[tokio::test]
    async fn notification_router_should_fan_out_account_notifications() {
        let router = NotificationRouter::new();
        let (_, mut first) = router.register("thread-a", "scope-a").await;
        let (_, mut second) = router.register("thread-b", "scope-b").await;

        router
            .dispatch(CodexNotification {
                method: "account/updated".to_owned(),
                params: json!({
                    "creditsRemaining": 42,
                }),
            })
            .await
            .expect("global dispatch should succeed");

        let first = first
            .recv()
            .await
            .expect("first notification should arrive")
            .expect("first notification should be valid");
        let second = second
            .recv()
            .await
            .expect("second notification should arrive")
            .expect("second notification should be valid");
        assert_eq!(first.method, "account/updated");
        assert_eq!(second.method, "account/updated");
    }

    #[tokio::test]
    async fn notification_router_should_ignore_stale_unregisters() {
        let router = NotificationRouter::new();
        let (first_registration, _first) = router.register("thread-1", "scope-1").await;
        let (second_registration, mut second) = router.register("thread-1", "scope-1").await;

        router
            .unregister_if_matches("thread-1", first_registration)
            .await;
        router
            .dispatch(CodexNotification {
                method: "turn/started".to_owned(),
                params: json!({
                    "threadId": "thread-1",
                }),
            })
            .await
            .expect("dispatch after stale unregister should succeed");

        let delivered = second
            .recv()
            .await
            .expect("replacement registration should still receive notifications")
            .expect("notification should be valid");
        assert_eq!(delivered.method, "turn/started");

        router
            .unregister_if_matches("thread-1", second_registration)
            .await;
        let error = router
            .dispatch_to_thread(
                "thread-1",
                CodexNotification {
                    method: "turn/completed".to_owned(),
                    params: json!({
                        "threadId": "thread-1",
                    }),
                },
            )
            .await;
        assert_eq!(error.is_ok(), true);
    }
}
