# Compozy Database Schema Outline

**Status:** Current database outline
**Date:** 2026-03-21

This document outlines the major tables and ownership boundaries for
`runtime.db` and `compozy.db`.

It is intentionally an outline, not the final SQL migration set.

The recommended migration and delivery order for these tables lives in
[IMPLEMENTATION-PLAN.md](IMPLEMENTATION-PLAN.md).

The detailed Phase 0 and Phase 1 migration slice lives in
[INITIAL-RUNTIME-MIGRATIONS.md](INITIAL-RUNTIME-MIGRATIONS.md).

## 1. Goals

The schema should:

- respect the ownership split defined in [STORAGE-MODEL.md](STORAGE-MODEL.md)
- back the public surfaces defined in [API-SPEC.md](API-SPEC.md)
- avoid duplicating file-backed definitions as a second source of truth

## 2. `runtime.db`

`runtime.db` backs platform-core runtime state.

### `agent_runtime`

Purpose:

- current runtime projection for a loaded agent

Representative columns:

- `agent_id`
- `loaded`
- `state`
- `mode`
- `healthy`
- `active_session_id`
- `active_dispatches`
- `last_active_at`
- `updated_at`

### `agent_session`

Purpose:

- durable session metadata for direct agent use

Representative columns:

- `session_id`
- `agent_id`
- `label`
- `active`
- `message_count`
- `dispatch_count`
- `created_at`
- `updated_at`
- `compacted_at`

### `agent_message`

Purpose:

- message history and message lifecycle for direct agent interaction

Representative columns:

- `message_id`
- `agent_id`
- `session_id`
- `direction`
- `payload_json`
- `status`
- `created_at`
- `completed_at`

### `schedule_runtime`

Purpose:

- runtime state for a file-backed schedule definition

Representative columns:

- `schedule_id`
- `enabled`
- `last_run`
- `next_run`
- `last_status`
- `consecutive_errors`
- `one_shot`
- `updated_at`

### `schedule_execution`

Purpose:

- recent schedule fire receipts and operational history

Representative columns:

- `execution_id`
- `schedule_id`
- `fired_at`
- `status`
- `effect_json`
- `error`

### `trigger_runtime`

Runtime projection for file-backed trigger definitions.

| Column | Type | Notes |
|--------|------|-------|
| `trigger_id` | TEXT PK | Matches the file-backed trigger definition ID |
| `enabled` | INTEGER NOT NULL DEFAULT 1 | Boolean: trigger participates in matching |
| `fire_count` | INTEGER NOT NULL DEFAULT 0 | Total times this trigger has fired |
| `last_fired_at` | TEXT | ISO 8601 timestamp of last fire |
| `loaded_at` | TEXT NOT NULL | When the trigger was loaded into runtime |
| `updated_at` | TEXT NOT NULL | Last state change |

### Optional advanced runtime tables

These are likely but not frozen yet:

- `trigger_runtime` (promoted above — now outlined)
- `channel_runtime`
- `network_peer`
- `runtime_receipt`

## 3. `compozy.db`

`compozy.db` backs product-domain state and durable workflow execution state.

### `workflow_run`

Purpose:

- durable workflow execution root

Representative columns:

- `run_id`
- `workflow_id`
- `workflow_version`
- `status`
- `input_json`
- `vars_json`
- `current_step_id`
- `waiting_kind`
- `waiting_ref`
- `active_dispatch_id`
- `active_hitl_request_id`
- `labels_json`
- `metadata_json`
- `error_json`
- `started_at`
- `updated_at`
- `completed_at`

### `workflow_checkpoint`

Purpose:

- durable state transition and recovery trail

Representative columns:

- `checkpoint_id`
- `run_id`
- `step_id`
- `kind`
- `data_json`
- `created_at`

### `workflow_signal`

Purpose:

- durable signals delivered to or consumed by workflow runs

Representative columns:

- `signal_id`
- `run_id`
- `name`
- `payload_json`
- `source`
- `consumed`
- `created_at`
- `consumed_at`

### `agent_dispatch`

Purpose:

- durable delegated execution inside a workflow run

Representative columns:

