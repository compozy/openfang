//! Cron job scheduler engine for the OpenFang kernel.
//!
//! Manages scheduled jobs (recurring and one-shot) across all agents.
//! This is separate from `scheduler.rs` which handles agent resource tracking.
//!
//! The scheduler stores jobs in a `DashMap` for concurrent access, persists
//! them to a JSON file on disk, and exposes methods for the kernel tick loop
//! to query due jobs and record outcomes.

use chrono::{Duration, Utc};
use dashmap::DashMap;
use openfang_memory::{
    ScheduleExecutionRecord, ScheduleExecutionStore, ScheduleRuntimeRecord, ScheduleRuntimeStore,
};
use openfang_types::agent::AgentId;
use openfang_types::error::{OpenFangError, OpenFangResult};
use openfang_types::scheduler::{
    CronDefinitionForkedFrom, CronDefinitionOrigin, CronJob, CronJobId, CronSchedule,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::{debug, info, warn};

/// Maximum consecutive errors before a job is auto-disabled.
const MAX_CONSECUTIVE_ERRORS: u32 = 5;

// ---------------------------------------------------------------------------
// JobMeta — extra runtime state not stored in CronJob itself
// ---------------------------------------------------------------------------

/// Runtime metadata for a cron job that extends the base `CronJob` type.
///
/// The `CronJob` struct in `openfang-types` is intentionally lean (no
/// `one_shot`, `last_status`, or error tracking). The scheduler tracks
/// these operational details separately.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobMeta {
    /// The underlying job definition.
    pub job: CronJob,
    /// Stable public schedule definition identifier.
    #[serde(default)]
    pub definition_id: String,
    /// Stable public agent reference (definition ID or runtime name).
    #[serde(default)]
    pub agent_ref: String,
    /// Public definition origin metadata.
    #[serde(default = "CronDefinitionOrigin::user")]
    pub origin: CronDefinitionOrigin,
    /// Optional upstream provenance when this definition is forked.
    #[serde(default)]
    pub forked_from: Option<CronDefinitionForkedFrom>,
    /// Last definition-level update timestamp (RFC 3339).
    #[serde(default)]
    pub updated_at: String,
    /// Whether this job should be removed after a single successful execution.
    pub one_shot: bool,
    /// Human-readable status of the last execution (e.g. `"ok"` or `"error: ..."`).
    pub last_status: Option<String>,
    /// Number of consecutive failed executions.
    pub consecutive_errors: u32,
}

impl JobMeta {
    /// Wrap a `CronJob` with default metadata.
    pub fn new(job: CronJob, one_shot: bool) -> Self {
        Self {
            definition_id: job.id.to_string(),
            agent_ref: job.agent_id.to_string(),
            origin: CronDefinitionOrigin::user(),
            forked_from: None,
            updated_at: job.created_at.to_rfc3339(),
            job,
            one_shot,
            last_status: None,
            consecutive_errors: 0,
        }
    }

    fn normalize(mut self) -> Self {
        if self.definition_id.is_empty() {
            self.definition_id = self.job.id.to_string();
        }
        if self.agent_ref.is_empty() {
            self.agent_ref = self.job.agent_id.to_string();
        }
        if self.updated_at.is_empty() {
            self.updated_at = self.job.created_at.to_rfc3339();
        }
        self
    }
}

// ---------------------------------------------------------------------------
// CronScheduler
// ---------------------------------------------------------------------------

/// Cron job scheduler — manages scheduled jobs for all agents.
///
/// Thread-safe via `DashMap`. The kernel should call [`due_jobs`] on a
/// regular interval (e.g. every 10-30 seconds) to discover jobs that need
/// to fire, then call [`record_success`] or [`record_failure`] after
/// execution completes.
pub struct CronScheduler {
    /// All tracked jobs, keyed by their unique ID.
    jobs: DashMap<CronJobId, JobMeta>,
    /// Path to the persistence file (`<home>/cron_jobs.json`).
    persist_path: PathBuf,
    /// Global cap on total jobs across all agents (atomic for hot-reload).
    max_total_jobs: AtomicUsize,
    /// Best-effort runtime projection store for schedule state.
    schedule_runtime_store: Option<ScheduleRuntimeStore>,
    /// Best-effort receipt store for fired schedule executions.
    schedule_execution_store: Option<ScheduleExecutionStore>,
}

impl CronScheduler {
    /// Create a new scheduler.
    ///
    /// `home_dir` is the OpenFang data directory; jobs are persisted to
    /// `<home_dir>/cron_jobs.json`. `max_total_jobs` caps the total number
    /// of jobs across all agents.
    pub fn new(home_dir: &Path, max_total_jobs: usize) -> Self {
        Self {
            jobs: DashMap::new(),
            persist_path: home_dir.join("cron_jobs.json"),
            max_total_jobs: AtomicUsize::new(max_total_jobs),
            schedule_runtime_store: None,
            schedule_execution_store: None,
        }
    }

    /// Attach best-effort runtime.db stores used for schedule projections.
    pub fn attach_runtime_stores(
        &mut self,
        schedule_runtime_store: ScheduleRuntimeStore,
        schedule_execution_store: ScheduleExecutionStore,
    ) {
        self.schedule_runtime_store = Some(schedule_runtime_store);
        self.schedule_execution_store = Some(schedule_execution_store);
    }

    /// Update the max total jobs limit (for hot-reload).
    pub fn set_max_total_jobs(&self, new_max: usize) {
        self.max_total_jobs.store(new_max, Ordering::Relaxed);
    }

    fn schedule_runtime_record(meta: &JobMeta) -> ScheduleRuntimeRecord {
        ScheduleRuntimeRecord {
            schedule_id: meta.definition_id.clone(),
            enabled: meta.job.enabled,
            last_run: meta.job.last_run.map(|timestamp| timestamp.to_rfc3339()),
            next_run: meta.job.next_run.map(|timestamp| timestamp.to_rfc3339()),
            last_status: meta.last_status.clone(),
            consecutive_errors: meta.consecutive_errors,
            one_shot: meta.one_shot,
            updated_at: Utc::now().to_rfc3339(),
        }
    }

    fn sync_runtime_projection(&self, meta: &JobMeta) {
        let Some(store) = &self.schedule_runtime_store else {
            return;
        };

        if let Err(error) = store.upsert_schedule_runtime(&Self::schedule_runtime_record(meta)) {
            warn!(
                schedule_id = %meta.definition_id,
                "Failed to persist schedule runtime projection: {error}"
            );
        }
    }

