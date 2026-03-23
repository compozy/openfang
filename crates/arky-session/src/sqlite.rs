//! SQLite-backed session store implementation.

use std::{collections::BTreeMap, path::Path, time::Duration};

use arky_protocol::{Message, PersistedEvent, ReplayCursor, SessionId, TurnCheckpoint};
use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_rusqlite::{
    Connection, Error as TokioRusqliteError,
    rusqlite::{self, ErrorCode, OptionalExtension, TransactionBehavior, params},
};

use crate::{
    NewSession, SessionError, SessionFilter, SessionMetadata, SessionSnapshot, SessionStore,
    support::{now_ms, validate_event_batch},
};

const DEFAULT_BUSY_TIMEOUT: Duration = Duration::from_millis(250);
const DEFAULT_RETRY_BACKOFF: Duration = Duration::from_millis(50);
const DEFAULT_MAX_BUSY_RETRIES: usize = 5;

/// Configuration for [`SqliteSessionStore`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SqliteSessionStoreConfig {
    /// Busy timeout configured on each `SQLite` connection.
    pub busy_timeout: Duration,
    /// Additional retry attempts when `SQLite` still returns busy/locked.
    pub max_busy_retries: usize,
    /// Delay between busy/locked retries.
    pub retry_backoff: Duration,
}

impl Default for SqliteSessionStoreConfig {
    fn default() -> Self {
        Self {
            busy_timeout: DEFAULT_BUSY_TIMEOUT,
            max_busy_retries: DEFAULT_MAX_BUSY_RETRIES,
            retry_backoff: DEFAULT_RETRY_BACKOFF,
        }
    }
}

/// SQLite-backed session store using WAL mode and a dedicated single writer.
#[derive(Debug)]
pub struct SqliteSessionStore {
    config: SqliteSessionStoreConfig,
    writer: Connection,
    reader: Connection,
    write_gate: Mutex<()>,
}

impl SqliteSessionStore {
    /// Opens or creates a `SQLite` session store at `path`.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, SessionError> {
        Self::open_with_config(path, SqliteSessionStoreConfig::default()).await
    }

    /// Opens or creates a `SQLite` session store with explicit configuration.
    pub async fn open_with_config(
        path: impl AsRef<Path>,
        config: SqliteSessionStoreConfig,
    ) -> Result<Self, SessionError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|error| {
                SessionError::storage_failure(
                    format!(
                        "failed to create SQLite parent directory `{}`: {error}",
                        parent.display()
                    ),
                    None,
                    "sqlite_open",
                )
            })?;
        }

        let writer = Connection::open(&path).await.map_err(|error| {
            SessionError::storage_failure(
                format!("failed to open SQLite writer connection: {error}"),
                None,
                "sqlite_open",
            )
        })?;
        let reader = Connection::open(&path).await.map_err(|error| {
            SessionError::storage_failure(
                format!("failed to open SQLite reader connection: {error}"),
                None,
                "sqlite_open",
            )
        })?;

        configure_connection(&writer, config).await?;
        configure_connection(&reader, config).await?;
        initialize_schema(&writer).await?;

        Ok(Self {
            config,
            writer,
            reader,
            write_gate: Mutex::new(()),
        })
    }

    async fn load_metadata_only(&self, id: &SessionId) -> Result<SessionMetadata, SessionError> {
        let session_id = id.clone();
        let metadata = self
            .with_sqlite_retry("load_metadata", Some(id), || {
                let session_id = session_id.clone();
                self.reader.call(move |conn| {
                    load_session_metadata(conn, &session_id)?
                        .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)
                })
            })
            .await?;

        if metadata.is_expired_at(now_ms()) {
            return Err(SessionError::expired(id.clone(), metadata.expires_at_ms));
        }

        Ok(metadata)
    }

    async fn with_sqlite_retry<R, MakeAttempt, AttemptFuture>(
        &self,
        operation: &'static str,
        session_id: Option<&SessionId>,
        make_attempt: MakeAttempt,
    ) -> Result<R, SessionError>
    where
        MakeAttempt: Fn() -> AttemptFuture,
        AttemptFuture: std::future::Future<Output = Result<R, TokioRusqliteError<rusqlite::Error>>>,
    {
        let mut attempt_index = 0usize;
        loop {
            match make_attempt().await {
                Ok(value) => return Ok(value),
                Err(error)
                    if is_busy_or_locked(&error)
                        && attempt_index < self.config.max_busy_retries =>
                {
                    attempt_index = attempt_index.saturating_add(1);
                    tokio::time::sleep(self.config.retry_backoff).await;
                }
                Err(error) => {
                    let retryable = is_busy_or_locked(&error);
                    let retry_after = retryable.then_some(self.config.retry_backoff);
                    if let Some(session_id) = session_id.cloned()
                        && is_query_returned_no_rows(&error)
                    {
                        return Err(SessionError::NotFound { session_id });
                    }
                    return Err(map_sqlite_error(
                        operation,
                        session_id.cloned(),
                        &error,
                        retryable,
                        retry_after,
                    ));
                }
            }
        }
    }
}

