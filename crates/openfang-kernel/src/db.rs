//! Kernel-local database bootstrap helpers.
//!
//! This module owns the dual-database startup mechanics for `runtime.db` and
//! `compozy.db` without exposing raw SQLite connections outside the kernel
//! crate.

use crate::error::{KernelError, KernelResult};

use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Health status for the kernel-owned SQLite databases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DatabaseHealth {
    /// Whether `runtime.db` is open and can answer a simple query.
    pub runtime_db: bool,
    /// Whether `compozy.db` is open and can answer a simple query.
    pub compozy_db: bool,
}

impl DatabaseHealth {
    /// Returns `true` when both databases are healthy.
    pub fn is_healthy(self) -> bool {
        self.runtime_db && self.compozy_db
    }
}

/// Owns the runtime and Compozy SQLite handles during bootstrap and health checks.
pub(crate) struct DatabaseManager {
    runtime_db: Arc<Mutex<Connection>>,
    compozy_db: Arc<Mutex<Connection>>,
}

impl DatabaseManager {
    /// Build a manager from already-opened handles.
    pub(crate) fn from_handles(
        runtime_db: Arc<Mutex<Connection>>,
        compozy_db: Arc<Mutex<Connection>>,
    ) -> Self {
        Self {
            runtime_db,
            compozy_db,
        }
    }

    /// Open both SQLite databases in the required boot order.
    pub(crate) fn open(runtime_path: &Path, compozy_path: &Path) -> KernelResult<Self> {
        let runtime_db = Self::open_connection(runtime_path, "runtime.db")?;
        let compozy_db = Self::open_connection(compozy_path, "compozy.db")?;

        Ok(Self {
            runtime_db,
            compozy_db,
        })
    }

    /// Return the kernel-owned `runtime.db` connection handle.
    pub(crate) fn runtime_db(&self) -> Arc<Mutex<Connection>> {
        Arc::clone(&self.runtime_db)
    }

    /// Ensure the parent directory for a database path exists before opening SQLite.
    pub(crate) fn ensure_parent_directory(path: &Path, database_name: &str) -> KernelResult<()> {
        let Some(parent) = path.parent() else {
            return Err(KernelError::BootFailed(format!(
                "Failed to prepare {database_name} at {}: missing parent directory",
                path.display()
            )));
        };

        if parent.as_os_str().is_empty() {
            return Ok(());
        }

        std::fs::create_dir_all(parent).map_err(|error| {
            KernelError::BootFailed(format!(
                "Failed to prepare parent directory for {database_name} at {}: {error}",
                path.display()
            ))
        })
    }

    /// Return the kernel-owned `compozy.db` connection handle.
    pub(crate) fn compozy_db(&self) -> Arc<Mutex<Connection>> {
        Arc::clone(&self.compozy_db)
    }

    /// Run a simple readiness probe against both database handles.
    pub(crate) fn health(&self) -> DatabaseHealth {
        DatabaseHealth {
            runtime_db: Self::ping(&self.runtime_db),
            compozy_db: Self::ping(&self.compozy_db),
        }
    }

    /// Convert persistence-validation failures into database-specific boot errors.
    pub(crate) fn map_validation_error(
        error: String,
        runtime_path: &Path,
        compozy_path: &Path,
    ) -> KernelError {
        if error.contains("persistence.runtime_db") {
            return KernelError::BootFailed(format!(
                "Failed to prepare runtime.db at {}: {error}",
                runtime_path.display()
            ));
        }

        if error.contains("persistence.compozy_db") {
            return KernelError::BootFailed(format!(
                "Failed to prepare compozy.db at {}: {error}",
                compozy_path.display()
            ));
        }

        KernelError::BootFailed(error)
    }

    fn open_connection(path: &Path, database_name: &str) -> KernelResult<Arc<Mutex<Connection>>> {
        let connection = Connection::open(path).map_err(|error| {
            KernelError::BootFailed(format!(
                "Failed to open {database_name} at {}: {error}",
                path.display()
            ))
        })?;

        connection
            .execute_batch(
                "
                PRAGMA foreign_keys = ON;
                PRAGMA journal_mode = WAL;
                ",
            )
            .map_err(|error| {
                KernelError::BootFailed(format!(
                    "Failed to configure SQLite pragmas for {database_name} at {}: {error}",
                    path.display()
                ))
            })?;
        connection
            .busy_timeout(Duration::from_millis(5000))
            .map_err(|error| {
                KernelError::BootFailed(format!(
                    "Failed to configure busy_timeout for {database_name} at {}: {error}",
                    path.display()
                ))
            })?;

        Ok(Arc::new(Mutex::new(connection)))
    }

    fn ping(connection: &Arc<Mutex<Connection>>) -> bool {
        let Ok(connection) = connection.lock() else {
            return false;
        };

        connection
            .query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
            .map(|value| value == 1)
            .unwrap_or(false)
    }
}