    fn remove_runtime_projection(&self, schedule_id: &str) {
        let Some(store) = &self.schedule_runtime_store else {
            return;
        };

        if let Err(error) = store.remove_schedule_runtime(schedule_id) {
            warn!(
                schedule_id = %schedule_id,
                "Failed to remove schedule runtime projection: {error}"
            );
        }
    }

    fn record_execution_receipt(&self, schedule_id: &str, status: &str, error: Option<&str>) {
        let Some(store) = &self.schedule_execution_store else {
            return;
        };

        let receipt = ScheduleExecutionRecord {
            execution_id: uuid::Uuid::new_v4().to_string(),
            schedule_id: schedule_id.to_string(),
            fired_at: Utc::now().to_rfc3339(),
            status: status.to_string(),
            effect_json: None,
            error: error.map(str::to_string),
        };

        if let Err(store_error) = store.record_execution(&receipt) {
            warn!(
                schedule_id = %schedule_id,
                "Failed to persist schedule execution receipt: {store_error}"
            );
        }
    }

    fn reconcile_runtime_projections(&self) {
        let Some(store) = &self.schedule_runtime_store else {
            return;
        };

        let live_schedule_ids: HashSet<String> = self
            .jobs
            .iter()
            .map(|entry| entry.value().definition_id.clone())
            .collect();

        match store.list_schedule_runtimes() {
            Ok(records) => {
                for record in records {
                    if live_schedule_ids.contains(&record.schedule_id) {
                        continue;
                    }

                    if let Err(error) = store.remove_schedule_runtime(&record.schedule_id) {
                        warn!(
                            schedule_id = %record.schedule_id,
                            "Failed to prune stale schedule runtime projection: {error}"
                        );
                    }
                }
            }
            Err(error) => {
                warn!("Failed to list schedule runtime projections: {error}");
            }
        }

        for meta in self.jobs.iter() {
            self.sync_runtime_projection(meta.value());
        }
    }

    // -- Persistence --------------------------------------------------------

    /// Load persisted jobs from disk.
    ///
    /// Returns the number of jobs loaded. If the persistence file does not
    /// exist, returns `Ok(0)` without error.
    pub fn load(&self) -> OpenFangResult<usize> {
        if !self.persist_path.exists() {
            self.reconcile_runtime_projections();
            return Ok(0);
        }
        let data = std::fs::read_to_string(&self.persist_path)
            .map_err(|e| OpenFangError::Internal(format!("Failed to read cron jobs: {e}")))?;
        let metas: Vec<JobMeta> = serde_json::from_str(&data)
            .map_err(|e| OpenFangError::Internal(format!("Failed to parse cron jobs: {e}")))?;
        let count = metas.len();
        for meta in metas {
            let normalized = meta.normalize();
            self.jobs.insert(normalized.job.id, normalized);
        }
        self.reconcile_runtime_projections();
        info!(count, "Loaded cron jobs from disk");
        Ok(count)
    }

    /// Persist all jobs to disk via atomic write (write to `.tmp`, then rename).
    pub fn persist(&self) -> OpenFangResult<()> {
        let metas: Vec<JobMeta> = self.jobs.iter().map(|r| r.value().clone()).collect();
        let data = serde_json::to_string_pretty(&metas)
            .map_err(|e| OpenFangError::Internal(format!("Failed to serialize cron jobs: {e}")))?;
        let tmp_path = self.persist_path.with_extension("json.tmp");
        std::fs::write(&tmp_path, data.as_bytes()).map_err(|e| {
            OpenFangError::Internal(format!("Failed to write cron jobs temp file: {e}"))
        })?;
        std::fs::rename(&tmp_path, &self.persist_path).map_err(|e| {
            OpenFangError::Internal(format!("Failed to rename cron jobs file: {e}"))
        })?;
        debug!(count = metas.len(), "Persisted cron jobs");
        Ok(())
    }

    // -- CRUD ---------------------------------------------------------------

    /// Add a new job. Validates fields, computes the initial `next_run`,
    /// and inserts it into the scheduler.
    ///
    /// `one_shot` controls whether the job is removed after a single
    /// successful execution.
    pub fn add_job(&self, job: CronJob, one_shot: bool) -> OpenFangResult<CronJobId> {
        let mut meta = JobMeta::new(job.clone(), one_shot);
        meta.updated_at = Utc::now().to_rfc3339();
        self.add_job_meta(meta)?;
        Ok(job.id)
    }

    /// Add a new job with explicit metadata.
    pub fn add_job_meta(&self, mut meta: JobMeta) -> OpenFangResult<CronJobId> {
        // Global limit
        let max_jobs = self.max_total_jobs.load(Ordering::Relaxed);
        if self.jobs.len() >= max_jobs {
            return Err(OpenFangError::Internal(format!(
                "Global cron job limit reached ({})",
                max_jobs
            )));
        }

        // Per-agent count
        let agent_count = self
            .jobs
            .iter()
            .filter(|r| r.value().job.agent_id == meta.job.agent_id)
            .count();

        // CronJob.validate returns Result<(), String>
        meta.job
            .validate(agent_count)
            .map_err(OpenFangError::InvalidInput)?;

        // Compute initial next_run
        meta = meta.normalize();
        meta.job.next_run = if meta.job.enabled {
            Some(compute_next_run(&meta.job.schedule))
        } else {
            None
        };

        let id = meta.job.id;
        self.jobs.insert(id, meta);
        if let Some(meta) = self.jobs.get(&id) {
            self.sync_runtime_projection(meta.value());
        }
        Ok(id)
    }

    /// Remove a job by ID. Returns the removed `CronJob`.
    pub fn remove_job(&self, id: CronJobId) -> OpenFangResult<CronJob> {
        let removed = self
            .jobs
            .remove(&id)
            .map(|(_, meta)| meta)
            .ok_or_else(|| OpenFangError::Internal(format!("Cron job {id} not found")))?;
        self.remove_runtime_projection(&removed.definition_id);
        Ok(removed.job)
    }

    /// Remove a job by its public schedule ID.
    pub fn remove_job_by_definition_id(&self, definition_id: &str) -> OpenFangResult<JobMeta> {
        let Some(id) = self.find_job_id_by_definition_id(definition_id) else {
            return Err(OpenFangError::Internal(format!(
                "Schedule {definition_id} not found"
            )));
        };
        let removed = self.jobs.remove(&id).map(|(_, meta)| meta).ok_or_else(|| {
            OpenFangError::Internal(format!("Schedule {definition_id} not found"))
        })?;
        self.remove_runtime_projection(&removed.definition_id);
        Ok(removed)
    }

