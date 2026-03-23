//! Integration tests for the `SQLite` session store backend.
#![cfg(feature = "sqlite")]

use std::{collections::BTreeMap, time::Duration};

use arky_protocol::{AgentEvent, EventMetadata, Message, SessionId, TurnId};
use arky_session::{
    NewSession, PersistedEvent, ReplayCursor, SessionFilter, SessionStore, SqliteSessionStore,
};
use pretty_assertions::assert_eq;
use rusqlite::{params, Connection, OptionalExtension};
use tempfile::TempDir;

fn temp_db_path() -> (TempDir, std::path::PathBuf) {
    let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
    let db_path = temp_dir.path().join("sessions.sqlite3");
    (temp_dir, db_path)
}

fn persisted_event(session_id: &SessionId, sequence: u64) -> PersistedEvent {
    PersistedEvent::new(AgentEvent::AgentStart {
        meta: EventMetadata::new(sequence * 10, sequence).with_session_id(session_id.clone()),
    })
}

#[tokio::test]
async fn sqlite_should_support_full_session_lifecycle() {
    let (_temp_dir, db_path) = temp_db_path();
    let store = SqliteSessionStore::open(&db_path)
        .await
        .expect("sqlite store should open");
    let mut labels = BTreeMap::new();
    labels.insert("project".to_owned(), "arky".to_owned());

    let session_id = store
        .create(NewSession {
            model_id: Some("codex".to_owned()),
            labels,
            expires_at_ms: None,
        })
        .await
        .expect("session should be created");

    store
        .append_messages(
            &session_id,
            &[Message::user("hello"), Message::assistant("world")],
        )
        .await
        .expect("messages should append");
    store
        .append_events(
            &session_id,
            &[
                persisted_event(&session_id, 4),
                persisted_event(&session_id, 5),
            ],
        )
        .await
        .expect("events should append");
    let checkpoint = arky_protocol::TurnCheckpoint::new(TurnId::new(), 5)
        .with_message(Message::assistant("world"))
        .with_provider_id("codex".into())
        .with_provider_session_id("provider-session-1")
        .mark_completed(500);
    store
        .save_turn_checkpoint(&session_id, checkpoint.clone())
        .await
        .expect("checkpoint should save");

    let snapshot = store.load(&session_id).await.expect("session should load");
    assert_eq!(snapshot.metadata.message_count, 2);
    assert_eq!(snapshot.metadata.event_count, 2);
    assert_eq!(snapshot.last_checkpoint, Some(checkpoint.clone()));
    assert_eq!(
        snapshot.replay_cursor,
        Some(ReplayCursor::from_checkpoint(5))
    );
    assert_eq!(
        snapshot.metadata.provider_session_id,
        Some("provider-session-1".to_owned())
    );

    let sessions = store
        .list(SessionFilter {
            label: Some(("project".to_owned(), "arky".to_owned())),
            since: None,
            limit: Some(10),
        })
        .await
        .expect("sessions should list");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, session_id);

    let raw = Connection::open(&db_path).expect("raw sqlite connection should open");
    let event_count = raw
        .query_row(
            "SELECT COUNT(*) FROM events WHERE session_id = ?1",
            params![session_id.to_string()],
            |row| row.get::<_, i64>(0),
        )
        .expect("event count should load");
    assert_eq!(event_count, 2);

    store
        .delete(&session_id)
        .await
        .expect("session should delete");
    let missing = store
        .load(&session_id)
        .await
        .expect_err("deleted session should be missing");
    assert_eq!(
        arky_error::ClassifiedError::error_code(&missing),
        "SESSION_NOT_FOUND"
    );
}

#[tokio::test]
async fn sqlite_should_enable_wal_mode() {
    let (_temp_dir, db_path) = temp_db_path();
    let _store = SqliteSessionStore::open(&db_path)
        .await
        .expect("sqlite store should open");

    let raw = Connection::open(&db_path).expect("raw sqlite connection should open");
    let journal_mode = raw
        .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
        .expect("journal mode should load");

    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
}

