//! Provider session identifier passthrough and reuse.

use std::{collections::HashMap, sync::Arc};

use arky_protocol::{SessionId, SessionRef};
use tokio::sync::Mutex;

/// Tracks provider-native Claude session identifiers per Arky session.
#[derive(Debug, Clone, Default)]
pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<String, String>>>,
}

impl SessionManager {
    /// Creates an empty session manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves the provider-native session identifier for the next turn.
    pub async fn resolve(&self, session: &SessionRef) -> Option<String> {
        if let Some(provider_session_id) = &session.provider_session_id {
            return Some(provider_session_id.clone());
        }

        let Some(session_id) = &session.id else {
            return None;
        };

        self.sessions
            .lock()
            .await
            .get(&session_id.to_string())
            .cloned()
    }

    /// Stores the provider-native session identifier associated with an Arky session.
    pub async fn record(&self, session_id: &SessionId, provider_session_id: impl Into<String>) {
        self.sessions
            .lock()
            .await
            .insert(session_id.to_string(), provider_session_id.into());
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::SessionManager;
    use arky_protocol::{SessionId, SessionRef};

    #[tokio::test]
    async fn session_manager_should_prefer_request_session_and_then_cached_value() {
        let manager = SessionManager::new();
        let session_id = SessionId::new();
        manager.record(&session_id, "cached-session").await;

        let cached = manager.resolve(&SessionRef::new(Some(session_id))).await;
        assert_eq!(cached, Some("cached-session".to_owned()));

        let explicit = manager
            .resolve(&SessionRef::new(None).with_provider_session_id("request-session"))
            .await;
        assert_eq!(explicit, Some("request-session".to_owned()));
    }

    #[tokio::test]
    async fn session_manager_should_not_reuse_provider_sessions_across_distinct_arky_sessions() {
        let manager = SessionManager::new();
        let first_session = SessionId::new();
        let second_session = SessionId::new();

        manager.record(&first_session, "provider-session-1").await;

        let resolved = manager
            .resolve(&SessionRef::new(Some(second_session)))
            .await;

        assert_eq!(resolved, None);
    }
}
