//! Typed `compozy.db` repositories for durable document versioning.

use openfang_types::artifact::{content_hash, ContentHash, ProvenanceKind, ProvenanceRef};
use openfang_types::doc::{
    DocId, DocListPage, DocListQuery, DocRecord, DocType, DocVersionId, DocVersionRecord, NewDoc,
    NewDocVersion,
};
use openfang_types::error::OpenFangError;
use rusqlite::{
    params, params_from_iter, types::Value as SqlValue, Connection, ErrorCode, OptionalExtension,
    TransactionBehavior,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};
use thiserror::Error;

const MAX_PAGE_SIZE: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DocCursor {
    created_at: String,
    doc_id: String,
}

/// Typed failures from the document repository layer.
#[derive(Debug, Error)]
pub enum DocStoreError {
    /// Failed to acquire the shared `compozy.db` connection lock.
    #[error("failed to acquire compozy.db connection lock: {0}")]
    ConnectionLock(String),
    /// SQLite returned an error for the requested operation.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// JSON serialization or deserialization failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// The requested document does not exist.
    #[error("doc '{doc_id}' was not found")]
    DocNotFound { doc_id: String },
    /// The requested document version does not exist.
    #[error("doc version '{doc_version_id}' was not found")]
    DocVersionNotFound { doc_version_id: String },
    /// A duplicate document identity was attempted.
    #[error("doc '{doc_id}' already exists")]
    DocAlreadyExists { doc_id: String },
    /// A duplicate document version identity was attempted.
    #[error("doc version '{doc_version_id}' already exists")]
    DocVersionAlreadyExists { doc_version_id: String },
    /// The persisted cursor could not be decoded.
    #[error("invalid doc cursor '{cursor}'")]
    InvalidCursor { cursor: String },
    /// A JSON field could not be parsed into the expected shape.
    #[error("invalid JSON in field '{field}': {message}")]
    InvalidJsonField {
        field: &'static str,
        message: String,
    },
    /// The stored metadata JSON was not an object.
    #[error("field '{field}' must contain a JSON object")]
    InvalidMetadataShape { field: &'static str },
    /// A stored provenance kind string was invalid.
    #[error("invalid doc provenance kind '{kind}'")]
    InvalidProvenanceKind { kind: String },
    /// The stored document current version pointer referenced a missing row.
    #[error("doc '{doc_id}' references missing current version '{current_version_id}'")]
    MissingCurrentVersion {
        doc_id: String,
        current_version_id: String,
    },
}

impl From<DocStoreError> for OpenFangError {
    fn from(error: DocStoreError) -> Self {
        OpenFangError::Memory(error.to_string())
    }
}

/// Repository for `doc` and `doc_version`.
///
/// Existing document versions are immutable. The public API intentionally
/// exposes no method that rewrites an existing version row.
///
/// ```compile_fail
/// use std::sync::{Arc, Mutex};
///
/// let conn = rusqlite::Connection::open_in_memory().unwrap();
/// let repo = openfang_memory::DocRepository::new(Arc::new(Mutex::new(conn)));
/// repo.update_version_content(
///     &openfang_types::doc::DocVersionId::new("doc_v1"),
///     serde_json::json!({ "body": "mutated" }),
/// );
/// ```
#[derive(Clone)]
pub struct DocRepository {
    conn: Arc<Mutex<Connection>>,
}

impl DocRepository {
    /// Create the repository from a shared `compozy.db` connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Create the stable document row and first immutable version in one
    /// transaction.
    pub fn create(&self, input: &NewDoc) -> Result<DocRecord, DocStoreError> {
        ensure_object_json("doc.metadata_json", &input.metadata)?;
        let metadata_json = serialize_json_field("doc.metadata_json", &input.metadata)?;

        let mut conn = lock_conn(&self.conn)?;
        let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        if let Err(error) = transaction.execute(
            "INSERT INTO doc (
                doc_id,
                type,
                current_version_id,
                metadata_json,
                created_at,
                updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![
                input.doc_id.as_ref(),
                input.type_name.as_ref(),
                input.doc_version_id.as_ref(),
                metadata_json,
                input.created_at,
            ],
        ) {
            return Err(map_insert_doc_error(error, input));
        }

        insert_doc_version(
            &transaction,
            &input.doc_id,
            &input.doc_version_id,
            &input.content,
            input.provenance.as_ref(),
            &input.created_at,
        )?;

        let doc = load_required_doc(&transaction, input.doc_id.as_ref())?;
        ensure_current_version_exists(&transaction, &doc)?;
        transaction.commit()?;

        Ok(doc)
    }

