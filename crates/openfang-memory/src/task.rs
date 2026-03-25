//! Typed `compozy.db` repositories for durable task and subtask state.

use chrono::Utc;
use openfang_types::error::OpenFangError;
use openfang_types::task::{
    ActorKind, AssigneeRef, Complexity, OwnerRef, PlannedSubtask, Priority, SortOrder, SubtaskId,
    SubtaskKind, SubtaskListPage, SubtaskListQuery, SubtaskPatch, SubtaskRecord, SubtaskStatus,
    TaskId, TaskListPage, TaskListQuery, TaskRecord, TaskReplanEffects, TaskReplanOperation,
    TaskReplanRequest, TaskSource, TaskStatus,
};
use rusqlite::{
    params, params_from_iter, types::Value as SqlValue, Connection, ErrorCode, TransactionBehavior,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard};
use thiserror::Error;
use uuid::Uuid;

/// SQL for migration `0010_task_subtask`.
pub const TASK_SUBTASK_MIGRATION_SQL: &str =
    include_str!("../migrations/compozy/20260324_010_task_subtask.sql");

const RESERVED_TASK_METADATA_KEY: &str = "__compozy_task";
const LAST_REPLAN_METADATA_KEY: &str = "last_replan";

/// Shared `compozy.db` repositories for durable task-domain state.
#[derive(Clone)]
pub struct TaskStoreSet {
    connection: Arc<Mutex<Connection>>,
    /// Repository for durable tasks.
    pub task: TaskRepository,
    /// Repository for durable subtasks.
    pub subtask: SubtaskRepository,
}

impl TaskStoreSet {
    /// Create the full task repository set from a shared `compozy.db`
    /// connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            connection: Arc::clone(&conn),
            task: TaskRepository::new(Arc::clone(&conn)),
            subtask: SubtaskRepository::new(conn),
        }
    }

    /// Return the underlying `compozy.db` handle for health probes.
    pub fn connection(&self) -> Arc<Mutex<Connection>> {
        Arc::clone(&self.connection)
    }
}

/// Typed failures from the task/subtask repository layer.
#[derive(Debug, Error)]
pub enum TaskStoreError {
    /// Failed to acquire the shared `compozy.db` connection lock.
    #[error("failed to acquire compozy.db connection lock: {0}")]
    ConnectionLock(String),
    /// SQLite returned an error for the requested operation.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// JSON serialization or deserialization failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// The requested task does not exist.
    #[error("task '{task_id}' was not found")]
    TaskNotFound { task_id: String },
    /// The requested subtask does not exist.
    #[error("subtask '{subtask_id}' was not found")]
    SubtaskNotFound { subtask_id: String },
    /// The requested subtask exists but belongs to a different task.
    #[error(
        "subtask '{subtask_id}' does not belong to task '{task_id}' (found under task '{actual_task_id}')"
    )]
    SubtaskOutsideTask {
        subtask_id: String,
        task_id: String,
        actual_task_id: String,
    },
    /// A task with the same slug already exists.
    #[error("task slug '{slug}' already exists")]
    DuplicateSlug { slug: String },
    /// A task with the same identifier already exists.
    #[error("task '{task_id}' already exists")]
    TaskAlreadyExists { task_id: String },
    /// A subtask with the same identifier already exists.
    #[error("subtask '{subtask_id}' already exists")]
    SubtaskAlreadyExists { subtask_id: String },
    /// A stored task status string was invalid.
    #[error("invalid task status '{status}'")]
    InvalidTaskStatus { status: String },
    /// A stored subtask status string was invalid.
    #[error("invalid subtask status '{status}'")]
    InvalidSubtaskStatus { status: String },
    /// A stored subtask kind string was invalid.
    #[error("invalid subtask kind '{kind}'")]
    InvalidSubtaskKind { kind: String },
    /// A stored priority string was invalid.
    #[error("invalid priority '{priority}'")]
    InvalidPriority { priority: String },
    /// A stored complexity string was invalid.
    #[error("invalid complexity '{complexity}'")]
    InvalidComplexity { complexity: String },
    /// A stored actor kind string was invalid.
    #[error("invalid actor kind '{kind}'")]
    InvalidActorKind { kind: String },
    /// The list cursor was malformed.
    #[error("invalid task cursor '{cursor}'")]
    InvalidCursor { cursor: String },
    /// A JSON column could not be parsed into the expected shape.
    #[error("invalid JSON in field '{field}': {message}")]
    InvalidJsonField {
        field: &'static str,
        message: String,
    },
    /// A metadata payload used an invalid top-level JSON shape.
    #[error("field '{field}' must be a JSON object")]
    InvalidMetadataShape { field: &'static str },
    /// The metadata payload attempted to overwrite an internal reserved key.
    #[error("field '{field}' may not contain reserved key '{key}'")]
    ReservedMetadataKey {
        field: &'static str,
        key: &'static str,
    },
    /// The stored task source metadata was malformed or inconsistent.
    #[error("task '{task_id}' has invalid source metadata: {reason}")]
    InvalidSourceEncoding { task_id: String, reason: String },
    /// A subtask depends on itself, which is never valid.
    #[error("subtask '{subtask_id}' may not depend on itself")]
    SelfDependency { subtask_id: String },
    /// A referenced dependency does not exist.
    #[error("subtask '{subtask_id}' depends on unknown subtask '{dependency_id}'")]
    DependencyNotFound {
        subtask_id: String,
        dependency_id: String,
    },
    /// A dependency belongs to a different task than the subtask being written.
    #[error(
        "subtask '{subtask_id}' for task '{task_id}' depends on subtask '{dependency_id}' from task '{dependency_task_id}'"
    )]
    DependencyOutsideTask {
        subtask_id: String,
        task_id: String,
        dependency_id: String,
        dependency_task_id: String,
    },
}