- `dispatch_id`
- `run_id`
- `step_id`
- `kind`
- `target_agent`
- `status`
- `input_json`
- `result_json`
- `error_json`
- `attempt`
- `parent_dispatch_id`
- `spawned_agent_id`
- `started_at`
- `updated_at`
- `completed_at`
- `provider_driver`
- `session_id`
- `provider_resume_token`

### `hitl_request`

Purpose:

- durable human interaction requests, including in-step HITL

Representative columns:

- `hitl_request_id`
- `run_id`
- `step_id`
- `dispatch_id`
- `kind`
- `status`
- `question`
- `context_json`
- `response_json`
- `sequence_no`
- `created_at`
- `answered_at`
- `timeout_at`

### `looper_run`

Purpose:

- durable looper execution root

Representative columns:

- `looper_run_id`
- `task_id`
- `source_run_id`
- `status`
- `execution_policy_json`
- `current_subtask_id`
- `progress_json`
- `error_json`
- `started_at`
- `updated_at`
- `completed_at`

### `looper_subtask`

Purpose:

- subtask-level execution view for looper runs

Representative columns:

- `looper_subtask_id`
- `looper_run_id`
- `subtask_id`
- `status`
- `dispatch_id`
- `result_json`
- `error_json`
- `updated_at`

### `artifact`

Purpose:

- stable artifact identity

Representative columns:

- `artifact_id`
- `type`
- `current_version_id`
- `metadata_json`
- `created_at`
- `updated_at`

### `artifact_version`

Purpose:

- immutable or append-only artifact revisions

Representative columns:

- `artifact_version_id`
- `artifact_id`
- `version_no`
- `content_json`
- `created_by_kind`
- `created_by_ref`
- `content_hash`
- `created_at`

### `doc`

Purpose:

- stable document identity

Representative columns:

- `doc_id`
- `type`
- `current_version_id`
- `metadata_json`
- `created_at`
- `updated_at`

### `doc_version`

Purpose:

- immutable or append-only document revisions

Representative columns:

- `doc_version_id`
- `doc_id`
- `version_no`
- `content_json`
- `created_by_kind`
- `created_by_ref`
- `content_hash`
- `created_at`

### `pack`

Installed pack metadata and managed object inventory.

| Column | Type | Notes |
|--------|------|-------|
| `pack_id` | TEXT PK | Unique pack identifier |
| `name` | TEXT NOT NULL | Display name |
| `version` | TEXT NOT NULL | Installed version string |
| `source_kind` | TEXT NOT NULL | `bundled` or `external` |
| `installed` | INTEGER NOT NULL DEFAULT 0 | Count of installed objects |
| `managed` | INTEGER NOT NULL DEFAULT 0 | Count of managed (non-forked) objects |
| `installed_at` | TEXT NOT NULL | ISO 8601 installation timestamp |
| `updated_at` | TEXT NOT NULL | Last modification timestamp |
| `objects_json` | TEXT NOT NULL DEFAULT '{}' | Object counts by type |

### `task`

Purpose:

- product-domain root object replacing the old Compozy `issue` concept

Representative columns:

- `task_id`
- `slug`
- `source_run_id`
- `title`
- `description`
- `status`
- `priority`
- `complexity`
- `position`
- `owner_kind`
- `owner_ref`
- `created_by_kind`
- `created_by_ref`
- `repository_refs_json`
- `label_refs_json`
- `artifact_refs_json`
- `doc_refs_json`
- `file_refs_json`
- `metadata_json`
- `created_at`
- `updated_at`
- `completed_at`

### `subtask`

Purpose:

- executable child work items inside a task
- with local execution context rather than the full durable task context

Representative columns:

- `subtask_id`
- `task_id`
- `title`
- `description`
- `kind`
- `status`
- `complexity`
- `position`
- `assignee_kind`
- `assignee_ref`
- `depends_on_json`
- `parallelizable`
- `input_json`
- `result_json`
- `metadata_json`
- `created_at`
- `updated_at`
- `completed_at`

## 4. Intentional Omissions

This outline does not yet freeze:

- exact SQL types
- exact indexes
- exact foreign key rules
- retention policies
- uniqueness rules for every domain object
- whether `workflow_step_run` is needed immediately

## 5. Relationship To Files

Definitions remain file-backed and are referenced by stable IDs.

These tables may store:

- runtime projections
- compiled projections
- operational metadata
- execution records
- domain records

They should not become the authoritative source of definition content for
config-first resources.