    /// Append a new immutable document version and advance the document head.
    pub fn append_version(
        &self,
        doc_id: &DocId,
        input: &NewDocVersion,
    ) -> Result<DocRecord, DocStoreError> {
        let mut conn = lock_conn(&self.conn)?;
        let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        load_required_doc(&transaction, doc_id.as_ref())?;

        insert_doc_version(
            &transaction,
            doc_id,
            &input.doc_version_id,
            &input.content,
            input.provenance.as_ref(),
            &input.created_at,
        )?;

        let rows = transaction.execute(
            "UPDATE doc
             SET current_version_id = ?1,
                 updated_at = ?2
             WHERE doc_id = ?3",
            params![
                input.doc_version_id.as_ref(),
                input.created_at,
                doc_id.as_ref(),
            ],
        )?;
        if rows == 0 {
            return Err(DocStoreError::DocNotFound {
                doc_id: doc_id.to_string(),
            });
        }

        let doc = load_required_doc(&transaction, doc_id.as_ref())?;
        ensure_current_version_exists(&transaction, &doc)?;
        transaction.commit()?;

        Ok(doc)
    }

    /// Load one document by ID.
    pub fn find_by_id(&self, doc_id: &DocId) -> Result<Option<DocRecord>, DocStoreError> {
        let conn = lock_conn(&self.conn)?;
        load_doc(&conn, doc_id.as_ref())
    }

    /// Load one document version by ID.
    pub fn find_version_by_id(
        &self,
        doc_version_id: &DocVersionId,
    ) -> Result<Option<DocVersionRecord>, DocStoreError> {
        let conn = lock_conn(&self.conn)?;
        load_doc_version(&conn, doc_version_id.as_ref())
    }

