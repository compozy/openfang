## markdown

## status: pending

<task_context>
<domain>cli/commands</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task1</dependencies>
</task_context>

# Task 2.0: CLI Run And Dispatch Commands

## Overview

Add `openfang run` and `openfang dispatch` command groups to the CLI, exposing
workflow run lifecycle control (PRD Tasks 39, 42) and dispatch control-plane
(PRD Task 33) to terminal users. These commands give visibility into active
workflow executions — the user can list runs, inspect dispatches, retry/cancel
failed dispatches, submit signals, and watch live events via SSE.

The `run` command is intentionally a top-level command (not nested under
`workflow`) because runs have rich sub-resources (dispatches, HITL requests,
signals, checkpoints) that would make nesting too deep.

This phase introduces the **SSE watch pattern** for the first time in the CLI.
The `run watch` and `dispatch watch` subcommands stream server-sent events to
the terminal using `reqwest::blocking` with line-by-line `BufReader` iteration.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-cli/techspec.md` for the full CLI architecture specification
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add `RunCommands` subcommand enum with 11 variants: `List`, `Get`,
  `Dispatches`, `Hitl`, `Signals`, `Signal`, `Checkpoints`, `Pause`, `Resume`,
  `Cancel`, `Watch` — each with the arguments defined in the techspec
- Add `DispatchCommands` subcommand enum with 6 variants: `List`, `Get`,
  `Children`, `Retry`, `Cancel`, `Watch`
- Register both as `#[command(subcommand)]` variants in the `Commands` enum
- Implement 16 handler functions (11 for run, 5 non-SSE for dispatch) plus 2
  SSE watch handlers following the pattern in the techspec
- `run list` must support `--workflow_id`, `--status`, and `--json` flags
- `dispatch list` must support `--run_id`, `--status`, and `--json` flags
- `run signal` accepts positional args: `<run_id> <name> <payload_json>` and
  POSTs to `/api/v1/runs/{id}/signals`
- SSE `watch` commands must: (a) set `Accept: text/event-stream` header,
  (b) read lines via `BufReader`, (c) parse `data:` lines as JSON,
  (d) pretty-print `[timestamp] kind: summary` per event,
  (e) handle connection errors gracefully
- Pause/resume/cancel commands must print the `AcceptedActionResponse` fields
  (`accepted`, `resource_id`, `status`) on success
- Table output for `run list`: `ID | WORKFLOW | STATUS | STEPS | STARTED | UPDATED`
- Table output for `dispatch list`: `ID | RUN_ID | STEP | KIND | TARGET | STATUS | UPDATED`
</requirements>

## Subtasks

- [ ] 2.1 Define `RunCommands` enum with clap `#[derive(Subcommand)]` and all
      11 variants with their arguments
- [ ] 2.2 Define `DispatchCommands` enum with clap `#[derive(Subcommand)]` and
      all 6 variants with their arguments
- [ ] 2.3 Add `Run(RunCommands)` and `Dispatch(DispatchCommands)` to the
      `Commands` enum and wire dispatch in the main `match` block
- [ ] 2.4 Implement `cmd_run_list`, `cmd_run_get`, `cmd_run_dispatches`,
      `cmd_run_hitl`, `cmd_run_signals`, `cmd_run_checkpoints` GET handlers
- [ ] 2.5 Implement `cmd_run_signal` POST handler (inline JSON from positional
      args, not file-based)
- [ ] 2.6 Implement `cmd_run_pause`, `cmd_run_resume`, `cmd_run_cancel` POST
      handlers returning `AcceptedActionResponse`
- [ ] 2.7 Implement `cmd_run_watch` SSE handler — the first SSE consumer in the
      CLI. Use `reqwest::blocking` with `BufReader::new(response)` to iterate
      lines, parse `data:` prefixed lines as JSON, and pretty-print events.
- [ ] 2.8 Implement `cmd_dispatch_list`, `cmd_dispatch_get`,
      `cmd_dispatch_children`, `cmd_dispatch_retry`, `cmd_dispatch_cancel`
      handlers
- [ ] 2.9 Implement `cmd_dispatch_watch` SSE handler reusing the pattern from
      2.7
- [ ] 2.10 Run `make fmt && make lint && make test` — all must pass with zero
       warnings before marking done

## Implementation Details

The SSE watch pattern is new to the CLI. The techspec defines the implementation:

```rust
fn cmd_run_watch(run_id: &str) {
    let base = require_daemon("run watch");
    let client = daemon_client();
    let resp = client
        .get(format!("{base}/api/v1/runs/{run_id}/events"))
        .header("Accept", "text/event-stream")
        .send();
    // BufReader line iteration, parse "data:" lines, pretty-print events
}
```

The `run signal` command is unique — it takes inline positional arguments
rather than a file. The handler constructs the JSON body inline:

```rust
serde_json::json!({
    "name": name,
    "payload": serde_json::from_str::<Value>(payload_json)?,
    "source": "cli"
})
```

### Relevant Files

- `crates/openfang-cli/src/main.rs` — all changes go here
- `crates/openfang-cli/src/ui.rs` — error/success helpers
- `crates/openfang-cli/src/progress.rs` — `Spinner` for watch startup

### Dependent Files

- `crates/openfang-api/src/server.rs` — route registration (read-only reference)
- `tasks/prd-cli/techspec.md` — full API mapping and SSE pattern specification

## Deliverables

- `RunCommands` and `DispatchCommands` enums registered in `Commands`
- 16 handler functions (11 run + 5 dispatch) plus 2 SSE watch handlers
- SSE watch pattern established and reusable for future phases (hitl, looper)
- `--json` flag support on all list/get commands
- Filter flags on list commands
- All lint and test checks passing

## Tests

### Unit Tests (Required)

- [ ] `openfang run --help` exits 0 and output contains all 11 subcommands
- [ ] `openfang dispatch --help` exits 0 and output contains all 6 subcommands
- [ ] `openfang run list` without a daemon prints "requires a running daemon"
- [ ] `openfang run signal` with missing arguments prints usage help

### Integration Tests (Required)

- [ ] With daemon: `openfang run list --json` returns valid JSON
- [ ] With daemon: `openfang dispatch list --run_id <id> --json` returns valid
      JSON filtered by run
- [ ] With daemon: `openfang run pause <id>` on a non-existent run returns an
      error message (not a panic)

### Regression and Anti-Pattern Guards

- [ ] SSE watch handlers must not block indefinitely on connection failure —
      use a timeout on the initial connection
- [ ] Existing CLI commands remain unchanged
- [ ] No `unwrap()` in handler code
- [ ] No new crate dependencies added (reqwest already supports SSE via blocking reads)

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- `openfang run list` and `openfang dispatch list` display formatted tables
- `openfang run watch <id>` streams events to terminal in real-time
- `openfang dispatch retry <id>` prints acceptance confirmation
- `openfang run signal <id> <name> <json>` submits a signal successfully
- SSE pattern is clean and reusable for hitl/looper watch commands in later phases
- `make fmt && make lint && make test` all pass at zero warnings and zero failures
