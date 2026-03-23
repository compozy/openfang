//! In-memory session store implementation.

use std::collections::{BTreeMap, VecDeque};

use arky_protocol::{Message, PersistedEvent, ReplayCursor, SessionId, TurnCheckpoint};
use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::{
    support::{now_ms, validate_event_batch},
    NewSession, SessionError, SessionFilter, SessionMetadata, SessionSnapshot, SessionStore,
};

/// Configuration for [`InMemorySessionStore`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InMemorySessionStoreConfig {
    /// Whether replay events should be persisted in memory.
    pub persist_replay: bool,
    /// Optional idle time-to-live in milliseconds.
    pub idle_ttl_ms: Option<u64>,
    /// Maximum number of sessions to retain before evicting the oldest entry.
    pub max_sessions: Option<usize>,
}

impl Default for InMemorySessionStoreConfig {
    fn default() -> Self {
        Self {
            persist_replay: true,
            idle_ttl_ms: None,
            max_sessions: None,
        }
    }
}

/// In-memory session store for zero-configuration and test usage.
#[derive(Debug, Default)]
pub struct InMemorySessionStore {
    config: InMemorySessionStoreConfig,
    state: RwLock<InMemoryState>,
}

#[derive(Debug, Default)]
struct InMemoryState {
    sessions: BTreeMap<SessionId, SessionEntry>,
    order: VecDeque<SessionId>,
}

#[derive(Debug, Clone)]
struct SessionEntry {
    metadata: SessionMetadata,
    messages: Vec<Message>,
    events: Vec<PersistedEvent>,
    last_checkpoint: Option<TurnCheckpoint>,
    last_accessed_ms: u64,
}

impl InMemorySessionStore {
    /// Creates a store with explicit configuration.
    #[must_use]
    pub fn new(config: InMemorySessionStoreConfig) -> Self {
        Self {
            config,
            state: RwLock::new(InMemoryState::default()),
        }
    }
}

impl SessionEntry {
    fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            metadata: self.metadata.clone(),
            messages: self.messages.clone(),
            last_checkpoint: self.last_checkpoint.clone(),
            replay_cursor: replay_cursor_for(
                self.metadata.replay_available,
                &self.events,
                self.last_checkpoint.as_ref(),
            ),
        }
    }
}

impl InMemoryState {
    fn touch(&mut self, session_id: &SessionId, now_ms: u64) {
        if let Some(entry) = self.sessions.get_mut(session_id) {
            entry.last_accessed_ms = now_ms;
        }
        if let Some(position) = self.order.iter().position(|current| current == session_id) {
            let _ = self.order.remove(position);
        }
        self.order.push_back(session_id.clone());
    }

    fn remove(&mut self, session_id: &SessionId) -> Option<SessionEntry> {
        self.order.retain(|current| current != session_id);
        self.sessions.remove(session_id)
    }

    fn evict_over_capacity(&mut self, max_sessions: Option<usize>) {
        let Some(limit) = max_sessions else {
            return;
        };

        while self.sessions.len() > limit {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.sessions.remove(&oldest);
        }
    }

    fn evict_expired(&mut self, now_ms: u64, idle_ttl_ms: Option<u64>) {
        let expired_ids = self
            .sessions
            .iter()
            .filter(|(_, entry)| {
                entry.metadata.is_expired_at(now_ms)
                    || matches!(
                        idle_ttl_ms,
                        Some(idle_ttl_ms)
                            if entry.last_accessed_ms.saturating_add(idle_ttl_ms) <= now_ms
                    )
            })
            .map(|(session_id, _)| session_id.clone())
            .collect::<Vec<_>>();

        for session_id in expired_ids {
            let _ = self.remove(&session_id);
        }
    }
}

#[async_trait]
impl SessionStore for InMemorySessionStore {
    async fn create(&self, new_session: NewSession) -> Result<SessionId, SessionError> {
        let session_id = SessionId::new();
        let metadata = SessionMetadata::from_new_session(
            session_id.clone(),
            new_session,
            now_ms(),
            self.config.persist_replay,
        );
        let entry = SessionEntry {
            metadata,
            messages: Vec::new(),
            events: Vec::new(),
            last_checkpoint: None,
            last_accessed_ms: now_ms(),
        };

        let mut state = self.state.write().await;
        state.evict_expired(now_ms(), self.config.idle_ttl_ms);
        state.sessions.insert(session_id.clone(), entry);
        state.touch(&session_id, now_ms());
        state.evict_over_capacity(self.config.max_sessions);
        drop(state);
        Ok(session_id)
    }