impl From<TaskStoreError> for OpenFangError {
    fn from(error: TaskStoreError) -> Self {
        OpenFangError::Memory(error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TaskCursor {
    position: i64,
    task_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SubtaskCursor {
    position: i64,
    task_id: String,
    subtask_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoredTaskMetadata {
    source: TaskSource,
}

/// Repository for `task`.
#[derive(Clone)]
pub struct TaskRepository {
    conn: Arc<Mutex<Connection>>,
}

/// Repository for `subtask`.
#[derive(Clone)]
pub struct SubtaskRepository {
    conn: Arc<Mutex<Connection>>,
}

impl TaskRepository {
    /// Create the repository from a shared `compozy.db` connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert a durable task row.
    pub fn create(&self, record: &TaskRecord) -> Result<TaskRecord, TaskStoreError> {
        let conn = lock_conn(&self.conn)?;
        insert_task(&conn, record)?;
        Ok(record.clone())
    }

    /// Load one durable task by ID.
    pub fn find_by_id(&self, task_id: &TaskId) -> Result<Option<TaskRecord>, TaskStoreError> {
        let conn = lock_conn(&self.conn)?;
        load_task(&conn, task_id.as_ref())
    }

    /// Load one durable task by slug.
    pub fn find_by_slug(&self, slug: &str) -> Result<Option<TaskRecord>, TaskStoreError> {
        let conn = lock_conn(&self.conn)?;
        load_task_by_slug(&conn, slug)
    }

    /// List tasks using cursor pagination and SQL-backed filters.
    pub fn list(&self, query: &TaskListQuery) -> Result<TaskListPage, TaskStoreError> {
        let conn = lock_conn(&self.conn)?;
        list_tasks(&conn, query)
    }

    /// List tasks produced by one workflow run.
    pub fn find_by_source_run_id(
        &self,
        source_run_id: &str,
    ) -> Result<Vec<TaskRecord>, TaskStoreError> {
        let conn = lock_conn(&self.conn)?;
        let mut stmt = conn.prepare(
            "SELECT
                task_id,
                slug,
                source_run_id,
                title,
                description,
                status,
                priority,
                complexity,
                position,
                owner_kind,
                owner_ref,
                created_by_kind,
                created_by_ref,
                repository_refs_json,
                label_refs_json,
                artifact_refs_json,
                doc_refs_json,
                file_refs_json,
                metadata_json,
                created_at,
                updated_at,
                completed_at
             FROM task
             WHERE source_run_id = ?1
             ORDER BY position ASC, task_id ASC",
        )?;
        let mut rows = stmt.query([source_run_id])?;
        collect_task_rows(&mut rows)
    }

    /// Replace the durable task row with the full provided record.
    pub fn update(&self, record: &TaskRecord) -> Result<TaskRecord, TaskStoreError> {
        let conn = lock_conn(&self.conn)?;
        let rows = update_task_row(&conn, record)?;
        if rows == 0 {
            return Err(TaskStoreError::TaskNotFound {
                task_id: record.task_id.to_string(),
            });
        }
        Ok(record.clone())
    }

    /// Delete one durable task and cascade-delete subtasks.
    pub fn delete(&self, task_id: &TaskId) -> Result<(), TaskStoreError> {
        let conn = lock_conn(&self.conn)?;
        let rows = conn.execute("DELETE FROM task WHERE task_id = ?1", [task_id.as_ref()])?;
        if rows == 0 {
            return Err(TaskStoreError::TaskNotFound {
                task_id: task_id.to_string(),
            });
        }
        Ok(())
    }

    /// Mark one task completed and populate `completed_at`.
    pub fn complete(
        &self,
        task_id: &TaskId,
        completed_at: &str,
    ) -> Result<TaskRecord, TaskStoreError> {
        let conn = lock_conn(&self.conn)?;
        let current = load_required_task(&conn, task_id.as_ref())?;
        let mut next = current.clone();
        next.status = TaskStatus::Completed;
        next.updated_at = completed_at.to_string();
        next.completed_at = Some(completed_at.to_string());
        update_task_row(&conn, &next)?;
        Ok(next)
    }

    /// Atomically cancel, create, and update subtasks for one task.
    pub fn replan(
        &self,
        task_id: &TaskId,
        request: &TaskReplanRequest,
    ) -> Result<TaskReplanEffects, TaskStoreError> {
        ensure_object_json("task_replan.metadata", &request.metadata)?;

        let mut conn = lock_conn(&self.conn)?;
        let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current_task = load_required_task(&transaction, task_id.as_ref())?;

        let mut effects = TaskReplanEffects::default();

        for operation in &request.operations {
            match operation {
                TaskReplanOperation::CancelSubtasks { subtask_ids } => {
                    let now = now_timestamp();
                    let subtasks =
                        load_required_subtasks_for_task(&transaction, task_id, subtask_ids)?;
                    for subtask in subtasks {
                        transaction.execute(
                            "UPDATE subtask
                             SET status = ?1,
                                 updated_at = ?2,
                                 completed_at = ?3
                             WHERE subtask_id = ?4",
                            params![
                                SubtaskStatus::Cancelled.as_str(),
                                now,
                                now,
                                subtask.subtask_id.as_ref(),
                            ],
                        )?;
                        effects.cancelled_subtasks += 1;
                    }
                }
                TaskReplanOperation::CreateSubtasks { items } => {
                    let now = now_timestamp();
                    let resolved = resolve_planned_subtasks(items, task_id, &now);
                    let allowed_ids = resolved
                        .iter()
                        .map(|record| record.subtask_id.as_ref().to_string())
                        .collect::<HashSet<_>>();
                    for record in &resolved {
                        validate_dependencies(
                            &transaction,
                            task_id,
                            &record.subtask_id,
                            &record.depends_on,
                            Some(&allowed_ids),
                        )?;
                    }
                    for record in &resolved {
                        insert_subtask(&transaction, record)?;
                        effects.created_subtasks += 1;
                    }
                }
                TaskReplanOperation::UpdateSubtasks { items } => {
                    let now = now_timestamp();
                    for patch in items {
                        let current = load_required_subtask(&transaction, patch.id.as_ref())?;
                        if current.task_id != *task_id {
                            return Err(TaskStoreError::SubtaskOutsideTask {
                                subtask_id: patch.id.to_string(),
                                task_id: task_id.to_string(),
                                actual_task_id: current.task_id.to_string(),
                            });
                        }
                        let next = apply_subtask_patch(&current, patch, &now);
                        validate_dependencies(
                            &transaction,
                            task_id,
                            &next.subtask_id,
                            &next.depends_on,
                            None,
                        )?;
                        update_subtask_row(&transaction, &next)?;
                        effects.updated_subtasks += 1;
                    }
                }
            }
        }

        let replan_at = now_timestamp();
        let mut next_task = current_task;
        next_task.updated_at = replan_at.clone();
        let mut metadata = metadata_object_clone("task.metadata_json", &next_task.metadata)?;
        metadata.insert(
            LAST_REPLAN_METADATA_KEY.to_string(),
            serde_json::json!({
                "reason": request.reason,
                "metadata": request.metadata,
                "requested_at": replan_at,
            }),
        );
        next_task.metadata = JsonValue::Object(metadata);
        update_task_row(&transaction, &next_task)?;

        transaction.commit()?;
        Ok(effects)
    }
}

impl SubtaskRepository {
    /// Create the repository from a shared `compozy.db` connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert a durable subtask row.
    pub fn create(&self, record: &SubtaskRecord) -> Result<SubtaskRecord, TaskStoreError> {
        let conn = lock_conn(&self.conn)?;
        load_required_task(&conn, record.task_id.as_ref())?;
        validate_dependencies(
            &conn,
            &record.task_id,
            &record.subtask_id,
            &record.depends_on,
            None,
        )?;
        insert_subtask(&conn, record)?;
        Ok(record.clone())
    }

    /// Load one durable subtask by ID.
    pub fn find_by_id(
        &self,
        subtask_id: &SubtaskId,
    ) -> Result<Option<SubtaskRecord>, TaskStoreError> {
        let conn = lock_conn(&self.conn)?;
        load_subtask(&conn, subtask_id.as_ref())
    }

    /// List subtasks for one task using SQL-backed filters, including ready
    /// and blocked dependency checks.
    pub fn list_for_task(
        &self,
        task_id: &TaskId,
        query: &SubtaskListQuery,
    ) -> Result<SubtaskListPage, TaskStoreError> {
        let conn = lock_conn(&self.conn)?;
        load_required_task(&conn, task_id.as_ref())?;
        let mut scoped_query = query.clone();
        scoped_query.task_id = Some(task_id.clone());
        list_subtasks(&conn, &scoped_query)
    }

    /// List subtasks globally using cursor pagination and SQL-backed filters.
    pub fn list(&self, query: &SubtaskListQuery) -> Result<SubtaskListPage, TaskStoreError> {
        let conn = lock_conn(&self.conn)?;
        list_subtasks(&conn, query)
    }

    /// Replace the durable subtask row with the full provided record.
    pub fn update(&self, record: &SubtaskRecord) -> Result<SubtaskRecord, TaskStoreError> {
        let conn = lock_conn(&self.conn)?;
        load_required_task(&conn, record.task_id.as_ref())?;
        validate_dependencies(
            &conn,
            &record.task_id,
            &record.subtask_id,
            &record.depends_on,
            None,
        )?;
        let rows = update_subtask_row(&conn, record)?;
        if rows == 0 {
            return Err(TaskStoreError::SubtaskNotFound {
                subtask_id: record.subtask_id.to_string(),
            });
        }
        Ok(record.clone())
    }

    /// Delete one durable subtask.
    pub fn delete(&self, subtask_id: &SubtaskId) -> Result<(), TaskStoreError> {
        let conn = lock_conn(&self.conn)?;
        let rows = conn.execute(
            "DELETE FROM subtask WHERE subtask_id = ?1",
            [subtask_id.as_ref()],
        )?;
        if rows == 0 {
            return Err(TaskStoreError::SubtaskNotFound {
                subtask_id: subtask_id.to_string(),
            });
        }
        Ok(())
    }

    /// Mark one subtask completed and populate `completed_at`.
    pub fn complete(
        &self,
        subtask_id: &SubtaskId,
        completed_at: &str,
    ) -> Result<SubtaskRecord, TaskStoreError> {
        let conn = lock_conn(&self.conn)?;
        let current = load_required_subtask(&conn, subtask_id.as_ref())?;
        let mut next = current.clone();
        next.status = SubtaskStatus::Completed;
        next.updated_at = completed_at.to_string();
        next.completed_at = Some(completed_at.to_string());
        update_subtask_row(&conn, &next)?;
        Ok(next)
    }
}

fn lock_conn(conn: &Arc<Mutex<Connection>>) -> Result<MutexGuard<'_, Connection>, TaskStoreError> {
    conn.lock()
        .map_err(|error| TaskStoreError::ConnectionLock(error.to_string()))
}

fn insert_task(conn: &Connection, record: &TaskRecord) -> Result<(), TaskStoreError> {
    match conn.execute(
        "INSERT INTO task (
            task_id,
            slug,
            source_run_id,
            title,
            description,
            status,
            priority,
            complexity,
            position,
            owner_kind,
            owner_ref,
            created_by_kind,
            created_by_ref,
            repository_refs_json,
            label_refs_json,
            artifact_refs_json,
            doc_refs_json,
            file_refs_json,
            metadata_json,
            created_at,
            updated_at,
            completed_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
        params![
            record.task_id.as_ref(),
            record.slug.as_str(),
            source_run_id(&record.source),
            record.title.as_str(),
            record.description.as_str(),
            record.status.as_str(),
            record.priority.as_str(),
            record.complexity.as_str(),
            record.position,
            record.owner.kind.as_str(),
            record.owner.ref_id.as_str(),
            record.created_by.kind.as_str(),
            record.created_by.ref_id.as_str(),
            serialize_json_field("task.repository_refs_json", &record.repository_refs)?,
            serialize_json_field("task.label_refs_json", &record.label_refs)?,
            serialize_json_field("task.artifact_refs_json", &record.artifact_refs)?,
            serialize_json_field("task.doc_refs_json", &record.doc_refs)?,
            serialize_json_field("task.file_refs_json", &record.file_refs)?,
            task_metadata_to_json(&record.metadata, &record.source)?,
            record.created_at.as_str(),
            record.updated_at.as_str(),
            record.completed_at.as_deref(),
        ],
    ) {
        Ok(_) => Ok(()),
        Err(error) if is_unique_constraint_for(&error, "task.slug") => {
            Err(TaskStoreError::DuplicateSlug {
                slug: record.slug.clone(),
            })
        }
        Err(error) if is_unique_constraint_for(&error, "task.task_id") => {
            Err(TaskStoreError::TaskAlreadyExists {
                task_id: record.task_id.to_string(),
            })
        }
        Err(error) => Err(error.into()),
    }
}

fn load_task(conn: &Connection, task_id: &str) -> Result<Option<TaskRecord>, TaskStoreError> {
    let mut stmt = conn.prepare(
        "SELECT
            task_id,
            slug,
            source_run_id,
            title,
            description,
            status,
            priority,
            complexity,
            position,
            owner_kind,
            owner_ref,
            created_by_kind,
            created_by_ref,
            repository_refs_json,
            label_refs_json,
            artifact_refs_json,
            doc_refs_json,
            file_refs_json,
            metadata_json,
            created_at,
            updated_at,
            completed_at
         FROM task
         WHERE task_id = ?1",
    )?;
    let mut rows = stmt.query([task_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(read_task_row(row)?)),
        None => Ok(None),
    }
}

fn load_required_task(conn: &Connection, task_id: &str) -> Result<TaskRecord, TaskStoreError> {
    load_task(conn, task_id)?.ok_or_else(|| TaskStoreError::TaskNotFound {
        task_id: task_id.to_string(),
    })
}

fn load_task_by_slug(conn: &Connection, slug: &str) -> Result<Option<TaskRecord>, TaskStoreError> {
    let mut stmt = conn.prepare(
        "SELECT
            task_id,
            slug,
            source_run_id,
            title,
            description,
            status,
            priority,
            complexity,
            position,
            owner_kind,
            owner_ref,
            created_by_kind,
            created_by_ref,
            repository_refs_json,
            label_refs_json,
            artifact_refs_json,
            doc_refs_json,
            file_refs_json,
            metadata_json,
            created_at,
            updated_at,
            completed_at
         FROM task
         WHERE slug = ?1",
    )?;
    let mut rows = stmt.query([slug])?;
    match rows.next()? {
        Some(row) => Ok(Some(read_task_row(row)?)),
        None => Ok(None),
    }
}

fn list_tasks(conn: &Connection, query: &TaskListQuery) -> Result<TaskListPage, TaskStoreError> {
    let limit = query.limit.clamp(1, 200);
    let mut sql = String::from(
        "SELECT
            task_id,
            slug,
            source_run_id,
            title,
            description,
            status,
            priority,
            complexity,
            position,
            owner_kind,
            owner_ref,
            created_by_kind,
            created_by_ref,
            repository_refs_json,
            label_refs_json,
            artifact_refs_json,
            doc_refs_json,
            file_refs_json,
            metadata_json,
            created_at,
            updated_at,
            completed_at
         FROM task",
    );
    let mut predicates = Vec::new();
    let mut params = Vec::new();

    if let Some(status) = query.status {
        predicates.push("status = ?".to_string());
        params.push(SqlValue::from(status.to_string()));
    }

    if let Some(priority) = query.priority {
        predicates.push("priority = ?".to_string());
        params.push(SqlValue::from(priority.to_string()));
    }

    if let Some(created_by) = query.created_by.as_deref() {
        predicates.push("created_by_ref = ?".to_string());
        params.push(SqlValue::from(created_by.to_string()));
    }

    if let Some(source_kind) = query.source_kind {
        predicates
            .push("json_extract(metadata_json, '$.__compozy_task.source.kind') = ?".to_string());
        params.push(SqlValue::from(source_kind.to_string()));
    }

    if let Some(label) = query.label.as_deref() {
        predicates.push(
            "EXISTS (
                SELECT 1
                FROM json_each(task.label_refs_json) AS label_ref
                WHERE label_ref.value = ?
            )"
            .to_string(),
        );
        params.push(SqlValue::from(label.to_string()));
    }

    if let Some(repository) = query.repository.as_deref() {
        predicates.push(
            "EXISTS (
                SELECT 1
                FROM json_each(task.repository_refs_json) AS repository_ref
                WHERE json_extract(repository_ref.value, '$.repository_id') = ?
            )"
            .to_string(),
        );
        params.push(SqlValue::from(repository.to_string()));
    }

    if let Some(search) = query.search.as_deref() {
        let needle = format!("%{}%", search.to_lowercase());
        predicates.push(
            "(lower(slug) LIKE ? OR lower(title) LIKE ? OR lower(description) LIKE ?)".to_string(),
        );
        params.push(SqlValue::from(needle.clone()));
        params.push(SqlValue::from(needle.clone()));
        params.push(SqlValue::from(needle));
    }

    if let Some(cursor) = query.cursor.as_deref() {
        let cursor = decode_cursor(cursor)?;
        match query.order {
            SortOrder::Asc => {
                predicates.push("(position > ? OR (position = ? AND task_id > ?))".to_string());
            }
            SortOrder::Desc => {
                predicates.push("(position < ? OR (position = ? AND task_id < ?))".to_string());
            }
        }
        params.push(SqlValue::from(cursor.position));
        params.push(SqlValue::from(cursor.position));
        params.push(SqlValue::from(cursor.task_id));
    }

    if !predicates.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&predicates.join(" AND "));
    }

