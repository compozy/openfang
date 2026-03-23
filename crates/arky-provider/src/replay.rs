//! Event replay persistence helpers used by streaming providers.

use std::sync::Arc;

use arky_protocol::{AgentEvent, PersistedEvent, SessionId, TurnCheckpoint};
use tracing::warn;

use crate::SessionStore;
use arky_session::SessionError;

/// Replay persistence configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayWriterConfig {
    /// Number of events buffered before flushing to the session store.
    pub flush_threshold: usize,
}

impl Default for ReplayWriterConfig {
    fn default() -> Self {
        Self {
            flush_threshold: 16,
        }
    }
}

/// Helper that batches replay events and turn checkpoints during streaming.
pub struct ReplayWriter {
    store: Arc<dyn SessionStore>,
    session_id: SessionId,
    config: ReplayWriterConfig,
    buffered_events: Vec<PersistedEvent>,
    pending_checkpoint: Option<TurnCheckpoint>,
}

impl ReplayWriter {
    /// Creates a replay writer for a concrete session.
    #[must_use]
    pub fn new(store: Arc<dyn SessionStore>, session_id: SessionId) -> Self {
        Self::with_config(store, session_id, ReplayWriterConfig::default())
    }

    /// Creates a replay writer with explicit configuration.
    #[must_use]
    pub fn with_config(
        store: Arc<dyn SessionStore>,
        session_id: SessionId,
        config: ReplayWriterConfig,
    ) -> Self {
        Self {
            store,
            session_id,
            config,
            buffered_events: Vec::new(),
            pending_checkpoint: None,
        }
    }

    /// Returns the number of buffered events waiting to be flushed.
    #[must_use]
    pub const fn buffered_len(&self) -> usize {
        self.buffered_events.len()
    }

    /// Records one provider or agent event.
    pub async fn record(&mut self, event: AgentEvent) -> Result<(), SessionError> {
        let persisted_event = PersistedEvent::new(event.clone());
        self.capture_checkpoint(&event);
        self.buffered_events.push(persisted_event);

        if self.buffered_events.len() >= self.config.flush_threshold {
            self.flush().await?;
        }

        Ok(())
    }

    /// Flushes buffered replay events and the latest checkpoint.
    pub async fn flush(&mut self) -> Result<(), SessionError> {
        if !self.buffered_events.is_empty() {
            self.store
                .append_events(&self.session_id, &self.buffered_events)
                .await?;
            self.buffered_events.clear();
        }

        if let Some(checkpoint) = self.pending_checkpoint.take() {
            self.store
                .save_turn_checkpoint(&self.session_id, checkpoint)
                .await?;
        }

        Ok(())
    }

    /// Flushes any remaining buffered data and consumes the writer.
    pub async fn finish(mut self) -> Result<(), SessionError> {
        self.flush().await
    }

    fn capture_checkpoint(&mut self, event: &AgentEvent) {
        if let AgentEvent::TurnEnd {
            meta,
            message,
            tool_results,
            ..
        } = event
        {
            let Some(turn_id) = meta.turn_id.clone() else {
                return;
            };

            let mut checkpoint = TurnCheckpoint::new(turn_id, meta.sequence)
                .with_message(message.clone())
                .with_tool_results(tool_results.clone())
                .mark_completed(meta.timestamp_ms);

            if let Some(provider_id) = meta.provider_id.clone() {
                checkpoint = checkpoint.with_provider_id(provider_id);
            }

            self.pending_checkpoint = Some(checkpoint);
        }
    }
}

impl Drop for ReplayWriter {
    fn drop(&mut self) {
        if self.buffered_events.is_empty() && self.pending_checkpoint.is_none() {
            return;
        }

        warn!(
            buffered_events = self.buffered_events.len(),
            has_pending_checkpoint = self.pending_checkpoint.is_some(),
            "replay writer dropped with unflushed persistence state"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use pretty_assertions::assert_eq;

    use super::{ReplayWriter, ReplayWriterConfig};
    use arky_protocol::{AgentEvent, EventMetadata, Message, ProviderId, TurnId};
    use arky_session::{InMemorySessionStore, NewSession, SessionStore};

    #[tokio::test]
    async fn replay_writer_should_persist_events_and_turn_checkpoints() {
        let store = Arc::new(InMemorySessionStore::default());
        let session_id = store
            .create(NewSession::default())
            .await
            .expect("session should be created");
        let mut writer = ReplayWriter::with_config(
            store.clone(),
            session_id.clone(),
            ReplayWriterConfig { flush_threshold: 2 },
        );
        let turn_id = TurnId::new();

        writer
            .record(AgentEvent::TurnStart {
                meta: EventMetadata::new(1, 1)
                    .with_session_id(session_id.clone())
                    .with_turn_id(turn_id.clone())
                    .with_provider_id(ProviderId::new("codex")),
            })
            .await
            .expect("event should record");
        writer
            .record(AgentEvent::TurnEnd {
                meta: EventMetadata::new(2, 2)
                    .with_session_id(session_id.clone())
                    .with_turn_id(turn_id.clone())
                    .with_provider_id(ProviderId::new("codex")),
                message: Message::assistant("done"),
                tool_results: Vec::new(),
                usage: None,
            })
            .await
            .expect("event should record");
        writer.finish().await.expect("writer should flush");

        let snapshot = store.load(&session_id).await.expect("session should load");

        assert_eq!(
            snapshot
                .replay_cursor
                .expect("cursor should exist")
                .next_sequence,
            3
        );
        assert_eq!(
            snapshot
                .last_checkpoint
                .expect("checkpoint should exist")
                .message
                .expect("message should exist"),
            Message::assistant("done")
        );
    }
}
