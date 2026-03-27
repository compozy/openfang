## markdown

## status: pending

<task_context>
<domain>cli/commands</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>medium</complexity>
<dependencies>task2</dependencies>
</task_context>

# Task 3.0: CLI HITL Commands

## Overview

Add `openfang hitl` command group to the CLI, exposing the Human-in-the-Loop
request management API (PRD Task 33) to terminal users. This is the primary
mechanism for humans to interact with running workflows — answering questions,
providing approvals, or cancelling HITL requests that are blocking execution.

The `hitl watch` command is particularly important for operational use: it
streams a global SSE feed of all incoming HITL requests so operators can monitor
and respond in real-time without needing the web dashboard.

This task depends on Phase 2 (task 2) because the SSE watch pattern is
established there. The HITL watch handler reuses the same `BufReader`-based
SSE consumption pattern.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-cli/techspec.md` for the full CLI architecture specification
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add `HitlCommands` subcommand enum with 5 variants: `List`, `Get`, `Answer`,
  `Cancel`, `Watch`
- `list` must support `--run_id`, `--status`, `--kind`, and `--json` flags
- `get` must support `--json` flag
- `answer` takes positional args: `<hitl_id> <response>` where response is a
  plain-text string. The handler POSTs to `/api/v1/hitl-requests/{id}/answer`
  with `{"response": "<text>"}` body
- `cancel` takes a positional `<hitl_id>` and POSTs to
  `/api/v1/hitl-requests/{id}/cancel`
- `watch` streams the global HITL SSE endpoint
  (`GET /api/v1/hitl-requests/stream`) using the SSE pattern from Phase 2.
  Each incoming request must be printed with: `[timestamp] [kind] question`
  format so operators can quickly identify what needs attention
- Register as `#[command(subcommand)]` variant `Hitl(HitlCommands)` in the
  `Commands` enum
- Table output for `hitl list`: `ID | RUN_ID | KIND | STATUS | QUESTION | CREATED`
- The `QUESTION` column must be truncated to 40 characters in table mode to
  prevent line wrapping
</requirements>

## Subtasks

- [ ] 3.1 Define `HitlCommands` enum with clap `#[derive(Subcommand)]` and all
      5 variants with their arguments
- [ ] 3.2 Add `Hitl(HitlCommands)` to the `Commands` enum and wire dispatch in
      the main `match` block
- [ ] 3.3 Implement `cmd_hitl_list` and `cmd_hitl_get` GET handlers with
      `--json` support and columnar table output
- [ ] 3.4 Implement `cmd_hitl_answer` POST handler — takes plain-text response
      string and sends as JSON body
- [ ] 3.5 Implement `cmd_hitl_cancel` POST handler
- [ ] 3.6 Implement `cmd_hitl_watch` SSE handler reusing the pattern from Phase
      2, targeting `/api/v1/hitl-requests/stream`
- [ ] 3.7 Run `make fmt && make lint && make test` — all must pass with zero
      warnings before marking done

## Implementation Details

The `answer` command is a key UX decision: the response is a plain-text
positional argument, not a JSON file. This makes the common case (typing a
quick response) fast:

```
openfang hitl answer req_123 "Yes, approve the deployment"
```

The handler wraps it in the required JSON shape:

```rust
client.post(format!("{base}/api/v1/hitl-requests/{hitl_id}/answer"))
    .json(&serde_json::json!({"response": response}))
    .send()
```

For the `watch` command, the global HITL SSE stream emits
`HitlDetailResponse` objects. The pretty-print format should highlight
pending requests that need human attention.

### Relevant Files

- `crates/openfang-cli/src/main.rs` — all changes go here
- `crates/openfang-cli/src/ui.rs` — error/success helpers

### Dependent Files

- `crates/openfang-api/src/server.rs` — route registration (read-only)
- `tasks/prd-cli/techspec.md` — full API mapping

## Deliverables

- `HitlCommands` enum registered in `Commands`
- 5 handler functions plus 1 SSE watch handler
- Plain-text `answer` UX (no JSON file needed)
- Global HITL watch stream for operator monitoring
- All lint and test checks passing

## Tests

### Unit Tests (Required)

- [ ] `openfang hitl --help` exits 0 and output contains all 5 subcommands
      (list, get, answer, cancel, watch)
- [ ] `openfang hitl list` without a daemon prints "requires a running daemon"
- [ ] `openfang hitl answer` with missing arguments prints usage help

### Integration Tests (Required)

- [ ] With daemon: `openfang hitl list --json` returns valid JSON
- [ ] With daemon: `openfang hitl list --status pending --json` filters correctly
- [ ] With daemon: `openfang hitl cancel <nonexistent_id>` returns error (not panic)

### Regression and Anti-Pattern Guards

- [ ] `answer` command must send response as JSON body, not as URL parameter
- [ ] `QUESTION` column in table output must be truncated to prevent terminal overflow
- [ ] Existing CLI commands remain unchanged
- [ ] No `unwrap()` in handler code

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- `openfang hitl list` displays pending HITL requests in a formatted table
- `openfang hitl answer <id> "text"` submits a response and prints confirmation
- `openfang hitl watch` streams incoming HITL requests in real-time
- Question text is truncated in table output to prevent line wrapping
- `make fmt && make lint && make test` all pass at zero warnings and zero failures