    let order_keyword = match query.order {
        SortOrder::Asc => "ASC",
        SortOrder::Desc => "DESC",
    };
    sql.push_str(&format!(
        " ORDER BY position {order_keyword}, task_id {order_keyword} LIMIT ?"
    ));
    params.push(SqlValue::from((limit + 1) as i64));

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    let mut items = collect_task_rows(&mut rows)?;

    let next_cursor = if items.len() > limit {
        items.pop().ok_or_else(|| TaskStoreError::InvalidCursor {
            cursor: "failed to build next_cursor".to_string(),
        })?;
        items.last().map(encode_cursor).transpose()?
    } else {
        None
    };

    Ok(TaskListPage { items, next_cursor })
}

fn update_task_row(conn: &Connection, record: &TaskRecord) -> Result<usize, TaskStoreError> {
    match conn.execute(
        "UPDATE task
         SET slug = ?1,
             source_run_id = ?2,
             title = ?3,
             description = ?4,
             status = ?5,
             priority = ?6,
             complexity = ?7,
             position = ?8,
             owner_kind = ?9,
             owner_ref = ?10,
             created_by_kind = ?11,
             created_by_ref = ?12,
             repository_refs_json = ?13,
             label_refs_json = ?14,
             artifact_refs_json = ?15,
             doc_refs_json = ?16,
             file_refs_json = ?17,
             metadata_json = ?18,
             created_at = ?19,
             updated_at = ?20,
             completed_at = ?21
         WHERE task_id = ?22",
        params![
            record.slug.as_str(),
            source_run_id(&record.source),
            record.title.as_str(),
            record.description.as_str(),
            record.status.as_str(),
            record.priority.as_str(),
            record.complexity.as_str(),
            record.position,
            record.owner.kind.as_str(),
            record.owner.ref_id.as_str(),
            record.created_by.kind.as_str(),
            record.created_by.ref_id.as_str(),
            serialize_json_field("task.repository_refs_json", &record.repository_refs)?,
            serialize_json_field("task.label_refs_json", &record.label_refs)?,
            serialize_json_field("task.artifact_refs_json", &record.artifact_refs)?,
            serialize_json_field("task.doc_refs_json", &record.doc_refs)?,
            serialize_json_field("task.file_refs_json", &record.file_refs)?,
            task_metadata_to_json(&record.metadata, &record.source)?,
            record.created_at.as_str(),
            record.updated_at.as_str(),
            record.completed_at.as_deref(),
            record.task_id.as_ref(),
        ],
    ) {
        Ok(rows) => Ok(rows),
        Err(error) if is_unique_constraint_for(&error, "task.slug") => {
            Err(TaskStoreError::DuplicateSlug {
                slug: record.slug.clone(),
            })
        }
        Err(error) => Err(error.into()),
    }
}

fn insert_subtask(conn: &Connection, record: &SubtaskRecord) -> Result<(), TaskStoreError> {
    match conn.execute(
        "INSERT INTO subtask (
            subtask_id,
            task_id,
            title,
            description,
            kind,
            status,
            complexity,
            position,
            assignee_kind,
            assignee_ref,
            depends_on_json,
            parallelizable,
            input_json,
            result_json,
            metadata_json,
            created_at,
            updated_at,
            completed_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            record.subtask_id.as_ref(),
            record.task_id.as_ref(),
            record.title.as_str(),
            record.description.as_str(),
            record.kind.as_str(),
            record.status.as_str(),
            record.complexity.as_str(),
            record.position,
            record.assignee.as_ref().map(|assignee| assignee.kind.to_string()),
            record.assignee.as_ref().map(|assignee| assignee.ref_id.as_str()),
            serialize_json_field("subtask.depends_on_json", &record.depends_on)?,
            bool_to_sql(record.parallelizable),
            serialize_json_field("subtask.input_json", &record.input)?,
            serialize_optional_json_field("subtask.result_json", record.result.as_ref())?,
            serialize_json_field("subtask.metadata_json", &record.metadata)?,
            record.created_at.as_str(),
            record.updated_at.as_str(),
            record.completed_at.as_deref(),
        ],
    ) {
        Ok(_) => Ok(()),
        Err(error) if is_unique_constraint_for(&error, "subtask.subtask_id") => {
            Err(TaskStoreError::SubtaskAlreadyExists {
                subtask_id: record.subtask_id.to_string(),
            })
        }
        Err(error) => Err(error.into()),
    }
}

