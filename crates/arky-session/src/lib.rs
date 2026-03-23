//! Session storage contracts, snapshots, and backends for Arky.
//!
//! This crate provides replay-aware session persistence contracts together with
//! in-memory and SQLite-backed implementations for local development and
//! durable runtime use.

mod error;
mod key;
mod memory;
mod snapshot;
#[cfg(feature = "sqlite")]
mod sqlite;
mod store;
mod support;

#[cfg(feature = "sqlite")]
pub use crate::sqlite::{SqliteSessionStore, SqliteSessionStoreConfig};
pub use crate::{
    error::SessionError,
    key::SessionKey,
    memory::{InMemorySessionStore, InMemorySessionStoreConfig},
    snapshot::{NewSession, SessionFilter, SessionMetadata, SessionSnapshot},
    store::SessionStore,
};
pub use arky_protocol::{PersistedEvent, ReplayCursor, SessionId, TurnCheckpoint};

#[cfg(test)]
mod tests {
    #[cfg(feature = "sqlite")]
    use super::SqliteSessionStore;
    use super::{InMemorySessionStore, SessionStore};

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn session_store_backends_should_be_send_and_sync() {
        assert_send_sync::<InMemorySessionStore>();
        #[cfg(feature = "sqlite")]
        assert_send_sync::<SqliteSessionStore>();
    }

    #[test]
    fn session_store_trait_object_should_be_send_and_sync() {
        fn assert_trait(_: &(dyn SessionStore + Send + Sync)) {}

        let store = InMemorySessionStore::default();
        assert_trait(&store);
    }
}