#[tokio::test]
async fn sqlite_should_allow_reads_during_a_separate_write_transaction() {
    let (_temp_dir, db_path) = temp_db_path();
    let store = SqliteSessionStore::open(&db_path)
        .await
        .expect("sqlite store should open");
    let session_id = store
        .create(NewSession::default())
        .await
        .expect("session should be created");
    store
        .append_messages(&session_id, &[Message::user("baseline")])
        .await
        .expect("message should append");

    let raw = Connection::open(&db_path).expect("raw sqlite connection should open");
    raw.execute_batch("PRAGMA journal_mode = WAL; BEGIN IMMEDIATE;")
        .expect("write transaction should begin");
    raw.execute(
        "UPDATE sessions SET updated_at_ms = updated_at_ms + 1 WHERE id = ?1",
        params![session_id.to_string()],
    )
    .expect("update should succeed");

    let snapshot = store.load(&session_id).await.expect("reader should load");
    assert_eq!(snapshot.messages.len(), 1);
    assert_eq!(snapshot.messages[0], Message::user("baseline"));

    raw.execute_batch("ROLLBACK")
        .expect("write transaction should roll back");
}

#[tokio::test]
async fn sqlite_should_resume_with_checkpoint_cursor() {
    let (_temp_dir, db_path) = temp_db_path();
    let store = SqliteSessionStore::open(&db_path)
        .await
        .expect("sqlite store should open");
    let session_id = store
        .create(NewSession {
            model_id: Some("claude".to_owned()),
            labels: BTreeMap::new(),
            expires_at_ms: None,
        })
        .await
        .expect("session should be created");
    store
        .append_messages(&session_id, &[Message::user("remember this")])
        .await
        .expect("message should append");
    let checkpoint = arky_protocol::TurnCheckpoint::new(TurnId::new(), 12)
        .with_provider_id("claude".into())
        .with_provider_session_id("provider-session-12")
        .mark_completed(1_200);
    store
        .save_turn_checkpoint(&session_id, checkpoint.clone())
        .await
        .expect("checkpoint should save");

    let snapshot = store.load(&session_id).await.expect("session should load");
    assert_eq!(snapshot.last_checkpoint, Some(checkpoint));
    assert_eq!(
        snapshot.replay_cursor,
        Some(ReplayCursor::from_checkpoint(12))
    );
}

#[tokio::test]
async fn sqlite_should_load_replay_events_with_filters() {
    let (_temp_dir, db_path) = temp_db_path();
    let store = SqliteSessionStore::open(&db_path)
        .await
        .expect("sqlite store should open");
    let session_id = store
        .create(NewSession::default())
        .await
        .expect("session should be created");
    let events = vec![
        persisted_event(&session_id, 1),
        persisted_event(&session_id, 2),
        persisted_event(&session_id, 3),
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
async fn sqlite_should_wait_for_brief_write_lock_contention() {
    let (_temp_dir, db_path) = temp_db_path();
    let store = SqliteSessionStore::open(&db_path)
        .await
        .expect("sqlite store should open");
    let session_id = store
        .create(NewSession::default())
        .await
        .expect("session should be created");

    let raw = Connection::open(&db_path).expect("raw sqlite connection should open");
    raw.execute_batch("PRAGMA journal_mode = WAL; BEGIN IMMEDIATE;")
        .expect("write transaction should begin");
    raw.execute(
        "UPDATE sessions SET updated_at_ms = updated_at_ms WHERE id = ?1",
        params![session_id.to_string()],
    )
    .expect("update should succeed");

    let release_lock = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        raw.execute_batch("COMMIT")
            .expect("write transaction should commit");
    });

    store
        .append_messages(&session_id, &[Message::assistant("after lock")])
        .await
        .expect("append should succeed after contention");
    release_lock
        .join()
        .expect("lock release thread should join cleanly");

    let snapshot = store.load(&session_id).await.expect("session should load");
    assert_eq!(snapshot.messages, vec![Message::assistant("after lock")]);
}

#[tokio::test]
async fn sqlite_should_remove_deleted_checkpoint_row() {
    let (_temp_dir, db_path) = temp_db_path();
    let store = SqliteSessionStore::open(&db_path)
        .await
        .expect("sqlite store should open");
    let session_id = store
        .create(NewSession::default())
        .await
        .expect("session should be created");
    store
        .save_turn_checkpoint(
            &session_id,
            arky_protocol::TurnCheckpoint::new(TurnId::new(), 1),
        )
        .await
        .expect("checkpoint should save");
    store
        .delete(&session_id)
        .await
        .expect("session should delete");

    let raw = Connection::open(&db_path).expect("raw sqlite connection should open");
    let checkpoint = raw
        .query_row(
            "SELECT payload FROM checkpoints WHERE session_id = ?1",
            params![session_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .expect("checkpoint query should succeed");
    assert_eq!(checkpoint, None);
}