    /// Enable or disable a job. Re-enabling resets errors and recomputes
    /// `next_run`.
    pub fn set_enabled(&self, id: CronJobId, enabled: bool) -> OpenFangResult<()> {
        let updated_meta = match self.jobs.get_mut(&id) {
            Some(mut meta) => {
                meta.job.enabled = enabled;
                if enabled {
                    meta.consecutive_errors = 0;
                    meta.job.next_run = Some(compute_next_run(&meta.job.schedule));
                } else {
                    meta.job.next_run = None;
                }
                meta.updated_at = Utc::now().to_rfc3339();
                Some(meta.clone())
            }
            None => None,
        };

        let Some(updated_meta) = updated_meta else {
            return Err(OpenFangError::Internal(format!("Cron job {id} not found")));
        };

        self.sync_runtime_projection(&updated_meta);
        Ok(())
    }

    /// Enable or disable a job by its public schedule ID.
    pub fn set_enabled_by_definition_id(
        &self,
        definition_id: &str,
        enabled: bool,
    ) -> OpenFangResult<JobMeta> {
        let Some(id) = self.find_job_id_by_definition_id(definition_id) else {
            return Err(OpenFangError::Internal(format!(
                "Schedule {definition_id} not found"
            )));
        };
        self.set_enabled(id, enabled)?;
        self.get_meta(id)
            .ok_or_else(|| OpenFangError::Internal(format!("Schedule {definition_id} not found")))
    }

    /// Replace one job definition in place by public schedule ID.
    pub fn replace_job_meta_by_definition_id(
        &self,
        definition_id: &str,
        mut replacement: JobMeta,
    ) -> OpenFangResult<JobMeta> {
        let Some(job_id) = self.find_job_id_by_definition_id(definition_id) else {
            return Err(OpenFangError::Internal(format!(
                "Schedule {definition_id} not found"
            )));
        };

        let existing = self.get_meta(job_id).ok_or_else(|| {
            OpenFangError::Internal(format!("Schedule {definition_id} not found"))
        })?;

        replacement = replacement.normalize();
        replacement.job.id = existing.job.id;
        replacement.definition_id = existing.definition_id.clone();
        replacement.job.created_at = existing.job.created_at;

        let agent_count = self
            .jobs
            .iter()
            .filter(|entry| {
                entry.value().job.agent_id == replacement.job.agent_id && *entry.key() != job_id
            })
            .count();
        replacement
            .job
            .validate(agent_count)
            .map_err(OpenFangError::InvalidInput)?;
        replacement.job.next_run = if replacement.job.enabled {
            Some(compute_next_run(&replacement.job.schedule))
        } else {
            None
        };
        replacement.updated_at = Utc::now().to_rfc3339();

        self.jobs.insert(job_id, replacement.clone());
        self.sync_runtime_projection(&replacement);
        Ok(replacement)
    }

    // -- Queries ------------------------------------------------------------

    /// Get a single job by ID.
    pub fn get_job(&self, id: CronJobId) -> Option<CronJob> {
        self.jobs.get(&id).map(|r| r.value().job.clone())
    }

    /// Get the full metadata for a job (includes `one_shot`, `last_status`,
    /// `consecutive_errors`).
    pub fn get_meta(&self, id: CronJobId) -> Option<JobMeta> {
        self.jobs.get(&id).map(|r| r.value().clone())
    }

    /// Get the full metadata for a job by its public schedule ID.
    pub fn get_meta_by_definition_id(&self, definition_id: &str) -> Option<JobMeta> {
        let id = self.find_job_id_by_definition_id(definition_id)?;
        self.get_meta(id)
    }

    /// List all jobs for a specific agent.
    pub fn list_jobs(&self, agent_id: AgentId) -> Vec<CronJob> {
        self.jobs
            .iter()
            .filter(|r| r.value().job.agent_id == agent_id)
            .map(|r| r.value().job.clone())
            .collect()
    }

    /// List all jobs across all agents.
    pub fn list_all_jobs(&self) -> Vec<CronJob> {
        self.jobs.iter().map(|r| r.value().job.clone()).collect()
    }

