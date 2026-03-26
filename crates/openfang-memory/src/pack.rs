//! Typed `compozy.db` repository for durable pack state.

use openfang_types::error::OpenFangError;
use openfang_types::pack::{PackObjectCounts, PackRecord, PackSourceKind};
use rusqlite::{params, Connection};
use std::sync::{Arc, Mutex, MutexGuard};
use thiserror::Error;

/// SQL for migration `0013_pack`.
pub const PACK_MIGRATION_SQL: &str = include_str!("../migrations/compozy/20260326_013_pack.sql");

/// Typed failures from the pack repository layer.
#[derive(Debug, Error)]
pub enum PackStoreError {
    /// Failed to acquire the shared `compozy.db` connection lock.
    #[error("failed to acquire compozy.db connection lock: {0}")]
    ConnectionLock(String),
    /// SQLite returned an error for the requested operation.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// JSON serialization or deserialization failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// The requested pack row does not exist.
    #[error("pack '{pack_id}' was not found")]
    PackNotFound { pack_id: String },
    /// A stored source-kind string was invalid.
    #[error("invalid pack source kind '{source_kind}'")]
    InvalidSourceKind { source_kind: String },
    /// A JSON column could not be parsed into the expected shape.
    #[error("invalid JSON in field '{field}': {message}")]
    InvalidJsonField {
        field: &'static str,
        message: String,
    },
}

impl From<PackStoreError> for OpenFangError {
    fn from(error: PackStoreError) -> Self {
        OpenFangError::Memory(error.to_string())
    }
}

/// Repository for durable `pack` rows.
#[derive(Clone)]
pub struct PackRepository {
    conn: Arc<Mutex<Connection>>,
}

impl PackRepository {
    /// Create the repository from a shared `compozy.db` connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert or replace one durable pack row.
    pub fn upsert(&self, record: &PackRecord) -> Result<PackRecord, PackStoreError> {
        let conn = lock_conn(&self.conn)?;
        conn.execute(
            "INSERT INTO pack (
                pack_id,
                name,
                version,
                source_kind,
                installed,
                managed,
                installed_at,
                updated_at,
                objects_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(pack_id) DO UPDATE SET
                name = excluded.name,
                version = excluded.version,
                source_kind = excluded.source_kind,
                installed = excluded.installed,
                managed = excluded.managed,
                installed_at = excluded.installed_at,
                updated_at = excluded.updated_at,
                objects_json = excluded.objects_json",
            params![
                record.pack_id,
                record.name,
                record.version,
                record.source_kind.as_str(),
                i64::from(record.installed),
                i64::from(record.managed),
                record.installed_at,
                record.updated_at,
                serde_json::to_string(&record.objects)?,
            ],
        )?;

        load_required_pack(&conn, &record.pack_id)
    }

    /// Load one durable pack row by ID.
    pub fn find_by_id(&self, pack_id: &str) -> Result<Option<PackRecord>, PackStoreError> {
        let conn = lock_conn(&self.conn)?;
        load_pack(&conn, pack_id)
    }

    /// List all durable pack rows ordered by pack ID.
    pub fn list(&self) -> Result<Vec<PackRecord>, PackStoreError> {
        let conn = lock_conn(&self.conn)?;
        let mut stmt = conn.prepare(
            "SELECT
                pack_id,
                name,
                version,
                source_kind,
                installed,
                managed,
                installed_at,
                updated_at,
                objects_json
             FROM pack
             ORDER BY pack_id ASC",
        )?;
        let mut rows = stmt.query([])?;
        let mut records = Vec::new();
        while let Some(row) = rows.next()? {
            records.push(read_pack_row(row)?);
        }
        Ok(records)
    }

    /// Delete one durable pack row by ID.
    pub fn delete(&self, pack_id: &str) -> Result<bool, PackStoreError> {
        let conn = lock_conn(&self.conn)?;
        Ok(conn.execute("DELETE FROM pack WHERE pack_id = ?1", [pack_id])? > 0)
    }
}