    /// Look up a document version by canonical content hash.
    pub fn find_version_by_hash(
        &self,
        hash: &ContentHash,
    ) -> Result<Option<DocVersionRecord>, DocStoreError> {
        let conn = lock_conn(&self.conn)?;
        let version_id = conn
            .query_row(
                "SELECT doc_version_id
                 FROM doc_version
                 WHERE content_hash = ?1
                 ORDER BY doc_version_id ASC
                 LIMIT 1",
                [hash.as_ref()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        version_id
            .map(|version_id| load_required_doc_version(&conn, &version_id))
            .transpose()
    }

    /// List all immutable versions for one document in ascending `version_no`
    /// order.
    pub fn list_versions(&self, doc_id: &DocId) -> Result<Vec<DocVersionRecord>, DocStoreError> {
        let conn = lock_conn(&self.conn)?;
        let mut stmt = conn.prepare(
            "SELECT
                doc_version_id,
                doc_id,
                version_no,
                content_json,
                content_hash,
                created_by_kind,
                created_by_ref,
                created_at
             FROM doc_version
             WHERE doc_id = ?1
             ORDER BY version_no ASC",
        )?;
        let mut rows = stmt.query([doc_id.as_ref()])?;
        collect_doc_version_rows(&mut rows)
    }

    /// List documents using cursor pagination and an optional type filter.
    pub fn list(&self, query: &DocListQuery) -> Result<DocListPage, DocStoreError> {
        let conn = lock_conn(&self.conn)?;
        list_docs(&conn, query)
    }
}

fn map_insert_doc_error(error: rusqlite::Error, input: &NewDoc) -> DocStoreError {
    if is_unique_constraint_for(&error, "doc.doc_id") || is_unique_constraint_for(&error, "doc_id")
    {
        return DocStoreError::DocAlreadyExists {
            doc_id: input.doc_id.to_string(),
        };
    }
    DocStoreError::Sqlite(error)
}

fn insert_doc_version(
    conn: &Connection,
    doc_id: &DocId,
    doc_version_id: &DocVersionId,
    content: &JsonValue,
    provenance: Option<&ProvenanceRef>,
    created_at: &str,
) -> Result<DocVersionRecord, DocStoreError> {
    let content_json = serialize_json_field("doc_version.content_json", content)?;
    let hash = content_hash(content);
    let (created_by_kind, created_by_ref) = provenance_parts(provenance);

    if let Err(error) = conn.execute(
        "INSERT INTO doc_version (
            doc_version_id,
            doc_id,
            version_no,
            content_json,
            content_hash,
            created_by_kind,
            created_by_ref,
            created_at
         )
         SELECT
            ?1,
            ?2,
            COALESCE(MAX(version_no), 0) + 1,
            ?3,
            ?4,
            ?5,
            ?6,
            ?7
         FROM doc_version
         WHERE doc_id = ?2",
        params![
            doc_version_id.as_ref(),
            doc_id.as_ref(),
            content_json,
            hash.as_ref(),
            created_by_kind,
            created_by_ref,
            created_at,
        ],
    ) {
        if is_unique_constraint_for(&error, "doc_version.doc_version_id")
            || is_unique_constraint_for(&error, "doc_version_id")
        {
            return Err(DocStoreError::DocVersionAlreadyExists {
                doc_version_id: doc_version_id.to_string(),
            });
        }
        return Err(DocStoreError::Sqlite(error));
    }

    load_required_doc_version(conn, doc_version_id.as_ref())
}

fn list_docs(conn: &Connection, query: &DocListQuery) -> Result<DocListPage, DocStoreError> {
    let limit = normalize_limit(query.limit);
    let mut clauses = Vec::new();
    let mut params = Vec::new();

    if let Some(doc_type) = query.doc_type.as_ref() {
        clauses.push("type = ?");
        params.push(SqlValue::from(doc_type.to_string()));
    }

    if let Some(cursor) = query.cursor.as_deref() {
        let cursor = decode_cursor(cursor)?;
        clauses.push("(created_at < ? OR (created_at = ? AND doc_id < ?))");
        params.push(SqlValue::from(cursor.created_at.clone()));
        params.push(SqlValue::from(cursor.created_at));
        params.push(SqlValue::from(cursor.doc_id));
    }

    let mut sql = String::from(
        "SELECT
            doc_id,
            type,
            current_version_id,
            metadata_json,
            created_at,
            updated_at
         FROM doc",
    );
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    sql.push_str(" ORDER BY created_at DESC, doc_id DESC LIMIT ?");
    params.push(SqlValue::from((limit + 1) as i64));

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    let mut items = collect_doc_rows(&mut rows)?;
    let next_cursor = if items.len() > limit {
        let _ = items.pop();
        items.last().map(encode_cursor).transpose()?
    } else {
        None
    };

    Ok(DocListPage { items, next_cursor })
}

fn load_doc(conn: &Connection, doc_id: &str) -> Result<Option<DocRecord>, DocStoreError> {
    let mut stmt = conn.prepare(
        "SELECT
            doc_id,
            type,
            current_version_id,
            metadata_json,
            created_at,
            updated_at
         FROM doc
         WHERE doc_id = ?1",
    )?;
    let mut rows = stmt.query([doc_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(read_doc_row(row)?)),
        None => Ok(None),
    }
}

fn load_required_doc(conn: &Connection, doc_id: &str) -> Result<DocRecord, DocStoreError> {
    load_doc(conn, doc_id)?.ok_or_else(|| DocStoreError::DocNotFound {
        doc_id: doc_id.to_string(),
    })
}

fn load_doc_version(
    conn: &Connection,
    doc_version_id: &str,
) -> Result<Option<DocVersionRecord>, DocStoreError> {
    let mut stmt = conn.prepare(
        "SELECT
            doc_version_id,
            doc_id,
            version_no,
            content_json,
            content_hash,
            created_by_kind,
            created_by_ref,
            created_at
         FROM doc_version
         WHERE doc_version_id = ?1",
    )?;
    let mut rows = stmt.query([doc_version_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(read_doc_version_row(row)?)),
        None => Ok(None),
    }
}

fn load_required_doc_version(
    conn: &Connection,
    doc_version_id: &str,
) -> Result<DocVersionRecord, DocStoreError> {
    load_doc_version(conn, doc_version_id)?.ok_or_else(|| DocStoreError::DocVersionNotFound {
        doc_version_id: doc_version_id.to_string(),
    })
}

fn ensure_current_version_exists(conn: &Connection, doc: &DocRecord) -> Result<(), DocStoreError> {
    let exists = conn
        .query_row(
            "SELECT doc_version_id
             FROM doc_version
             WHERE doc_version_id = ?1",
            [doc.current_version_id.as_ref()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .is_some();

    if exists {
        return Ok(());
    }

    Err(DocStoreError::MissingCurrentVersion {
        doc_id: doc.doc_id.to_string(),
        current_version_id: doc.current_version_id.to_string(),
    })
}

fn read_doc_row(row: &rusqlite::Row<'_>) -> Result<DocRecord, DocStoreError> {
    let metadata: JsonValue = parse_json_field("doc.metadata_json", &row.get::<_, String>(3)?)?;
    ensure_object_json("doc.metadata_json", &metadata)?;

    Ok(DocRecord {
        doc_id: DocId::from(row.get::<_, String>(0)?),
        type_name: DocType::from(row.get::<_, String>(1)?),
        current_version_id: DocVersionId::from(row.get::<_, String>(2)?),
        metadata,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn read_doc_version_row(row: &rusqlite::Row<'_>) -> Result<DocVersionRecord, DocStoreError> {
    let provenance_kind = row.get::<_, Option<String>>(5)?;
    let provenance_ref = row.get::<_, Option<String>>(6)?;

    let provenance = match (provenance_kind, provenance_ref) {
        (Some(kind), Some(ref_id)) => Some(ProvenanceRef {
            kind: ProvenanceKind::from_str(&kind)
                .map_err(|_| DocStoreError::InvalidProvenanceKind { kind })?,
            ref_id,
        }),
        _ => None,
    };

    Ok(DocVersionRecord {
        doc_version_id: DocVersionId::from(row.get::<_, String>(0)?),
        doc_id: DocId::from(row.get::<_, String>(1)?),
        version_no: row.get(2)?,
        content: parse_json_field("doc_version.content_json", &row.get::<_, String>(3)?)?,
        content_hash: ContentHash::from(row.get::<_, String>(4)?),
        provenance,
        created_at: row.get(7)?,
    })
}

fn collect_doc_rows(rows: &mut rusqlite::Rows<'_>) -> Result<Vec<DocRecord>, DocStoreError> {
    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        items.push(read_doc_row(row)?);
    }
    Ok(items)
}

fn collect_doc_version_rows(
    rows: &mut rusqlite::Rows<'_>,
) -> Result<Vec<DocVersionRecord>, DocStoreError> {
    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        items.push(read_doc_version_row(row)?);
    }
    Ok(items)
}

fn provenance_parts(provenance: Option<&ProvenanceRef>) -> (Option<&str>, Option<&str>) {
    match provenance {
        Some(provenance) => (
            Some(provenance.kind.as_str()),
            Some(provenance.ref_id.as_str()),
        ),
        None => (None, None),
    }
}

fn ensure_object_json(field: &'static str, value: &JsonValue) -> Result<(), DocStoreError> {
    if value.is_object() {
        return Ok(());
    }
    Err(DocStoreError::InvalidMetadataShape { field })
}

fn serialize_json_field<T: Serialize>(
    field: &'static str,
    value: &T,
) -> Result<String, DocStoreError> {
    serde_json::to_string(value).map_err(|error| DocStoreError::InvalidJsonField {
        field,
        message: error.to_string(),
    })
}

fn parse_json_field<T: for<'de> Deserialize<'de>>(
    field: &'static str,
    value: &str,
) -> Result<T, DocStoreError> {
    serde_json::from_str(value).map_err(|error| DocStoreError::InvalidJsonField {
        field,
        message: error.to_string(),
    })
}

fn normalize_limit(limit: usize) -> usize {
    limit.clamp(1, MAX_PAGE_SIZE)
}

fn encode_cursor(record: &DocRecord) -> Result<String, DocStoreError> {
    serde_json::to_string(&DocCursor {
        created_at: record.created_at.clone(),
        doc_id: record.doc_id.to_string(),
    })
    .map_err(Into::into)
}

fn decode_cursor(cursor: &str) -> Result<DocCursor, DocStoreError> {
    serde_json::from_str(cursor).map_err(|_| DocStoreError::InvalidCursor {
        cursor: cursor.to_string(),
    })
}

fn is_unique_constraint_for(error: &rusqlite::Error, target: &str) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(inner, Some(message))
            if matches!(inner.code, ErrorCode::ConstraintViolation)
                && message.contains(target)
    )
}

fn lock_conn<'a>(
    conn: &'a Arc<Mutex<Connection>>,
) -> Result<MutexGuard<'a, Connection>, DocStoreError> {
    conn.lock()
        .map_err(|error| DocStoreError::ConnectionLock(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rusqlite::Connection;
    use std::path::Path;
    use std::time::Duration;
    use tempfile::tempdir;

    fn configure_test_connection(conn: &Connection) {
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            ",
        )
        .expect("configure sqlite pragmas");
        conn.busy_timeout(Duration::from_millis(5_000))
            .expect("set busy timeout");
    }

    fn migrated_in_memory_connection() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().expect("open in-memory compozy.db");
        configure_test_connection(&conn);
        conn.execute_batch(crate::artifact::ARTIFACT_DOC_VERSIONING_MIGRATION_SQL)
            .expect("apply artifact/doc migration");
        Arc::new(Mutex::new(conn))
    }

