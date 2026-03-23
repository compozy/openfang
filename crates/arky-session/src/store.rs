//! Session persistence trait shared by all backends.

use arky_protocol::{Message, PersistedEvent, SessionId, TurnCheckpoint};
use async_trait::async_trait;

use crate::{NewSession, SessionError, SessionFilter, SessionMetadata, SessionSnapshot};

/// Persistent storage contract for session state and replay metadata.
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Creates a new session and returns its stable identifier.
    async fn create(&self, new_session: NewSession) -> Result<SessionId, SessionError>;

    /// Loads the current session snapshot for resume and replay.
    async fn load(&self, id: &SessionId) -> Result<SessionSnapshot, SessionError>;

    /// Appends additional messages to the stored transcript.
    async fn append_messages(
        &self,
        id: &SessionId,
        messages: &[Message],
    ) -> Result<(), SessionError>;

    /// Appends replay events for the session.
    async fn append_events(
        &self,
        id: &SessionId,
        events: &[PersistedEvent],
    ) -> Result<(), SessionError>;

    /// Persists the latest completed turn checkpoint.
    async fn save_turn_checkpoint(
        &self,
        id: &SessionId,
        checkpoint: TurnCheckpoint,
    ) -> Result<(), SessionError>;

    /// Loads historical replay events for a session in ascending sequence order.
    async fn replay_events(
        &self,
        id: &SessionId,
        after_sequence: Option<u64>,
        limit: Option<usize>,
    ) -> Result<Vec<PersistedEvent>, SessionError>;

    /// Lists stored sessions that match the provided filter.
    async fn list(&self, filter: SessionFilter) -> Result<Vec<SessionMetadata>, SessionError>;

    /// Deletes a stored session and all of its persisted state.
    async fn delete(&self, id: &SessionId) -> Result<(), SessionError>;
}