    async fn load(&self, id: &SessionId) -> Result<SessionSnapshot, SessionError> {
        let mut state = self.state.write().await;
        let now_ms = now_ms();
        state.evict_expired(now_ms, self.config.idle_ttl_ms);
        let Some(entry) = state.sessions.get(id).cloned() else {
            return Err(SessionError::NotFound {
                session_id: id.clone(),
            });
        };

        state.touch(id, now_ms);
        drop(state);
        Ok(entry.snapshot())
    }

    async fn append_messages(
        &self,
        id: &SessionId,
        messages: &[Message],
    ) -> Result<(), SessionError> {
        if messages.is_empty() {
            return Ok(());
        }

        let mut state = self.state.write().await;
        let now_ms = now_ms();
        state.evict_expired(now_ms, self.config.idle_ttl_ms);
        let Some(expires_at_ms) = state
            .sessions
            .get(id)
            .map(|entry| entry.metadata.expires_at_ms)
        else {
            return Err(SessionError::NotFound {
                session_id: id.clone(),
            });
        };
        if matches!(expires_at_ms, Some(value) if value <= now_ms) {
            let _ = state.remove(id);
            return Err(SessionError::expired(id.clone(), expires_at_ms));
        }
        let Some(entry) = state.sessions.get_mut(id) else {
            return Err(SessionError::NotFound {
                session_id: id.clone(),
            });
        };

        entry.messages.extend_from_slice(messages);
        entry.metadata.message_count = entry.messages.len();
        entry.metadata.updated_at_ms = now_ms;
        state.touch(id, now_ms);
        drop(state);
        Ok(())
    }

    async fn append_events(
        &self,
        id: &SessionId,
        events: &[PersistedEvent],
    ) -> Result<(), SessionError> {
        if events.is_empty() {
            return Ok(());
        }
        if !self.config.persist_replay {
            return Err(SessionError::replay_unavailable(
                id.clone(),
                "in-memory replay persistence is disabled",
            ));
        }
        validate_event_batch(id, events, "append_events")?;

        let mut state = self.state.write().await;
        let now_ms = now_ms();
        state.evict_expired(now_ms, self.config.idle_ttl_ms);
        let Some(expires_at_ms) = state
            .sessions
            .get(id)
            .map(|entry| entry.metadata.expires_at_ms)
        else {
            return Err(SessionError::NotFound {
                session_id: id.clone(),
            });
        };
        if matches!(expires_at_ms, Some(value) if value <= now_ms) {
            let _ = state.remove(id);
            return Err(SessionError::expired(id.clone(), expires_at_ms));
        }
        let Some(entry) = state.sessions.get_mut(id) else {
            return Err(SessionError::NotFound {
                session_id: id.clone(),
            });
        };

        let last_sequence = entry.metadata.last_sequence.unwrap_or(0);
        if events[0].sequence <= last_sequence {
            return Err(SessionError::storage_failure(
                format!("event batch for session `{id}` must start after sequence {last_sequence}"),
                Some(id.clone()),
                "append_events",
            ));
        }

        entry.events.extend_from_slice(events);
        entry.metadata.event_count = entry.events.len();
        entry.metadata.last_sequence = Some(events[events.len() - 1].sequence);
        entry.metadata.updated_at_ms = now_ms;
        state.touch(id, now_ms);
        drop(state);
        Ok(())
    }

    async fn save_turn_checkpoint(
        &self,
        id: &SessionId,
        checkpoint: TurnCheckpoint,
    ) -> Result<(), SessionError> {
        let mut state = self.state.write().await;
        let now_ms = now_ms();
        state.evict_expired(now_ms, self.config.idle_ttl_ms);
        let Some(expires_at_ms) = state
            .sessions
            .get(id)
            .map(|entry| entry.metadata.expires_at_ms)
        else {
            return Err(SessionError::NotFound {
                session_id: id.clone(),
            });
        };
        if matches!(expires_at_ms, Some(value) if value <= now_ms) {
            let _ = state.remove(id);
            return Err(SessionError::expired(id.clone(), expires_at_ms));
        }
        let Some(entry) = state.sessions.get_mut(id) else {
            return Err(SessionError::NotFound {
                session_id: id.clone(),
            });
        };

        entry.metadata.apply_checkpoint(&checkpoint, now_ms);
        entry.last_checkpoint = Some(checkpoint);
        state.touch(id, now_ms);
        drop(state);
        Ok(())
    }