    fn migrated_file_connection(path: &Path) -> Arc<Mutex<Connection>> {
        let conn = Connection::open(path).expect("open file-backed compozy.db");
        configure_test_connection(&conn);
        conn.execute_batch(crate::artifact::ARTIFACT_DOC_VERSIONING_MIGRATION_SQL)
            .expect("apply artifact/doc migration");
        Arc::new(Mutex::new(conn))
    }

    fn repository(conn: Arc<Mutex<Connection>>) -> DocRepository {
        DocRepository::new(conn)
    }

    fn sample_new_doc(doc_id: &str, version_id: &str, created_at: &str) -> NewDoc {
        NewDoc {
            doc_id: DocId::new(doc_id),
            doc_version_id: DocVersionId::new(version_id),
            type_name: DocType::new("brief"),
            metadata: serde_json::json!({
                "origin": "workflow",
            }),
            content: serde_json::json!({
                "summary": "Iteration zero",
            }),
            provenance: None,
            created_at: created_at.to_string(),
        }
    }

    #[test]
    fn doc_repository_create_should_store_null_provenance_when_absent() {
        let conn = migrated_in_memory_connection();
        let repo = repository(Arc::clone(&conn));
        let created = repo
            .create(&sample_new_doc(
                "doc_001",
                "doc_001_v1",
                "2026-03-25T10:00:00Z",
            ))
            .expect("create doc");
        let version = repo
            .find_version_by_id(&created.current_version_id)
            .expect("find version")
            .expect("version exists");
        let raw = {
            let guard = conn.lock().expect("lock sqlite connection");
            guard
                .query_row(
                    "SELECT created_by_kind, created_by_ref
                     FROM doc_version
                     WHERE doc_version_id = ?1",
                    [created.current_version_id.as_ref()],
                    |row| {
                        Ok((
                            row.get::<_, Option<String>>(0)?,
                            row.get::<_, Option<String>>(1)?,
                        ))
                    },
                )
                .expect("query raw provenance columns")
        };

        assert_eq!(version.provenance, None);
        assert_eq!(raw, (None, None));
    }