fn load_subtask(
    conn: &Connection,
    subtask_id: &str,
) -> Result<Option<SubtaskRecord>, TaskStoreError> {
    let mut stmt = conn.prepare(
        "SELECT
            subtask_id,
            task_id,
            title,
            description,
            kind,
            status,
            complexity,
            position,
            assignee_kind,
            assignee_ref,
            depends_on_json,
            parallelizable,
            input_json,
            result_json,
            metadata_json,
            created_at,
            updated_at,
            completed_at
         FROM subtask
         WHERE subtask_id = ?1",
    )?;
    let mut rows = stmt.query([subtask_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(read_subtask_row(row)?)),
        None => Ok(None),
    }
}

fn load_required_subtask(
    conn: &Connection,
    subtask_id: &str,
) -> Result<SubtaskRecord, TaskStoreError> {
    load_subtask(conn, subtask_id)?.ok_or_else(|| TaskStoreError::SubtaskNotFound {
        subtask_id: subtask_id.to_string(),
    })
}

fn list_subtasks(
    conn: &Connection,
    query: &SubtaskListQuery,
) -> Result<SubtaskListPage, TaskStoreError> {
    let limit = query.limit.max(1);
    let mut sql = String::from(
        "SELECT
            subtask_id,
            task_id,
            title,
            description,
            kind,
            status,
            complexity,
            position,
            assignee_kind,
            assignee_ref,
            depends_on_json,
            parallelizable,
            input_json,
            result_json,
            metadata_json,
            created_at,
            updated_at,
            completed_at
         FROM subtask",
    );
    let mut predicates = Vec::new();
    let mut params = Vec::new();

    if let Some(task_id) = query.task_id.as_ref() {
        predicates.push("task_id = ?".to_string());
        params.push(SqlValue::from(task_id.to_string()));
    }

    if let Some(status) = query.status {
        predicates.push("status = ?".to_string());
        params.push(SqlValue::from(status.to_string()));
    }

    if let Some(assignee_kind) = query.assignee_kind {
        predicates.push("assignee_kind = ?".to_string());
        params.push(SqlValue::from(assignee_kind.to_string()));
    }

    if let Some(assignee_ref) = query.assignee_ref.as_deref() {
        predicates.push("assignee_ref = ?".to_string());
        params.push(SqlValue::from(assignee_ref.to_string()));
    }

    if let Some(kind) = query.kind {
        predicates.push("kind = ?".to_string());
        params.push(SqlValue::from(kind.to_string()));
    }

    if let Some(ready) = query.ready {
        if ready {
            predicates.push(
                "NOT EXISTS (
                    SELECT 1
                    FROM json_each(subtask.depends_on_json) AS dep
                    LEFT JOIN subtask AS dependency ON dependency.subtask_id = dep.value
                    WHERE dependency.subtask_id IS NULL OR dependency.status != 'completed'
                )"
                .to_string(),
            );
        } else {
            predicates.push(
                "EXISTS (
                    SELECT 1
                    FROM json_each(subtask.depends_on_json) AS dep
                    LEFT JOIN subtask AS dependency ON dependency.subtask_id = dep.value
                    WHERE dependency.subtask_id IS NULL OR dependency.status != 'completed'
                )"
                .to_string(),
            );
        }
    }

    if let Some(blocked) = query.blocked {
        if blocked {
            predicates.push(
                "EXISTS (
                    SELECT 1
                    FROM json_each(subtask.depends_on_json) AS dep
                    LEFT JOIN subtask AS dependency ON dependency.subtask_id = dep.value
                    WHERE dependency.subtask_id IS NULL OR dependency.status != 'completed'
                )"
                .to_string(),
            );
        } else {
            predicates.push(
                "NOT EXISTS (
                    SELECT 1
                    FROM json_each(subtask.depends_on_json) AS dep
                    LEFT JOIN subtask AS dependency ON dependency.subtask_id = dep.value
                    WHERE dependency.subtask_id IS NULL OR dependency.status != 'completed'
                )"
                .to_string(),
            );
        }
    }

    if let Some(cursor) = query.cursor.as_deref() {
        let cursor = decode_subtask_cursor(cursor)?;
        match query.order {
            SortOrder::Asc => predicates.push(
                "(position > ? OR (position = ? AND task_id > ?) OR (position = ? AND task_id = ? AND subtask_id > ?))"
                    .to_string(),
            ),
            SortOrder::Desc => predicates.push(
                "(position < ? OR (position = ? AND task_id < ?) OR (position = ? AND task_id = ? AND subtask_id < ?))"
                    .to_string(),
            ),
        }
        params.push(SqlValue::from(cursor.position));
        params.push(SqlValue::from(cursor.position));
        params.push(SqlValue::from(cursor.task_id.clone()));
        params.push(SqlValue::from(cursor.position));
        params.push(SqlValue::from(cursor.task_id));
        params.push(SqlValue::from(cursor.subtask_id));
    }

    if !predicates.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(
            &predicates
                .into_iter()
                .map(|predicate| predicate.trim_start_matches(" AND ").to_string())
                .collect::<Vec<_>>()
                .join(" AND "),
        );
    }

    let order_keyword = match query.order {
        SortOrder::Asc => "ASC",
        SortOrder::Desc => "DESC",
    };
    sql.push_str(&format!(
        " ORDER BY position {order_keyword}, task_id {order_keyword}, subtask_id {order_keyword} LIMIT ?"
    ));
    params.push(SqlValue::from((limit + 1) as i64));

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    let mut items = collect_subtask_rows(&mut rows)?;

    let next_cursor = if items.len() > limit {
        items.pop().ok_or_else(|| TaskStoreError::InvalidCursor {
            cursor: "failed to build subtask next_cursor".to_string(),
        })?;
        items.last().map(encode_subtask_cursor).transpose()?
    } else {
        None
    };

    Ok(SubtaskListPage { items, next_cursor })
}

fn update_subtask_row(conn: &Connection, record: &SubtaskRecord) -> Result<usize, TaskStoreError> {
    conn.execute(
        "UPDATE subtask
         SET task_id = ?1,
             title = ?2,
             description = ?3,
             kind = ?4,
             status = ?5,
             complexity = ?6,
             position = ?7,
             assignee_kind = ?8,
             assignee_ref = ?9,
             depends_on_json = ?10,
             parallelizable = ?11,
             input_json = ?12,
             result_json = ?13,
             metadata_json = ?14,
             created_at = ?15,
             updated_at = ?16,
             completed_at = ?17
         WHERE subtask_id = ?18",
        params![
            record.task_id.as_ref(),
            record.title.as_str(),
            record.description.as_str(),
            record.kind.as_str(),
            record.status.as_str(),
            record.complexity.as_str(),
            record.position,
            record
                .assignee
                .as_ref()
                .map(|assignee| assignee.kind.to_string()),
            record
                .assignee
                .as_ref()
                .map(|assignee| assignee.ref_id.as_str()),
            serialize_json_field("subtask.depends_on_json", &record.depends_on)?,
            bool_to_sql(record.parallelizable),
            serialize_json_field("subtask.input_json", &record.input)?,
            serialize_optional_json_field("subtask.result_json", record.result.as_ref())?,
            serialize_json_field("subtask.metadata_json", &record.metadata)?,
            record.created_at.as_str(),
            record.updated_at.as_str(),
            record.completed_at.as_deref(),
            record.subtask_id.as_ref(),
        ],
    )
    .map_err(Into::into)
}

fn load_required_subtasks_for_task(
    conn: &Connection,
    task_id: &TaskId,
    subtask_ids: &[SubtaskId],
) -> Result<Vec<SubtaskRecord>, TaskStoreError> {
    let mut subtasks = Vec::with_capacity(subtask_ids.len());
    for subtask_id in subtask_ids {
        let subtask = load_required_subtask(conn, subtask_id.as_ref())?;
        if subtask.task_id != *task_id {
            return Err(TaskStoreError::SubtaskOutsideTask {
                subtask_id: subtask_id.to_string(),
                task_id: task_id.to_string(),
                actual_task_id: subtask.task_id.to_string(),
            });
        }
        subtasks.push(subtask);
    }
    Ok(subtasks)
}

