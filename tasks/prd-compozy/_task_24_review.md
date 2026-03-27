# Task 24 Review: hitl_request Schema And Persistence Layer

## Status: PASS

## Checklist
- [x] Migration `20260324_009_hitl_request.sql` adds `hitl_request` table with all required columns — all 13 columns present including `sequence_no`, `timeout_at`
- [x] `dispatch_id` is nullable FK to `agent_dispatch` — present with `ON DELETE SET NULL`
- [x] `run_id` FK to `workflow_run` with `ON DELETE CASCADE` — present
- [x] `CHECK` constraint on `kind` covers `clarification`, `approval`, `choice`, `freeform` — present
- [x] `CHECK` constraint on `status` covers `pending`, `answered`, `cancelled`, `timed_out` — present
- [x] `CHECK` constraint linking `answered` status to non-null `response_json` and `answered_at` — present
- [x] `sequence_no` has `CHECK (sequence_no >= 1)` — present
- [x] Indexes: `idx_hitl_run`, `idx_hitl_dispatch`, `idx_hitl_status`, `idx_hitl_run_step_sequence` — all four present in migration SQL
- [x] `HitlKind` enum (`Clarification`, `Approval`, `Choice`, `Freeform`) with `Display`, `FromStr`, `serde` derives — implemented in `hitl.rs`
- [x] `HitlStatus` enum (`Pending`, `Answered`, `Cancelled`, `TimedOut`) with `Display`, `FromStr`, `serde` derives — implemented
- [x] Snake_case serialization: `"timed_out"` for `TimedOut`, `"clarification"` etc. — correct
- [x] `HitlRecord` struct with all schema columns — implemented with `DateTime<Utc>` for timestamps (RFC 3339) and `serde_json::Value` for JSON columns
- [x] `NewHitlRequest` input struct — present
- [x] `HitlRepository` async trait (`Send + Sync`) with `create`, `find_by_id`, `find_pending_for_run`, `find_by_dispatch`, `list`, `answer`, `cancel`, `mark_timed_out` — all present
- [x] `SqliteHitlRepository` backed by `Arc<Mutex<Connection>>` — implemented
- [x] `sequence_no` assignment is atomic — uses `IMMEDIATE` transaction with `SELECT MAX(sequence_no) + 1` scoped to `(run_id, step_id, dispatch_id)` within the same write transaction as the insert
- [x] Sequence restarts across steps and dispatches — query correctly scopes by all three columns
- [x] `answer` method writes `response_json`, `answered_at`, `status = answered` in one transaction — implemented
- [x] Terminal states (`answered`, `cancelled`, `timed_out`) cannot transition further — `ensure_pending_transition` enforces that only `pending` status can transition
- [x] `HitlStoreError` with `thiserror` — present with typed variants for all failure modes
- [x] No `unwrap()` in repository code — confirmed, all errors propagate via `?`
- [x] `ApprovalManager` not referenced — confirmed, HITL is a separate concept
- [x] Unit tests: `hitl_record_should_persist_all_required_fields`, `hitl_sequence_numbers_should_be_ordered_within_step`, `hitl_sequence_numbers_should_restart_across_steps`, `hitl_answer_should_write_response_and_timestamp_atomically`, `hitl_answer_should_fail_on_non_pending_request`, `hitl_status_terminal_states_should_not_transition`, `hitl_find_pending_for_run_should_return_only_pending`, `hitl_find_by_dispatch_should_scope_correctly` — all present in `hitl.rs`
- [x] Integration tests: `compozy_db_migration_should_add_hitl_table_cleanly`, `compozy_db_migration_should_be_idempotent_with_hitl_table`, `hitl_repository_should_survive_connection_restart`, `hitl_repository_sequence_assignment_should_be_race_safe` — all present
- [x] Bonus test: `hitl_requests_without_dispatch_should_sequence_independently` — handles the `dispatch_id IS NULL` scoping case

## Findings
- All deliverables fully implemented. The HITL table is properly first-class and independent from `workflow_checkpoint` payloads and `ApprovalManager`.
- The `ensure_pending_transition` function is the sole transition guard and is called uniformly from `answer`, `cancel`, and `mark_timed_out`, preventing any terminal state from transitioning.
- The atomic sequence assignment uses `TransactionBehavior::Immediate` to block concurrent writers and prevent duplicate sequence numbers — the race-safety test confirms this with a concurrent barrier test that asserts `[1, 2]` as the outcome.
- The `timeout_at` column is stored and returned but not enforced — correctly deferred to task 30 as specified.
- The `answer` SQL update uses the same `IMMEDIATE` transaction as the sequence insert, ensuring all three fields (`response_json`, `answered_at`, `status`) are written atomically.
- The `WorkflowStoreSet` integrates `SqliteHitlRepository` as `stores.hitl`, making it available kernel-wide.
- The test fixtures correctly seed both `workflow_run` and `agent_dispatch` parent rows to satisfy FK constraints before creating HITL records.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/hitl.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/migrations/compozy/20260324_009_hitl_request.sql`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/workflow_store.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/db_migration.rs`
