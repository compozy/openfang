//! Typed `compozy.db` repositories for durable artifact versioning.

use openfang_types::artifact::{
    content_hash, ArtifactId, ArtifactListPage, ArtifactListQuery, ArtifactRecord, ArtifactType,
    ArtifactVersionId, ArtifactVersionRecord, ContentHash, NewArtifact, NewArtifactVersion,
    ProvenanceKind, ProvenanceRef,
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

/// SQL for migration `0012_artifact_doc_versioning`.
pub const ARTIFACT_DOC_VERSIONING_MIGRATION_SQL: &str =
    include_str!("../migrations/compozy/20260325_012_artifact_doc_versioning.sql");

const MAX_PAGE_SIZE: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ArtifactCursor {
    created_at: String,
    artifact_id: String,
}

/// Typed failures from the artifact repository layer.
#[derive(Debug, Error)]
pub enum ArtifactStoreError {
    /// Failed to acquire the shared `compozy.db` connection lock.
    #[error("failed to acquire compozy.db connection lock: {0}")]
    ConnectionLock(String),
    /// SQLite returned an error for the requested operation.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// JSON serialization or deserialization failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// The requested artifact does not exist.
    #[error("artifact '{artifact_id}' was not found")]
    ArtifactNotFound { artifact_id: String },
    /// The requested artifact version does not exist.
    #[error("artifact version '{artifact_version_id}' was not found")]
    ArtifactVersionNotFound { artifact_version_id: String },
    /// A duplicate artifact identity was attempted.
    #[error("artifact '{artifact_id}' already exists")]
    ArtifactAlreadyExists { artifact_id: String },
    /// A duplicate artifact version identity was attempted.
    #[error("artifact version '{artifact_version_id}' already exists")]
    ArtifactVersionAlreadyExists { artifact_version_id: String },
    /// The persisted cursor could not be decoded.
    #[error("invalid artifact cursor '{cursor}'")]
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
    #[error("invalid artifact provenance kind '{kind}'")]
    InvalidProvenanceKind { kind: String },
    /// The stored artifact current version pointer referenced a missing row.
    #[error("artifact '{artifact_id}' references missing current version '{current_version_id}'")]
    MissingCurrentVersion {
        artifact_id: String,
        current_version_id: String,
    },
}

impl From<ArtifactStoreError> for OpenFangError {
    fn from(error: ArtifactStoreError) -> Self {
        OpenFangError::Memory(error.to_string())
    }
}

/// Repository for `artifact` and `artifact_version`.
///
/// Existing artifact versions are immutable. The public API intentionally
/// exposes no method that rewrites an existing version row.
///
/// ```compile_fail
/// use std::sync::{Arc, Mutex};
///
/// let conn = rusqlite::Connection::open_in_memory().unwrap();
/// let repo = openfang_memory::ArtifactRepository::new(Arc::new(Mutex::new(conn)));
/// repo.update_version_content(
///     &openfang_types::artifact::ArtifactVersionId::new("artifact_v1"),
///     serde_json::json!({ "body": "mutated" }),
/// );
/// ```
#[derive(Clone)]
pub struct ArtifactRepository {
    conn: Arc<Mutex<Connection>>,
}

impl ArtifactRepository {
    /// Create the repository from a shared `compozy.db` connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Create the stable artifact row and first immutable version in one
    /// transaction.
    pub fn create(&self, input: &NewArtifact) -> Result<ArtifactRecord, ArtifactStoreError> {
        ensure_object_json("artifact.metadata_json", &input.metadata)?;
        let metadata_json = serialize_json_field("artifact.metadata_json", &input.metadata)?;

        let mut conn = lock_conn(&self.conn)?;
        let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        if let Err(error) = transaction.execute(
            "INSERT INTO artifact (
                artifact_id,
                type,
                current_version_id,
                metadata_json,
                created_at,
                updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![
                input.artifact_id.as_ref(),
                input.type_name.as_ref(),
                input.artifact_version_id.as_ref(),
                metadata_json,
                input.created_at,
            ],
        ) {
            return Err(map_insert_artifact_error(error, input));
        }

        insert_artifact_version(
            &transaction,
            &input.artifact_id,
            &input.artifact_version_id,
            &input.content,
            input.provenance.as_ref(),
            &input.created_at,
        )?;

        let artifact = load_required_artifact(&transaction, input.artifact_id.as_ref())?;
        ensure_current_version_exists(&transaction, &artifact)?;
        transaction.commit()?;

        Ok(artifact)
    }

    /// Append a new immutable artifact version and advance the artifact head.
    pub fn append_version(
        &self,
        artifact_id: &ArtifactId,
        input: &NewArtifactVersion,
    ) -> Result<ArtifactRecord, ArtifactStoreError> {
        let mut conn = lock_conn(&self.conn)?;
        let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        load_required_artifact(&transaction, artifact_id.as_ref())?;

        insert_artifact_version(
            &transaction,
            artifact_id,
            &input.artifact_version_id,
            &input.content,
            input.provenance.as_ref(),
            &input.created_at,
        )?;

        let rows = transaction.execute(
            "UPDATE artifact
             SET current_version_id = ?1,
                 updated_at = ?2
             WHERE artifact_id = ?3",
            params![
                input.artifact_version_id.as_ref(),
                input.created_at,
                artifact_id.as_ref(),
            ],
        )?;
        if rows == 0 {
            return Err(ArtifactStoreError::ArtifactNotFound {
                artifact_id: artifact_id.to_string(),
            });
        }

        let artifact = load_required_artifact(&transaction, artifact_id.as_ref())?;
        ensure_current_version_exists(&transaction, &artifact)?;
        transaction.commit()?;

        Ok(artifact)
    }

    /// Load one artifact by ID.
    pub fn find_by_id(
        &self,
        artifact_id: &ArtifactId,
    ) -> Result<Option<ArtifactRecord>, ArtifactStoreError> {
        let conn = lock_conn(&self.conn)?;
        load_artifact(&conn, artifact_id.as_ref())
    }

    /// Load one artifact version by ID.
    pub fn find_version_by_id(
        &self,
        artifact_version_id: &ArtifactVersionId,
    ) -> Result<Option<ArtifactVersionRecord>, ArtifactStoreError> {
        let conn = lock_conn(&self.conn)?;
        load_artifact_version(&conn, artifact_version_id.as_ref())
    }

    /// Look up an artifact version by canonical content hash.
    pub fn find_version_by_hash(
        &self,
        hash: &ContentHash,
    ) -> Result<Option<ArtifactVersionRecord>, ArtifactStoreError> {
        let conn = lock_conn(&self.conn)?;
        let version_id = conn
            .query_row(
                "SELECT artifact_version_id
                 FROM artifact_version
                 WHERE content_hash = ?1
                 ORDER BY artifact_version_id ASC
                 LIMIT 1",
                [hash.as_ref()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        version_id
            .map(|version_id| load_required_artifact_version(&conn, &version_id))
            .transpose()
    }

    /// List all immutable versions for one artifact in ascending `version_no`
    /// order.
    pub fn list_versions(
        &self,
        artifact_id: &ArtifactId,
    ) -> Result<Vec<ArtifactVersionRecord>, ArtifactStoreError> {
        let conn = lock_conn(&self.conn)?;
        let mut stmt = conn.prepare(
            "SELECT
                artifact_version_id,
                artifact_id,
                version_no,
                content_json,
                content_hash,
                created_by_kind,
                created_by_ref,
                created_at
             FROM artifact_version
             WHERE artifact_id = ?1
             ORDER BY version_no ASC",
        )?;
        let mut rows = stmt.query([artifact_id.as_ref()])?;
        collect_artifact_version_rows(&mut rows)
    }

    /// List artifacts using cursor pagination and an optional type filter.
    pub fn list(&self, query: &ArtifactListQuery) -> Result<ArtifactListPage, ArtifactStoreError> {
        let conn = lock_conn(&self.conn)?;
        list_artifacts(&conn, query)
    }
}

fn map_insert_artifact_error(error: rusqlite::Error, input: &NewArtifact) -> ArtifactStoreError {
    if is_unique_constraint_for(&error, "artifact.artifact_id")
        || is_unique_constraint_for(&error, "artifact_id")
    {
        return ArtifactStoreError::ArtifactAlreadyExists {
            artifact_id: input.artifact_id.to_string(),
        };
    }
    ArtifactStoreError::Sqlite(error)
}

fn insert_artifact_version(
    conn: &Connection,
    artifact_id: &ArtifactId,
    artifact_version_id: &ArtifactVersionId,
    content: &JsonValue,
    provenance: Option<&ProvenanceRef>,
    created_at: &str,
) -> Result<ArtifactVersionRecord, ArtifactStoreError> {
    let content_json = serialize_json_field("artifact_version.content_json", content)?;
    let hash = content_hash(content);
    let (created_by_kind, created_by_ref) = provenance_parts(provenance);

    if let Err(error) = conn.execute(
        "INSERT INTO artifact_version (
            artifact_version_id,
            artifact_id,
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
         FROM artifact_version
         WHERE artifact_id = ?2",
        params![
            artifact_version_id.as_ref(),
            artifact_id.as_ref(),
            content_json,
            hash.as_ref(),
            created_by_kind,
            created_by_ref,
            created_at,
        ],
    ) {
        if is_unique_constraint_for(&error, "artifact_version.artifact_version_id")
            || is_unique_constraint_for(&error, "artifact_version_id")
        {
            return Err(ArtifactStoreError::ArtifactVersionAlreadyExists {
                artifact_version_id: artifact_version_id.to_string(),
            });
        }
        return Err(ArtifactStoreError::Sqlite(error));
    }

    load_required_artifact_version(conn, artifact_version_id.as_ref())
}

fn list_artifacts(
    conn: &Connection,
    query: &ArtifactListQuery,
) -> Result<ArtifactListPage, ArtifactStoreError> {
    let limit = normalize_limit(query.limit);
    let mut clauses = Vec::new();
    let mut params = Vec::new();

    if let Some(artifact_type) = query.artifact_type.as_ref() {
        clauses.push("type = ?");
        params.push(SqlValue::from(artifact_type.to_string()));
    }

    if let Some(cursor) = query.cursor.as_deref() {
        let cursor = decode_cursor(cursor)?;
        clauses.push("(created_at < ? OR (created_at = ? AND artifact_id < ?))");
        params.push(SqlValue::from(cursor.created_at.clone()));
        params.push(SqlValue::from(cursor.created_at));
        params.push(SqlValue::from(cursor.artifact_id));
    }

    let mut sql = String::from(
        "SELECT
            artifact_id,
            type,
            current_version_id,
            metadata_json,
            created_at,
            updated_at
         FROM artifact",
    );
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    sql.push_str(" ORDER BY created_at DESC, artifact_id DESC LIMIT ?");
    params.push(SqlValue::from((limit + 1) as i64));

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    let mut items = collect_artifact_rows(&mut rows)?;
    let next_cursor = if items.len() > limit {
        let _ = items.pop();
        items.last().map(encode_cursor).transpose()?
    } else {
        None
    };

    Ok(ArtifactListPage { items, next_cursor })
}

fn load_artifact(
    conn: &Connection,
    artifact_id: &str,
) -> Result<Option<ArtifactRecord>, ArtifactStoreError> {
    let mut stmt = conn.prepare(
        "SELECT
            artifact_id,
            type,
            current_version_id,
            metadata_json,
            created_at,
            updated_at
         FROM artifact
         WHERE artifact_id = ?1",
    )?;
    let mut rows = stmt.query([artifact_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(read_artifact_row(row)?)),
        None => Ok(None),
    }
}

fn load_required_artifact(
    conn: &Connection,
    artifact_id: &str,
) -> Result<ArtifactRecord, ArtifactStoreError> {
    load_artifact(conn, artifact_id)?.ok_or_else(|| ArtifactStoreError::ArtifactNotFound {
        artifact_id: artifact_id.to_string(),
    })
}

fn load_artifact_version(
    conn: &Connection,
    artifact_version_id: &str,
) -> Result<Option<ArtifactVersionRecord>, ArtifactStoreError> {
    let mut stmt = conn.prepare(
        "SELECT
            artifact_version_id,
            artifact_id,
            version_no,
            content_json,
            content_hash,
            created_by_kind,
            created_by_ref,
            created_at
         FROM artifact_version
         WHERE artifact_version_id = ?1",
    )?;
    let mut rows = stmt.query([artifact_version_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(read_artifact_version_row(row)?)),
        None => Ok(None),
    }
}

fn load_required_artifact_version(
    conn: &Connection,
    artifact_version_id: &str,
) -> Result<ArtifactVersionRecord, ArtifactStoreError> {
    load_artifact_version(conn, artifact_version_id)?.ok_or_else(|| {
        ArtifactStoreError::ArtifactVersionNotFound {
            artifact_version_id: artifact_version_id.to_string(),
        }
    })
}

fn ensure_current_version_exists(
    conn: &Connection,
    artifact: &ArtifactRecord,
) -> Result<(), ArtifactStoreError> {
    let exists = conn
        .query_row(
            "SELECT artifact_version_id
             FROM artifact_version
             WHERE artifact_version_id = ?1",
            [artifact.current_version_id.as_ref()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .is_some();

    if exists {
        return Ok(());
    }

    Err(ArtifactStoreError::MissingCurrentVersion {
        artifact_id: artifact.artifact_id.to_string(),
        current_version_id: artifact.current_version_id.to_string(),
    })
}

fn read_artifact_row(row: &rusqlite::Row<'_>) -> Result<ArtifactRecord, ArtifactStoreError> {
    let metadata: JsonValue =
        parse_json_field("artifact.metadata_json", &row.get::<_, String>(3)?)?;
    ensure_object_json("artifact.metadata_json", &metadata)?;

    Ok(ArtifactRecord {
        artifact_id: ArtifactId::from(row.get::<_, String>(0)?),
        type_name: ArtifactType::from(row.get::<_, String>(1)?),
        current_version_id: ArtifactVersionId::from(row.get::<_, String>(2)?),
        metadata,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn read_artifact_version_row(
    row: &rusqlite::Row<'_>,
) -> Result<ArtifactVersionRecord, ArtifactStoreError> {
    let provenance_kind = row.get::<_, Option<String>>(5)?;
    let provenance_ref = row.get::<_, Option<String>>(6)?;

    let provenance = match (provenance_kind, provenance_ref) {
        (Some(kind), Some(ref_id)) => Some(ProvenanceRef {
            kind: ProvenanceKind::from_str(&kind)
                .map_err(|_| ArtifactStoreError::InvalidProvenanceKind { kind })?,
            ref_id,
        }),
        _ => None,
    };

    Ok(ArtifactVersionRecord {
        artifact_version_id: ArtifactVersionId::from(row.get::<_, String>(0)?),
        artifact_id: ArtifactId::from(row.get::<_, String>(1)?),
        version_no: row.get(2)?,
        content: parse_json_field("artifact_version.content_json", &row.get::<_, String>(3)?)?,
        content_hash: ContentHash::from(row.get::<_, String>(4)?),
        provenance,
        created_at: row.get(7)?,
    })
}

fn collect_artifact_rows(
    rows: &mut rusqlite::Rows<'_>,
) -> Result<Vec<ArtifactRecord>, ArtifactStoreError> {
    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        items.push(read_artifact_row(row)?);
    }
    Ok(items)
}

fn collect_artifact_version_rows(
    rows: &mut rusqlite::Rows<'_>,
) -> Result<Vec<ArtifactVersionRecord>, ArtifactStoreError> {
    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        items.push(read_artifact_version_row(row)?);
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

fn ensure_object_json(field: &'static str, value: &JsonValue) -> Result<(), ArtifactStoreError> {
    if value.is_object() {
        return Ok(());
    }
    Err(ArtifactStoreError::InvalidMetadataShape { field })
}

fn serialize_json_field<T: Serialize>(
    field: &'static str,
    value: &T,
) -> Result<String, ArtifactStoreError> {
    serde_json::to_string(value).map_err(|error| ArtifactStoreError::InvalidJsonField {
        field,
        message: error.to_string(),
    })
}

fn parse_json_field<T: for<'de> Deserialize<'de>>(
    field: &'static str,
    value: &str,
) -> Result<T, ArtifactStoreError> {
    serde_json::from_str(value).map_err(|error| ArtifactStoreError::InvalidJsonField {
        field,
        message: error.to_string(),
    })
}

fn normalize_limit(limit: usize) -> usize {
    limit.clamp(1, MAX_PAGE_SIZE)
}

fn encode_cursor(record: &ArtifactRecord) -> Result<String, ArtifactStoreError> {
    serde_json::to_string(&ArtifactCursor {
        created_at: record.created_at.clone(),
        artifact_id: record.artifact_id.to_string(),
    })
    .map_err(Into::into)
}

fn decode_cursor(cursor: &str) -> Result<ArtifactCursor, ArtifactStoreError> {
    serde_json::from_str(cursor).map_err(|_| ArtifactStoreError::InvalidCursor {
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
) -> Result<MutexGuard<'a, Connection>, ArtifactStoreError> {
    conn.lock()
        .map_err(|error| ArtifactStoreError::ConnectionLock(error.to_string()))
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
        conn.execute_batch(ARTIFACT_DOC_VERSIONING_MIGRATION_SQL)
            .expect("apply artifact/doc migration");
        Arc::new(Mutex::new(conn))
    }

    fn migrated_file_connection(path: &Path) -> Arc<Mutex<Connection>> {
        let conn = Connection::open(path).expect("open file-backed compozy.db");
        configure_test_connection(&conn);
        conn.execute_batch(ARTIFACT_DOC_VERSIONING_MIGRATION_SQL)
            .expect("apply artifact/doc migration");
        Arc::new(Mutex::new(conn))
    }

    fn repository(conn: Arc<Mutex<Connection>>) -> ArtifactRepository {
        ArtifactRepository::new(conn)
    }

    fn sample_new_artifact(artifact_id: &str, version_id: &str, created_at: &str) -> NewArtifact {
        NewArtifact {
            artifact_id: ArtifactId::new(artifact_id),
            artifact_version_id: ArtifactVersionId::new(version_id),
            type_name: ArtifactType::new("prd"),
            metadata: serde_json::json!({
                "origin": "workflow",
            }),
            content: serde_json::json!({
                "title": "Planning doc",
                "sections": ["overview"],
            }),
            provenance: Some(ProvenanceRef {
                kind: ProvenanceKind::Run,
                ref_id: "run_123".to_string(),
            }),
            created_at: created_at.to_string(),
        }
    }

    fn sample_new_version(version_id: &str, section: &str, created_at: &str) -> NewArtifactVersion {
        NewArtifactVersion {
            artifact_version_id: ArtifactVersionId::new(version_id),
            content: serde_json::json!({
                "title": "Planning doc",
                "sections": [section],
            }),
            provenance: Some(ProvenanceRef {
                kind: ProvenanceKind::Dispatch,
                ref_id: format!("dispatch_{section}"),
            }),
            created_at: created_at.to_string(),
        }
    }

    fn explain_details(conn: &Connection, sql: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .expect("prepare explain query plan");
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(3))
            .expect("run explain query plan");

        rows.map(|row| row.expect("detail row")).collect()
    }

    fn index_exists(conn: &Connection, index_name: &str) -> bool {
        conn.query_row(
            "SELECT name
             FROM sqlite_master
             WHERE type = 'index' AND name = ?1",
            [index_name],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .expect("query index existence")
        .is_some()
    }

    #[test]
    fn artifact_repository_create_should_create_head_and_first_version() {
        let repo = repository(migrated_in_memory_connection());
        let input = sample_new_artifact("artifact_001", "artifact_v1", "2026-03-25T10:00:00Z");

        let created = repo.create(&input).expect("create artifact");
        let version = repo
            .find_version_by_id(&created.current_version_id)
            .expect("find first version")
            .expect("first version exists");

        assert_eq!(created.artifact_id, input.artifact_id);
        assert_eq!(created.current_version_id, input.artifact_version_id);
        assert_eq!(created.created_at, input.created_at);
        assert_eq!(created.updated_at, input.created_at);
        assert_eq!(version.version_no, 1);
        assert_eq!(version.artifact_version_id, input.artifact_version_id);
        assert_eq!(version.artifact_id, input.artifact_id);
    }

    #[test]
    fn artifact_repository_append_version_should_advance_head_without_mutating_history() {
        let repo = repository(migrated_in_memory_connection());
        let created = repo
            .create(&sample_new_artifact(
                "artifact_002",
                "artifact_002_v1",
                "2026-03-25T10:00:00Z",
            ))
            .expect("create artifact");
        let artifact_id = created.artifact_id.clone();
        let version_1 = repo
            .find_version_by_id(&created.current_version_id)
            .expect("find v1")
            .expect("v1 exists");
        repo.append_version(
            &artifact_id,
            &sample_new_version("artifact_002_v2", "draft", "2026-03-25T10:01:00Z"),
        )
        .expect("append v2");

        let updated = repo
            .append_version(
                &artifact_id,
                &sample_new_version("artifact_002_v3", "final", "2026-03-25T10:02:00Z"),
            )
            .expect("append v3");
        let version_2 = repo
            .find_version_by_id(&ArtifactVersionId::new("artifact_002_v2"))
            .expect("find v2")
            .expect("v2 exists");
        let version_3 = repo
            .find_version_by_id(&updated.current_version_id)
            .expect("find v3")
            .expect("v3 exists");

        assert_eq!(version_1.version_no, 1);
        assert_eq!(
            version_1.content,
            serde_json::json!({
                "title": "Planning doc",
                "sections": ["overview"],
            })
        );
        assert_eq!(version_2.version_no, 2);
        assert_eq!(version_2.content["sections"], serde_json::json!(["draft"]));
        assert_eq!(version_3.version_no, 3);
        assert_eq!(version_3.content["sections"], serde_json::json!(["final"]));
        assert_eq!(
            updated.current_version_id,
            ArtifactVersionId::new("artifact_002_v3")
        );
        assert_eq!(updated.updated_at, "2026-03-25T10:02:00Z");
    }

    #[test]
    fn artifact_repository_find_version_by_hash_should_use_covering_index_and_resolve_rows() {
        let conn = migrated_in_memory_connection();
        let repo = repository(Arc::clone(&conn));
        let created = repo
            .create(&sample_new_artifact(
                "artifact_003",
                "artifact_003_v1",
                "2026-03-25T10:00:00Z",
            ))
            .expect("create artifact");
        let version = repo
            .find_version_by_id(&created.current_version_id)
            .expect("find version")
            .expect("version exists");
        let plan = {
            let guard = conn.lock().expect("lock sqlite connection");
            explain_details(
                &guard,
                "SELECT artifact_version_id
                 FROM artifact_version
                 WHERE content_hash = 'placeholder'
                 ORDER BY artifact_version_id ASC
                 LIMIT 1",
            )
        };

        let found = repo
            .find_version_by_hash(&version.content_hash)
            .expect("lookup by hash");
        let missing = repo
            .find_version_by_hash(&ContentHash::new("0".repeat(64)))
            .expect("lookup missing hash");

        assert_eq!(found, Some(version));
        assert_eq!(missing, None);
        assert!(plan.iter().any(|detail| {
            detail.contains("USING COVERING INDEX idx_artifact_version_content_hash")
        }));
    }

    #[test]
    fn artifact_repository_list_should_filter_and_paginate_by_created_at_desc() {
        let repo = repository(migrated_in_memory_connection());
        let first = repo
            .create(&sample_new_artifact(
                "artifact_010",
                "artifact_010_v1",
                "2026-03-25T10:00:00Z",
            ))
            .expect("create first");
        let mut second_input =
            sample_new_artifact("artifact_011", "artifact_011_v1", "2026-03-25T10:01:00Z");
        second_input.type_name = ArtifactType::new("brief");
        repo.create(&second_input).expect("create second");
        let third = repo
            .create(&sample_new_artifact(
                "artifact_012",
                "artifact_012_v1",
                "2026-03-25T10:02:00Z",
            ))
            .expect("create third");

        let first_page = repo
            .list(&ArtifactListQuery {
                limit: 1,
                artifact_type: Some(ArtifactType::new("prd")),
                ..ArtifactListQuery::default()
            })
            .expect("list first page");
        let second_page = repo
            .list(&ArtifactListQuery {
                limit: 1,
                cursor: first_page.next_cursor.clone(),
                artifact_type: Some(ArtifactType::new("prd")),
            })
            .expect("list second page");

        assert_eq!(first_page.items, vec![third]);
        assert_eq!(second_page.items, vec![first]);
        assert_eq!(second_page.next_cursor, None);
    }

    #[test]
    fn artifact_versions_should_round_trip_after_reopen_with_stable_content_json_bytes() {
        let dir = tempdir().expect("create temp dir");
        let db_path = dir.path().join("compozy.db");

        {
            let repo = repository(migrated_file_connection(&db_path));
            let created = repo
                .create(&sample_new_artifact(
                    "artifact_file",
                    "artifact_file_v1",
                    "2026-03-25T10:00:00Z",
                ))
                .expect("create file-backed artifact");
            let artifact_id = created.artifact_id.clone();
            repo.append_version(
                &artifact_id,
                &sample_new_version("artifact_file_v2", "draft", "2026-03-25T10:01:00Z"),
            )
            .expect("append v2");
            repo.append_version(
                &artifact_id,
                &sample_new_version("artifact_file_v3", "review", "2026-03-25T10:02:00Z"),
            )
            .expect("append v3");
            repo.append_version(
                &artifact_id,
                &sample_new_version("artifact_file_v4", "final", "2026-03-25T10:03:00Z"),
            )
            .expect("append v4");
        }

        let repo = repository(migrated_file_connection(&db_path));
        let versions = repo
            .list_versions(&ArtifactId::new("artifact_file"))
            .expect("list versions after reopen");

        let raw_json_rows = {
            let conn = Connection::open(&db_path).expect("open file-backed sqlite");
            let mut stmt = conn
                .prepare(
                    "SELECT content_json
                     FROM artifact_version
                     WHERE artifact_id = ?1
                     ORDER BY version_no ASC",
                )
                .expect("prepare raw content query");
            let rows = stmt
                .query_map(["artifact_file"], |row| row.get::<_, String>(0))
                .expect("query raw content");
            rows.map(|row| row.expect("raw json row"))
                .collect::<Vec<_>>()
        };

        assert_eq!(
            versions
                .iter()
                .map(|version| version.version_no)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(raw_json_rows.len(), 4);
        assert_eq!(
            raw_json_rows[0],
            serde_json::json!({
                "title": "Planning doc",
                "sections": ["overview"],
            })
            .to_string()
        );
        assert_eq!(
            raw_json_rows[3],
            serde_json::json!({
                "title": "Planning doc",
                "sections": ["final"],
            })
            .to_string()
        );
    }

    #[test]
    fn artifact_provenance_query_should_filter_specific_dispatch_ref() {
        let conn = migrated_in_memory_connection();
        let repo = repository(Arc::clone(&conn));
        let created = repo
            .create(&sample_new_artifact(
                "artifact_provenance",
                "artifact_provenance_v1",
                "2026-03-25T10:00:00Z",
            ))
            .expect("create artifact");
        let artifact_id = created.artifact_id.clone();
        repo.append_version(
            &artifact_id,
            &NewArtifactVersion {
                artifact_version_id: ArtifactVersionId::new("artifact_provenance_v2"),
                content: serde_json::json!({ "title": "Planning doc", "sections": ["draft"] }),
                provenance: Some(ProvenanceRef {
                    kind: ProvenanceKind::Dispatch,
                    ref_id: "dispatch_123".to_string(),
                }),
                created_at: "2026-03-25T10:01:00Z".to_string(),
            },
        )
        .expect("append v2");
        repo.append_version(
            &artifact_id,
            &NewArtifactVersion {
                artifact_version_id: ArtifactVersionId::new("artifact_provenance_v3"),
                content: serde_json::json!({ "title": "Planning doc", "sections": ["final"] }),
                provenance: Some(ProvenanceRef {
                    kind: ProvenanceKind::Dispatch,
                    ref_id: "dispatch_456".to_string(),
                }),
                created_at: "2026-03-25T10:02:00Z".to_string(),
            },
        )
        .expect("append v3");

        let matches = {
            let guard = conn.lock().expect("lock sqlite connection");
            let mut stmt = guard
                .prepare(
                    "SELECT artifact_version_id
                     FROM artifact_version
                     WHERE created_by_ref = ?1
                     ORDER BY version_no ASC",
                )
                .expect("prepare provenance query");
            let rows = stmt
                .query_map(["dispatch_456"], |row| row.get::<_, String>(0))
                .expect("query provenance rows");
            rows.map(|row| row.expect("artifact version row"))
                .collect::<Vec<_>>()
        };

        assert_eq!(matches, vec!["artifact_provenance_v3".to_string()]);
    }

    #[test]
    fn artifact_repository_should_produce_strictly_sequential_version_numbers() {
        let repo = repository(migrated_in_memory_connection());
        let created = repo
            .create(&sample_new_artifact(
                "artifact_seq",
                "artifact_seq_v1",
                "2026-03-25T10:00:00Z",
            ))
            .expect("create artifact");
        let artifact_id = created.artifact_id.clone();
        repo.append_version(
            &artifact_id,
            &sample_new_version("artifact_seq_v2", "draft", "2026-03-25T10:01:00Z"),
        )
        .expect("append v2");
        repo.append_version(
            &artifact_id,
            &sample_new_version("artifact_seq_v3", "final", "2026-03-25T10:02:00Z"),
        )
        .expect("append v3");

        let versions = repo
            .list_versions(&artifact_id)
            .expect("list versions after sequential appends");

        assert_eq!(
            versions
                .iter()
                .map(|version| version.version_no)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn artifact_content_hash_lookup_should_match_external_hash_computation() {
        let repo = repository(migrated_in_memory_connection());
        let created = repo
            .create(&sample_new_artifact(
                "artifact_hash",
                "artifact_hash_v1",
                "2026-03-25T10:00:00Z",
            ))
            .expect("create artifact");
        let expected_hash = content_hash(&serde_json::json!({
            "title": "Planning doc",
            "sections": ["overview"],
        }));

        let found = repo
            .find_version_by_hash(&expected_hash)
            .expect("lookup by external hash")
            .expect("version found");

        assert_eq!(found.artifact_version_id, created.current_version_id);
        assert_eq!(found.content_hash, expected_hash);
    }

    #[test]
    fn artifact_version_migration_should_create_retention_index_on_created_at() {
        let conn = migrated_in_memory_connection();
        let guard = conn.lock().expect("lock sqlite connection");
        let plan = explain_details(
            &guard,
            "SELECT artifact_version_id
             FROM artifact_version
             WHERE created_at < '2026-03-25T12:00:00Z'",
        );

        assert!(index_exists(&guard, "idx_artifact_version_created_at"));
        assert!(plan
            .iter()
            .any(|detail| detail.contains("idx_artifact_version_created_at")));
    }
}
