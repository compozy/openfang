//! Internal helpers shared across session backends.

use std::time::{SystemTime, UNIX_EPOCH};

use arky_protocol::{PersistedEvent, SessionId};

use crate::SessionError;

/// Returns the current Unix timestamp in milliseconds.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Validates that a batch of persisted events belongs to the session and is
/// strictly increasing.
pub fn validate_event_batch(
    session_id: &SessionId,
    events: &[PersistedEvent],
    operation: &'static str,
) -> Result<(), SessionError> {
    for event in events {
        if let Some(event_session_id) = event.event.metadata().session_id.as_ref()
            && event_session_id != session_id
        {
            return Err(SessionError::storage_failure(
                format!(
                    "event sequence {} belongs to session `{event_session_id}` instead of `{session_id}`",
                    event.sequence
                ),
                Some(session_id.clone()),
                operation,
            ));
        }
    }

    for pair in events.windows(2) {
        if pair[1].sequence <= pair[0].sequence {
            return Err(SessionError::storage_failure(
                "persisted event batches must be strictly increasing",
                Some(session_id.clone()),
                operation,
            ));
        }
    }

    Ok(())
}
