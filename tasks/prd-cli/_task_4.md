## markdown

## status: completed

<task_context>
<domain>cli/commands</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>medium</complexity>
<dependencies>task2</dependencies>
</task_context>

# Task 4.0: CLI Looper Commands

## Overview

Add `openfang looper` command group to the CLI, exposing the Looper run
management API (PRD Tasks 34, 39) to terminal users. The looper is Compozy's
specialized executor for iterative subtask work — it runs subtasks in
sequential or parallel mode with progress tracking and durable checkpoints.

The looper commands cover the full lifecycle: create a looper run, list/inspect
runs, view subtask execution state, pause/resume/cancel, and watch live
progress via SSE. The `looper create` command accepts a JSON file with
`task_id`, `subtask_ids`, and `execution_policy`.

This task depends on Phase 2 (task 2) because the SSE watch pattern is
established there. The looper watch handler reuses the same `BufReader`-based
SSE consumption pattern.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-cli/techspec.md` for the full CLI architecture specification
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add `LooperCommands` subcommand enum with 8 variants: `List`, `Get`, `Create`,
  `Subtasks`, `Pause`, `Resume`, `Cancel`, `Watch`
- `list` must support `--task_id`, `--status`, `--execution_mode`, and `--json`
  flags
- `get` must support `--json` flag
- `create` takes a JSON file path and POSTs to `/api/v1/looper-runs`
- `subtasks` shows the looper subtask execution view (not the canonical subtask
  records)
- Pause/resume/cancel commands POST to the corresponding control-plane endpoints
  and print the `AcceptedActionResponse`
- `watch` streams the looper run SSE endpoint
  (`GET /api/v1/looper-runs/{id}/events`) with pretty-printed events
- Register as `#[command(subcommand)]` variant `Looper(LooperCommands)` in the
  `Commands` enum
- Table output for `looper list`: `ID | TASK_ID | STATUS | MODE | PROGRESS | UPDATED`
- The `PROGRESS` column must show `completed/total` format (e.g., `3/12`)
</requirements>

## Subtasks

- [x] 4.1 Define `LooperCommands` enum with clap `#[derive(Subcommand)]` and
      all 8 variants with their arguments
- [x] 4.2 Add `Looper(LooperCommands)` to the `Commands` enum and wire dispatch
- [x] 4.3 Implement `cmd_looper_list` with filter flags and progress column
      formatting (`completed/total`)
- [x] 4.4 Implement `cmd_looper_get` with `--json` support
- [x] 4.5 Implement `cmd_looper_create` — read JSON file, POST, print accepted
      response with `looper_run_id`
- [x] 4.6 Implement `cmd_looper_subtasks` showing looper subtask execution view
- [x] 4.7 Implement `cmd_looper_pause`, `cmd_looper_resume`, `cmd_looper_cancel`
- [x] 4.8 Implement `cmd_looper_watch` SSE handler reusing the pattern from
      Phase 2
- [x] 4.9 Run `make fmt && make lint && make test` — all must pass with zero
      warnings before marking done

## Implementation Details

The `create` command reads a JSON file containing the execution policy:

```json
{
  "task_id": "task_001",
  "subtask_ids": null,
  "execution_policy": {
    "mode": "parallel",
    "max_parallelism": 4,
    "selection": "priority"
  }
}
```

The list handler must format the progress column by extracting nested fields:

```rust
let progress = format!("{}/{}",
    item["progress"]["completed"].as_u64().unwrap_or(0),
    item["progress"]["total"].as_u64().unwrap_or(0),
);
```

### Relevant Files

- `crates/openfang-cli/src/main.rs` — all changes go here
- `crates/openfang-cli/src/ui.rs` — error/success helpers

### Dependent Files

- `crates/openfang-api/src/server.rs` — route registration (read-only)
- `tasks/prd-cli/techspec.md` — full API mapping

## Deliverables

- `LooperCommands` enum registered in `Commands`
- 8 handler functions plus 1 SSE watch handler
- Progress column formatted as `completed/total`
- JSON file input for `create` command
- All lint and test checks passing

## Tests

### Unit Tests (Required)

- [x] `openfang looper --help` exits 0 and output contains all 8 subcommands
- [x] `openfang looper list` without a daemon prints "requires a running daemon"
- [x] `openfang looper create nonexistent.json` prints file-not-found error

### Integration Tests (Required)

- [x] With daemon: `openfang looper list --json` returns valid JSON
- [x] With daemon: `openfang looper list --status running --json` filters correctly
- [x] With daemon: `openfang looper pause <nonexistent_id>` returns error

### Regression and Anti-Pattern Guards

- [x] Progress column must not panic on missing `progress` field — default to `0/0`
- [x] Existing CLI commands remain unchanged
- [x] No `unwrap()` in handler code

### Verification Commands

- [x] `make fmt`
- [x] `make lint`
- [x] `make test`

## Success Criteria

- `openfang looper list` displays runs with progress column (e.g., `3/12`)
- `openfang looper create <file>` starts a looper run and prints the run ID
- `openfang looper watch <id>` streams events in real-time
- Pause/resume/cancel print acceptance confirmation
- `make fmt && make lint && make test` all pass at zero warnings and zero failures