    /// List full schedule metadata across all agents.
    pub fn list_all_metas(&self) -> Vec<JobMeta> {
        self.jobs
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Reassign all cron jobs from `old_agent_id` to `new_agent_id`.
    ///
    /// Used when a hand agent is respawned (e.g. after daemon restart) and
    /// gets a new UUID. Without this, persisted cron jobs would reference
    /// the stale old agent ID and fail silently.
    ///
    /// Returns the number of jobs reassigned.
    pub fn reassign_agent_jobs(&self, old_agent_id: AgentId, new_agent_id: AgentId) -> usize {
        let mut count = 0;
        let mut touched = Vec::new();
        for mut entry in self.jobs.iter_mut() {
            if entry.value().job.agent_id == old_agent_id {
                entry.value_mut().job.agent_id = new_agent_id;
                // Reset consecutive errors so the job gets a fresh start
                // with the new agent.
                entry.value_mut().consecutive_errors = 0;
                if !entry.value().job.enabled {
                    // Re-enable jobs that were auto-disabled due to the stale
                    // agent ID causing repeated failures.
                    if entry
                        .value()
                        .last_status
                        .as_deref()
                        .is_some_and(|s| s.contains("not found") || s.contains("No such agent"))
                    {
                        entry.value_mut().job.enabled = true;
                        entry.value_mut().job.next_run =
                            Some(compute_next_run(&entry.value().job.schedule));
                    }
                }
                count += 1;
                touched.push(entry.value().clone());
            }
        }
        for meta in touched {
            self.sync_runtime_projection(&meta);
        }
        if count > 0 {
            info!(
                old_agent = %old_agent_id,
                new_agent = %new_agent_id,
                count,
                "Reassigned cron jobs to new agent"
            );
        }
        count
    }

    /// Remove all cron jobs belonging to a specific agent.
    ///
    /// Used when an agent is deleted so its cron entries don't linger as
    /// orphans pointing at a dead UUID. Returns the number of jobs removed.
    pub fn remove_agent_jobs(&self, agent_id: AgentId) -> usize {
        let ids: Vec<CronJobId> = self
            .jobs
            .iter()
            .filter(|r| r.value().job.agent_id == agent_id)
            .map(|r| *r.key())
            .collect();
        let count = ids.len();
        for id in ids {
            if let Some((_, meta)) = self.jobs.remove(&id) {
                self.remove_runtime_projection(&meta.definition_id);
            }
        }
        if count > 0 {
            info!(agent = %agent_id, count, "Removed cron jobs for deleted agent");
        }
        count
    }

    /// Total number of tracked jobs.
    pub fn total_jobs(&self) -> usize {
        self.jobs.len()
    }

    /// Return jobs whose `next_run` is at or before `now` and are enabled.
    ///
    /// **Important**: This also pre-advances each due job's `next_run` to the
    /// next scheduled time. This prevents the same job from being returned as
    /// "due" on subsequent tick iterations while it's still executing.
    pub fn due_jobs(&self) -> Vec<CronJob> {
        self.due_job_metas()
            .into_iter()
            .map(|meta| meta.job)
            .collect()
    }

    /// Return full metadata for jobs whose `next_run` is at or before `now`.
    pub fn due_job_metas(&self) -> Vec<JobMeta> {
        let now = Utc::now();
        let mut due = Vec::new();
        let mut updated = Vec::new();
        for mut entry in self.jobs.iter_mut() {
            let meta = entry.value_mut();
            if meta.job.enabled && meta.job.next_run.map(|t| t <= now).unwrap_or(false) {
                due.push(meta.clone());
                // Pre-advance next_run so the job won't fire again on the next
                // tick while it's still executing. Use `now` as the base so the
                // next fire time is computed strictly after the current moment.
                meta.job.next_run = Some(compute_next_run_after(&meta.job.schedule, now));
                meta.updated_at = Utc::now().to_rfc3339();
                updated.push(meta.clone());
            }
        }
        for meta in updated {
            self.sync_runtime_projection(&meta);
        }
        due
    }

    // -- Outcome recording --------------------------------------------------

    /// Record a successful execution for a job.
    ///
    /// Updates `last_run`, resets errors, and either removes the job (if
    /// one-shot) or advances `next_run`.
    pub fn record_success(&self, id: CronJobId) {
        // We need to check one_shot first, then potentially remove.
        let (should_remove, updated_meta) = {
            if let Some(mut meta) = self.jobs.get_mut(&id) {
                meta.job.last_run = Some(Utc::now());
                meta.last_status = Some("ok".to_string());
                meta.consecutive_errors = 0;
                meta.updated_at = Utc::now().to_rfc3339();
                // one_shot jobs get removed; recurring jobs keep the next_run
                // already pre-advanced by due_jobs() — no recompute needed.
                (meta.one_shot, Some(meta.clone()))
            } else {
                return;
            }
        };
        let Some(updated_meta) = updated_meta else {
            return;
        };
        self.record_execution_receipt(&updated_meta.definition_id, "ok", None);
        if should_remove {
            self.jobs.remove(&id);
            self.remove_runtime_projection(&updated_meta.definition_id);
        } else {
            self.sync_runtime_projection(&updated_meta);
        }
    }

    /// Record a failed execution for a job.
    ///
    /// Increments the consecutive error counter. If it reaches
    /// [`MAX_CONSECUTIVE_ERRORS`], the job is automatically disabled.
    pub fn record_failure(&self, id: CronJobId, error_msg: &str) {
        let updated_meta = if let Some(mut meta) = self.jobs.get_mut(&id) {
            meta.job.last_run = Some(Utc::now());
            meta.last_status = Some(format!(
                "error: {}",
                openfang_types::truncate_str(error_msg, 256)
            ));
            meta.consecutive_errors += 1;
            if meta.consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                warn!(
                    job_id = %id,
                    errors = meta.consecutive_errors,
                    "Auto-disabling cron job after repeated failures"
                );
                meta.job.enabled = false;
                meta.job.next_run = None;
            } else {
                meta.job.next_run = Some(compute_next_run_after(&meta.job.schedule, Utc::now()));
            }
            meta.updated_at = Utc::now().to_rfc3339();
            Some(meta.clone())
        } else {
            None
        };

        if let Some(updated_meta) = updated_meta {
            self.sync_runtime_projection(&updated_meta);
            self.record_execution_receipt(&updated_meta.definition_id, "error", Some(error_msg));
        }
    }

    fn find_job_id_by_definition_id(&self, definition_id: &str) -> Option<CronJobId> {
        self.jobs
            .iter()
            .find(|entry| entry.value().definition_id == definition_id)
            .map(|entry| *entry.key())
    }
}

// ---------------------------------------------------------------------------
// compute_next_run
// ---------------------------------------------------------------------------

/// Compute the next fire time for a schedule, based on `now`.
///
/// - `At { at }` — returns `at` directly.
/// - `Every { every_secs }` — returns `now + every_secs`.
/// - `Cron { expr, tz }` — parses the cron expression and computes the next
///   matching time. Supports standard 5-field (`min hour dom month dow`) and
///   6-field (`sec min hour dom month dow`) formats by converting to the
///   7-field format required by the `cron` crate.
pub fn compute_next_run(schedule: &CronSchedule) -> chrono::DateTime<Utc> {
    compute_next_run_after(schedule, Utc::now())
}

