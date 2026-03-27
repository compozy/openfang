## markdown

## status: completed

<task_context>
<domain>cli/commands</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>medium</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 8.0: CLI Trigger V2 Upgrade

## Overview

Upgrade the existing `openfang trigger` command group with 9 new subcommands
that expose the Trigger v2 features (PRD Task 35) to terminal users. The
current trigger CLI only has `list`, `create`, and `delete`. This task adds:
`get`, `update`, `enable`, `disable`, `test`, `fork`, `validate`, `compile`,
and `runtime`.

The existing `TriggerCommands` enum and its 3 handlers remain unchanged. This
task only adds new variants and new handler functions. The `list` and `create`
commands already work against the `/api/v1/triggers` endpoints, so the new
commands complement them without breaking backward compatibility.

The `trigger test` command is particularly valuable — it lets users dry-run a
trigger against a synthetic event to verify match behavior without actually
firing any workflows.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-cli/techspec.md` for the full CLI architecture specification
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add 9 new variants to the existing `TriggerCommands` enum: `Get`, `Update`,
  `Enable`, `Disable`, `Test`, `Fork`, `Validate`, `Compile`, `Runtime`
- `get` takes a positional `<trigger_id>` and `--json` flag
- `update` takes `<trigger_id>` and a JSON file path
- `enable` and `disable` take a positional `<trigger_id>` and POST to the
  corresponding enable/disable endpoints
- `test` takes `<trigger_id>` and an `<event_json>` string (inline JSON, not
  file) and POSTs to `/api/v1/triggers/{id}/test`. Prints: matched (bool),
  resolved target, would_dispatch, and explanation
- `fork` takes a positional `<trigger_id>` and POSTs to
  `/api/v1/triggers/{id}/fork`
- `validate` takes a JSON file path and POSTs to `/api/v1/triggers/validate`.
  Prints: valid (bool), issues list, normalized definition
- `compile` takes a JSON file path and POSTs to `/api/v1/triggers/compile`.
  Prints: definition_id, compiled payload summary
- `runtime` takes a positional `<trigger_id>` and `--json` flag. GETs
  `/api/v1/triggers/{id}/runtime` and prints runtime status
- Wire all new variants in the existing `TriggerCommands` dispatch arm
- Ensure existing `list`, `create`, `delete` commands remain exactly as-is
</requirements>

## Subtasks

- [x] 8.1 Add 9 new variants to the existing `TriggerCommands` enum with their
      arguments. Ensure existing `List`, `Create`, `Delete` variants are not
      modified.
- [x] 8.2 Add dispatch arms for all 9 new variants in the existing trigger
      match block
- [x] 8.3 Implement `cmd_trigger_get` and `cmd_trigger_runtime` GET handlers
      with `--json` support
- [x] 8.4 Implement `cmd_trigger_update` — read JSON file, PUT to
      `/api/v1/triggers/{id}`
- [x] 8.5 Implement `cmd_trigger_enable` and `cmd_trigger_disable` — simple
      POST with success/error output
- [x] 8.6 Implement `cmd_trigger_test` — takes inline event JSON string, POSTs
      to test endpoint, prints match result with explanation
- [x] 8.7 Implement `cmd_trigger_fork` — POST and print new trigger ID
- [x] 8.8 Implement `cmd_trigger_validate` and `cmd_trigger_compile` — read
      JSON file, POST, print validation/compilation results
- [x] 8.9 Run `make fmt && make lint && make test` — all must pass with zero
      warnings before marking done

## Implementation Details

The `test` command uses inline JSON (not a file) because test events are
typically small:

```
openfang trigger test trg_123 '{"event":"deploy.completed","source":"ci"}'
```

The handler constructs the request:

```rust
fn cmd_trigger_test(trigger_id: &str, event_json: &str) {
    let base = require_daemon("trigger test");
    let event: serde_json::Value = serde_json::from_str(event_json).unwrap_or_else(|e| {
        eprintln!("Invalid event JSON: {e}");
        std::process::exit(1);
    });
    let client = daemon_client();
    let body = daemon_json(
        client.post(format!("{base}/api/v1/triggers/{trigger_id}/test"))
            .json(&serde_json::json!({"event": event}))
            .send(),
    );
    // Print: matched, resolved_target, would_dispatch, explanation
}
```

The `validate` command output should be user-friendly:

```
Validation result: VALID
Issues: none
```

or:

```
Validation result: INVALID
Issues:
  - [error] missing field "event" in match block
  - [warning] target agent_id not resolvable
```

### Relevant Files

- `crates/openfang-cli/src/main.rs` — modify existing `TriggerCommands` enum
  and add new handler functions
- `crates/openfang-cli/src/ui.rs` — error/success helpers

### Dependent Files

- `crates/openfang-api/src/server.rs` — route registration (read-only)
- `tasks/prd-cli/techspec.md` — full API mapping

## Deliverables

- 9 new variants added to `TriggerCommands` (existing variants untouched)
- 9 new handler functions
- Inline JSON input for `test` command
- File-based JSON input for `validate`, `compile`, `update` commands
- User-friendly validation/compilation output
- All lint and test checks passing

## Tests

### Unit Tests (Required)

- [x] `openfang trigger --help` exits 0 and output contains all 12 subcommands
      (3 existing + 9 new)
- [x] Existing `openfang trigger list` behavior is unchanged
- [x] `openfang trigger test` with missing arguments prints usage help
- [x] `openfang trigger validate nonexistent.json` prints file-not-found error

### Integration Tests (Required)

- [x] With daemon: `openfang trigger get <id> --json` returns valid JSON
- [x] With daemon: `openfang trigger test <id> <event_json>` returns match
      result with explanation
- [x] With daemon: `openfang trigger enable <id>` and `openfang trigger
      disable <id>` toggle trigger state
- [x] With daemon: `openfang trigger validate <file>` returns valid/invalid
      with issues list

### Regression and Anti-Pattern Guards

- [x] Existing `list`, `create`, `delete` handlers must not be modified
- [x] `test` command must use inline JSON, not file input
- [x] `validate` and `compile` must use file input, not inline JSON
- [x] No `unwrap()` in handler code
- [x] No new crate dependencies

### Verification Commands

- [x] `make fmt`
- [x] `make lint`
- [x] `make test`

## Success Criteria

- All 12 trigger subcommands (3 existing + 9 new) appear in help
- `openfang trigger test <id> <json>` shows match result with explanation
- `openfang trigger validate <file>` reports valid/invalid with issue details
- `openfang trigger enable/disable <id>` toggles trigger state
- Existing trigger commands work exactly as before
- `make fmt && make lint && make test` all pass at zero warnings and zero failures

---

## Notes

- This is the only task in the PRD-CLI that modifies an existing command group
  rather than creating a new one. Extra care must be taken to not break existing
  `list`, `create`, `delete` functionality.