fn resolve_planned_subtasks(
    items: &[PlannedSubtask],
    task_id: &TaskId,
    now: &str,
) -> Vec<SubtaskRecord> {
    items
        .iter()
        .map(|item| {
            let status = item.status.unwrap_or(SubtaskStatus::Planned);
            let completed_at = if status.is_terminal() {
                Some(now.to_string())
            } else {
                None
            };
            SubtaskRecord {
                subtask_id: item.subtask_id.clone().unwrap_or_else(|| {
                    SubtaskId::new(format!("subtask_{}", Uuid::new_v4().simple()))
                }),
                task_id: task_id.clone(),
                title: item.title.clone(),
                description: item.description.clone(),
                kind: item.kind,
                status,
                complexity: item.complexity.unwrap_or_default(),
                position: item.position,
                assignee: item.assignee.clone(),
                depends_on: item.depends_on.clone(),
                parallelizable: item.parallelizable,
                input: item.input.clone(),
                result: item.result.clone(),
                metadata: item.metadata.clone(),
                created_at: now.to_string(),
                updated_at: now.to_string(),
                completed_at,
            }
        })
        .collect()
}

fn apply_subtask_patch(
    current: &SubtaskRecord,
    patch: &SubtaskPatch,
    updated_at: &str,
) -> SubtaskRecord {
    let mut next = current.clone();
    if let Some(title) = patch.title.as_ref() {
        next.title = title.clone();
    }
    if let Some(description) = patch.description.as_ref() {
        next.description = description.clone();
    }
    if let Some(kind) = patch.kind {
        next.kind = kind;
    }
    if let Some(status) = patch.status {
        next.status = status;
    }
    if let Some(complexity) = patch.complexity {
        next.complexity = complexity;
    }
    if let Some(position) = patch.position {
        next.position = position;
    }
    if let Some(assignee) = patch.assignee.as_ref() {
        next.assignee = assignee.clone();
    }
    if let Some(depends_on) = patch.depends_on.as_ref() {
        next.depends_on = depends_on.clone();
    }
    if let Some(parallelizable) = patch.parallelizable {
        next.parallelizable = parallelizable;
    }
    if let Some(input) = patch.input.as_ref() {
        next.input = input.clone();
    }
    if let Some(result) = patch.result.as_ref() {
        next.result = result.clone();
    }
    if let Some(metadata) = patch.metadata.as_ref() {
        next.metadata = metadata.clone();
    }
    next.updated_at = updated_at.to_string();
    next.completed_at = match (
        current.status.is_terminal(),
        next.status.is_terminal(),
        current.completed_at.as_ref(),
    ) {
        (false, true, _) => Some(updated_at.to_string()),
        (true, true, Some(existing_completed_at)) => Some(existing_completed_at.clone()),
        (true, true, None) => Some(updated_at.to_string()),
        (_, false, _) => None,
    };
    next
}

fn validate_dependencies(
    conn: &Connection,
    task_id: &TaskId,
    subtask_id: &SubtaskId,
    depends_on: &[SubtaskId],
    allowed_new_ids: Option<&HashSet<String>>,
) -> Result<(), TaskStoreError> {
    let mut unresolved = Vec::new();
    for dependency_id in depends_on {
        if dependency_id == subtask_id {
            return Err(TaskStoreError::SelfDependency {
                subtask_id: subtask_id.to_string(),
            });
        }
        if allowed_new_ids
            .map(|ids| ids.contains(dependency_id.as_ref()))
            .unwrap_or(false)
        {
            continue;
        }
        unresolved.push(dependency_id.as_ref().to_string());
    }

    if unresolved.is_empty() {
        return Ok(());
    }

    let placeholders = std::iter::repeat_n("?", unresolved.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql =
        format!("SELECT subtask_id, task_id FROM subtask WHERE subtask_id IN ({placeholders})");
    let params = unresolved
        .iter()
        .cloned()
        .map(SqlValue::from)
        .collect::<Vec<_>>();

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    let mut found = Vec::new();
    while let Some(row) = rows.next()? {
        found.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
    }

    for dependency_id in &unresolved {
        let Some((_, dependency_task_id)) = found
            .iter()
            .find(|(candidate_id, _)| candidate_id == dependency_id)
        else {
            return Err(TaskStoreError::DependencyNotFound {
                subtask_id: subtask_id.to_string(),
                dependency_id: dependency_id.clone(),
            });
        };
        if dependency_task_id != task_id.as_ref() {
            return Err(TaskStoreError::DependencyOutsideTask {
                subtask_id: subtask_id.to_string(),
                task_id: task_id.to_string(),
                dependency_id: dependency_id.clone(),
                dependency_task_id: dependency_task_id.clone(),
            });
        }
    }

    Ok(())
}

fn read_task_row(row: &rusqlite::Row<'_>) -> Result<TaskRecord, TaskStoreError> {
    let status_text = row.get::<_, String>(5)?;
    let priority_text = row.get::<_, String>(6)?;
    let complexity_text = row.get::<_, String>(7)?;
    let owner_kind_text = row.get::<_, String>(9)?;
    let created_by_kind_text = row.get::<_, String>(11)?;
    let task_id = TaskId::from(row.get::<_, String>(0)?);
    let metadata_text = row.get::<_, String>(18)?;
    let source_run_id = row.get::<_, Option<String>>(2)?;
    let (metadata, source) =
        task_metadata_from_json(&task_id, &metadata_text, source_run_id.as_deref())?;

    Ok(TaskRecord {
        task_id: task_id.clone(),
        slug: row.get(1)?,
        source,
        title: row.get(3)?,
        description: row.get(4)?,
        status: parse_task_status(&status_text)?,
        priority: parse_priority(&priority_text)?,
        complexity: parse_complexity(&complexity_text)?,
        position: row.get(8)?,
        owner: OwnerRef {
            kind: parse_actor_kind(&owner_kind_text)?,
            ref_id: row.get(10)?,
        },
        created_by: OwnerRef {
            kind: parse_actor_kind(&created_by_kind_text)?,
            ref_id: row.get(12)?,
        },
        repository_refs: parse_json_field("task.repository_refs_json", &row.get::<_, String>(13)?)?,
        label_refs: parse_json_field("task.label_refs_json", &row.get::<_, String>(14)?)?,
        artifact_refs: parse_json_field("task.artifact_refs_json", &row.get::<_, String>(15)?)?,
        doc_refs: parse_json_field("task.doc_refs_json", &row.get::<_, String>(16)?)?,
        file_refs: parse_json_field("task.file_refs_json", &row.get::<_, String>(17)?)?,
        metadata,
        created_at: row.get(19)?,
        updated_at: row.get(20)?,
        completed_at: row.get(21)?,
    })
}

fn read_subtask_row(row: &rusqlite::Row<'_>) -> Result<SubtaskRecord, TaskStoreError> {
    let kind_text = row.get::<_, String>(4)?;
    let status_text = row.get::<_, String>(5)?;
    let complexity_text = row.get::<_, String>(6)?;
    let assignee_kind_text = row.get::<_, Option<String>>(8)?;
    let assignee_ref = row.get::<_, Option<String>>(9)?;
    let metadata: JsonValue =
        parse_json_field("subtask.metadata_json", &row.get::<_, String>(14)?)?;
    ensure_object_json("subtask.metadata_json", &metadata)?;

    Ok(SubtaskRecord {
        subtask_id: SubtaskId::from(row.get::<_, String>(0)?),
        task_id: TaskId::from(row.get::<_, String>(1)?),
        title: row.get(2)?,
        description: row.get(3)?,
        kind: parse_subtask_kind(&kind_text)?,
        status: parse_subtask_status(&status_text)?,
        complexity: parse_complexity(&complexity_text)?,
        position: row.get(7)?,
        assignee: match (assignee_kind_text, assignee_ref) {
            (Some(kind), Some(reference)) => Some(AssigneeRef {
                kind: parse_actor_kind(&kind)?,
                ref_id: reference,
            }),
            _ => None,
        },
        depends_on: parse_json_field("subtask.depends_on_json", &row.get::<_, String>(10)?)?,
        parallelizable: sql_to_bool(row.get::<_, i64>(11)?),
        input: parse_json_field("subtask.input_json", &row.get::<_, String>(12)?)?,
        result: match row.get::<_, Option<String>>(13)? {
            Some(value) => Some(parse_json_field("subtask.result_json", &value)?),
            None => None,
        },
        metadata,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
        completed_at: row.get(17)?,
    })
}

fn collect_task_rows(rows: &mut rusqlite::Rows<'_>) -> Result<Vec<TaskRecord>, TaskStoreError> {
    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        items.push(read_task_row(row)?);
    }
    Ok(items)
}

fn collect_subtask_rows(
    rows: &mut rusqlite::Rows<'_>,
) -> Result<Vec<SubtaskRecord>, TaskStoreError> {
    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        items.push(read_subtask_row(row)?);
    }
    Ok(items)
}

fn parse_task_status(value: &str) -> Result<TaskStatus, TaskStoreError> {
    value
        .parse()
        .map_err(|_| TaskStoreError::InvalidTaskStatus {
            status: value.to_string(),
        })
}

fn parse_subtask_status(value: &str) -> Result<SubtaskStatus, TaskStoreError> {
    value
        .parse()
        .map_err(|_| TaskStoreError::InvalidSubtaskStatus {
            status: value.to_string(),
        })
}

fn parse_subtask_kind(value: &str) -> Result<SubtaskKind, TaskStoreError> {
    value
        .parse()
        .map_err(|_| TaskStoreError::InvalidSubtaskKind {
            kind: value.to_string(),
        })
}

