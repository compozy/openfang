## markdown

## status: completed

<task_context>
<domain>cli/commands</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>low</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 5.0: CLI Event Ingress Commands

## Overview

Add `openfang event` command group to the CLI, exposing the event ingress
pipeline (PRD Task 36) to terminal users. This is the lightest command group
in the PRD-CLI — only 2 subcommands — but it enables a critical workflow:
injecting events from the terminal that trigger automated workflows.

The `event send` command posts an event payload to the trigger match engine.
The `event dry-run` command lets users test whether their event would match
any triggers without actually firing them. Both accept a JSON file as input.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-cli/techspec.md` for the full CLI architecture specification
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add `EventCommands` subcommand enum with 2 variants: `Send`, `DryRun`
- Both variants take a positional `file: PathBuf` argument for the JSON event
  payload
- `send` POSTs to `/api/v1/events` and prints: event ID, number of matched
  triggers, number of effects, and any failures
- `dry-run` POSTs to `/api/v1/events/dry-run` and prints: whether it would
  execute, number of resolved triggers, effects, and explanation
- Register as `#[command(subcommand)]` variant `Event(EventCommands)` in the
  `Commands` enum
- Both commands must validate that the file exists and contains valid JSON
  before sending
- Error response from the API must be displayed with `ui::error()` and exit 1
</requirements>

## Subtasks

- [x] 5.1 Define `EventCommands` enum with clap `#[derive(Subcommand)]` and
      both variants
- [x] 5.2 Add `Event(EventCommands)` to the `Commands` enum and wire dispatch
- [x] 5.3 Implement `cmd_event_send` — read JSON file, POST to `/api/v1/events`,
      print summary (event_id, matched_triggers count, effects count, failures)
- [x] 5.4 Implement `cmd_event_dry_run` — read JSON file, POST to
      `/api/v1/events/dry-run`, print summary (would_execute, resolved count,
      explanation)
- [x] 5.5 Run `make fmt && make lint && make test` — all must pass with zero
      warnings before marking done

## Implementation Details

The event JSON payload has this shape:

```json
{
  "event": "deployment.completed",
  "source": "ci-pipeline",
  "payload": {"repo": "openfang", "branch": "main", "sha": "abc123"},
  "idempotency_key": "deploy-abc123",
  "metadata": {}
}
```

The `send` response includes rich information about what happened:

```rust
if let Some(id) = body["event_id"].as_str() {
    ui::success(&format!("Event accepted: {id}"));
    println!("  Matched triggers: {}", body["matched_triggers"].as_u64().unwrap_or(0));
    println!("  Effects: {}", body["effects"].as_array().map(|a| a.len()).unwrap_or(0));
    if let Some(failures) = body["failures"].as_array() {
        if !failures.is_empty() {
            ui::error(&format!("{} trigger(s) failed", failures.len()));
        }
    }
}
```

### Relevant Files

- `crates/openfang-cli/src/main.rs` — all changes go here
- `crates/openfang-cli/src/ui.rs` — error/success helpers

### Dependent Files

- `crates/openfang-api/src/server.rs` — route registration (read-only)
- `tasks/prd-cli/techspec.md` — full API mapping

## Deliverables

- `EventCommands` enum registered in `Commands`
- 2 handler functions
- Rich output for `send` (event ID, matches, effects, failures)
- Explanatory output for `dry-run`
- All lint and test checks passing

## Tests

### Unit Tests (Required)

- [x] `openfang event --help` exits 0 and output contains both subcommands
      (send, dry-run)
- [x] `openfang event send nonexistent.json` prints file-not-found error
- [x] `openfang event send` with no file argument prints usage help

### Integration Tests (Required)

- [x] With daemon: `openfang event dry-run <valid_file>` returns response
      without triggering any actual workflow execution
- [x] With daemon: `openfang event send <file>` with invalid JSON content
      returns a structured error

### Regression and Anti-Pattern Guards

- [x] File must be validated as valid JSON before sending to API
- [x] Existing CLI commands remain unchanged
- [x] No `unwrap()` in handler code

### Verification Commands

- [x] `make fmt`
- [x] `make lint`
- [x] `make test`

## Success Criteria

- `openfang event send <file>` posts an event and prints match summary
- `openfang event dry-run <file>` shows what would happen without executing
- Invalid files produce clear error messages (not panics)
- `make fmt && make lint && make test` all pass at zero warnings and zero failures