    #[test]
    fn doc_repository_append_version_should_preserve_dispatch_provenance() {
        let repo = repository(migrated_in_memory_connection());
        let created = repo
            .create(&sample_new_doc(
                "doc_002",
                "doc_002_v1",
                "2026-03-25T10:00:00Z",
            ))
            .expect("create doc");
        let updated = repo
            .append_version(
                &created.doc_id,
                &NewDocVersion {
                    doc_version_id: DocVersionId::new("doc_002_v2"),
                    content: serde_json::json!({
                        "summary": "Iteration one",
                    }),
                    provenance: Some(ProvenanceRef {
                        kind: ProvenanceKind::Dispatch,
                        ref_id: "dispatch_456".to_string(),
                    }),
                    created_at: "2026-03-25T10:01:00Z".to_string(),
                },
            )
            .expect("append doc version");
        let version = repo
            .find_version_by_id(&updated.current_version_id)
            .expect("find current version")
            .expect("current version exists");

        assert_eq!(version.version_no, 2);
        assert_eq!(
            version.provenance,
            Some(ProvenanceRef {
                kind: ProvenanceKind::Dispatch,
                ref_id: "dispatch_456".to_string(),
            })
        );
    }

    #[test]
    fn doc_repository_should_filter_and_paginate() {
        let repo = repository(migrated_in_memory_connection());
        let first = repo
            .create(&sample_new_doc(
                "doc_010",
                "doc_010_v1",
                "2026-03-25T10:00:00Z",
            ))
            .expect("create first doc");
        let mut second_input = sample_new_doc("doc_011", "doc_011_v1", "2026-03-25T10:01:00Z");
        second_input.type_name = DocType::new("research");
        repo.create(&second_input).expect("create second doc");
        let third = repo
            .create(&sample_new_doc(
                "doc_012",
                "doc_012_v1",
                "2026-03-25T10:02:00Z",
            ))
            .expect("create third doc");

        let first_page = repo
            .list(&DocListQuery {
                limit: 1,
                doc_type: Some(DocType::new("brief")),
                ..DocListQuery::default()
            })
            .expect("list first page");
        let second_page = repo
            .list(&DocListQuery {
                limit: 1,
                cursor: first_page.next_cursor.clone(),
                doc_type: Some(DocType::new("brief")),
            })
            .expect("list second page");

        assert_eq!(first_page.items, vec![third]);
        assert_eq!(second_page.items, vec![first]);
        assert_eq!(second_page.next_cursor, None);
    }

