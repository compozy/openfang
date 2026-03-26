//! Shared bounded SSE stream primitives used by control-plane watch endpoints.

use dashmap::DashMap;
use serde_json::Value as JsonValue;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::broadcast;

/// Default broadcast channel size for per-resource SSE fan-out.
pub const DEFAULT_BROADCAST_CAPACITY: usize = 128;

/// One buffered SSE event retained for bounded replay.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BufferedSseEvent {
    /// Monotonically increasing event id scoped to one stream handle.
    pub id: u64,
    /// Stable event name.
    pub event: String,
    /// Event JSON payload.
    pub data: JsonValue,
}

/// Replay result for one `Last-Event-ID` request.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayResult {
    /// Whether the caller requested an event outside the retained window.
    pub reset_required: bool,
    /// Retained events newer than the requested id.
    pub events: Vec<BufferedSseEvent>,
}

/// Bounded in-memory stream handle with replay support and optional dedupe.
#[derive(Debug)]
pub struct BoundedSseHandle<const CAPACITY: usize> {
    sender: broadcast::Sender<BufferedSseEvent>,
    history: RwLock<VecDeque<BufferedSseEvent>>,
    next_event_id: AtomicU64,
    fingerprints: Mutex<HashMap<String, String>>,
}

impl<const CAPACITY: usize> BoundedSseHandle<CAPACITY> {
    /// Create a new bounded stream handle.
    #[must_use]
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(DEFAULT_BROADCAST_CAPACITY);
        Self {
            sender,
            history: RwLock::new(VecDeque::with_capacity(CAPACITY)),
            next_event_id: AtomicU64::new(0),
            fingerprints: Mutex::new(HashMap::new()),
        }
    }

    /// Return the latest assigned id for this handle.
    #[must_use]
    pub fn latest_event_id(&self) -> u64 {
        self.next_event_id.load(Ordering::Relaxed)
    }

    /// Subscribe to live events.
    pub fn subscribe(&self) -> broadcast::Receiver<BufferedSseEvent> {
        self.sender.subscribe()
    }

    /// Publish one event and retain it in the bounded history ring.
    pub fn publish(&self, event: impl Into<String>, data: JsonValue) -> BufferedSseEvent {
        let buffered = BufferedSseEvent {
            id: self.next_event_id.fetch_add(1, Ordering::Relaxed) + 1,
            event: event.into(),
            data,
        };

        {
            let mut history = self
                .history
                .write()
                .unwrap_or_else(|error| error.into_inner());
            history.push_back(buffered.clone());
            while history.len() > CAPACITY {
                history.pop_front();
            }
        }

        let _ = self.sender.send(buffered.clone());
        buffered
    }

    /// Publish an event only when the fingerprint for `dedupe_key` changes.
    ///
    /// Returns `Some(event)` when published, `None` when suppressed.
    pub fn publish_if_changed(
        &self,
        dedupe_key: impl Into<String>,
        fingerprint: String,
        event: impl Into<String>,
        data: JsonValue,
    ) -> Option<BufferedSseEvent> {
        let key = dedupe_key.into();
        {
            let mut fingerprints = self
                .fingerprints
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if fingerprints
                .get(&key)
                .is_some_and(|current| current == &fingerprint)
            {
                return None;
            }
            fingerprints.insert(key, fingerprint);
        }

        Some(self.publish(event, data))
    }

    /// Best-effort replay for one `Last-Event-ID` request.
    #[must_use]
    pub fn replay_after(&self, last_event_id: Option<u64>) -> ReplayResult {
        let history = self
            .history
            .read()
            .unwrap_or_else(|error| error.into_inner());
        let Some(last_event_id) = last_event_id else {
            return ReplayResult {
                reset_required: false,
                events: Vec::new(),
            };
        };

        if history.is_empty() {
            return ReplayResult {
                reset_required: last_event_id > 0,
                events: Vec::new(),
            };
        }

        let oldest_id = history.front().map(|event| event.id).unwrap_or(0);
        if last_event_id < oldest_id.saturating_sub(1) {
            return ReplayResult {
                reset_required: true,
                events: Vec::new(),
            };
        }

        ReplayResult {
            reset_required: false,
            events: history
                .iter()
                .filter(|event| event.id > last_event_id)
                .cloned()
                .collect(),
        }
    }
}

impl<const CAPACITY: usize> Default for BoundedSseHandle<CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry of bounded per-resource stream handles.
#[derive(Debug, Default)]
pub struct ResourceSseRegistry<const CAPACITY: usize> {
    handles: DashMap<String, Arc<BoundedSseHandle<CAPACITY>>>,
}

impl<const CAPACITY: usize> ResourceSseRegistry<CAPACITY> {
    /// Return an existing handle or create one on demand.
    pub fn handle(&self, resource_id: &str) -> Arc<BoundedSseHandle<CAPACITY>> {
        Arc::clone(
            self.handles
                .entry(resource_id.to_owned())
                .or_insert_with(|| Arc::new(BoundedSseHandle::new()))
                .value(),
        )
    }
}