#[async_trait]
impl SessionStore for SqliteSessionStore {
    async fn create(&self, new_session: NewSession) -> Result<SessionId, SessionError> {
        let session_id = SessionId::new();
        let metadata =
            SessionMetadata::from_new_session(session_id.clone(), new_session, now_ms(), true);
        let _write_guard = self.write_gate.lock().await;
        self.with_sqlite_retry("create", Some(&session_id), || {
            let metadata = metadata.clone();
            let session_id = session_id.clone();
            self.writer.call(move |conn| {
                let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
                insert_session(&transaction, &metadata)?;
                transaction.commit()?;
                Ok(session_id)
            })
        })
        .await
    }

    async fn load(&self, id: &SessionId) -> Result<SessionSnapshot, SessionError> {
        let session_id = id.clone();
        let snapshot = self
            .with_sqlite_retry("load", Some(id), || {
                let session_id = session_id.clone();
                self.reader.call(move |conn| {
                    let Some(metadata) = load_session_metadata(conn, &session_id)? else {
                        return Err(rusqlite::Error::QueryReturnedNoRows);
                    };
                    let messages = load_messages(conn, &session_id)?;
                    let last_checkpoint = load_checkpoint(conn, &session_id)?;
                    let replay_cursor = load_replay_cursor(
                        conn,
                        &session_id,
                        metadata.replay_available,
                        last_checkpoint.as_ref(),
                    )?;
                    Ok(SessionSnapshot {
                        metadata,
                        messages,
                        last_checkpoint,
                        replay_cursor,
                    })
                })
            })
            .await?;

        if snapshot.metadata.is_expired_at(now_ms()) {
            let _ = self.delete(id).await;
            return Err(SessionError::expired(
                id.clone(),
                snapshot.metadata.expires_at_ms,
            ));
        }

        Ok(snapshot)
    }

    async fn append_messages(
        &self,
        id: &SessionId,
        messages: &[Message],
    ) -> Result<(), SessionError> {
        if messages.is_empty() {
            return Ok(());
        }
        let session_id = id.clone();
        let encoded_messages = encode_items(messages, "append_messages", Some(id))?;

        let _write_guard = self.write_gate.lock().await;
        let _ = self.load_metadata_only(id).await?;
        self.with_sqlite_retry("append_messages", Some(id), || {
            let session_id = session_id.clone();
            let encoded_messages = encoded_messages.clone();
            self.writer.call(move |conn| {
                let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let Some(mut metadata) = load_session_metadata(&transaction, &session_id)? else {
                    return Err(rusqlite::Error::QueryReturnedNoRows);
                };
                let start_ordinal = i64::try_from(metadata.message_count)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
                for (offset, payload) in encoded_messages.iter().enumerate() {
                    let ordinal = start_ordinal
                        + i64::try_from(offset).map_err(|error| {
                            rusqlite::Error::ToSqlConversionFailure(Box::new(error))
                        })?;
                    transaction.execute(
                        "INSERT INTO messages (session_id, ordinal, payload) VALUES (?1, ?2, ?3)",
                        params![session_id.to_string(), ordinal, payload],
                    )?;
                }
                metadata.message_count = metadata
                    .message_count
                    .saturating_add(encoded_messages.len());
                metadata.updated_at_ms = now_ms();
                update_session_row(&transaction, &metadata)?;
                transaction.commit()?;
                Ok(())
            })
        })
        .await
    }