/// Compute the next fire time for a schedule, strictly after `after`.
///
/// Uses `after + 1 second` as the base time so the `cron` crate's
/// inclusive `.after()` always returns a strictly future time. Without
/// this offset, calling `compute_next_run` right after a job fires can
/// return the same minute (or even the same second), causing the
/// scheduler to re-fire immediately.
pub fn compute_next_run_after(
    schedule: &CronSchedule,
    after: chrono::DateTime<Utc>,
) -> chrono::DateTime<Utc> {
    match schedule {
        CronSchedule::At { at } => *at,
        CronSchedule::Every { every_secs } => after + Duration::seconds(*every_secs as i64),
        CronSchedule::Cron { expr, tz } => {
            // Convert standard 5/6-field cron to 7-field for the `cron` crate.
            // Standard 5-field: min hour dom month dow
            // 6-field:          sec min hour dom month dow
            // cron crate:       sec min hour dom month dow year
            let trimmed = expr.trim();
            let fields: Vec<&str> = trimmed.split_whitespace().collect();
            let seven_field = match fields.len() {
                5 => format!("0 {trimmed} *"),
                6 => format!("{trimmed} *"),
                _ => expr.clone(),
            };

            // Add 1 second so `.after()` (inclusive) skips the current second.
            let base = after + Duration::seconds(1);

            match seven_field.parse::<cron::Schedule>() {
                Ok(sched) => {
                    // If a timezone is specified, compute the next fire time in
                    // that timezone so DST and local offsets are respected, then
                    // convert back to UTC for storage.
                    let next_utc = match tz.as_deref() {
                        Some(tz_str) if !tz_str.is_empty() && tz_str != "UTC" => {
                            match tz_str.parse::<chrono_tz::Tz>() {
                                Ok(timezone) => {
                                    let base_local = base.with_timezone(&timezone);
                                    sched
                                        .after(&base_local)
                                        .next()
                                        .map(|dt| dt.with_timezone(&Utc))
                                }
                                Err(_) => {
                                    warn!(
                                        "Invalid timezone '{}' in cron job, falling back to UTC",
                                        tz_str
                                    );
                                    sched.after(&base).next()
                                }
                            }
                        }
                        _ => sched.after(&base).next(),
                    };
                    next_utc.unwrap_or_else(|| after + Duration::hours(1))
                }
                Err(e) => {
                    warn!("Failed to parse cron expression '{}': {}", expr, e);
                    after + Duration::hours(1)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Timelike};
    use openfang_types::scheduler::{CronAction, CronDelivery};

    /// Build a minimal valid `CronJob` with an `Every` schedule.
    fn make_job(agent_id: AgentId) -> CronJob {
        CronJob {
            id: CronJobId::new(),
            agent_id,
            name: "test-job".into(),
            enabled: true,
            schedule: CronSchedule::Every { every_secs: 3600 },
            action: CronAction::SystemEvent {
                event: "ping".into(),
                payload: serde_json::Value::Null,
            },
            delivery: CronDelivery::None,
            created_at: Utc::now(),
            last_run: None,
            next_run: None,
        }
    }

    /// Create a scheduler backed by a temp directory.
    fn make_scheduler(max_total: usize) -> (CronScheduler, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let sched = CronScheduler::new(tmp.path(), max_total);
        (sched, tmp)
    }

    // -- test_add_job_and_list ----------------------------------------------

    #[test]
    fn test_add_job_and_list() {
        let (sched, _tmp) = make_scheduler(100);
        let agent = AgentId::new();
        let job = make_job(agent);

        let id = sched.add_job(job, false).unwrap();

        // Should appear in agent list
        let jobs = sched.list_jobs(agent);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, id);
        assert_eq!(jobs[0].name, "test-job");

        // Should appear in global list
        let all = sched.list_all_jobs();
        assert_eq!(all.len(), 1);

        // get_job should return it
        let fetched = sched.get_job(id).unwrap();
        assert_eq!(fetched.agent_id, agent);

        // next_run should have been computed
        assert!(fetched.next_run.is_some());
        assert_eq!(sched.total_jobs(), 1);
    }

    // -- test_remove_job ----------------------------------------------------

    #[test]
    fn test_remove_job() {
        let (sched, _tmp) = make_scheduler(100);
        let agent = AgentId::new();
        let job = make_job(agent);
        let id = sched.add_job(job, false).unwrap();

        let removed = sched.remove_job(id).unwrap();
        assert_eq!(removed.name, "test-job");
        assert_eq!(sched.total_jobs(), 0);

        // Removing again should fail
        assert!(sched.remove_job(id).is_err());
    }

    // -- test_add_job_global_limit ------------------------------------------

    #[test]
    fn test_add_job_global_limit() {
        let (sched, _tmp) = make_scheduler(2);
        let agent = AgentId::new();

        let j1 = make_job(agent);
        let j2 = make_job(agent);
        let j3 = make_job(agent);

        sched.add_job(j1, false).unwrap();
        sched.add_job(j2, false).unwrap();

        // Third should hit global limit
        let err = sched.add_job(j3, false).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("limit"),
            "Expected global limit error, got: {msg}"
        );
    }

    // -- test_add_job_per_agent_limit ---------------------------------------

    #[test]
    fn test_add_job_per_agent_limit() {
        // MAX_JOBS_PER_AGENT = 50 in openfang-types
        let (sched, _tmp) = make_scheduler(1000);
        let agent = AgentId::new();

        for i in 0..50 {
            let mut job = make_job(agent);
            job.name = format!("job-{i}");
            sched.add_job(job, false).unwrap();
        }

        // 51st should be rejected by validate()
        let mut overflow = make_job(agent);
        overflow.name = "overflow".into();
        let err = sched.add_job(overflow, false).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("50"),
            "Expected per-agent limit error, got: {msg}"
        );
    }

    // -- test_record_success_removes_one_shot --------------------------------

    #[test]
    fn test_record_success_removes_one_shot() {
        let (sched, _tmp) = make_scheduler(100);
        let agent = AgentId::new();
        let job = make_job(agent);
        let id = sched.add_job(job, true).unwrap(); // one_shot = true

        assert_eq!(sched.total_jobs(), 1);

        sched.record_success(id);

        // One-shot job should have been removed
        assert_eq!(sched.total_jobs(), 0);
        assert!(sched.get_job(id).is_none());
    }

    #[test]
    fn test_record_success_keeps_recurring() {
        let (sched, _tmp) = make_scheduler(100);
        let agent = AgentId::new();
        let job = make_job(agent);
        let id = sched.add_job(job, false).unwrap(); // one_shot = false

        sched.record_success(id);

        // Recurring job should still be there
        assert_eq!(sched.total_jobs(), 1);
        let meta = sched.get_meta(id).unwrap();
        assert_eq!(meta.last_status.as_deref(), Some("ok"));
        assert_eq!(meta.consecutive_errors, 0);
        assert!(meta.job.last_run.is_some());
    }

    // -- test_record_failure_auto_disable -----------------------------------

    #[test]
    fn test_record_failure_auto_disable() {
        let (sched, _tmp) = make_scheduler(100);
        let agent = AgentId::new();
        let job = make_job(agent);
        let id = sched.add_job(job, false).unwrap();

        // Fail MAX_CONSECUTIVE_ERRORS - 1 times: should still be enabled
        for i in 0..(MAX_CONSECUTIVE_ERRORS - 1) {
            sched.record_failure(id, &format!("error {i}"));
            let meta = sched.get_meta(id).unwrap();
            assert!(
                meta.job.enabled,
                "Job should still be enabled after {} failures",
                i + 1
            );
            assert_eq!(meta.consecutive_errors, i + 1);
        }

        // One more failure should auto-disable
        sched.record_failure(id, "final error");
        let meta = sched.get_meta(id).unwrap();
        assert!(
            !meta.job.enabled,
            "Job should be auto-disabled after {MAX_CONSECUTIVE_ERRORS} failures"
        );
        assert_eq!(meta.consecutive_errors, MAX_CONSECUTIVE_ERRORS);
        assert!(
            meta.last_status.as_ref().unwrap().starts_with("error:"),
            "last_status should record the error"
        );
    }

    // -- test_due_jobs_only_enabled -----------------------------------------

    #[test]
    fn test_due_jobs_only_enabled() {
        let (sched, _tmp) = make_scheduler(100);
        let agent = AgentId::new();

        // Job 1: enabled, next_run in the past
        let mut j1 = make_job(agent);
        j1.name = "enabled-due".into();
        let id1 = sched.add_job(j1, false).unwrap();

        // Job 2: disabled
        let mut j2 = make_job(agent);
        j2.name = "disabled-job".into();
        let id2 = sched.add_job(j2, false).unwrap();
        sched.set_enabled(id2, false).unwrap();

        // Force job 1's next_run to the past
        if let Some(mut meta) = sched.jobs.get_mut(&id1) {
            meta.job.next_run = Some(Utc::now() - Duration::seconds(10));
        }

        // Force job 2's next_run to the past too (but it's disabled)
        if let Some(mut meta) = sched.jobs.get_mut(&id2) {
            meta.job.next_run = Some(Utc::now() - Duration::seconds(10));
        }

        let due = sched.due_jobs();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].name, "enabled-due");
    }

    #[test]
    fn test_due_jobs_future_not_included() {
        let (sched, _tmp) = make_scheduler(100);
        let agent = AgentId::new();

        let job = make_job(agent);
        sched.add_job(job, false).unwrap();

        // The job was just added with next_run = now + 3600s, so it should
        // not be due yet.
        let due = sched.due_jobs();
        assert!(due.is_empty());
    }

    // -- test_set_enabled ---------------------------------------------------

    #[test]
    fn test_set_enabled() {
        let (sched, _tmp) = make_scheduler(100);
        let agent = AgentId::new();

        let job = make_job(agent);
        let id = sched.add_job(job, false).unwrap();

        // Disable
        sched.set_enabled(id, false).unwrap();
        let meta = sched.get_meta(id).unwrap();
        assert!(!meta.job.enabled);
        assert!(meta.job.next_run.is_none());

        // Re-enable resets error count
        sched.record_failure(id, "ignored because disabled");
        // Actually the job is disabled so record_failure still updates it.
        // Let's first re-enable to test reset.
        sched.set_enabled(id, true).unwrap();
        let meta = sched.get_meta(id).unwrap();
        assert!(meta.job.enabled);
        assert_eq!(meta.consecutive_errors, 0);
        assert!(meta.job.next_run.is_some());

        // Non-existent ID should fail
        let fake_id = CronJobId::new();
        assert!(sched.set_enabled(fake_id, true).is_err());
    }

    // -- test_persist_and_load ----------------------------------------------

    #[test]
    fn test_persist_and_load() {
        let tmp = tempfile::tempdir().unwrap();
        let agent = AgentId::new();

        // Create scheduler, add jobs, persist
        {
            let sched = CronScheduler::new(tmp.path(), 100);
            let mut j1 = make_job(agent);
            j1.name = "persist-a".into();
            let mut j2 = make_job(agent);
            j2.name = "persist-b".into();

            sched.add_job(j1, false).unwrap();
            sched.add_job(j2, true).unwrap(); // one_shot

            sched.persist().unwrap();
        }

        // Create a new scheduler and load from disk
        {
            let sched = CronScheduler::new(tmp.path(), 100);
            let count = sched.load().unwrap();
            assert_eq!(count, 2);
            assert_eq!(sched.total_jobs(), 2);

            let jobs = sched.list_jobs(agent);
            assert_eq!(jobs.len(), 2);

            let names: Vec<&str> = jobs.iter().map(|j| j.name.as_str()).collect();
            assert!(names.contains(&"persist-a"));
            assert!(names.contains(&"persist-b"));

            // Verify one_shot flag was preserved
            let b_id = jobs.iter().find(|j| j.name == "persist-b").unwrap().id;
            let meta = sched.get_meta(b_id).unwrap();
            assert!(meta.one_shot);
        }
    }

    #[test]
    fn test_load_no_file_returns_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let sched = CronScheduler::new(tmp.path(), 100);
        assert_eq!(sched.load().unwrap(), 0);
    }

    // -- compute_next_run ---------------------------------------------------

    #[test]
    fn test_compute_next_run_at() {
        let target = Utc::now() + Duration::hours(2);
        let schedule = CronSchedule::At { at: target };
        let next = compute_next_run(&schedule);
        assert_eq!(next, target);
    }

    #[test]
    fn test_compute_next_run_every() {
        let before = Utc::now();
        let schedule = CronSchedule::Every { every_secs: 300 };
        let next = compute_next_run(&schedule);
        let after = Utc::now();

        // Should be roughly now + 300s
        assert!(next >= before + Duration::seconds(300));
        assert!(next <= after + Duration::seconds(300));
    }

    #[test]
    fn test_compute_next_run_cron_daily() {
        let now = Utc::now();
        let schedule = CronSchedule::Cron {
            expr: "0 9 * * *".into(),
            tz: None,
        };
        let next = compute_next_run(&schedule);

        // Should be within the next 24 hours (next 09:00 UTC)
        assert!(next > now);
        assert!(next <= now + Duration::hours(24));
        assert_eq!(next.format("%M").to_string(), "00");
        assert_eq!(next.format("%H").to_string(), "09");
    }

    #[test]
    fn test_compute_next_run_cron_with_dow() {
        let now = Utc::now();
        let schedule = CronSchedule::Cron {
            expr: "30 14 * * 1-5".into(),
            tz: None,
        };
        let next = compute_next_run(&schedule);

        // Should be within the next 7 days and at 14:30
        assert!(next > now);
        assert!(next <= now + Duration::days(7));
        assert_eq!(next.format("%H:%M").to_string(), "14:30");
    }

    #[test]
    fn test_compute_next_run_cron_invalid_expr() {
        let now = Utc::now();
        let schedule = CronSchedule::Cron {
            expr: "not a cron".into(),
            tz: None,
        };
        let next = compute_next_run(&schedule);
        // Invalid expression falls back to 1 hour from now
        assert!(next > now + Duration::minutes(59));
        assert!(next <= now + Duration::minutes(61));
    }

    // -- error message truncation in record_failure -------------------------

    #[test]
    fn test_compute_next_run_after_skips_current_second() {
        // When the caller is exactly on a firing boundary, the scheduler must
        // skip that instant and advance to the next one instead of refiring
        // immediately (the bug from #55).
        let schedule = CronSchedule::Cron {
            expr: "0 */4 * * *".into(),
            tz: None,
        };
        let now = Utc
            .with_ymd_and_hms(2026, 3, 23, 4, 0, 0)
            .single()
            .expect("fixture timestamp should resolve");
        let next = compute_next_run_after(&schedule, now);

        assert_eq!(
            next,
            Utc.with_ymd_and_hms(2026, 3, 23, 8, 0, 0)
                .single()
                .expect("fixture timestamp should resolve")
        );
    }

    #[test]
    fn test_record_failure_truncates_long_error() {
        let (sched, _tmp) = make_scheduler(100);
        let agent = AgentId::new();
        let job = make_job(agent);
        let id = sched.add_job(job, false).unwrap();

        let long_error = "x".repeat(1000);
        sched.record_failure(id, &long_error);

        let meta = sched.get_meta(id).unwrap();
        let status = meta.last_status.unwrap();
        // "error: " is 7 chars + 256 chars of truncated message = 263 max
        assert!(
            status.len() <= 263,
            "Status should be truncated, got {} chars",
            status.len()
        );
    }

    // -- timezone-aware cron (#473) -----------------------------------------

    #[test]
    fn test_cron_tz_shifts_next_run() {
        // "0 9 * * *" in America/New_York (UTC-5 or UTC-4 depending on DST).
        // The next fire time in UTC should differ from a plain UTC "0 9 * * *".
        let schedule_utc = CronSchedule::Cron {
            expr: "0 9 * * *".into(),
            tz: None,
        };
        let schedule_ny = CronSchedule::Cron {
            expr: "0 9 * * *".into(),
            tz: Some("America/New_York".into()),
        };
        let now = Utc::now();
        let next_utc = compute_next_run_after(&schedule_utc, now);
        let next_ny = compute_next_run_after(&schedule_ny, now);

        // The New York schedule should fire at 09:00 Eastern, which is 13:00
        // or 14:00 UTC (depending on DST). In either case, it should NOT
        // equal the plain UTC 09:00 result.
        assert_ne!(
            next_utc, next_ny,
            "Timezone-aware schedule should produce a different UTC time"
        );

        // Verify the New York result, when converted to ET, shows hour 09.
        let ny_tz: chrono_tz::Tz = "America/New_York".parse().unwrap();
        let next_ny_local = next_ny.with_timezone(&ny_tz);
        assert_eq!(
            next_ny_local.hour(),
            9,
            "Expected 09:00 in America/New_York, got {:02}:{:02}",
            next_ny_local.hour(),
            next_ny_local.minute()
        );
    }

    #[test]
    fn test_cron_tz_none_defaults_to_utc() {
        // tz: None should behave identically to tz: Some("UTC").
        let schedule_none = CronSchedule::Cron {
            expr: "30 12 * * *".into(),
            tz: None,
        };
        let schedule_utc = CronSchedule::Cron {
            expr: "30 12 * * *".into(),
            tz: Some("UTC".into()),
        };
        let now = Utc::now();
        let next_none = compute_next_run_after(&schedule_none, now);
        let next_utc = compute_next_run_after(&schedule_utc, now);
        assert_eq!(next_none, next_utc);
    }

    #[test]
    fn test_cron_tz_empty_string_defaults_to_utc() {
        let schedule_empty = CronSchedule::Cron {
            expr: "30 12 * * *".into(),
            tz: Some(String::new()),
        };
        let schedule_none = CronSchedule::Cron {
            expr: "30 12 * * *".into(),
            tz: None,
        };
        let now = Utc::now();
        assert_eq!(
            compute_next_run_after(&schedule_empty, now),
            compute_next_run_after(&schedule_none, now)
        );
    }

    #[test]
    fn test_cron_tz_invalid_falls_back_to_utc() {
        // An invalid timezone string should fall back to UTC, not panic.
        let schedule_bad = CronSchedule::Cron {
            expr: "0 9 * * *".into(),
            tz: Some("Not/A_Timezone".into()),
        };
        let schedule_utc = CronSchedule::Cron {
            expr: "0 9 * * *".into(),
            tz: None,
        };
        let now = Utc::now();
        let next_bad = compute_next_run_after(&schedule_bad, now);
        let next_utc = compute_next_run_after(&schedule_utc, now);
        // Invalid tz falls back to UTC computation — same result.
        assert_eq!(next_bad, next_utc);
    }

    #[test]
    fn test_cron_tz_asia_shanghai() {
        // "0 8 * * *" in Asia/Shanghai (UTC+8) should fire at 00:00 UTC.
        let schedule = CronSchedule::Cron {
            expr: "0 8 * * *".into(),
            tz: Some("Asia/Shanghai".into()),
        };
        let now = Utc::now();
        let next = compute_next_run_after(&schedule, now);

        let shanghai_tz: chrono_tz::Tz = "Asia/Shanghai".parse().unwrap();
        let local = next.with_timezone(&shanghai_tz);
        assert_eq!(local.hour(), 8);
        assert_eq!(local.minute(), 0);

        // In UTC, 08:00 Shanghai = 00:00 UTC.
        assert_eq!(next.hour(), 0, "08:00 CST should be 00:00 UTC");
    }

    // -- reassign_agent_jobs (#461) -----------------------------------------

    #[test]
    fn test_reassign_agent_jobs_basic() {
        let (sched, _tmp) = make_scheduler(100);
        let old_agent = AgentId::new();
        let new_agent = AgentId::new();

        let mut j1 = make_job(old_agent);
        j1.name = "cron-a".into();
        let mut j2 = make_job(old_agent);
        j2.name = "cron-b".into();

        let id1 = sched.add_job(j1, false).unwrap();
        let id2 = sched.add_job(j2, false).unwrap();

        let count = sched.reassign_agent_jobs(old_agent, new_agent);
        assert_eq!(count, 2);

        // Both jobs should now belong to the new agent
        let job1 = sched.get_job(id1).unwrap();
        assert_eq!(job1.agent_id, new_agent);
        let job2 = sched.get_job(id2).unwrap();
        assert_eq!(job2.agent_id, new_agent);

        // Old agent should have zero jobs
        assert!(sched.list_jobs(old_agent).is_empty());
        // New agent should have both
        assert_eq!(sched.list_jobs(new_agent).len(), 2);
    }

    #[test]
    fn test_reassign_agent_jobs_does_not_touch_other_agents() {
        let (sched, _tmp) = make_scheduler(100);
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();
        let agent_c = AgentId::new();

        let mut ja = make_job(agent_a);
        ja.name = "job-a".into();
        let mut jb = make_job(agent_b);
        jb.name = "job-b".into();

        let _id_a = sched.add_job(ja, false).unwrap();
        let id_b = sched.add_job(jb, false).unwrap();

        // Reassign agent_a -> agent_c
        let count = sched.reassign_agent_jobs(agent_a, agent_c);
        assert_eq!(count, 1);

        // agent_b's job should be untouched
        let job_b = sched.get_job(id_b).unwrap();
        assert_eq!(job_b.agent_id, agent_b);
    }

    #[test]
    fn test_reassign_agent_jobs_no_match_returns_zero() {
        let (sched, _tmp) = make_scheduler(100);
        let agent = AgentId::new();
        let other = AgentId::new();

        let job = make_job(agent);
        sched.add_job(job, false).unwrap();

        // Reassign a non-existent agent
        let count = sched.reassign_agent_jobs(AgentId::new(), other);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_reassign_agent_jobs_resets_consecutive_errors() {
        let (sched, _tmp) = make_scheduler(100);
        let old_agent = AgentId::new();
        let new_agent = AgentId::new();

        let job = make_job(old_agent);
        let id = sched.add_job(job, false).unwrap();

        // Simulate some failures
        sched.record_failure(id, "agent not found");
        sched.record_failure(id, "agent not found");
        let meta = sched.get_meta(id).unwrap();
        assert_eq!(meta.consecutive_errors, 2);

        // Reassign
        sched.reassign_agent_jobs(old_agent, new_agent);

        // Errors should be reset
        let meta = sched.get_meta(id).unwrap();
        assert_eq!(meta.consecutive_errors, 0);
        assert_eq!(meta.job.agent_id, new_agent);
    }

    #[test]
    fn test_reassign_agent_jobs_reenables_disabled_stale_jobs() {
        let (sched, _tmp) = make_scheduler(100);
        let old_agent = AgentId::new();
        let new_agent = AgentId::new();

        let job = make_job(old_agent);
        let id = sched.add_job(job, false).unwrap();

        // Simulate enough failures to auto-disable (with "not found" message)
        for _ in 0..MAX_CONSECUTIVE_ERRORS {
            sched.record_failure(id, "No such agent");
        }
        let meta = sched.get_meta(id).unwrap();
        assert!(!meta.job.enabled, "Job should be auto-disabled");

        // Reassign should re-enable it
        sched.reassign_agent_jobs(old_agent, new_agent);

        let meta = sched.get_meta(id).unwrap();
        assert!(
            meta.job.enabled,
            "Job should be re-enabled after reassignment"
        );
        assert_eq!(meta.consecutive_errors, 0);
        assert_eq!(meta.job.agent_id, new_agent);
    }

    #[test]
    fn test_reassign_agent_jobs_persists_after_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let old_agent = AgentId::new();
        let new_agent = AgentId::new();

        // Create scheduler, add job, reassign, persist
        let id = {
            let sched = CronScheduler::new(tmp.path(), 100);
            let job = make_job(old_agent);
            let id = sched.add_job(job, false).unwrap();

            sched.reassign_agent_jobs(old_agent, new_agent);
            sched.persist().unwrap();
            id
        };

        // Load from disk and verify the agent_id was persisted
        {
            let sched = CronScheduler::new(tmp.path(), 100);
            sched.load().unwrap();

            let job = sched.get_job(id).unwrap();
            assert_eq!(job.agent_id, new_agent);
            assert!(sched.list_jobs(old_agent).is_empty());
        }
    }

    // -- remove_agent_jobs (#504) -------------------------------------------

    #[test]
    fn test_remove_agent_jobs_basic() {
        let (sched, _tmp) = make_scheduler(100);
        let agent = AgentId::new();
        let other = AgentId::new();

        let mut j1 = make_job(agent);
        j1.name = "job-a".into();
        let mut j2 = make_job(agent);
        j2.name = "job-b".into();
        let mut j3 = make_job(other);
        j3.name = "job-other".into();

        sched.add_job(j1, false).unwrap();
        sched.add_job(j2, false).unwrap();
        let id3 = sched.add_job(j3, false).unwrap();

        assert_eq!(sched.total_jobs(), 3);

        let removed = sched.remove_agent_jobs(agent);
        assert_eq!(removed, 2);
        assert_eq!(sched.total_jobs(), 1);

        // The other agent's job should still exist
        assert!(sched.list_jobs(agent).is_empty());
        assert_eq!(sched.list_jobs(other).len(), 1);
        assert!(sched.get_job(id3).is_some());
    }

    #[test]
    fn test_remove_agent_jobs_no_match() {
        let (sched, _tmp) = make_scheduler(100);
        let agent = AgentId::new();

        let job = make_job(agent);
        sched.add_job(job, false).unwrap();

        // Remove for a non-existent agent
        let removed = sched.remove_agent_jobs(AgentId::new());
        assert_eq!(removed, 0);
        assert_eq!(sched.total_jobs(), 1);
    }

    #[test]
    fn test_remove_agent_jobs_persists() {
        let tmp = tempfile::tempdir().unwrap();
        let agent = AgentId::new();
        let other = AgentId::new();

        // Add jobs for two agents, remove one agent's jobs, persist
        {
            let sched = CronScheduler::new(tmp.path(), 100);
            let mut j1 = make_job(agent);
            j1.name = "doomed".into();
            let mut j2 = make_job(other);
            j2.name = "survivor".into();

            sched.add_job(j1, false).unwrap();
            sched.add_job(j2, false).unwrap();

            sched.remove_agent_jobs(agent);
            sched.persist().unwrap();
        }

        // Reload and verify
        {
            let sched = CronScheduler::new(tmp.path(), 100);
            sched.load().unwrap();
            assert_eq!(sched.total_jobs(), 1);
            assert!(sched.list_jobs(agent).is_empty());
            assert_eq!(sched.list_jobs(other).len(), 1);
        }
    }
}