fn parse_priority(value: &str) -> Result<Priority, TaskStoreError> {
    value.parse().map_err(|_| TaskStoreError::InvalidPriority {
        priority: value.to_string(),
    })
}

fn parse_complexity(value: &str) -> Result<Complexity, TaskStoreError> {
    value
        .parse()
        .map_err(|_| TaskStoreError::InvalidComplexity {
            complexity: value.to_string(),
        })
}

fn parse_actor_kind(value: &str) -> Result<ActorKind, TaskStoreError> {
    value.parse().map_err(|_| TaskStoreError::InvalidActorKind {
        kind: value.to_string(),
    })
}

fn serialize_json_field<T: Serialize>(
    field: &'static str,
    value: &T,
) -> Result<String, TaskStoreError> {
    serde_json::to_string(value).map_err(|error| TaskStoreError::InvalidJsonField {
        field,
        message: error.to_string(),
    })
}

fn serialize_optional_json_field<T: Serialize>(
    field: &'static str,
    value: Option<&T>,
) -> Result<Option<String>, TaskStoreError> {
    value
        .map(|value| serialize_json_field(field, value))
        .transpose()
}

fn parse_json_field<T: DeserializeOwned>(
    field: &'static str,
    value: &str,
) -> Result<T, TaskStoreError> {
    serde_json::from_str(value).map_err(|error| TaskStoreError::InvalidJsonField {
        field,
        message: error.to_string(),
    })
}

fn task_metadata_to_json(
    metadata: &JsonValue,
    source: &TaskSource,
) -> Result<String, TaskStoreError> {
    ensure_object_json("task.metadata_json", metadata)?;
    let mut object = metadata_object_clone("task.metadata_json", metadata)?;
    if object.contains_key(RESERVED_TASK_METADATA_KEY) {
        return Err(TaskStoreError::ReservedMetadataKey {
            field: "task.metadata_json",
            key: RESERVED_TASK_METADATA_KEY,
        });
    }
    object.insert(
        RESERVED_TASK_METADATA_KEY.to_string(),
        serde_json::to_value(StoredTaskMetadata {
            source: source.clone(),
        })?,
    );
    serde_json::to_string(&JsonValue::Object(object)).map_err(Into::into)
}

fn task_metadata_from_json(
    task_id: &TaskId,
    metadata_text: &str,
    source_run_id: Option<&str>,
) -> Result<(JsonValue, TaskSource), TaskStoreError> {
    let metadata: JsonValue = parse_json_field("task.metadata_json", metadata_text)?;
    ensure_object_json("task.metadata_json", &metadata)?;
    let mut object = metadata_object_clone("task.metadata_json", &metadata)?;
    let stored = object.remove(RESERVED_TASK_METADATA_KEY).ok_or_else(|| {
        TaskStoreError::InvalidSourceEncoding {
            task_id: task_id.to_string(),
            reason: "missing internal source metadata".to_string(),
        }
    })?;
    let stored: StoredTaskMetadata =
        serde_json::from_value(stored).map_err(|error| TaskStoreError::InvalidSourceEncoding {
            task_id: task_id.to_string(),
            reason: error.to_string(),
        })?;
    match (&stored.source, source_run_id) {
        (TaskSource::Workflow { run_id, .. }, Some(stored_run_id)) if run_id == stored_run_id => {}
        (TaskSource::Workflow { run_id, .. }, Some(stored_run_id)) => {
            return Err(TaskStoreError::InvalidSourceEncoding {
                task_id: task_id.to_string(),
                reason: format!(
                    "workflow source run_id '{run_id}' does not match source_run_id '{stored_run_id}'"
                ),
            });
        }
        (TaskSource::Workflow { .. }, None) => {
            return Err(TaskStoreError::InvalidSourceEncoding {
                task_id: task_id.to_string(),
                reason: "workflow source missing source_run_id column".to_string(),
            });
        }
        (TaskSource::Manual | TaskSource::Api, Some(stored_run_id)) => {
            return Err(TaskStoreError::InvalidSourceEncoding {
                task_id: task_id.to_string(),
                reason: format!("non-workflow source unexpectedly stored run_id '{stored_run_id}'"),
            });
        }
        (TaskSource::Manual | TaskSource::Api, None) => {}
    }
    Ok((JsonValue::Object(object), stored.source))
}

fn metadata_object_clone(
    field: &'static str,
    metadata: &JsonValue,
) -> Result<JsonMap<String, JsonValue>, TaskStoreError> {
    metadata
        .as_object()
        .cloned()
        .ok_or(TaskStoreError::InvalidMetadataShape { field })
}

fn ensure_object_json(field: &'static str, value: &JsonValue) -> Result<(), TaskStoreError> {
    if value.is_object() {
        return Ok(());
    }
    Err(TaskStoreError::InvalidMetadataShape { field })
}

fn source_run_id(source: &TaskSource) -> Option<&str> {
    match source {
        TaskSource::Workflow { run_id, .. } => Some(run_id.as_str()),
        TaskSource::Manual | TaskSource::Api => None,
    }
}

fn encode_cursor(record: &TaskRecord) -> Result<String, TaskStoreError> {
    serde_json::to_string(&TaskCursor {
        position: record.position,
        task_id: record.task_id.to_string(),
    })
    .map_err(Into::into)
}

fn decode_cursor(cursor: &str) -> Result<TaskCursor, TaskStoreError> {
    serde_json::from_str(cursor).map_err(|_| TaskStoreError::InvalidCursor {
        cursor: cursor.to_string(),
    })
}

fn encode_subtask_cursor(record: &SubtaskRecord) -> Result<String, TaskStoreError> {
    serde_json::to_string(&SubtaskCursor {
        position: record.position,
        task_id: record.task_id.to_string(),
        subtask_id: record.subtask_id.to_string(),
    })
    .map_err(Into::into)
}

