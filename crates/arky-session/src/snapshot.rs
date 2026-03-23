//! Session snapshots, metadata, and list filters.

use std::collections::BTreeMap;

use arky_protocol::{Message, ProviderId, ReplayCursor, SessionId, TurnCheckpoint};
use serde::{Deserialize, Serialize};

/// Input payload used when creating a new session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct NewSession {
    /// Provider model identifier chosen for the session, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    /// Arbitrary labels used for grouping and lookup.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    /// Absolute expiration timestamp in milliseconds since the Unix epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
}

/// Summary metadata stored for a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// Stable SDK session identifier.
    pub id: SessionId,
    /// Creation timestamp in milliseconds since the Unix epoch.
    pub created_at_ms: u64,
    /// Last update timestamp in milliseconds since the Unix epoch.
    pub updated_at_ms: u64,
    /// Total number of persisted transcript messages.
    pub message_count: usize,
    /// Total number of persisted replay events.
    pub event_count: usize,
    /// Highest known session sequence from events or checkpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sequence: Option<u64>,
    /// Provider model identifier chosen for the session, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    /// Owning provider identifier inferred from checkpoints, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<ProviderId>,
    /// Provider-native session identifier used for resume, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    /// Arbitrary labels used for grouping and lookup.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    /// Whether this backend keeps replay events available for this session.
    pub replay_available: bool,
    /// Absolute expiration timestamp in milliseconds since the Unix epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
}

impl SessionMetadata {
    /// Creates session metadata from a freshly created session request.
    #[must_use]
    pub fn from_new_session(
        id: SessionId,
        new_session: NewSession,
        created_at_ms: u64,
        replay_available: bool,
    ) -> Self {
        Self {
            id,
            created_at_ms,
            updated_at_ms: created_at_ms,
            message_count: 0,
            event_count: 0,
            last_sequence: None,
            model_id: new_session.model_id,
            provider_id: None,
            provider_session_id: None,
            labels: new_session.labels,
            replay_available,
            expires_at_ms: new_session.expires_at_ms,
        }
    }

    /// Returns whether the session should be treated as expired at `now_ms`.
    #[must_use]
    pub const fn is_expired_at(&self, now_ms: u64) -> bool {
        matches!(self.expires_at_ms, Some(expires_at_ms) if expires_at_ms <= now_ms)
    }

    /// Applies checkpoint-derived provider metadata and sequence progress.
    pub fn apply_checkpoint(&mut self, checkpoint: &TurnCheckpoint, updated_at_ms: u64) {
        self.updated_at_ms = updated_at_ms;
        let last_sequence = self.last_sequence.unwrap_or(checkpoint.sequence);
        self.last_sequence = Some(last_sequence.max(checkpoint.sequence));
        self.provider_id.clone_from(&checkpoint.provider_id);
        self.provider_session_id
            .clone_from(&checkpoint.provider_session_id);
    }
}

/// Full session state loaded from a backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// Summary metadata for the session.
    pub metadata: SessionMetadata,
    /// Full transcript persisted for the session.
    pub messages: Vec<Message>,
    /// Most recent turn checkpoint, when one has been stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checkpoint: Option<TurnCheckpoint>,
    /// Replay position for the next resume/replay operation, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_cursor: Option<ReplayCursor>,
}

/// Listing criteria for [`crate::SessionStore::list`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SessionFilter {
    /// Exact label match to apply during listing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<(String, String)>,
    /// Minimum `updated_at_ms` value to include.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
    /// Maximum number of sessions to return.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}