    async fn replay_events(
        &self,
        id: &SessionId,
        after_sequence: Option<u64>,
        limit: Option<usize>,
    ) -> Result<Vec<PersistedEvent>, SessionError> {
        let mut state = self.state.write().await;
        let now_ms = now_ms();
        state.evict_expired(now_ms, self.config.idle_ttl_ms);
        let Some(entry) = state.sessions.get(id).cloned() else {
            return Err(SessionError::NotFound {
                session_id: id.clone(),
            });
        };

        if !entry.metadata.replay_available {
            return Err(SessionError::replay_unavailable(
                id.clone(),
                "in-memory replay persistence is disabled",
            ));
        }

        state.touch(id, now_ms);
        drop(state);

        let mut events = entry
            .events
            .into_iter()
            .filter(|event| {
                if let Some(sequence) = after_sequence {
                    return event.sequence > sequence;
                }

                true
            })
            .collect::<Vec<_>>();
        if let Some(limit) = limit {
            events.truncate(limit);
        }

        Ok(events)
    }

    async fn list(&self, filter: SessionFilter) -> Result<Vec<SessionMetadata>, SessionError> {
        let now_ms = now_ms();
        let mut state = self.state.write().await;
        state.evict_expired(now_ms, self.config.idle_ttl_ms);

        let mut sessions = state
            .sessions
            .values()
            .map(|entry| entry.metadata.clone())
            .filter(|metadata| filter.matches(metadata))
            .collect::<Vec<_>>();
        sessions.sort_by(|left, right| {
            right
                .updated_at_ms
                .cmp(&left.updated_at_ms)
                .then_with(|| right.created_at_ms.cmp(&left.created_at_ms))
        });
        if let Some(limit) = filter.limit {
            sessions.truncate(limit);
        }
        drop(state);
        Ok(sessions)
    }

    async fn delete(&self, id: &SessionId) -> Result<(), SessionError> {
        let mut state = self.state.write().await;
        let Some(_) = state.remove(id) else {
            return Err(SessionError::NotFound {
                session_id: id.clone(),
            });
        };
        drop(state);
        Ok(())
    }
}

impl SessionFilter {
    fn matches(&self, metadata: &SessionMetadata) -> bool {
        if let Some((key, expected)) = &self.label {
            let Some(actual) = metadata.labels.get(key) else {
                return false;
            };
            if actual != expected {
                return false;
            }
        }
        if let Some(since) = self.since {
            if metadata.updated_at_ms < since {
                return false;
            }
        }
        true
    }
}