    #[test]
    fn doc_versions_should_round_trip_after_reopen_with_stable_hashes() {
        let dir = tempdir().expect("create temp dir");
        let db_path = dir.path().join("compozy.db");

        {
            let repo = repository(migrated_file_connection(&db_path));
            let created = repo
                .create(&sample_new_doc(
                    "doc_file",
                    "doc_file_v1",
                    "2026-03-25T10:00:00Z",
                ))
                .expect("create file-backed doc");
            repo.append_version(
                &created.doc_id,
                &NewDocVersion {
                    doc_version_id: DocVersionId::new("doc_file_v2"),
                    content: serde_json::json!({
                        "summary": "Iteration one",
                    }),
                    provenance: Some(ProvenanceRef {
                        kind: ProvenanceKind::Agent,
                        ref_id: "writer".to_string(),
                    }),
                    created_at: "2026-03-25T10:01:00Z".to_string(),
                },
            )
            .expect("append doc version");
        }

        let repo = repository(migrated_file_connection(&db_path));
        let versions = repo
            .list_versions(&DocId::new("doc_file"))
            .expect("list versions after reopen");

        assert_eq!(versions.len(), 2);
        assert_eq!(
            versions[0].content_hash,
            content_hash(&serde_json::json!({
                "summary": "Iteration zero",
            }))
        );
        assert_eq!(
            versions[1].content_hash,
            content_hash(&serde_json::json!({
                "summary": "Iteration one",
            }))
        );
    }

    #[test]
    fn doc_repository_find_version_by_hash_should_return_matching_version() {
        let repo = repository(migrated_in_memory_connection());
        let created = repo
            .create(&sample_new_doc(
                "doc_hash",
                "doc_hash_v1",
                "2026-03-25T10:00:00Z",
            ))
            .expect("create doc");
        let hash = content_hash(&serde_json::json!({
            "summary": "Iteration zero",
        }));

        let found = repo
            .find_version_by_hash(&hash)
            .expect("lookup by hash")
            .expect("version exists");

        assert_eq!(found.doc_version_id, created.current_version_id);
        assert_eq!(found.content_hash, hash);
    }
}