fn load_pack(conn: &Connection, pack_id: &str) -> Result<Option<PackRecord>, PackStoreError> {
    let mut stmt = conn.prepare(
        "SELECT
            pack_id,
            name,
            version,
            source_kind,
            installed,
            managed,
            installed_at,
            updated_at,
            objects_json
         FROM pack
         WHERE pack_id = ?1",
    )?;
    let mut rows = stmt.query([pack_id])?;
    match rows.next()? {
        Some(row) => read_pack_row(row).map(Some),
        None => Ok(None),
    }
}

fn load_required_pack(conn: &Connection, pack_id: &str) -> Result<PackRecord, PackStoreError> {
    load_pack(conn, pack_id)?.ok_or_else(|| PackStoreError::PackNotFound {
        pack_id: pack_id.to_string(),
    })
}

fn read_pack_row(row: &rusqlite::Row<'_>) -> Result<PackRecord, PackStoreError> {
    let source_kind = match row.get::<_, String>(3)?.as_str() {
        "bundled" => PackSourceKind::Bundled,
        "external" => PackSourceKind::External,
        other => {
            return Err(PackStoreError::InvalidSourceKind {
                source_kind: other.to_string(),
            });
        }
    };
    let objects_json = row.get::<_, String>(8)?;
    let objects = serde_json::from_str::<PackObjectCounts>(&objects_json).map_err(|error| {
        PackStoreError::InvalidJsonField {
            field: "objects_json",
            message: error.to_string(),
        }
    })?;

    Ok(PackRecord {
        pack_id: row.get(0)?,
        name: row.get(1)?,
        version: row.get(2)?,
        source_kind,
        installed: row.get::<_, u32>(4)?,
        managed: row.get::<_, u32>(5)?,
        installed_at: row.get(6)?,
        updated_at: row.get(7)?,
        objects,
    })
}

fn lock_conn<'a>(
    conn: &'a Arc<Mutex<Connection>>,
) -> Result<MutexGuard<'a, Connection>, PackStoreError> {
    conn.lock()
        .map_err(|error| PackStoreError::ConnectionLock(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{PackRepository, PACK_MIGRATION_SQL};
    use openfang_types::pack::{PackObjectCounts, PackRecord, PackSourceKind};
    use pretty_assertions::assert_eq;
    use rusqlite::Connection;
    use std::sync::{Arc, Mutex};

    fn repository() -> PackRepository {
        let conn = Connection::open_in_memory().expect("open in-memory compozy.db");
        conn.execute_batch(PACK_MIGRATION_SQL)
            .expect("pack migration should apply");
        PackRepository::new(Arc::new(Mutex::new(conn)))
    }

    fn record() -> PackRecord {
        PackRecord {
            pack_id: "sdlc".to_string(),
            name: "SDLC".to_string(),
            version: "1.2.0".to_string(),
            source_kind: PackSourceKind::Bundled,
            installed: 4,
            managed: 3,
            installed_at: "2026-03-26T12:00:00Z".to_string(),
            updated_at: "2026-03-26T12:00:00Z".to_string(),
            objects: PackObjectCounts {
                agents: 2,
                workflows: 1,
                triggers: 1,
                schedules: 0,
                templates: 0,
            },
        }
    }

    #[test]
    fn upsert_should_round_trip_pack_record() {
        let repository = repository();
        let expected = record();

        let stored = repository
            .upsert(&expected)
            .expect("pack upsert should work");
        let loaded = repository
            .find_by_id(&expected.pack_id)
            .expect("pack lookup should work");

        assert_eq!(stored, expected);
        assert_eq!(loaded, Some(expected));
    }

    #[test]
    fn delete_should_remove_existing_pack_record() {
        let repository = repository();
        let record = record();
        repository.upsert(&record).expect("pack upsert should work");

        let deleted = repository
            .delete(&record.pack_id)
            .expect("pack delete should work");
        let loaded = repository
            .find_by_id(&record.pack_id)
            .expect("pack lookup should work");

        assert!(deleted);
        assert_eq!(loaded, None);
    }
}