    async fn append_events(
        &self,
        id: &SessionId,
        events: &[PersistedEvent],
    ) -> Result<(), SessionError> {
        if events.is_empty() {
            return Ok(());
        }
        validate_event_batch(id, events, "append_events")?;
        let session_id = id.clone();
        let encoded_events = events
            .iter()
            .map(|event| {
                serde_json::to_string(event).map(|payload| EncodedEvent {
                    sequence: event.sequence,
                    recorded_at_ms: event.recorded_at_ms,
                    payload,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                SessionError::storage_failure(
                    format!("failed to serialize replay events: {error}"),
                    Some(id.clone()),
                    "append_events",
                )
            })?;

        let _write_guard = self.write_gate.lock().await;
        let _ = self.load_metadata_only(id).await?;
        self.with_sqlite_retry("append_events", Some(id), || {
            let session_id = session_id.clone();
            let encoded_events = encoded_events.clone();
            self.writer.call(move |conn| {
                let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let Some(mut metadata) = load_session_metadata(&transaction, &session_id)? else {
                    return Err(rusqlite::Error::QueryReturnedNoRows);
                };
                let last_sequence = metadata.last_sequence.unwrap_or(0);
                if encoded_events[0].sequence <= last_sequence {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                for event in &encoded_events {
                    transaction.execute(
                        "INSERT INTO events (session_id, sequence, recorded_at_ms, payload)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![
                            session_id.to_string(),
                            i64::try_from(event.sequence).map_err(|error| {
                                rusqlite::Error::ToSqlConversionFailure(Box::new(error))
                            })?,
                            i64::try_from(event.recorded_at_ms).map_err(|error| {
                                rusqlite::Error::ToSqlConversionFailure(Box::new(error))
                            })?,
                            event.payload,
                        ],
                    )?;
                }
                metadata.event_count = metadata.event_count.saturating_add(encoded_events.len());
                metadata.last_sequence = Some(encoded_events[encoded_events.len() - 1].sequence);
                metadata.updated_at_ms = now_ms();
                update_session_row(&transaction, &metadata)?;
                transaction.commit()?;
                Ok(())
            })
        })
        .await
    }

    async fn save_turn_checkpoint(
        &self,
        id: &SessionId,
        checkpoint: TurnCheckpoint,
    ) -> Result<(), SessionError> {
        let session_id = id.clone();
        let encoded_checkpoint = serde_json::to_string(&checkpoint).map_err(|error| {
            SessionError::storage_failure(
                format!("failed to serialize turn checkpoint: {error}"),
                Some(id.clone()),
                "save_turn_checkpoint",
            )
        })?;

        let _write_guard = self.write_gate.lock().await;
        let _ = self.load_metadata_only(id).await?;
        self.with_sqlite_retry("save_turn_checkpoint", Some(id), || {
            let session_id = session_id.clone();
            let checkpoint = checkpoint.clone();
            let encoded_checkpoint = encoded_checkpoint.clone();
            self.writer.call(move |conn| {
                let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let Some(mut metadata) = load_session_metadata(&transaction, &session_id)? else {
                    return Err(rusqlite::Error::QueryReturnedNoRows);
                };
                transaction.execute(
                    "INSERT INTO checkpoints (session_id, sequence, payload)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(session_id)
                     DO UPDATE SET sequence = excluded.sequence, payload = excluded.payload",
                    params![
                        session_id.to_string(),
                        i64::try_from(checkpoint.sequence).map_err(|error| {
                            rusqlite::Error::ToSqlConversionFailure(Box::new(error))
                        })?,
                        encoded_checkpoint,
                    ],
                )?;
                metadata.apply_checkpoint(&checkpoint, now_ms());
                update_session_row(&transaction, &metadata)?;
                transaction.commit()?;
                Ok(())
            })
        })
        .await
    }

    async fn replay_events(
        &self,
        id: &SessionId,
        after_sequence: Option<u64>,
        limit: Option<usize>,
    ) -> Result<Vec<PersistedEvent>, SessionError> {
        let session_id = id.clone();
        let metadata = self.load_metadata_only(id).await?;
        if !metadata.replay_available {
            return Err(SessionError::replay_unavailable(
                id.clone(),
                "sqlite replay persistence is disabled",
            ));
        }

        self.with_sqlite_retry("replay_events", Some(id), || {
            let session_id = session_id.clone();
            self.reader
                .call(move |conn| load_events(conn, &session_id, after_sequence, limit))
        })
        .await
    }

    async fn list(&self, filter: SessionFilter) -> Result<Vec<SessionMetadata>, SessionError> {
        let filter_copy = filter.clone();
        let mut sessions = self
            .with_sqlite_retry("list", None, || {
                let filter = filter_copy.clone();
                self.reader.call(move |conn| list_sessions(conn, &filter))
            })
            .await?;

        let expired_ids = sessions
            .iter()
            .filter(|metadata| metadata.is_expired_at(now_ms()))
            .map(|metadata| metadata.id.clone())
            .collect::<Vec<_>>();
        for expired_id in expired_ids {
            let _ = self.delete(&expired_id).await;
        }
        sessions.retain(|metadata| !metadata.is_expired_at(now_ms()));
        Ok(sessions)
    }

    async fn delete(&self, id: &SessionId) -> Result<(), SessionError> {
        let _write_guard = self.write_gate.lock().await;
        let session_id = id.clone();
        self.with_sqlite_retry("delete", Some(id), || {
            let session_id = session_id.clone();
            self.writer.call(move |conn| {
                let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let deleted = transaction.execute(
                    "DELETE FROM sessions WHERE id = ?1",
                    params![session_id.to_string()],
                )?;
                if deleted == 0 {
                    return Err(rusqlite::Error::QueryReturnedNoRows);
                }
                transaction.commit()?;
                Ok(())
            })
        })
        .await
    }
}

#[derive(Debug, Clone)]
struct EncodedEvent {
    sequence: u64,
    recorded_at_ms: u64,
    payload: String,
}

fn map_sqlite_error(
    operation: &str,
    session_id: Option<SessionId>,
    error: &TokioRusqliteError<rusqlite::Error>,
    retryable: bool,
    retry_after: Option<Duration>,
) -> SessionError {
    let message = match &error {
        TokioRusqliteError::ConnectionClosed => {
            format!("SQLite connection closed during `{operation}`")
        }
        TokioRusqliteError::Close((_, inner)) => {
            format!("SQLite close failure during `{operation}`: {inner}")
        }
        TokioRusqliteError::Error(inner) => {
            format!("SQLite `{operation}` failed: {inner}")
        }
        _ => format!("SQLite `{operation}` failed with a non-exhaustive driver error"),
    };

    if retryable {
        SessionError::retryable_storage_failure(message, session_id, operation, retry_after)
    } else {
        SessionError::storage_failure(message, session_id, operation)
    }
}

const fn is_query_returned_no_rows(error: &TokioRusqliteError<rusqlite::Error>) -> bool {
    matches!(
        error,
        TokioRusqliteError::Error(rusqlite::Error::QueryReturnedNoRows)
            | TokioRusqliteError::Close((_, rusqlite::Error::QueryReturnedNoRows))
    )
}

fn is_busy_or_locked(error: &TokioRusqliteError<rusqlite::Error>) -> bool {
    match error {
        TokioRusqliteError::Error(inner) => matches!(
            inner.sqlite_error_code(),
            Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
        ),
        TokioRusqliteError::Close((_, inner)) => matches!(
            inner.sqlite_error_code(),
            Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
        ),
        _ => false,
    }
}

async fn configure_connection(
    connection: &Connection,
    config: SqliteSessionStoreConfig,
) -> Result<(), SessionError> {
    let busy_timeout = config.busy_timeout;
    let journal_mode = connection
        .call(move |conn| {
            conn.busy_timeout(busy_timeout)?;
            conn.execute_batch(
                "
                PRAGMA foreign_keys = ON;
                PRAGMA synchronous = NORMAL;
                PRAGMA wal_autocheckpoint = 1000;
                ",
            )?;
            conn.query_row("PRAGMA journal_mode = WAL", [], |row| {
                row.get::<_, String>(0)
            })
        })
        .await
        .map_err(|error| map_sqlite_error("configure_connection", None, &error, false, None))?;

    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(SessionError::storage_failure(
            format!("expected SQLite WAL mode, got `{journal_mode}`"),
            None,
            "configure_connection",
        ));
    }

    Ok(())
}

async fn initialize_schema(connection: &Connection) -> Result<(), SessionError> {
    connection
        .call(|conn| {
            conn.execute_batch(
                "
                CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    message_count INTEGER NOT NULL,
                    event_count INTEGER NOT NULL,
                    last_sequence INTEGER,
                    model_id TEXT,
                    provider_id TEXT,
                    provider_session_id TEXT,
                    replay_available INTEGER NOT NULL,
                    expires_at_ms INTEGER
                );
                CREATE TABLE IF NOT EXISTS session_labels (
                    session_id TEXT NOT NULL,
                    label_key TEXT NOT NULL,
                    label_value TEXT NOT NULL,
                    PRIMARY KEY (session_id, label_key),
                    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_sessions_updated_at
                    ON sessions(updated_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_session_labels_lookup
                    ON session_labels(label_key, label_value, session_id);
                CREATE TABLE IF NOT EXISTS messages (
                    session_id TEXT NOT NULL,
                    ordinal INTEGER NOT NULL,
                    payload TEXT NOT NULL,
                    PRIMARY KEY (session_id, ordinal),
                    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
                );
                CREATE TABLE IF NOT EXISTS events (
                    session_id TEXT NOT NULL,
                    sequence INTEGER NOT NULL,
                    recorded_at_ms INTEGER NOT NULL,
                    payload TEXT NOT NULL,
                    PRIMARY KEY (session_id, sequence),
                    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
                );
                CREATE TABLE IF NOT EXISTS checkpoints (
                    session_id TEXT PRIMARY KEY,
                    sequence INTEGER NOT NULL,
                    payload TEXT NOT NULL,
                    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
                );
                ",
            )
        })
        .await
        .map_err(|error| map_sqlite_error("initialize_schema", None, &error, false, None))?;

    Ok(())
}

fn insert_session(
    connection: &rusqlite::Transaction<'_>,
    metadata: &SessionMetadata,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO sessions (
            id,
            created_at_ms,
            updated_at_ms,
            message_count,
            event_count,
            last_sequence,
            model_id,
            provider_id,
            provider_session_id,
            replay_available,
            expires_at_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            metadata.id.to_string(),
            i64::try_from(metadata.created_at_ms)
                .map_err(|error| { rusqlite::Error::ToSqlConversionFailure(Box::new(error)) })?,
            i64::try_from(metadata.updated_at_ms)
                .map_err(|error| { rusqlite::Error::ToSqlConversionFailure(Box::new(error)) })?,
            i64::try_from(metadata.message_count)
                .map_err(|error| { rusqlite::Error::ToSqlConversionFailure(Box::new(error)) })?,
            i64::try_from(metadata.event_count)
                .map_err(|error| { rusqlite::Error::ToSqlConversionFailure(Box::new(error)) })?,
            metadata
                .last_sequence
                .map(i64::try_from)
                .transpose()
                .map_err(|error| { rusqlite::Error::ToSqlConversionFailure(Box::new(error)) })?,
            metadata.model_id,
            metadata.provider_id.as_ref().map(ToString::to_string),
            metadata.provider_session_id,
            i64::from(metadata.replay_available),
            metadata
                .expires_at_ms
                .map(i64::try_from)
                .transpose()
                .map_err(|error| { rusqlite::Error::ToSqlConversionFailure(Box::new(error)) })?,
        ],
    )?;

    for (key, value) in &metadata.labels {
        connection.execute(
            "INSERT INTO session_labels (session_id, label_key, label_value)
             VALUES (?1, ?2, ?3)",
            params![metadata.id.to_string(), key, value],
        )?;
    }

    Ok(())
}

fn update_session_row(
    connection: &rusqlite::Transaction<'_>,
    metadata: &SessionMetadata,
) -> rusqlite::Result<()> {
    connection.execute(
        "UPDATE sessions
         SET updated_at_ms = ?2,
             message_count = ?3,
             event_count = ?4,
             last_sequence = ?5,
             model_id = ?6,
             provider_id = ?7,
             provider_session_id = ?8,
             replay_available = ?9,
             expires_at_ms = ?10
         WHERE id = ?1",
        params![
            metadata.id.to_string(),
            i64::try_from(metadata.updated_at_ms)
                .map_err(|error| { rusqlite::Error::ToSqlConversionFailure(Box::new(error)) })?,
            i64::try_from(metadata.message_count)
                .map_err(|error| { rusqlite::Error::ToSqlConversionFailure(Box::new(error)) })?,
            i64::try_from(metadata.event_count)
                .map_err(|error| { rusqlite::Error::ToSqlConversionFailure(Box::new(error)) })?,
            metadata
                .last_sequence
                .map(i64::try_from)
                .transpose()
                .map_err(|error| { rusqlite::Error::ToSqlConversionFailure(Box::new(error)) })?,
            metadata.model_id,
            metadata.provider_id.as_ref().map(ToString::to_string),
            metadata.provider_session_id,
            i64::from(metadata.replay_available),
            metadata
                .expires_at_ms
                .map(i64::try_from)
                .transpose()
                .map_err(|error| { rusqlite::Error::ToSqlConversionFailure(Box::new(error)) })?,
        ],
    )?;

    connection.execute(
        "DELETE FROM session_labels WHERE session_id = ?1",
        params![metadata.id.to_string()],
    )?;
    for (key, value) in &metadata.labels {
        connection.execute(
            "INSERT INTO session_labels (session_id, label_key, label_value)
             VALUES (?1, ?2, ?3)",
            params![metadata.id.to_string(), key, value],
        )?;
    }

    Ok(())
}

fn load_session_metadata(
    connection: &rusqlite::Connection,
    session_id: &SessionId,
) -> rusqlite::Result<Option<SessionMetadata>> {
    let session_id_string = session_id.to_string();
    let row = connection
        .query_row(
            "SELECT
            id,
            created_at_ms,
            updated_at_ms,
            message_count,
            event_count,
            last_sequence,
            model_id,
            provider_id,
            provider_session_id,
            replay_available,
            expires_at_ms
         FROM sessions
         WHERE id = ?1",
            params![session_id_string],
            |row| {
                Ok(SessionMetadata {
                    id: SessionId::parse_str(&row.get::<_, String>(0)?).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?,
                    created_at_ms: as_u64(row.get::<_, i64>(1)?)?,
                    updated_at_ms: as_u64(row.get::<_, i64>(2)?)?,
                    message_count: as_usize(row.get::<_, i64>(3)?)?,
                    event_count: as_usize(row.get::<_, i64>(4)?)?,
                    last_sequence: row.get::<_, Option<i64>>(5)?.map(as_u64).transpose()?,
                    model_id: row.get(6)?,
                    provider_id: row.get::<_, Option<String>>(7)?.map(Into::into),
                    provider_session_id: row.get(8)?,
                    replay_available: row.get::<_, i64>(9)? != 0,
                    expires_at_ms: row.get::<_, Option<i64>>(10)?.map(as_u64).transpose()?,
                    labels: BTreeMap::new(),
                })
            },
        )
        .optional()?;

    let Some(mut metadata) = row else {
        return Ok(None);
    };

    let mut statement = connection.prepare(
        "SELECT label_key, label_value
         FROM session_labels
         WHERE session_id = ?1
         ORDER BY label_key ASC",
    )?;
    let label_rows = statement.query_map(params![session_id.to_string()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut labels = BTreeMap::new();
    for label_row in label_rows {
        let (key, value) = label_row?;
        labels.insert(key, value);
    }
    metadata.labels = labels;
    Ok(Some(metadata))
}

fn load_messages(
    connection: &rusqlite::Connection,
    session_id: &SessionId,
) -> rusqlite::Result<Vec<Message>> {
    let mut statement = connection.prepare(
        "SELECT payload
         FROM messages
         WHERE session_id = ?1
         ORDER BY ordinal ASC",
    )?;
    let rows = statement.query_map(params![session_id.to_string()], |row| {
        let payload = row.get::<_, String>(0)?;
        serde_json::from_str::<Message>(&payload).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
    })?;
    rows.collect()
}

fn load_events(
    connection: &rusqlite::Connection,
    session_id: &SessionId,
    after_sequence: Option<u64>,
    limit: Option<usize>,
) -> rusqlite::Result<Vec<PersistedEvent>> {
    let mut statement = connection.prepare(
        "SELECT payload
         FROM events
         WHERE session_id = ?1
           AND (?2 IS NULL OR sequence > ?2)
         ORDER BY sequence ASC
         LIMIT ?3",
    )?;
    let effective_limit = limit.unwrap_or(usize::MAX);
    let rows = statement.query_map(
        params![
            session_id.to_string(),
            after_sequence
                .map(i64::try_from)
                .transpose()
                .map_err(|error| { rusqlite::Error::ToSqlConversionFailure(Box::new(error)) })?,
            i64::try_from(effective_limit)
                .map_err(|error| { rusqlite::Error::ToSqlConversionFailure(Box::new(error)) })?,
        ],
        |row| {
            let payload = row.get::<_, String>(0)?;
            serde_json::from_str::<PersistedEvent>(&payload).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        },
    )?;
    rows.collect()
}

fn load_checkpoint(
    connection: &rusqlite::Connection,
    session_id: &SessionId,
) -> rusqlite::Result<Option<TurnCheckpoint>> {
    connection
        .query_row(
            "SELECT payload
         FROM checkpoints
         WHERE session_id = ?1",
            params![session_id.to_string()],
            |row| {
                let payload = row.get::<_, String>(0)?;
                serde_json::from_str::<TurnCheckpoint>(&payload).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })
            },
        )
        .optional()
}

fn load_replay_cursor(
    connection: &rusqlite::Connection,
    session_id: &SessionId,
    replay_available: bool,
    checkpoint: Option<&TurnCheckpoint>,
) -> rusqlite::Result<Option<ReplayCursor>> {
    if let Some(checkpoint) = checkpoint {
        return Ok(Some(ReplayCursor::from_checkpoint(checkpoint.sequence)));
    }
    if !replay_available {
        return Ok(None);
    }
    let first_sequence = connection.query_row(
        "SELECT MIN(sequence)
         FROM events
         WHERE session_id = ?1",
        params![session_id.to_string()],
        |row| row.get::<_, Option<i64>>(0),
    )?;
    first_sequence
        .map(as_u64)
        .transpose()
        .map(|sequence| sequence.map(ReplayCursor::new))
}

fn list_sessions(
    connection: &rusqlite::Connection,
    filter: &SessionFilter,
) -> rusqlite::Result<Vec<SessionMetadata>> {
    let mut sessions = if filter.label.is_some() {
        let (key, value) = filter.label.clone().expect("checked above");
        let mut statement = connection.prepare(
            "SELECT s.id
             FROM sessions AS s
             INNER JOIN session_labels AS l
                ON l.session_id = s.id
             WHERE l.label_key = ?1
               AND l.label_value = ?2
               AND (?3 IS NULL OR s.updated_at_ms >= ?3)
             ORDER BY s.updated_at_ms DESC, s.created_at_ms DESC",
        )?;
        let ids = statement.query_map(
            params![
                key,
                value,
                filter
                    .since
                    .map(i64::try_from)
                    .transpose()
                    .map_err(|error| {
                        rusqlite::Error::ToSqlConversionFailure(Box::new(error))
                    })?,
            ],
            |row| row.get::<_, String>(0),
        )?;
        let parsed_ids = ids
            .map(|row| {
                let raw_id = row?;
                SessionId::parse_str(&raw_id).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })
            })
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut values = Vec::with_capacity(parsed_ids.len());
        for session_id in parsed_ids {
            if let Some(metadata) = load_session_metadata(connection, &session_id)? {
                values.push(metadata);
            }
        }
        values
    } else {
        let mut statement = connection.prepare(
            "SELECT id
             FROM sessions
             WHERE (?1 IS NULL OR updated_at_ms >= ?1)
             ORDER BY updated_at_ms DESC, created_at_ms DESC",
        )?;
        let ids = statement.query_map(
            params![
                filter
                    .since
                    .map(i64::try_from)
                    .transpose()
                    .map_err(|error| {
                        rusqlite::Error::ToSqlConversionFailure(Box::new(error))
                    })?,
            ],
            |row| row.get::<_, String>(0),
        )?;
        let parsed_ids = ids
            .map(|row| {
                let raw_id = row?;
                SessionId::parse_str(&raw_id).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })
            })
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut values = Vec::with_capacity(parsed_ids.len());
        for session_id in parsed_ids {
            if let Some(metadata) = load_session_metadata(connection, &session_id)? {
                values.push(metadata);
            }
        }
        values
    };

    if let Some(limit) = filter.limit {
        sessions.truncate(limit);
    }
    Ok(sessions)
}

fn encode_items<T: serde::Serialize>(
    items: &[T],
    operation: &str,
    session_id: Option<&SessionId>,
) -> Result<Vec<String>, SessionError> {
    items
        .iter()
        .map(|item| {
            serde_json::to_string(item).map_err(|error| {
                SessionError::storage_failure(
                    format!("failed to serialize items for `{operation}`: {error}"),
                    session_id.cloned(),
                    operation,
                )
            })
        })
        .collect()
}

fn as_u64(value: i64) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

fn as_usize(value: i64) -> rusqlite::Result<usize> {
    usize::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}