fn replay_cursor_for(
    replay_available: bool,
    events: &[PersistedEvent],
    checkpoint: Option<&TurnCheckpoint>,
) -> Option<ReplayCursor> {
    if let Some(checkpoint) = checkpoint {
        return Some(ReplayCursor::from_checkpoint(checkpoint.sequence));
    }
    if replay_available {
        return events
            .first()
            .map(|event| ReplayCursor::new(event.sequence));
    }
    None
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{
        InMemorySessionStore, InMemorySessionStoreConfig, NewSession, PersistedEvent, ReplayCursor,
        SessionFilter, SessionStore, TurnCheckpoint,
    };
    use arky_error::ClassifiedError;
    use arky_protocol::{AgentEvent, EventMetadata, Message, SessionId, ToolResult, TurnId};
    use pretty_assertions::assert_eq;

    fn sample_event(session_id: &SessionId, sequence: u64) -> PersistedEvent {
        PersistedEvent::new(AgentEvent::AgentStart {
            meta: EventMetadata::new(sequence * 10, sequence).with_session_id(session_id.clone()),
        })
    }

    #[tokio::test]
    async fn create_session_should_load_full_snapshot() {
        let store = InMemorySessionStore::default();
        let mut labels = BTreeMap::new();
        labels.insert("project".to_owned(), "arky".to_owned());
        let session_id = store
            .create(NewSession {
                model_id: Some("claude-sonnet".to_owned()),
                labels,
                expires_at_ms: None,
            })
            .await
            .expect("session should be created");

        let snapshot = store.load(&session_id).await.expect("session should load");

        assert_eq!(snapshot.metadata.id, session_id);
        assert_eq!(snapshot.metadata.message_count, 0);
        assert!(snapshot.messages.is_empty());
        assert_eq!(snapshot.replay_cursor, None);
    }

    #[tokio::test]
    async fn append_messages_should_extend_transcript() {
        let store = InMemorySessionStore::default();
        let session_id = store
            .create(NewSession::default())
            .await
            .expect("session should be created");

        store
            .append_messages(
                &session_id,
                &[Message::user("hello"), Message::assistant("world")],
            )
            .await
            .expect("messages should append");

        let snapshot = store.load(&session_id).await.expect("session should load");
        assert_eq!(snapshot.messages.len(), 2);
        assert_eq!(snapshot.metadata.message_count, 2);
    }

    #[tokio::test]
    async fn append_events_should_store_replay_metadata() {
        let store = InMemorySessionStore::default();
        let session_id = store
            .create(NewSession::default())
            .await
            .expect("session should be created");
        let events = vec![sample_event(&session_id, 4), sample_event(&session_id, 5)];

        store
            .append_events(&session_id, &events)
            .await
            .expect("events should append");

        let snapshot = store.load(&session_id).await.expect("session should load");
        assert_eq!(snapshot.metadata.event_count, 2);
        assert_eq!(snapshot.replay_cursor, Some(ReplayCursor::new(4)));
    }

    #[tokio::test]
    async fn replay_events_should_return_filtered_sequences_in_order() {
        let store = InMemorySessionStore::default();
        let session_id = store
            .create(NewSession::default())
            .await
            .expect("session should be created");
        let events = vec![
            sample_event(&session_id, 1),
            sample_event(&session_id, 2),
            sample_event(&session_id, 3),
        ];

        store
            .append_events(&session_id, &events)
            .await
            .expect("events should append");

        let replay = store
            .replay_events(&session_id, Some(1), Some(1))
            .await
            .expect("replay should load");

        assert_eq!(replay, vec![events[1].clone()]);
    }

    #[tokio::test]
    async fn append_events_should_fail_when_replay_is_disabled() {
        let store = InMemorySessionStore::new(InMemorySessionStoreConfig {
            persist_replay: false,
            idle_ttl_ms: None,
            max_sessions: None,
        });
        let session_id = store
            .create(NewSession::default())
            .await
            .expect("session should be created");

        let error = store
            .append_events(&session_id, &[sample_event(&session_id, 1)])
            .await
            .expect_err("disabled replay should reject events");

        assert_eq!(error.error_code(), "SESSION_REPLAY_UNAVAILABLE");
    }

    #[tokio::test]
    async fn replay_events_should_fail_when_replay_is_disabled() {
        let store = InMemorySessionStore::new(InMemorySessionStoreConfig {
            persist_replay: false,
            idle_ttl_ms: None,
            max_sessions: None,
        });
        let session_id = store
            .create(NewSession::default())
            .await
            .expect("session should be created");

        let error = store
            .replay_events(&session_id, None, None)
            .await
            .expect_err("disabled replay should reject reads");

        assert_eq!(error.error_code(), "SESSION_REPLAY_UNAVAILABLE");
    }

    #[tokio::test]
    async fn save_turn_checkpoint_should_be_visible_on_load() {
        let store = InMemorySessionStore::default();
        let session_id = store
            .create(NewSession::default())
            .await
            .expect("session should be created");
        let checkpoint = TurnCheckpoint::new(TurnId::new(), 9)
            .with_message(Message::assistant("done"))
            .with_tool_results(vec![ToolResult::success("call-1", "read_file", Vec::new())])
            .with_provider_id("codex".into())
            .with_provider_session_id("provider-session")
            .mark_completed(900);

        store
            .save_turn_checkpoint(&session_id, checkpoint.clone())
            .await
            .expect("checkpoint should save");

        let snapshot = store.load(&session_id).await.expect("session should load");
        assert_eq!(snapshot.last_checkpoint, Some(checkpoint));
        assert_eq!(
            snapshot.replay_cursor,
            Some(ReplayCursor::from_checkpoint(9))
        );
        assert_eq!(
            snapshot.metadata.provider_session_id,
            Some("provider-session".to_owned())
        );
    }

    #[tokio::test]
    async fn list_should_filter_by_label_and_limit() {
        let store = InMemorySessionStore::default();
        let mut first_labels = BTreeMap::new();
        first_labels.insert("project".to_owned(), "arky".to_owned());
        let first_id = store
            .create(NewSession {
                model_id: None,
                labels: first_labels,
                expires_at_ms: None,
            })
            .await
            .expect("first session should be created");
        store
            .append_messages(&first_id, &[Message::user("hello")])
            .await
            .expect("message should append");

        let mut second_labels = BTreeMap::new();
        second_labels.insert("project".to_owned(), "other".to_owned());
        let second_id = store
            .create(NewSession {
                model_id: None,
                labels: second_labels,
                expires_at_ms: None,
            })
            .await
            .expect("second session should be created");
        store
            .append_messages(&second_id, &[Message::user("world")])
            .await
            .expect("message should append");

        let matching = store
            .list(SessionFilter {
                label: Some(("project".to_owned(), "arky".to_owned())),
                since: None,
                limit: Some(1),
            })
            .await
            .expect("sessions should list");

        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].id, first_id);
    }

    #[tokio::test]
    async fn delete_should_remove_session() {
        let store = InMemorySessionStore::default();
        let session_id = store
            .create(NewSession::default())
            .await
            .expect("session should be created");

        store
            .delete(&session_id)
            .await
            .expect("session should be deleted");

        let error = store
            .load(&session_id)
            .await
            .expect_err("deleted session should not load");
        assert_eq!(error.error_code(), "SESSION_NOT_FOUND");
    }

    #[test]
    fn replay_cursor_should_advance_monotonically() {
        let mut cursor = ReplayCursor::new(3);
        cursor.advance_to(8);

        assert_eq!(cursor, ReplayCursor::from_checkpoint(8));
    }

    #[tokio::test]
    async fn session_should_expire_when_expiration_has_passed() {
        let store = InMemorySessionStore::default();
        let session_id = store
            .create(NewSession {
                model_id: None,
                labels: BTreeMap::new(),
                expires_at_ms: Some(1),
            })
            .await
            .expect("session should be created");

        let error = store
            .load(&session_id)
            .await
            .expect_err("expired session should not load");

        assert_eq!(error.error_code(), "SESSION_NOT_FOUND");
    }

    #[tokio::test]
    async fn expired_idle_session_should_not_load() {
        let store = InMemorySessionStore::new(InMemorySessionStoreConfig {
            persist_replay: true,
            idle_ttl_ms: Some(0),
            max_sessions: None,
        });
        let session_id = store
            .create(NewSession::default())
            .await
            .expect("session should be created");

        let error = store
            .load(&session_id)
            .await
            .expect_err("idle-expired session should not load");

        assert_eq!(error.error_code(), "SESSION_NOT_FOUND");
    }

    #[tokio::test]
    async fn access_should_refresh_idle_ttl() {
        let store = InMemorySessionStore::new(InMemorySessionStoreConfig {
            persist_replay: true,
            idle_ttl_ms: Some(u64::MAX),
            max_sessions: None,
        });
        let session_id = store
            .create(NewSession::default())
            .await
            .expect("session should be created");

        let snapshot = store.load(&session_id).await.expect("session should load");

        assert_eq!(snapshot.metadata.id, session_id);
    }

    #[tokio::test]
    async fn over_capacity_should_evict_oldest_entry() {
        let store = InMemorySessionStore::new(InMemorySessionStoreConfig {
            persist_replay: true,
            idle_ttl_ms: None,
            max_sessions: Some(1),
        });
        let first = store
            .create(NewSession::default())
            .await
            .expect("first session should create");
        let second = store
            .create(NewSession::default())
            .await
            .expect("second session should create");

        let error = store
            .load(&first)
            .await
            .expect_err("first session should be evicted");
        let second_snapshot = store
            .load(&second)
            .await
            .expect("second session should load");

        assert_eq!(error.error_code(), "SESSION_NOT_FOUND");
        assert_eq!(second_snapshot.metadata.id, second);
    }

    #[test]
    fn event_batches_should_require_strict_monotonic_sequences() {
        let session_id = SessionId::new();
        let error = crate::support::validate_event_batch(
            &session_id,
            &[sample_event(&session_id, 5), sample_event(&session_id, 5)],
            "append_events",
        )
        .expect_err("duplicate sequence should fail");

        assert_eq!(error.error_code(), "SESSION_STORAGE_FAILURE");
    }

    #[test]
    fn session_filter_should_respect_updated_since() {
        let metadata = crate::SessionMetadata {
            id: SessionId::new(),
            created_at_ms: 10,
            updated_at_ms: 20,
            message_count: 0,
            event_count: 0,
            last_sequence: None,
            model_id: None,
            provider_id: None,
            provider_session_id: None,
            labels: BTreeMap::from([(String::from("project"), String::from("arky"))]),
            replay_available: true,
            expires_at_ms: None,
        };

        assert!(SessionFilter {
            label: Some((String::from("project"), String::from("arky"))),
            since: Some(20),
            limit: None,
        }
        .matches(&metadata));
        assert!(!SessionFilter {
            label: Some((String::from("project"), String::from("arky"))),
            since: Some(21),
            limit: None,
        }
        .matches(&metadata));
    }
}