fn decode_subtask_cursor(cursor: &str) -> Result<SubtaskCursor, TaskStoreError> {
    serde_json::from_str(cursor).map_err(|_| TaskStoreError::InvalidCursor {
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

fn bool_to_sql(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn sql_to_bool(value: i64) -> bool {
    value != 0
}

fn now_timestamp() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openfang_types::task::{ArtifactRef, DocRef, FileRef, RepositoryRef};
    use pretty_assertions::assert_eq;
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
        conn.execute_batch(TASK_SUBTASK_MIGRATION_SQL)
            .expect("apply task/subtask migration");
        Arc::new(Mutex::new(conn))
    }

    fn migrated_file_connection(path: &Path) -> Arc<Mutex<Connection>> {
        let conn = Connection::open(path).expect("open file-backed compozy.db");
        configure_test_connection(&conn);
        conn.execute_batch(TASK_SUBTASK_MIGRATION_SQL)
            .expect("apply task/subtask migration");
        Arc::new(Mutex::new(conn))
    }

    fn task_stores(conn: Arc<Mutex<Connection>>) -> TaskStoreSet {
        TaskStoreSet::new(conn)
    }

    fn sample_task(task_id: &str, slug: &str) -> TaskRecord {
        TaskRecord {
            task_id: TaskId::new(task_id),
            slug: slug.to_string(),
            source: TaskSource::Workflow {
                workflow_id: "sdlc".to_string(),
                run_id: "run_123".to_string(),
            },
            title: "Prepare PRD".to_string(),
            description: "Write the PRD".to_string(),
            status: TaskStatus::Planned,
            priority: Priority::High,
            complexity: Complexity::Medium,
            position: 1,
            owner: OwnerRef {
                kind: ActorKind::AgentGroup,
                ref_id: "sdlc".to_string(),
            },
            created_by: OwnerRef {
                kind: ActorKind::Agent,
                ref_id: "planner".to_string(),
            },
            repository_refs: vec![RepositoryRef {
                repository_id: "repo_main".to_string(),
                role: Some("primary".to_string()),
            }],
            label_refs: vec!["planning".into(), "prd".into()],
            artifact_refs: vec![ArtifactRef {
                artifact_id: "artifact_001".to_string(),
                type_name: "prd".to_string(),
                current_version_id: Some("artifact_v3".to_string()),
            }],
            doc_refs: vec![DocRef {
                doc_id: "doc_001".to_string(),
                type_name: "brief".to_string(),
                current_version_id: Some("doc_v2".to_string()),
            }],
            file_refs: vec![FileRef {
                path: "docs/prd.md".to_string(),
                kind: "workspace".to_string(),
                description: Some("Current PRD draft".to_string()),
            }],
            metadata: serde_json::json!({ "source": "agent" }),
            created_at: "2026-03-24T10:00:00Z".to_string(),
            updated_at: "2026-03-24T10:00:00Z".to_string(),
            completed_at: None,
        }
    }

    fn sample_subtask(subtask_id: &str, task_id: &TaskId, position: i64) -> SubtaskRecord {
        SubtaskRecord {
            subtask_id: SubtaskId::new(subtask_id),
            task_id: task_id.clone(),
            title: format!("Subtask {subtask_id}"),
            description: "Write the next section".to_string(),
            kind: SubtaskKind::DocChange,
            status: SubtaskStatus::Planned,
            complexity: Complexity::Medium,
            position,
            assignee: Some(AssigneeRef {
                kind: ActorKind::Agent,
                ref_id: "prd-writer".to_string(),
            }),
            depends_on: Vec::new(),
            parallelizable: false,
            input: serde_json::json!({ "artifact_id": "artifact_001" }),
            result: None,
            metadata: serde_json::json!({}),
            created_at: "2026-03-24T10:01:00Z".to_string(),
            updated_at: "2026-03-24T10:01:00Z".to_string(),
            completed_at: None,
        }
    }

    #[test]
    fn task_repository_should_round_trip_all_columns() {
        let stores = task_stores(migrated_in_memory_connection());
        let task = sample_task("task_001", "onboarding-revamp-prd");

        let created = stores.task.create(&task).expect("create task");
        let loaded = stores
            .task
            .find_by_id(&task.task_id)
            .expect("load task")
            .expect("task exists");

        assert_eq!(created, task);
        assert_eq!(loaded, task);
    }

    #[test]
    fn task_repository_should_reject_duplicate_slug_with_domain_error() {
        let stores = task_stores(migrated_in_memory_connection());
        let first = sample_task("task_001", "shared-slug");
        let second = sample_task("task_002", "shared-slug");

        stores.task.create(&first).expect("create first task");
        let error = stores
            .task
            .create(&second)
            .expect_err("duplicate slug should fail");

        assert_eq!(
            error.to_string(),
            TaskStoreError::DuplicateSlug {
                slug: "shared-slug".to_string(),
            }
            .to_string()
        );
    }

    #[test]
    fn subtask_repository_should_reject_cross_task_dependencies() {
        let stores = task_stores(migrated_in_memory_connection());
        let task_a = sample_task("task_a", "task-a");
        let task_b = sample_task("task_b", "task-b");
        stores.task.create(&task_a).expect("create task a");
        stores.task.create(&task_b).expect("create task b");

        let dependency = sample_subtask("subtask_dependency", &task_b.task_id, 1);
        stores
            .subtask
            .create(&dependency)
            .expect("create dependency subtask");

        let mut invalid = sample_subtask("subtask_invalid", &task_a.task_id, 1);
        invalid.depends_on = vec![dependency.subtask_id.clone()];

        let error = stores
            .subtask
            .create(&invalid)
            .expect_err("cross-task dependency should fail");

        assert_eq!(
            error.to_string(),
            TaskStoreError::DependencyOutsideTask {
                subtask_id: "subtask_invalid".to_string(),
                task_id: "task_a".to_string(),
                dependency_id: "subtask_dependency".to_string(),
                dependency_task_id: "task_b".to_string(),
            }
            .to_string()
        );
    }

    #[test]
    fn subtask_repository_should_filter_ready_subtasks_in_sql() {
        let stores = task_stores(migrated_in_memory_connection());
        let task = sample_task("task_ready", "task-ready");
        stores.task.create(&task).expect("create task");

        let mut completed = sample_subtask("subtask_completed", &task.task_id, 1);
        completed.status = SubtaskStatus::Completed;
        completed.completed_at = Some("2026-03-24T10:02:00Z".to_string());
        completed.updated_at = "2026-03-24T10:02:00Z".to_string();
        stores
            .subtask
            .create(&completed)
            .expect("create completed subtask");

        let mut ready = sample_subtask("subtask_ready", &task.task_id, 2);
        ready.depends_on = vec![completed.subtask_id.clone()];
        stores.subtask.create(&ready).expect("create ready subtask");

        let mut blocked = sample_subtask("subtask_blocked", &task.task_id, 3);
        blocked.depends_on = vec![SubtaskId::new("subtask_pending")];
        let pending = sample_subtask("subtask_pending", &task.task_id, 4);
        stores
            .subtask
            .create(&pending)
            .expect("create pending subtask");
        stores
            .subtask
            .create(&blocked)
            .expect("create blocked subtask");

        let items = stores
            .subtask
            .list_for_task(
                &task.task_id,
                &SubtaskListQuery {
                    ready: Some(true),
                    ..SubtaskListQuery::default()
                },
            )
            .expect("list ready subtasks")
            .items;

        assert_eq!(
            items
                .into_iter()
                .map(|item| item.subtask_id)
                .collect::<Vec<_>>(),
            vec![completed.subtask_id, ready.subtask_id, pending.subtask_id]
        );
    }

    #[test]
    fn subtask_repository_should_filter_blocked_subtasks_in_sql() {
        let stores = task_stores(migrated_in_memory_connection());
        let task = sample_task("task_blocked", "task-blocked");
        stores.task.create(&task).expect("create task");

        let pending = sample_subtask("subtask_pending", &task.task_id, 1);
        stores.subtask.create(&pending).expect("create pending");

        let mut blocked = sample_subtask("subtask_blocked", &task.task_id, 2);
        blocked.depends_on = vec![pending.subtask_id.clone()];
        stores.subtask.create(&blocked).expect("create blocked");

        let items = stores
            .subtask
            .list_for_task(
                &task.task_id,
                &SubtaskListQuery {
                    blocked: Some(true),
                    ..SubtaskListQuery::default()
                },
            )
            .expect("list blocked subtasks")
            .items;

        assert_eq!(
            items
                .into_iter()
                .map(|item| item.subtask_id)
                .collect::<Vec<_>>(),
            vec![blocked.subtask_id]
        );
    }

    #[test]
    fn subtask_repository_should_persist_parallelizable_updates() {
        let stores = task_stores(migrated_in_memory_connection());
        let task = sample_task("task_parallel", "task-parallel");
        stores.task.create(&task).expect("create task");

        let mut subtask = sample_subtask("subtask_parallel", &task.task_id, 1);
        stores.subtask.create(&subtask).expect("create subtask");
        subtask.parallelizable = true;
        subtask.updated_at = "2026-03-24T10:10:00Z".to_string();

        stores.subtask.update(&subtask).expect("update subtask");
        let loaded = stores
            .subtask
            .find_by_id(&subtask.subtask_id)
            .expect("load subtask")
            .expect("subtask exists");

        assert_eq!(loaded.parallelizable, true);
        assert_eq!(loaded, subtask);
    }

    #[test]
    fn task_repository_replan_should_roll_back_on_create_failure() {
        let stores = task_stores(migrated_in_memory_connection());
        let task = sample_task("task_replan_atomic", "task-replan-atomic");
        let other_task = sample_task("task_replan_other", "task-replan-other");
        stores.task.create(&task).expect("create task");
        stores.task.create(&other_task).expect("create other task");

        let existing = sample_subtask("subtask_existing", &task.task_id, 1);
        stores
            .subtask
            .create(&existing)
            .expect("create existing subtask");

        let foreign = sample_subtask("subtask_foreign", &other_task.task_id, 1);
        stores
            .subtask
            .create(&foreign)
            .expect("create foreign subtask");

        let effects = stores.task.replan(
            &task.task_id,
            &TaskReplanRequest {
                reason: "split the work".to_string(),
                operations: vec![
                    TaskReplanOperation::CancelSubtasks {
                        subtask_ids: vec![existing.subtask_id.clone()],
                    },
                    TaskReplanOperation::CreateSubtasks {
                        items: vec![PlannedSubtask {
                            subtask_id: Some(SubtaskId::new("subtask_new")),
                            title: "Invalid dependency".to_string(),
                            description: "Should fail".to_string(),
                            kind: SubtaskKind::ReviewItem,
                            status: None,
                            complexity: None,
                            position: 2,
                            assignee: None,
                            depends_on: vec![foreign.subtask_id.clone()],
                            parallelizable: false,
                            input: serde_json::json!({}),
                            result: None,
                            metadata: serde_json::json!({}),
                        }],
                    },
                ],
                metadata: serde_json::json!({}),
            },
        );

        let error = effects.expect_err("replan should fail");
        assert_eq!(
            error.to_string(),
            TaskStoreError::DependencyOutsideTask {
                subtask_id: "subtask_new".to_string(),
                task_id: "task_replan_atomic".to_string(),
                dependency_id: "subtask_foreign".to_string(),
                dependency_task_id: "task_replan_other".to_string(),
            }
            .to_string()
        );

        let loaded = stores
            .subtask
            .find_by_id(&existing.subtask_id)
            .expect("load existing subtask")
            .expect("existing subtask");
        assert_eq!(loaded.status, SubtaskStatus::Planned);
        assert_eq!(
            stores
                .subtask
                .find_by_id(&SubtaskId::new("subtask_new"))
                .expect("lookup new subtask"),
            None
        );
    }

    #[test]
    fn task_and_subtask_should_round_trip_after_reopen() {
        let dir = tempdir().expect("create temp dir");
        let db_path = dir.path().join("compozy.db");

        {
            let stores = task_stores(migrated_file_connection(&db_path));
            let mut task = sample_task("task_file", "task-file");
            task.source = TaskSource::Api;
            stores.task.create(&task).expect("create task");
            let mut subtask = sample_subtask("subtask_file", &task.task_id, 1);
            subtask.depends_on = vec![SubtaskId::new("subtask_seed")];
            let seed = sample_subtask("subtask_seed", &task.task_id, 0);
            stores.subtask.create(&seed).expect("create seed");
            stores.subtask.create(&subtask).expect("create subtask");
        }

        let reopened = task_stores(migrated_file_connection(&db_path));
        let task = reopened
            .task
            .find_by_id(&TaskId::new("task_file"))
            .expect("find task by id")
            .expect("task exists");
        let by_slug = reopened
            .task
            .find_by_slug("task-file")
            .expect("find task by slug")
            .expect("task exists by slug");
        let subtask = reopened
            .subtask
            .find_by_id(&SubtaskId::new("subtask_file"))
            .expect("find subtask")
            .expect("subtask exists");

        assert_eq!(task, by_slug);
        assert_eq!(task.source, TaskSource::Api);
        assert_eq!(subtask.depends_on, vec![SubtaskId::new("subtask_seed")]);
    }

    #[test]
    fn subtask_dependency_json_should_survive_reopen_byte_for_byte() {
        let dir = tempdir().expect("create temp dir");
        let db_path = dir.path().join("compozy.db");
        let expected_json = serde_json::to_string(&vec![
            SubtaskId::new("subtask_a"),
            SubtaskId::new("subtask_b"),
        ])
        .expect("serialize dependency array");

        {
            let stores = task_stores(migrated_file_connection(&db_path));
            let task = sample_task("task_chain", "task-chain");
            stores.task.create(&task).expect("create task");
            let a = sample_subtask("subtask_a", &task.task_id, 1);
            let b = sample_subtask("subtask_b", &task.task_id, 2);
            stores.subtask.create(&a).expect("create a");
            stores.subtask.create(&b).expect("create b");

            let mut chain = sample_subtask("subtask_chain", &task.task_id, 3);
            chain.depends_on = vec![a.subtask_id.clone(), b.subtask_id.clone()];
            stores.subtask.create(&chain).expect("create chain");
        }

        let reopened = migrated_file_connection(&db_path);
        let conn = reopened.lock().expect("lock reopened connection");
        let depends_on_json: String = conn
            .query_row(
                "SELECT depends_on_json FROM subtask WHERE subtask_id = ?1",
                ["subtask_chain"],
                |row| row.get(0),
            )
            .expect("read depends_on_json");

        assert_eq!(depends_on_json, expected_json);
    }

    #[test]
    fn task_repository_should_list_by_source_run_id() {
        let stores = task_stores(migrated_in_memory_connection());
        let first = sample_task("task_source_1", "task-source-1");
        let mut second = sample_task("task_source_2", "task-source-2");
        second.source = TaskSource::Workflow {
            workflow_id: "sdlc".to_string(),
            run_id: "run_456".to_string(),
        };
        stores.task.create(&first).expect("create first");
        stores.task.create(&second).expect("create second");

        let items = stores
            .task
            .find_by_source_run_id("run_123")
            .expect("list by source run id");

        assert_eq!(items, vec![first]);
    }

    #[test]
    fn task_repository_should_paginate_by_position_cursor() {
        let stores = task_stores(migrated_in_memory_connection());
        for index in 1..=3 {
            let mut task =
                sample_task(&format!("task_page_{index}"), &format!("task-page-{index}"));
            task.position = index;
            task.updated_at = format!("2026-03-24T10:0{index}:00Z");
            stores.task.create(&task).expect("create task");
        }

        let first_page = stores
            .task
            .list(&TaskListQuery {
                limit: 2,
                ..TaskListQuery::default()
            })
            .expect("list first page");
        let second_page = stores
            .task
            .list(&TaskListQuery {
                limit: 2,
                cursor: first_page.next_cursor.clone(),
                ..TaskListQuery::default()
            })
            .expect("list second page");

        assert_eq!(
            first_page
                .items
                .iter()
                .map(|item| item.task_id.clone())
                .collect::<Vec<_>>(),
            vec![TaskId::new("task_page_1"), TaskId::new("task_page_2")]
        );
        assert_eq!(
            second_page
                .items
                .iter()
                .map(|item| item.task_id.clone())
                .collect::<Vec<_>>(),
            vec![TaskId::new("task_page_3")]
        );
        assert_eq!(second_page.next_cursor, None);
    }

    #[test]
    fn task_repository_replan_should_apply_round_trip_changes() {
        let stores = task_stores(migrated_in_memory_connection());
        let task = sample_task("task_replan", "task-replan");
        stores.task.create(&task).expect("create task");

        let first = sample_subtask("subtask_001", &task.task_id, 1);
        let second = sample_subtask("subtask_002", &task.task_id, 2);
        let third = sample_subtask("subtask_003", &task.task_id, 3);
        stores.subtask.create(&first).expect("create first");
        stores.subtask.create(&second).expect("create second");
        stores.subtask.create(&third).expect("create third");

        let effects = stores
            .task
            .replan(
                &task.task_id,
                &TaskReplanRequest {
                    reason: "split into smaller steps".to_string(),
                    operations: vec![
                        TaskReplanOperation::CancelSubtasks {
                            subtask_ids: vec![third.subtask_id.clone()],
                        },
                        TaskReplanOperation::CreateSubtasks {
                            items: vec![PlannedSubtask {
                                subtask_id: Some(SubtaskId::new("subtask_004")),
                                title: "Review clarity issues".to_string(),
                                description: "Resolve review comments".to_string(),
                                kind: SubtaskKind::ReviewItem,
                                status: Some(SubtaskStatus::Ready),
                                complexity: Some(Complexity::Medium),
                                position: 4,
                                assignee: Some(AssigneeRef {
                                    kind: ActorKind::Agent,
                                    ref_id: "reviewer".to_string(),
                                }),
                                depends_on: vec![first.subtask_id.clone()],
                                parallelizable: true,
                                input: serde_json::json!({}),
                                result: None,
                                metadata: serde_json::json!({}),
                            }],
                        },
                        TaskReplanOperation::UpdateSubtasks {
                            items: vec![SubtaskPatch {
                                id: second.subtask_id.clone(),
                                title: Some("Refine acceptance criteria".to_string()),
                                description: None,
                                kind: None,
                                status: Some(SubtaskStatus::InProgress),
                                complexity: None,
                                position: None,
                                assignee: None,
                                depends_on: Some(vec![first.subtask_id.clone()]),
                                parallelizable: Some(true),
                                input: None,
                                result: None,
                                metadata: None,
                            }],
                        },
                    ],
                    metadata: serde_json::json!({ "source": "agent" }),
                },
            )
            .expect("replan task");

        let subtasks = stores
            .subtask
            .list_for_task(&task.task_id, &SubtaskListQuery::default())
            .expect("list subtasks")
            .items;

        assert_eq!(
            effects,
            TaskReplanEffects {
                created_subtasks: 1,
                updated_subtasks: 1,
                cancelled_subtasks: 1,
            }
        );
        assert_eq!(
            subtasks
                .iter()
                .map(|item| (item.subtask_id.clone(), item.status, item.position))
                .collect::<Vec<_>>(),
            vec![
                (SubtaskId::new("subtask_001"), SubtaskStatus::Planned, 1),
                (SubtaskId::new("subtask_002"), SubtaskStatus::InProgress, 2),
                (SubtaskId::new("subtask_003"), SubtaskStatus::Cancelled, 3),
                (SubtaskId::new("subtask_004"), SubtaskStatus::Ready, 4),
            ]
        );
    }

    #[test]
    fn task_repository_replan_should_preserve_completed_at_for_terminal_patch_updates() {
        let stores = task_stores(migrated_in_memory_connection());
        let task = sample_task("task_terminal_patch", "task-terminal-patch");
        stores.task.create(&task).expect("create task");

        let mut completed = sample_subtask("subtask_completed", &task.task_id, 1);
        completed.status = SubtaskStatus::Completed;
        completed.completed_at = Some("2026-03-24T10:05:00Z".to_string());
        completed.updated_at = "2026-03-24T10:05:00Z".to_string();
        stores
            .subtask
            .create(&completed)
            .expect("create completed subtask");

        stores
            .task
            .replan(
                &task.task_id,
                &TaskReplanRequest {
                    reason: "update terminal metadata".to_string(),
                    operations: vec![TaskReplanOperation::UpdateSubtasks {
                        items: vec![SubtaskPatch {
                            id: completed.subtask_id.clone(),
                            title: Some("Updated terminal title".to_string()),
                            description: None,
                            kind: None,
                            status: None,
                            complexity: None,
                            position: None,
                            assignee: None,
                            depends_on: None,
                            parallelizable: None,
                            input: None,
                            result: None,
                            metadata: Some(serde_json::json!({ "reviewed": true })),
                        }],
                    }],
                    metadata: serde_json::json!({}),
                },
            )
            .expect("replan terminal patch");

        let loaded = stores
            .subtask
            .find_by_id(&completed.subtask_id)
            .expect("load subtask")
            .expect("subtask exists");

        assert_eq!(loaded.status, SubtaskStatus::Completed);
        assert_eq!(loaded.completed_at, completed.completed_at);
        assert_eq!(loaded.title, "Updated terminal title");
        assert_eq!(loaded.metadata, serde_json::json!({ "reviewed": true }));
    }
}
