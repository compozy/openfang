//! Durable looper runtime for task/subtask execution.

use async_trait::async_trait;
use openfang_memory::{
    now_timestamp, DispatchKind, DispatchRecord, DispatchRepository, DispatchStatus,
    DispatchStoreError, LooperStoreError, TaskStoreError, WorkflowStoreSet,
};
use openfang_types::looper::{
    LooperExecutionMode, LooperProgress, LooperRunId, LooperRunRecord, LooperRunStatus,
    LooperSubtaskRecord, LooperSubtaskStatus,
};
use openfang_types::task::{SubtaskId, SubtaskListQuery, SubtaskRecord, SubtaskStatus, TaskId};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Async executor used by the looper runtime to drive one durable dispatch.
#[async_trait]
pub(crate) trait LooperDispatchExecutor: Send + Sync {
    /// Execute the durable dispatch associated with one subtask.
    async fn execute_dispatch(
        &self,
        looper_run: &LooperRunRecord,
        subtask: &SubtaskRecord,
        dispatch_id: &str,
    ) -> Result<JsonValue, String>;
}

/// Typed failures surfaced by the looper runtime.
#[derive(Debug, Error)]
pub(crate) enum LooperRuntimeError {
    /// Durable looper store returned an error.
    #[error(transparent)]
    LooperStore(#[from] LooperStoreError),
    /// Durable task/subtask store returned an error.
    #[error(transparent)]
    TaskStore(#[from] TaskStoreError),
    /// Durable dispatch store returned an error.
    #[error(transparent)]
    DispatchStore(#[from] DispatchStoreError),
    /// One asynchronous task failed to join cleanly.
    #[error("looper task join failed: {0}")]
    Join(String),
    /// The shared semaphore was unexpectedly closed.
    #[error("looper semaphore was unexpectedly closed")]
    SemaphoreClosed,
    /// The looper run could not make forward progress.
    #[error("{0}")]
    Progress(String),
}

/// One durable looper runtime instance bound to shared stores and a dispatch executor.
#[derive(Clone)]
pub(crate) struct LooperRuntime {
    stores: WorkflowStoreSet,
    executor: Arc<dyn LooperDispatchExecutor>,
}

#[derive(Debug)]
struct InFlightSubtask {
    looper_subtask_id: String,
    dispatch_id: String,
    parallelizable: bool,
}

#[derive(Debug)]
struct SubtaskCompletion {
    subtask_id: String,
    result: Result<JsonValue, String>,
}

impl LooperRuntime {
    /// Create a new looper runtime from durable stores and a dispatch executor.
    pub(crate) fn new(stores: WorkflowStoreSet, executor: Arc<dyn LooperDispatchExecutor>) -> Self {
        Self { stores, executor }
    }

    /// Resume or execute one looper run until it settles or is externally stopped.
    pub(crate) async fn run(
        &self,
        looper_run_id: &LooperRunId,
        stop_dispatching: CancellationToken,
    ) -> Result<(), LooperRuntimeError> {
        let mut looper_run = self
            .stores
            .looper_run
            .find_by_id(looper_run_id)?
            .ok_or_else(|| LooperStoreError::RunNotFound {
                looper_run_id: looper_run_id.to_string(),
            })?;

        let effective_parallelism = match looper_run.execution_policy.mode {
            LooperExecutionMode::Sequential => 1usize,
            LooperExecutionMode::Parallel => looper_run.execution_policy.max_parallelism as usize,
        };
        let semaphore = Arc::new(Semaphore::new(effective_parallelism));

        let ordered_subtasks = self.load_all_subtasks(&looper_run.task_id)?;
        let mut subtasks_by_id = ordered_subtasks
            .iter()
            .cloned()
            .map(|subtask| (subtask.subtask_id.to_string(), subtask))
            .collect::<HashMap<_, _>>();
        let mut execution_view = self
            .ensure_execution_view(&looper_run, &ordered_subtasks)
            .await?;
        self.sync_run_projection(&mut looper_run, &ordered_subtasks, &execution_view)?;

        let mut join_set = JoinSet::new();
        let mut in_flight = HashMap::<String, InFlightSubtask>::new();

        loop {
            looper_run = self
                .stores
                .looper_run
                .find_by_id(looper_run_id)?
                .ok_or_else(|| LooperStoreError::RunNotFound {
                    looper_run_id: looper_run_id.to_string(),
                })?;
            self.sync_run_projection(&mut looper_run, &ordered_subtasks, &execution_view)?;

            if looper_run.status == LooperRunStatus::Running
                && !stop_dispatching.is_cancelled()
                && !has_failed_execution(&execution_view)
            {
                while in_flight.len() < effective_parallelism {
                    let Some(subtask_id) = select_next_subtask(
                        &ordered_subtasks,
                        &execution_view,
                        &in_flight,
                        effective_parallelism,
                    ) else {
                        break;
                    };

                    let subtask = subtasks_by_id.get(&subtask_id).cloned().ok_or_else(|| {
                        LooperRuntimeError::Progress(format!(
                            "subtask '{subtask_id}' disappeared from the runtime index"
                        ))
                    })?;

                    let looper_subtask =
                        execution_view.get(&subtask_id).cloned().ok_or_else(|| {
                            LooperRuntimeError::Progress(format!(
                                "looper execution view missing subtask '{subtask_id}'"
                            ))
                        })?;

                    let dispatch_record =
                        self.create_pending_dispatch(&looper_run, &subtask).await?;
                    let started_at = now_timestamp();
                    let updated_looper_subtask = self.stores.looper_subtask.update_status(
                        &looper_subtask.looper_subtask_id,
                        LooperSubtaskStatus::Running,
                        None,
                        None,
                        &started_at,
                    )?;
                    let updated_looper_subtask = self.stores.looper_subtask.set_dispatch(
                        &updated_looper_subtask.looper_subtask_id,
                        Some(&dispatch_record.dispatch_id),
                        &started_at,
                    )?;
                    execution_view.insert(subtask_id.clone(), updated_looper_subtask.clone());

                    let mut next_subtask = subtask.clone();
                    next_subtask.status = SubtaskStatus::InProgress;
                    next_subtask.result = None;
                    next_subtask.updated_at = started_at.clone();
                    next_subtask.completed_at = None;
                    self.stores.subtask.update(&next_subtask)?;
                    subtasks_by_id.insert(subtask_id.clone(), next_subtask.clone());

                    let permit = Arc::clone(&semaphore)
                        .acquire_owned()
                        .await
                        .map_err(|_| LooperRuntimeError::SemaphoreClosed)?;
                    let executor = Arc::clone(&self.executor);
                    let looper_run_for_task = looper_run.clone();
                    let dispatch_id = dispatch_record.dispatch_id.clone();
                    join_set.spawn(async move {
                        let _permit = permit;
                        let result = executor
                            .execute_dispatch(&looper_run_for_task, &next_subtask, &dispatch_id)
                            .await;
                        SubtaskCompletion { subtask_id, result }
                    });

                    in_flight.insert(
                        subtask.subtask_id.to_string(),
                        InFlightSubtask {
                            looper_subtask_id: updated_looper_subtask.looper_subtask_id.to_string(),
                            dispatch_id: dispatch_record.dispatch_id.clone(),
                            parallelizable: subtask.parallelizable,
                        },
                    );
                    self.sync_run_projection(&mut looper_run, &ordered_subtasks, &execution_view)?;
                }
            }

            if in_flight.is_empty() {
                match self.determine_idle_outcome(&looper_run, &ordered_subtasks, &execution_view) {
                    IdleOutcome::Pause => {
                        self.stores.looper_run.set_current_subtask(
                            &looper_run.looper_run_id,
                            None,
                            &now_timestamp(),
                        )?;
                        return Ok(());
                    }
                    IdleOutcome::Cancel => {
                        self.stores.looper_run.set_current_subtask(
                            &looper_run.looper_run_id,
                            None,
                            &now_timestamp(),
                        )?;
                        return Ok(());
                    }
                    IdleOutcome::Complete => {
                        let completed_at = now_timestamp();
                        self.stores.looper_run.update_status(
                            &looper_run.looper_run_id,
                            LooperRunStatus::Completed,
                            None,
                            &completed_at,
                            Some(&completed_at),
                        )?;
                        self.stores.looper_run.set_current_subtask(
                            &looper_run.looper_run_id,
                            None,
                            &completed_at,
                        )?;
                        return Ok(());
                    }
                    IdleOutcome::Fail(message) => {
                        let failed_at = now_timestamp();
                        self.stores.looper_run.update_status(
                            &looper_run.looper_run_id,
                            LooperRunStatus::Failed,
                            Some(&serde_json::json!({ "message": message })),
                            &failed_at,
                            Some(&failed_at),
                        )?;
                        self.stores.looper_run.set_current_subtask(
                            &looper_run.looper_run_id,
                            None,
                            &failed_at,
                        )?;
                        return Ok(());
                    }
                }
            }

            let completion = join_set
                .join_next()
                .await
                .ok_or_else(|| LooperRuntimeError::Join("looper join set terminated".to_string()))?
                .map_err(|error| LooperRuntimeError::Join(error.to_string()))?;
            let Some(in_flight_state) = in_flight.remove(&completion.subtask_id) else {
                return Err(LooperRuntimeError::Progress(format!(
                    "completion arrived for unknown subtask '{}'",
                    completion.subtask_id
                )));
            };
            let Some(current_subtask) = subtasks_by_id.get(&completion.subtask_id).cloned() else {
                return Err(LooperRuntimeError::Progress(format!(
                    "completion arrived for missing canonical subtask '{}'",
                    completion.subtask_id
                )));
            };

            let completed_at = now_timestamp();
            match completion.result {
                Ok(result_json) => {
                    self.settle_dispatch_success_if_needed(
                        &in_flight_state.dispatch_id,
                        &result_json,
                        &completed_at,
                    )
                    .await?;
                    self.stores.looper_subtask.update_status(
                        &openfang_types::looper::LooperSubtaskId::new(
                            in_flight_state.looper_subtask_id.clone(),
                        ),
                        LooperSubtaskStatus::Completed,
                        Some(&result_json),
                        None,
                        &completed_at,
                    )?;
                    let mut next_subtask = current_subtask;
                    next_subtask.status = SubtaskStatus::Completed;
                    next_subtask.result = Some(result_json);
                    next_subtask.updated_at = completed_at.clone();
                    next_subtask.completed_at = Some(completed_at.clone());
                    self.stores.subtask.update(&next_subtask)?;
                    subtasks_by_id.insert(completion.subtask_id.clone(), next_subtask);
                }
                Err(message) => {
                    self.settle_dispatch_failure_if_needed(
                        &in_flight_state.dispatch_id,
                        &message,
                        &completed_at,
                    )
                    .await?;
                    let error_json = serde_json::json!({ "message": message });
                    self.stores.looper_subtask.update_status(
                        &openfang_types::looper::LooperSubtaskId::new(
                            in_flight_state.looper_subtask_id.clone(),
                        ),
                        LooperSubtaskStatus::Failed,
                        None,
                        Some(&error_json),
                        &completed_at,
                    )?;
                    let mut next_subtask = current_subtask;
                    next_subtask.status = SubtaskStatus::Failed;
                    next_subtask.result = None;
                    next_subtask.updated_at = completed_at.clone();
                    next_subtask.completed_at = Some(completed_at.clone());
                    self.stores.subtask.update(&next_subtask)?;
                    subtasks_by_id.insert(completion.subtask_id.clone(), next_subtask);
                }
            }

            execution_view = self
                .reload_execution_view(&looper_run.looper_run_id)?
                .into_iter()
                .map(|record| (record.subtask_id.to_string(), record))
                .collect();
        }
    }

    async fn ensure_execution_view(
        &self,
        looper_run: &LooperRunRecord,
        ordered_subtasks: &[SubtaskRecord],
    ) -> Result<HashMap<String, LooperSubtaskRecord>, LooperRuntimeError> {
        let execution_view = self
            .reload_execution_view(&looper_run.looper_run_id)?
            .into_iter()
            .map(|record| (record.subtask_id.to_string(), record))
            .collect::<HashMap<_, _>>();

        for subtask in ordered_subtasks {
            if let Some(record) = execution_view.get(&subtask.subtask_id.to_string()) {
                if record.status == LooperSubtaskStatus::Running {
                    let reset_at = now_timestamp();
                    if let Some(dispatch_id) = record.dispatch_id.as_deref() {
                        self.mark_interrupted_dispatch_failed(dispatch_id, &reset_at)
                            .await?;
                    }
                    self.stores.looper_subtask.update_status(
                        &record.looper_subtask_id,
                        LooperSubtaskStatus::Pending,
                        None,
                        None,
                        &reset_at,
                    )?;
                    self.stores.looper_subtask.set_dispatch(
                        &record.looper_subtask_id,
                        None,
                        &reset_at,
                    )?;
                }
                continue;
            }

            self.stores
                .looper_subtask
                .create_for_run(looper_run, subtask)?;
        }

        Ok(self
            .reload_execution_view(&looper_run.looper_run_id)?
            .into_iter()
            .map(|record| (record.subtask_id.to_string(), record))
            .collect())
    }

    async fn mark_interrupted_dispatch_failed(
        &self,
        dispatch_id: &str,
        timestamp: &str,
    ) -> Result<(), LooperRuntimeError> {
        let Some(current_dispatch) = self.stores.dispatch.find_by_id(dispatch_id).await? else {
            return Ok(());
        };
        if current_dispatch.status.is_terminal() {
            return Ok(());
        }
        self.stores
            .dispatch
            .mark_failed(
                &current_dispatch,
                &serde_json::json!({
                    "message": "looper runtime restarted before the dispatch settled",
                }),
                timestamp,
            )
            .await?;
        Ok(())
    }

    fn reload_execution_view(
        &self,
        looper_run_id: &LooperRunId,
    ) -> Result<Vec<LooperSubtaskRecord>, LooperRuntimeError> {
        Ok(self
            .stores
            .looper_subtask
            .find_by_looper_run(looper_run_id)?)
    }

    fn load_all_subtasks(
        &self,
        task_id: &TaskId,
    ) -> Result<Vec<SubtaskRecord>, LooperRuntimeError> {
        let mut items = Vec::new();
        let mut cursor = None;
        loop {
            let page = self.stores.subtask.list_for_task(
                task_id,
                &SubtaskListQuery {
                    limit: 200,
                    cursor: cursor.clone(),
                    ..SubtaskListQuery::default()
                },
            )?;
            items.extend(page.items);
            if page.next_cursor.is_none() {
                break;
            }
            cursor = page.next_cursor;
        }

        items.sort_by(|left, right| {
            (left.position, left.subtask_id.as_ref())
                .cmp(&(right.position, right.subtask_id.as_ref()))
        });
        Ok(items)
    }

    fn sync_run_projection(
        &self,
        looper_run: &mut LooperRunRecord,
        ordered_subtasks: &[SubtaskRecord],
        execution_view: &HashMap<String, LooperSubtaskRecord>,
    ) -> Result<(), LooperRuntimeError> {
        let progress = compute_progress(execution_view);
        let current_subtask_id = current_running_subtask_id(ordered_subtasks, execution_view);

        if looper_run.progress != progress {
            *looper_run = self.stores.looper_run.update_progress(
                &looper_run.looper_run_id,
                progress,
                &now_timestamp(),
            )?;
        }

        if looper_run.current_subtask_id != current_subtask_id {
            *looper_run = self.stores.looper_run.set_current_subtask(
                &looper_run.looper_run_id,
                current_subtask_id.as_ref(),
                &now_timestamp(),
            )?;
        }

        Ok(())
    }

    async fn create_pending_dispatch(
        &self,
        looper_run: &LooperRunRecord,
        subtask: &SubtaskRecord,
    ) -> Result<DispatchRecord, LooperRuntimeError> {
        let timestamp = now_timestamp();
        let dispatch_record = DispatchRecord {
            dispatch_id: Uuid::new_v4().to_string(),
            run_id: looper_run
                .source_run_id
                .clone()
                .unwrap_or_else(|| looper_run.looper_run_id.to_string()),
            step_id: Some(subtask.subtask_id.to_string()),
            kind: DispatchKind::Call,
            target_agent: subtask
                .assignee
                .as_ref()
                .map(|assignee| assignee.ref_id.clone())
                .unwrap_or_else(|| "unassigned-subtask".to_string()),
            status: DispatchStatus::Pending,
            input_json: dispatch_input_payload(looper_run, subtask),
            result_json: None,
            error_json: None,
            attempt: 1,
            parent_dispatch_id: None,
            spawned_agent_id: None,
            provider_driver: None,
            session_id: None,
            provider_resume_token: None,
            started_at: timestamp.clone(),
            updated_at: timestamp,
            completed_at: None,
        };
        self.stores.dispatch.create(&dispatch_record).await?;
        Ok(dispatch_record)
    }

    async fn settle_dispatch_success_if_needed(
        &self,
        dispatch_id: &str,
        result_json: &JsonValue,
        completed_at: &str,
    ) -> Result<(), LooperRuntimeError> {
        let Some(current_dispatch) = self.stores.dispatch.find_by_id(dispatch_id).await? else {
            return Ok(());
        };
        if current_dispatch.status == DispatchStatus::Completed {
            return Ok(());
        }
        let current_dispatch = self
            .ensure_dispatch_running_before_completion(current_dispatch, completed_at)
            .await?;
        self.stores
            .dispatch
            .mark_completed(&current_dispatch, Some(result_json), completed_at)
            .await?;
        Ok(())
    }

    async fn settle_dispatch_failure_if_needed(
        &self,
        dispatch_id: &str,
        message: &str,
        completed_at: &str,
    ) -> Result<(), LooperRuntimeError> {
        let Some(current_dispatch) = self.stores.dispatch.find_by_id(dispatch_id).await? else {
            return Ok(());
        };
        if current_dispatch.status.is_terminal() {
            return Ok(());
        }
        self.stores
            .dispatch
            .mark_failed(
                &current_dispatch,
                &serde_json::json!({
                    "message": message,
                }),
                completed_at,
            )
            .await?;
        Ok(())
    }

    async fn ensure_dispatch_running_before_completion(
        &self,
        current_dispatch: DispatchRecord,
        updated_at: &str,
    ) -> Result<DispatchRecord, LooperRuntimeError> {
        match current_dispatch.status {
            DispatchStatus::Completed => Ok(current_dispatch),
            DispatchStatus::Running => Ok(current_dispatch),
            DispatchStatus::Pending | DispatchStatus::WaitingHitl => {
                let mut next_dispatch = current_dispatch.clone();
                next_dispatch.status = DispatchStatus::Running;
                next_dispatch.updated_at = updated_at.to_string();
                next_dispatch.completed_at = None;
                Ok(self
                    .stores
                    .dispatch
                    .update_status(&current_dispatch, &next_dispatch)
                    .await?)
            }
            DispatchStatus::Failed | DispatchStatus::Cancelled => {
                Err(LooperRuntimeError::Progress(format!(
                    "dispatch '{dispatch_id}' reached terminal status '{}' before looper completion",
                    current_dispatch.status,
                    dispatch_id = current_dispatch.dispatch_id,
                )))
            }
        }
    }

    fn determine_idle_outcome(
        &self,
        looper_run: &LooperRunRecord,
        ordered_subtasks: &[SubtaskRecord],
        execution_view: &HashMap<String, LooperSubtaskRecord>,
    ) -> IdleOutcome {
        match looper_run.status {
            LooperRunStatus::Paused => return IdleOutcome::Pause,
            LooperRunStatus::Cancelled => return IdleOutcome::Cancel,
            LooperRunStatus::Completed => return IdleOutcome::Complete,
            LooperRunStatus::Failed => {
                return IdleOutcome::Fail("looper run is already marked failed".to_string());
            }
            LooperRunStatus::Pending | LooperRunStatus::Running => {}
        }

        let pending = ordered_subtasks
            .iter()
            .filter(|subtask| {
                execution_view
                    .get(&subtask.subtask_id.to_string())
                    .map(|record| record.status == LooperSubtaskStatus::Pending)
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        if pending.is_empty() {
            if has_failed_execution(execution_view) {
                return IdleOutcome::Fail(
                    "one or more subtasks failed during looper execution".to_string(),
                );
            }
            return IdleOutcome::Complete;
        }

        let settled = execution_view.values().fold(
            HashMap::<String, LooperSubtaskStatus>::new(),
            |mut statuses, record| {
                statuses.insert(record.subtask_id.to_string(), record.status);
                statuses
            },
        );
        let blocked_by_terminal_dependency = pending.iter().any(|subtask| {
            subtask.depends_on.iter().any(|dependency_id| {
                matches!(
                    settled.get(dependency_id.as_ref()),
                    Some(LooperSubtaskStatus::Failed | LooperSubtaskStatus::Cancelled)
                )
            })
        });
        if blocked_by_terminal_dependency {
            return IdleOutcome::Fail(
                "pending subtasks are blocked by failed or cancelled dependencies".to_string(),
            );
        }

        IdleOutcome::Fail(
            "no eligible subtasks remain and the looper cannot make further progress".to_string(),
        )
    }
}

#[derive(Debug)]
enum IdleOutcome {
    Pause,
    Cancel,
    Complete,
    Fail(String),
}

fn compute_progress(execution_view: &HashMap<String, LooperSubtaskRecord>) -> LooperProgress {
    execution_view
        .values()
        .fold(LooperProgress::default(), |mut progress, record| {
            progress.total += 1;
            match record.status {
                LooperSubtaskStatus::Completed => progress.completed += 1,
                LooperSubtaskStatus::Failed => progress.failed += 1,
                LooperSubtaskStatus::Pending
                | LooperSubtaskStatus::Running
                | LooperSubtaskStatus::Cancelled => {}
            }
            progress
        })
}

fn current_running_subtask_id(
    ordered_subtasks: &[SubtaskRecord],
    execution_view: &HashMap<String, LooperSubtaskRecord>,
) -> Option<SubtaskId> {
    ordered_subtasks.iter().find_map(|subtask| {
        execution_view
            .get(subtask.subtask_id.as_ref())
            .filter(|record| record.status == LooperSubtaskStatus::Running)
            .map(|_| subtask.subtask_id.clone())
    })
}

fn has_failed_execution(execution_view: &HashMap<String, LooperSubtaskRecord>) -> bool {
    execution_view
        .values()
        .any(|record| record.status == LooperSubtaskStatus::Failed)
}

fn select_next_subtask(
    ordered_subtasks: &[SubtaskRecord],
    execution_view: &HashMap<String, LooperSubtaskRecord>,
    in_flight: &HashMap<String, InFlightSubtask>,
    effective_parallelism: usize,
) -> Option<String> {
    if in_flight.len() >= effective_parallelism {
        return None;
    }

    if in_flight.values().any(|entry| !entry.parallelizable) {
        return None;
    }

    let settled = execution_view
        .iter()
        .map(|(subtask_id, record)| (subtask_id.clone(), record.status))
        .collect::<HashMap<_, _>>();
    let in_flight_ids = in_flight.keys().cloned().collect::<HashSet<_>>();

    for subtask in ordered_subtasks {
        if in_flight_ids.contains(subtask.subtask_id.as_ref()) {
            continue;
        }
        let Some(record) = execution_view.get(subtask.subtask_id.as_ref()) else {
            continue;
        };
        if record.status != LooperSubtaskStatus::Pending {
            continue;
        }
        if !dependencies_completed(&subtask.depends_on, &settled) {
            continue;
        }
        if !subtask.parallelizable && !in_flight.is_empty() {
            return None;
        }

        return Some(subtask.subtask_id.to_string());
    }

    None
}

fn dependencies_completed(
    depends_on: &[SubtaskId],
    settled: &HashMap<String, LooperSubtaskStatus>,
) -> bool {
    depends_on.iter().all(|dependency_id| {
        settled
            .get(dependency_id.as_ref())
            .copied()
            .unwrap_or(LooperSubtaskStatus::Pending)
            == LooperSubtaskStatus::Completed
    })
}

fn dispatch_input_payload(looper_run: &LooperRunRecord, subtask: &SubtaskRecord) -> JsonValue {
    let mut payload = BTreeMap::new();
    payload.insert(
        "looper_run_id".to_string(),
        JsonValue::String(looper_run.looper_run_id.to_string()),
    );
    payload.insert(
        "task_id".to_string(),
        JsonValue::String(looper_run.task_id.to_string()),
    );
    payload.insert(
        "subtask_id".to_string(),
        JsonValue::String(subtask.subtask_id.to_string()),
    );
    payload.insert(
        "title".to_string(),
        JsonValue::String(subtask.title.clone()),
    );
    payload.insert(
        "description".to_string(),
        JsonValue::String(subtask.description.clone()),
    );
    payload.insert("input".to_string(), subtask.input.clone());
    JsonValue::Object(payload.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use openfang_memory::{
        AGENT_DISPATCH_MIGRATION_SQL, LOOPER_RUNTIME_MIGRATION_SQL, TASK_SUBTASK_MIGRATION_SQL,
    };
    use openfang_types::task::{
        ActorKind, AssigneeRef, Complexity, OwnerRef, Priority, SubtaskKind, TaskSource, TaskStatus,
    };
    use pretty_assertions::assert_eq;
    use rusqlite::Connection;
    use std::collections::HashMap as StdHashMap;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex as StdMutex};
    use std::time::Duration;
    use tempfile::tempdir;
    use tokio::sync::{oneshot, Mutex};

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

    fn in_memory_stores() -> WorkflowStoreSet {
        let conn = Connection::open_in_memory().expect("open in-memory compozy.db");
        configure_test_connection(&conn);
        conn.execute_batch(TASK_SUBTASK_MIGRATION_SQL)
            .expect("apply task/subtask migration");
        conn.execute_batch(AGENT_DISPATCH_MIGRATION_SQL)
            .expect("apply agent dispatch migration");
        conn.execute_batch(LOOPER_RUNTIME_MIGRATION_SQL)
            .expect("apply looper runtime migration");
        WorkflowStoreSet::new(Arc::new(std::sync::Mutex::new(conn)))
    }

    fn file_backed_stores(path: &Path) -> WorkflowStoreSet {
        let conn = Connection::open(path).expect("open file-backed compozy.db");
        configure_test_connection(&conn);
        conn.execute_batch(TASK_SUBTASK_MIGRATION_SQL)
            .expect("apply task/subtask migration");
        conn.execute_batch(AGENT_DISPATCH_MIGRATION_SQL)
            .expect("apply agent dispatch migration");
        conn.execute_batch(LOOPER_RUNTIME_MIGRATION_SQL)
            .expect("apply looper runtime migration");
        WorkflowStoreSet::new(Arc::new(std::sync::Mutex::new(conn)))
    }

    fn sample_task(task_id: &str) -> openfang_types::task::TaskRecord {
        openfang_types::task::TaskRecord {
            task_id: TaskId::new(task_id),
            slug: format!("slug-{task_id}"),
            source: TaskSource::Workflow {
                workflow_id: "sdlc".to_string(),
                run_id: "run_123".to_string(),
            },
            title: format!("Task {task_id}"),
            description: "Run the looper".to_string(),
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
            repository_refs: vec![],
            label_refs: vec![],
            artifact_refs: vec![],
            doc_refs: vec![],
            file_refs: vec![],
            metadata: serde_json::json!({}),
            created_at: "2026-03-25T10:00:00Z".to_string(),
            updated_at: "2026-03-25T10:00:00Z".to_string(),
            completed_at: None,
        }
    }

    fn sample_subtask(
        task_id: &TaskId,
        subtask_id: &str,
        position: i64,
        parallelizable: bool,
        depends_on: Vec<SubtaskId>,
    ) -> SubtaskRecord {
        SubtaskRecord {
            subtask_id: SubtaskId::new(subtask_id),
            task_id: task_id.clone(),
            title: format!("Subtask {subtask_id}"),
            description: format!("Execute {subtask_id}"),
            kind: SubtaskKind::CodeChange,
            status: SubtaskStatus::Ready,
            complexity: Complexity::Medium,
            position,
            assignee: Some(AssigneeRef {
                kind: ActorKind::Agent,
                ref_id: "looper-agent".to_string(),
            }),
            depends_on,
            parallelizable,
            input: serde_json::json!({
                "subtask_id": subtask_id,
            }),
            result: None,
            metadata: serde_json::json!({}),
            created_at: "2026-03-25T10:01:00Z".to_string(),
            updated_at: "2026-03-25T10:01:00Z".to_string(),
            completed_at: None,
        }
    }

    fn seed_run(
        stores: &WorkflowStoreSet,
        task_id: &str,
        mode: LooperExecutionMode,
        max_parallelism: u32,
        subtasks: Vec<SubtaskRecord>,
    ) -> LooperRunRecord {
        let task = sample_task(task_id);
        stores.task.create(&task).expect("task should persist");
        for subtask in &subtasks {
            stores
                .subtask
                .create(subtask)
                .expect("subtask should persist");
        }
        stores
            .looper_run
            .create(&openfang_memory::NewLooperRun {
                looper_run_id: LooperRunId::new(format!("loop-{task_id}")),
                task_id: task.task_id,
                source_run_id: Some("run_123".to_string()),
                status: LooperRunStatus::Running,
                execution_policy_json: serde_json::json!({
                    "mode": mode,
                    "max_parallelism": max_parallelism,
                    "selection": "priority",
                }),
                current_subtask_id: None,
                progress_json: serde_json::json!({
                    "total": 0,
                    "completed": 0,
                    "failed": 0,
                }),
                error_json: None,
                started_at: "2026-03-25T10:02:00Z".to_string(),
                updated_at: "2026-03-25T10:02:00Z".to_string(),
                completed_at: None,
            })
            .expect("looper run should persist")
    }

    async fn wait_for_started_count(executor: &ControlledExecutor, count: usize) {
        for _ in 0..200 {
            if executor.started_order().await.len() >= count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for {count} started subtasks");
    }

    async fn wait_for_looper_completion(
        stores: &WorkflowStoreSet,
        looper_run_id: &LooperRunId,
    ) -> LooperRunRecord {
        for _ in 0..200 {
            let record = stores
                .looper_run
                .find_by_id(looper_run_id)
                .expect("looper run lookup should succeed")
                .expect("looper run should exist");
            if record.status.is_terminal() {
                return record;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for looper completion");
    }

    fn running_subtask_count(stores: &WorkflowStoreSet, looper_run_id: &LooperRunId) -> usize {
        stores
            .looper_subtask
            .find_by_looper_run(looper_run_id)
            .expect("looper subtask lookup should succeed")
            .into_iter()
            .filter(|record| record.status == LooperSubtaskStatus::Running)
            .count()
    }

    fn dispatch_count(stores: &WorkflowStoreSet) -> usize {
        let conn = stores.connection();
        let guard = conn.lock().expect("connection lock");
        guard
            .query_row("SELECT COUNT(*) FROM agent_dispatch", [], |row| {
                row.get::<_, usize>(0)
            })
            .expect("dispatch count query")
    }

    struct ImmediateExecutor;

    #[async_trait]
    impl LooperDispatchExecutor for ImmediateExecutor {
        async fn execute_dispatch(
            &self,
            _looper_run: &LooperRunRecord,
            subtask: &SubtaskRecord,
            _dispatch_id: &str,
        ) -> Result<JsonValue, String> {
            Ok(serde_json::json!({
                "subtask_id": subtask.subtask_id.to_string(),
            }))
        }
    }

    struct ControlledExecutor {
        receivers: Mutex<StdHashMap<String, oneshot::Receiver<Result<JsonValue, String>>>>,
        senders: StdMutex<StdHashMap<String, oneshot::Sender<Result<JsonValue, String>>>>,
        started: Mutex<Vec<String>>,
        current_concurrency: AtomicUsize,
        max_concurrency: AtomicUsize,
    }

    impl ControlledExecutor {
        fn new(subtask_ids: &[String]) -> Self {
            let mut receivers = StdHashMap::new();
            let mut senders = StdHashMap::new();
            for subtask_id in subtask_ids {
                let (sender, receiver) = oneshot::channel();
                receivers.insert(subtask_id.clone(), receiver);
                senders.insert(subtask_id.clone(), sender);
            }
            Self {
                receivers: Mutex::new(receivers),
                senders: StdMutex::new(senders),
                started: Mutex::new(Vec::new()),
                current_concurrency: AtomicUsize::new(0),
                max_concurrency: AtomicUsize::new(0),
            }
        }

        async fn started_order(&self) -> Vec<String> {
            self.started.lock().await.clone()
        }

        fn max_concurrency(&self) -> usize {
            self.max_concurrency.load(Ordering::SeqCst)
        }

        fn complete_success(&self, subtask_id: &str) {
            let sender = self
                .senders
                .lock()
                .expect("sender lock")
                .remove(subtask_id)
                .expect("sender should exist");
            sender
                .send(Ok(serde_json::json!({
                    "subtask_id": subtask_id,
                })))
                .expect("completion should send");
        }
    }

    #[async_trait]
    impl LooperDispatchExecutor for ControlledExecutor {
        async fn execute_dispatch(
            &self,
            _looper_run: &LooperRunRecord,
            subtask: &SubtaskRecord,
            _dispatch_id: &str,
        ) -> Result<JsonValue, String> {
            let current = self.current_concurrency.fetch_add(1, Ordering::SeqCst) + 1;
            loop {
                let observed = self.max_concurrency.load(Ordering::SeqCst);
                if current <= observed {
                    break;
                }
                if self
                    .max_concurrency
                    .compare_exchange(observed, current, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    break;
                }
            }

            self.started
                .lock()
                .await
                .push(subtask.subtask_id.to_string());
            let receiver = self
                .receivers
                .lock()
                .await
                .remove(subtask.subtask_id.as_ref())
                .expect("receiver should exist");
            let result = receiver.await.expect("completion should arrive");
            self.current_concurrency.fetch_sub(1, Ordering::SeqCst);
            result
        }
    }

    #[tokio::test]
    async fn sequential_mode_should_dispatch_one_subtask_at_a_time() {
        let stores = in_memory_stores();
        let subtasks = vec![
            sample_subtask(&TaskId::new("task-seq"), "subtask-1", 1, true, vec![]),
            sample_subtask(&TaskId::new("task-seq"), "subtask-2", 2, true, vec![]),
            sample_subtask(&TaskId::new("task-seq"), "subtask-3", 3, true, vec![]),
        ];
        let looper_run = seed_run(
            &stores,
            "task-seq",
            LooperExecutionMode::Sequential,
            1,
            subtasks,
        );
        let executor = Arc::new(ControlledExecutor::new(&[
            "subtask-1".to_string(),
            "subtask-2".to_string(),
            "subtask-3".to_string(),
        ]));
        let runtime = LooperRuntime::new(stores.clone(), executor.clone());
        let looper_run_id = looper_run.looper_run_id.clone();

        let task = tokio::spawn(async move {
            runtime
                .run(&looper_run_id, CancellationToken::new())
                .await
                .expect("runtime should complete");
        });

        wait_for_started_count(&executor, 1).await;
        assert_eq!(running_subtask_count(&stores, &looper_run.looper_run_id), 1);
        assert_eq!(executor.max_concurrency(), 1);
        assert!(tokio::time::timeout(
            Duration::from_millis(100),
            wait_for_started_count(&executor, 2)
        )
        .await
        .is_err());

        executor.complete_success("subtask-1");
        wait_for_started_count(&executor, 2).await;
        assert_eq!(running_subtask_count(&stores, &looper_run.looper_run_id), 1);

        executor.complete_success("subtask-2");
        wait_for_started_count(&executor, 3).await;
        assert_eq!(running_subtask_count(&stores, &looper_run.looper_run_id), 1);

        executor.complete_success("subtask-3");
        task.await.expect("join should succeed");
    }

    #[tokio::test]
    async fn parallel_mode_should_respect_max_parallelism() {
        let stores = in_memory_stores();
        let subtasks = vec![
            sample_subtask(&TaskId::new("task-par"), "subtask-1", 1, true, vec![]),
            sample_subtask(&TaskId::new("task-par"), "subtask-2", 2, true, vec![]),
            sample_subtask(&TaskId::new("task-par"), "subtask-3", 3, true, vec![]),
        ];
        let looper_run = seed_run(
            &stores,
            "task-par",
            LooperExecutionMode::Parallel,
            2,
            subtasks,
        );
        let executor = Arc::new(ControlledExecutor::new(&[
            "subtask-1".to_string(),
            "subtask-2".to_string(),
            "subtask-3".to_string(),
        ]));
        let runtime = LooperRuntime::new(stores.clone(), executor.clone());
        let looper_run_id = looper_run.looper_run_id.clone();

        let task = tokio::spawn(async move {
            runtime
                .run(&looper_run_id, CancellationToken::new())
                .await
                .expect("runtime should complete");
        });

        wait_for_started_count(&executor, 2).await;
        assert_eq!(executor.max_concurrency(), 2);
        assert_eq!(running_subtask_count(&stores, &looper_run.looper_run_id), 2);
        assert!(tokio::time::timeout(
            Duration::from_millis(100),
            wait_for_started_count(&executor, 3)
        )
        .await
        .is_err());

        executor.complete_success("subtask-1");
        wait_for_started_count(&executor, 3).await;
        executor.complete_success("subtask-2");
        executor.complete_success("subtask-3");
        task.await.expect("join should succeed");
    }

    #[tokio::test]
    async fn non_parallelizable_subtask_should_wait_for_in_flight_dispatches() {
        let stores = in_memory_stores();
        let subtasks = vec![
            sample_subtask(&TaskId::new("task-serial"), "subtask-1", 1, true, vec![]),
            sample_subtask(&TaskId::new("task-serial"), "subtask-2", 2, true, vec![]),
            sample_subtask(&TaskId::new("task-serial"), "subtask-3", 3, false, vec![]),
        ];
        let looper_run = seed_run(
            &stores,
            "task-serial",
            LooperExecutionMode::Parallel,
            2,
            subtasks,
        );
        let executor = Arc::new(ControlledExecutor::new(&[
            "subtask-1".to_string(),
            "subtask-2".to_string(),
            "subtask-3".to_string(),
        ]));
        let runtime = LooperRuntime::new(stores.clone(), executor.clone());
        let looper_run_id = looper_run.looper_run_id.clone();

        let task = tokio::spawn(async move {
            runtime
                .run(&looper_run_id, CancellationToken::new())
                .await
                .expect("runtime should complete");
        });

        wait_for_started_count(&executor, 2).await;
        assert!(tokio::time::timeout(
            Duration::from_millis(100),
            wait_for_started_count(&executor, 3)
        )
        .await
        .is_err());

        executor.complete_success("subtask-1");
        assert!(tokio::time::timeout(
            Duration::from_millis(100),
            wait_for_started_count(&executor, 3)
        )
        .await
        .is_err());

        executor.complete_success("subtask-2");
        wait_for_started_count(&executor, 3).await;
        executor.complete_success("subtask-3");
        task.await.expect("join should succeed");
    }

    #[tokio::test]
    async fn depends_on_should_delay_selection_until_dependency_completes() {
        let stores = in_memory_stores();
        let dependency = SubtaskId::new("subtask-1");
        let subtasks = vec![
            sample_subtask(&TaskId::new("task-deps"), "subtask-1", 1, true, vec![]),
            sample_subtask(
                &TaskId::new("task-deps"),
                "subtask-2",
                2,
                true,
                vec![dependency],
            ),
            sample_subtask(&TaskId::new("task-deps"), "subtask-3", 3, true, vec![]),
        ];
        let looper_run = seed_run(
            &stores,
            "task-deps",
            LooperExecutionMode::Parallel,
            2,
            subtasks,
        );
        let executor = Arc::new(ControlledExecutor::new(&[
            "subtask-1".to_string(),
            "subtask-2".to_string(),
            "subtask-3".to_string(),
        ]));
        let runtime = LooperRuntime::new(stores.clone(), executor.clone());
        let looper_run_id = looper_run.looper_run_id.clone();

        let task = tokio::spawn(async move {
            runtime
                .run(&looper_run_id, CancellationToken::new())
                .await
                .expect("runtime should complete");
        });

        wait_for_started_count(&executor, 2).await;
        let started = executor.started_order().await;
        assert_eq!(
            started,
            vec!["subtask-1".to_string(), "subtask-3".to_string()]
        );

        executor.complete_success("subtask-3");
        assert!(tokio::time::timeout(
            Duration::from_millis(100),
            wait_for_started_count(&executor, 3)
        )
        .await
        .is_err());

        executor.complete_success("subtask-1");
        wait_for_started_count(&executor, 3).await;
        executor.complete_success("subtask-2");
        task.await.expect("join should succeed");
    }

    #[tokio::test]
    async fn sequential_run_should_complete_all_five_subtasks() {
        let stores = in_memory_stores();
        let task_id = TaskId::new("task-five");
        let subtasks = (1..=5)
            .map(|index| {
                sample_subtask(
                    &task_id,
                    &format!("subtask-{index}"),
                    i64::from(index),
                    true,
                    vec![],
                )
            })
            .collect::<Vec<_>>();
        let looper_run = seed_run(
            &stores,
            "task-five",
            LooperExecutionMode::Sequential,
            1,
            subtasks,
        );
        let runtime = LooperRuntime::new(stores.clone(), Arc::new(ImmediateExecutor));

        runtime
            .run(&looper_run.looper_run_id, CancellationToken::new())
            .await
            .expect("runtime should complete");

        let completed = stores
            .looper_run
            .find_by_id(&looper_run.looper_run_id)
            .expect("looper run lookup should succeed")
            .expect("looper run should exist");
        assert_eq!(completed.status, LooperRunStatus::Completed);
        assert_eq!(completed.progress.total, 5);
        assert_eq!(completed.progress.completed, 5);
        assert_eq!(completed.progress.failed, 0);
    }

    #[tokio::test]
    async fn restart_recovery_should_resume_only_pending_subtasks() {
        let tmp = tempdir().expect("temp dir");
        let db_path = tmp.path().join("compozy.db");
        let looper_run_id = {
            let stores = file_backed_stores(&db_path);
            let task_id = TaskId::new("task-recovery");
            let subtasks = (1..=5)
                .map(|index| {
                    sample_subtask(
                        &task_id,
                        &format!("subtask-{index}"),
                        i64::from(index),
                        true,
                        vec![],
                    )
                })
                .collect::<Vec<_>>();
            let looper_run = seed_run(
                &stores,
                "task-recovery",
                LooperExecutionMode::Sequential,
                1,
                subtasks.clone(),
            );
            for subtask in &subtasks {
                stores
                    .looper_subtask
                    .create_for_run(&looper_run, subtask)
                    .expect("looper execution row should persist");
            }
            for subtask_id in ["subtask-1", "subtask-2", "subtask-3"] {
                let execution_view = stores
                    .looper_subtask
                    .find_by_looper_run(&looper_run.looper_run_id)
                    .expect("execution view query should succeed");
                let record = execution_view
                    .into_iter()
                    .find(|record| record.subtask_id.as_ref() == subtask_id)
                    .expect("looper subtask should exist");
                stores
                    .looper_subtask
                    .update_status(
                        &record.looper_subtask_id,
                        LooperSubtaskStatus::Completed,
                        Some(&serde_json::json!({ "subtask_id": subtask_id })),
                        None,
                        "2026-03-25T10:03:00Z",
                    )
                    .expect("looper subtask should complete");
                let mut canonical = stores
                    .subtask
                    .find_by_id(&SubtaskId::new(subtask_id))
                    .expect("canonical subtask lookup should succeed")
                    .expect("canonical subtask should exist");
                canonical.status = SubtaskStatus::Completed;
                canonical.result = Some(serde_json::json!({ "subtask_id": subtask_id }));
                canonical.updated_at = "2026-03-25T10:03:00Z".to_string();
                canonical.completed_at = Some("2026-03-25T10:03:00Z".to_string());
                stores
                    .subtask
                    .update(&canonical)
                    .expect("canonical subtask should update");
            }
            stores
                .looper_run
                .update_progress(
                    &looper_run.looper_run_id,
                    LooperProgress {
                        total: 5,
                        completed: 3,
                        failed: 0,
                    },
                    "2026-03-25T10:03:00Z",
                )
                .expect("progress should update");

            looper_run.looper_run_id
        };

        let recovered_stores = file_backed_stores(&db_path);
        let executor = Arc::new(ControlledExecutor::new(&[
            "subtask-4".to_string(),
            "subtask-5".to_string(),
        ]));
        let runtime = LooperRuntime::new(recovered_stores.clone(), executor.clone());
        let recovery_task = tokio::spawn({
            let looper_run_id = looper_run_id.clone();
            async move {
                runtime
                    .run(&looper_run_id, CancellationToken::new())
                    .await
                    .expect("recovered runtime should complete");
            }
        });

        wait_for_started_count(&executor, 1).await;
        executor.complete_success("subtask-4");
        wait_for_started_count(&executor, 2).await;
        executor.complete_success("subtask-5");
        recovery_task.await.expect("join should succeed");

        let completed = recovered_stores
            .looper_run
            .find_by_id(&looper_run_id)
            .expect("looper run lookup should succeed")
            .expect("looper run should exist");
        assert_eq!(completed.status, LooperRunStatus::Completed);
        assert_eq!(completed.progress.total, 5);
        assert_eq!(completed.progress.completed, 5);
        assert_eq!(
            executor.started_order().await,
            vec!["subtask-4".to_string(), "subtask-5".to_string()]
        );
    }

    #[tokio::test]
    async fn pause_and_resume_should_continue_from_remaining_pending_subtasks() {
        let stores = in_memory_stores();
        let task_id = TaskId::new("task-pause");
        let subtasks = (1..=3)
            .map(|index| {
                sample_subtask(
                    &task_id,
                    &format!("subtask-{index}"),
                    i64::from(index),
                    true,
                    vec![],
                )
            })
            .collect::<Vec<_>>();
        let looper_run = seed_run(
            &stores,
            "task-pause",
            LooperExecutionMode::Sequential,
            1,
            subtasks,
        );
        let executor = Arc::new(ControlledExecutor::new(&[
            "subtask-1".to_string(),
            "subtask-2".to_string(),
            "subtask-3".to_string(),
        ]));
        let runtime = LooperRuntime::new(stores.clone(), executor.clone());

        let first_pass = tokio::spawn({
            let looper_run_id = looper_run.looper_run_id.clone();
            async move {
                runtime
                    .run(&looper_run_id, CancellationToken::new())
                    .await
                    .expect("runtime should pause cleanly");
            }
        });

        wait_for_started_count(&executor, 1).await;
        stores
            .looper_run
            .pause(&looper_run.looper_run_id, "2026-03-25T10:04:00Z")
            .expect("pause should persist");
        assert!(tokio::time::timeout(
            Duration::from_millis(100),
            wait_for_started_count(&executor, 2)
        )
        .await
        .is_err());

        executor.complete_success("subtask-1");
        first_pass.await.expect("join should succeed");

        let paused = stores
            .looper_run
            .find_by_id(&looper_run.looper_run_id)
            .expect("looper run lookup should succeed")
            .expect("looper run should exist");
        assert_eq!(paused.status, LooperRunStatus::Paused);
        assert_eq!(paused.progress.completed, 1);

        stores
            .looper_run
            .resume(&looper_run.looper_run_id, "2026-03-25T10:05:00Z")
            .expect("resume should persist");
        let resumed_runtime = LooperRuntime::new(stores.clone(), executor.clone());
        let second_pass = tokio::spawn({
            let looper_run_id = looper_run.looper_run_id.clone();
            async move {
                resumed_runtime
                    .run(&looper_run_id, CancellationToken::new())
                    .await
                    .expect("runtime should complete after resume");
            }
        });

        wait_for_started_count(&executor, 2).await;
        executor.complete_success("subtask-2");
        wait_for_started_count(&executor, 3).await;
        executor.complete_success("subtask-3");
        second_pass.await.expect("join should succeed");

        let completed = stores
            .looper_run
            .find_by_id(&looper_run.looper_run_id)
            .expect("looper run lookup should succeed")
            .expect("looper run should exist");
        assert_eq!(completed.status, LooperRunStatus::Completed);
        assert_eq!(completed.progress.completed, 3);
    }

    #[tokio::test]
    async fn cancel_should_stop_future_subtask_selection() {
        let stores = in_memory_stores();
        let task_id = TaskId::new("task-cancel");
        let subtasks = (1..=3)
            .map(|index| {
                sample_subtask(
                    &task_id,
                    &format!("subtask-{index}"),
                    i64::from(index),
                    true,
                    vec![],
                )
            })
            .collect::<Vec<_>>();
        let looper_run = seed_run(
            &stores,
            "task-cancel",
            LooperExecutionMode::Sequential,
            1,
            subtasks,
        );
        let executor = Arc::new(ControlledExecutor::new(&[
            "subtask-1".to_string(),
            "subtask-2".to_string(),
            "subtask-3".to_string(),
        ]));
        let runtime = LooperRuntime::new(stores.clone(), executor.clone());

        let cancel_task = tokio::spawn({
            let looper_run_id = looper_run.looper_run_id.clone();
            async move {
                runtime
                    .run(&looper_run_id, CancellationToken::new())
                    .await
                    .expect("runtime should cancel cleanly");
            }
        });

        wait_for_started_count(&executor, 1).await;
        stores
            .looper_run
            .cancel(
                &looper_run.looper_run_id,
                Some(&serde_json::json!({ "message": "cancelled" })),
                "2026-03-25T10:06:00Z",
            )
            .expect("cancel should persist");
        executor.complete_success("subtask-1");
        cancel_task.await.expect("join should succeed");

        assert!(tokio::time::timeout(
            Duration::from_millis(100),
            wait_for_started_count(&executor, 2)
        )
        .await
        .is_err());
        let cancelled = stores
            .looper_run
            .find_by_id(&looper_run.looper_run_id)
            .expect("looper run lookup should succeed")
            .expect("looper run should exist");
        assert_eq!(cancelled.status, LooperRunStatus::Cancelled);
        assert_eq!(cancelled.progress.completed, 1);
    }

    #[tokio::test]
    async fn parallel_run_should_complete_ten_subtasks_with_bounded_concurrency() {
        let stores = in_memory_stores();
        let task_id = TaskId::new("task-ten");
        let subtasks = (1..=10)
            .map(|index| {
                sample_subtask(
                    &task_id,
                    &format!("subtask-{index}"),
                    i64::from(index),
                    true,
                    vec![],
                )
            })
            .collect::<Vec<_>>();
        let looper_run = seed_run(
            &stores,
            "task-ten",
            LooperExecutionMode::Parallel,
            3,
            subtasks,
        );
        let executor = Arc::new(ControlledExecutor::new(
            &(1..=10)
                .map(|index| format!("subtask-{index}"))
                .collect::<Vec<_>>(),
        ));
        let runtime = LooperRuntime::new(stores.clone(), executor.clone());
        let looper_run_id = looper_run.looper_run_id.clone();

        let task = tokio::spawn(async move {
            runtime
                .run(&looper_run_id, CancellationToken::new())
                .await
                .expect("runtime should complete");
        });

        let mut released = 0usize;
        for expected_started in [3usize, 6, 9, 10] {
            wait_for_started_count(&executor, expected_started).await;
            let started = executor.started_order().await;
            for subtask_id in &started[released..expected_started] {
                executor.complete_success(subtask_id);
            }
            released = expected_started;
        }
        task.await.expect("join should succeed");

        let completed = wait_for_looper_completion(&stores, &looper_run.looper_run_id).await;
        assert_eq!(completed.status, LooperRunStatus::Completed);
        assert_eq!(completed.progress.total, 10);
        assert_eq!(completed.progress.completed, 10);
        assert_eq!(completed.progress.failed, 0);
        assert_eq!(executor.max_concurrency(), 3);
        assert_eq!(dispatch_count(&stores), 10);
    }
}
