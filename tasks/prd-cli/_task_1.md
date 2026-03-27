## markdown

## status: pending

<task_context>
<domain>cli/commands</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 1.0: CLI Task And Subtask Commands

## Overview

Add `openfang task` and `openfang subtask` command groups to the CLI, exposing
the full Task/Subtask CRUD and replan API (PRD Task 32) to terminal users.
These are the foundational domain objects of Compozy — every other CLI command
group (runs, dispatches, looper, artifacts, docs) references tasks. Delivering
these first unblocks all subsequent phases.

The backend already implements all required endpoints under `/api/v1/tasks` and
`/api/v1/subtasks`. The CLI commands are thin HTTP wrappers using the existing
`daemon_client()` + `daemon_json()` pattern.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-cli/techspec.md` for the full CLI architecture specification
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add `TaskCommands` subcommand enum to `crates/openfang-cli/src/main.rs` with
  variants: `List`, `Get`, `Create`, `Update`, `Delete`, `Replan`, `Subtasks`,
  `Artifacts`, `Docs` — each with the arguments defined in the techspec
- Add `SubtaskCommands` subcommand enum with variants: `List`, `Get`, `Create`,
  `Update`, `Delete` — each with the arguments defined in the techspec
- Register both as `#[command(subcommand)]` variants in the `Commands` enum:
  `Task(TaskCommands)` and `Subtask(SubtaskCommands)`
- Wire dispatch in the main `match` block to the corresponding `cmd_task_*` and
  `cmd_subtask_*` handler functions
- Implement 14 handler functions (9 for task, 5 for subtask) following the
  existing `cmd_workflow_*` pattern: `require_daemon()` -> `daemon_client()` ->
  HTTP call -> `daemon_json()` -> columnar or `--json` output
- `list` commands must support `--status`, `--priority`, `--limit`, and `--json`
  flags for filtering and output control
- `create`, `update`, and `replan` commands accept a JSON file path (`<FILE>`)
  as input, read the file, parse as JSON, and POST/PUT to the API
- `get` commands with `--json` must print `serde_json::to_string_pretty(&body)`
- Error handling must use `ui::error()` and `ui::error_with_fix()` with
  `std::process::exit(1)` on failure, matching existing CLI patterns
- Table output for `task list` must show: `ID | TITLE | STATUS | PRIORITY | OWNER | CREATED`
- Table output for `subtask list` must show: `ID | TASK_ID | TITLE | STATUS | KIND | ASSIGNEE`
</requirements>

## Subtasks

- [ ] 1.1 Define `TaskCommands` enum with clap `#[derive(Subcommand)]` and all
      9 variants with their arguments (status, priority, limit, json flags;
      task_id positional; file PathBuf for create/update/replan)
- [ ] 1.2 Define `SubtaskCommands` enum with clap `#[derive(Subcommand)]` and
      all 5 variants with their arguments
- [ ] 1.3 Add `Task(TaskCommands)` and `Subtask(SubtaskCommands)` to the
      `Commands` enum with doc comments
- [ ] 1.4 Add dispatch arms in the main `match` block for both command groups
- [ ] 1.5 Implement `cmd_task_list`, `cmd_task_get`, `cmd_task_create`,
      `cmd_task_update`, `cmd_task_delete` handler functions
- [ ] 1.6 Implement `cmd_task_replan`, `cmd_task_subtasks`, `cmd_task_artifacts`,
      `cmd_task_docs` handler functions
- [ ] 1.7 Implement `cmd_subtask_list`, `cmd_subtask_get`, `cmd_subtask_create`,
      `cmd_subtask_update`, `cmd_subtask_delete` handler functions
- [ ] 1.8 Verify `openfang task --help` and `openfang subtask --help` render
      correctly with all subcommands visible
- [ ] 1.9 Run `make fmt && make lint && make test` — all must pass with zero
      warnings before marking done

## Implementation Details

All code goes in `crates/openfang-cli/src/main.rs` following the existing
monolithic pattern. See the techspec for the complete API mapping table.

Key patterns to follow (from existing code):
- Subcommand enum definition: see `WorkflowCommands` (lines 519-551)
- GET list handler: see `cmd_workflow_list()` (lines 3139-3161)
- POST create handler: see `cmd_workflow_create()` (lines 3163-3196)
- PUT update handler: see `cmd_workflow_update()` (lines 3260-3296)
- DELETE handler: see `cmd_workflow_delete()` (lines 3298-3317)
- `--json` flag pattern: see `cmd_agent_list()` (lines 1700-1738)
- Query parameter construction: see `cmd_trigger_list()` (lines 3327-3331)

The `replan` command is unique — it POSTs to `/api/v1/tasks/{id}/replan` with
an atomic bulk operation payload read from a JSON file. Follow the same
file-read pattern as `cmd_workflow_create`.

### Relevant Files

- `crates/openfang-cli/src/main.rs` — all changes go here
- `crates/openfang-cli/src/ui.rs` — `error()`, `success()`, `hint()` helpers
- `crates/openfang-cli/src/table.rs` — available but not used by existing list commands

### Dependent Files

- `crates/openfang-api/src/server.rs` — route registration (already done, read-only reference)
- `crates/openfang-api/src/routes.rs` — handler implementations (already done, read-only reference)
- `tasks/prd-cli/techspec.md` — full API mapping and handler specification

## Deliverables

- `TaskCommands` and `SubtaskCommands` enums registered in `Commands`
- 14 handler functions (9 task + 5 subtask) wired and functional
- `--json` flag support on all list/get commands
- `--status`, `--priority`, `--limit` filter flags on list commands
- JSON file input for create/update/replan commands
- All lint and test checks passing

## Tests

### Unit Tests (Required)

- [ ] `openfang task --help` exits 0 and output contains all 9 subcommands
      (list, get, create, update, delete, replan, subtasks, artifacts, docs)
- [ ] `openfang subtask --help` exits 0 and output contains all 5 subcommands
      (list, get, create, update, delete)
- [ ] `openfang task list` without a running daemon prints error message
      containing "requires a running daemon" and exits 1
- [ ] `openfang task create nonexistent.json` prints file-not-found error and
      exits 1

### Integration Tests (Required)

- [ ] With a running daemon: `openfang task list --json` returns valid JSON
      array to stdout
- [ ] With a running daemon: `openfang task create <file>` followed by
      `openfang task get <id> --json` returns the created task
- [ ] With a running daemon: `openfang subtask list --task_id <id> --json`
      returns valid JSON

### Regression and Anti-Pattern Guards

- [ ] Existing CLI commands (agent, workflow, trigger, etc.) continue to work
      unchanged after adding new command groups
- [ ] No `unwrap()` in handler code — use `unwrap_or()` or `unwrap_or_default()`
- [ ] No new crate dependencies added

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- `openfang task list` and `openfang subtask list` display formatted tables
  when tasks exist
- `openfang task get <id> --json` outputs valid JSON matching the API response
- Create/update/delete/replan round-trips work end-to-end against a running
  daemon
- All 14 handler functions follow the exact same pattern as existing workflow
  handlers
- `make fmt && make lint && make test` all pass at zero warnings and zero
  failures
