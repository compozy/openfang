## markdown

## status: pending

<task_context>
<domain>cli/commands</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>medium</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 7.0: CLI Pack Commands

## Overview

Add `openfang pack` command group to the CLI, exposing the full pack lifecycle
management API (PRD Tasks 40, 41) to terminal users. Packs are Compozy's
distribution units — bundles of workflows, triggers, skills, and agent
templates that can be installed, upgraded, forked, and uninstalled.

The pack commands cover the complete lifecycle: browse installed packs, inspect
their contents, install new packs, upgrade with dry-run preview, fork to create
user-owned copies, and uninstall. The `upgrade --dry-run` flag is particularly
important as it lets users preview what would change before committing.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-cli/techspec.md` for the full CLI architecture specification
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add `PackCommands` subcommand enum with 7 variants: `List`, `Get`, `Objects`,
  `Install`, `Upgrade`, `Uninstall`, `Fork`
- `list` must support `--json` flag
- `get` must support `--json` flag
- `objects` shows managed objects inside a pack with `--json` flag
- `install` takes a positional `<source>` argument (pack name, path, or URL)
  and POSTs to `/api/v1/packs/install` with `{"source": "<value>"}`
- `upgrade` takes a positional `<pack_id>` and an optional `--dry-run` flag.
  Without `--dry-run`: POSTs to `/api/v1/packs/{id}/upgrade`.
  With `--dry-run`: POSTs to `/api/v1/packs/{id}/upgrade/dry-run` and prints
  the effects report (what would be added, changed, removed)
- `uninstall` takes a positional `<pack_id>` and POSTs to
  `/api/v1/packs/{id}/uninstall`
- `fork` takes a positional `<pack_id>` and POSTs to `/api/v1/packs/{id}/fork`
- Register as `#[command(subcommand)]` variant `Pack(PackCommands)` in the
  `Commands` enum
- Table output for `pack list`: `ID | NAME | VERSION | SOURCE | OBJECTS | INSTALLED`
- Install/upgrade/uninstall/fork must print `ui::success()` with the action
  result on success
</requirements>

## Subtasks

- [ ] 7.1 Define `PackCommands` enum with clap `#[derive(Subcommand)]` and all
      7 variants with their arguments (including `--dry-run` flag on `Upgrade`)
- [ ] 7.2 Add `Pack(PackCommands)` to the `Commands` enum and wire dispatch
- [ ] 7.3 Implement `cmd_pack_list` and `cmd_pack_get` with `--json` support
- [ ] 7.4 Implement `cmd_pack_objects` showing managed objects inside a pack
- [ ] 7.5 Implement `cmd_pack_install` — POST with source string
- [ ] 7.6 Implement `cmd_pack_upgrade` with `--dry-run` branching logic:
      different endpoint + different output formatting for dry-run vs real
      upgrade
- [ ] 7.7 Implement `cmd_pack_uninstall` and `cmd_pack_fork`
- [ ] 7.8 Run `make fmt && make lint && make test` — all must pass with zero
      warnings before marking done

## Implementation Details

The `upgrade` command has dual behavior based on the `--dry-run` flag:

```rust
fn cmd_pack_upgrade(pack_id: &str, dry_run: bool) {
    let base = require_daemon("pack upgrade");
    let client = daemon_client();

    let url = if dry_run {
        format!("{base}/api/v1/packs/{pack_id}/upgrade/dry-run")
    } else {
        format!("{base}/api/v1/packs/{pack_id}/upgrade")
    };

    let body = daemon_json(client.post(&url).json(&serde_json::json!({})).send());

    if dry_run {
        // Print effects report: added, changed, removed objects
    } else {
        // Print success message
    }
}
```

The `objects` subcommand lists what a pack manages:

```
TYPE        NAME                    STATUS
workflow    onboarding-flow         active
trigger     deploy-completed        enabled
skill       code-review             installed
template    bug-report-agent        available
```

### Relevant Files

- `crates/openfang-cli/src/main.rs` — all changes go here
- `crates/openfang-cli/src/ui.rs` — error/success helpers

### Dependent Files

- `crates/openfang-api/src/server.rs` — route registration (read-only)
- `tasks/prd-cli/techspec.md` — full API mapping

## Deliverables

- `PackCommands` enum registered in `Commands`
- 7 handler functions with `--dry-run` branching on upgrade
- Pack objects display with type/name/status columns
- Install/upgrade/uninstall/fork success messaging
- All lint and test checks passing

## Tests

### Unit Tests (Required)

- [ ] `openfang pack --help` exits 0 and output contains all 7 subcommands
      (list, get, objects, install, upgrade, uninstall, fork)
- [ ] `openfang pack list` without a daemon prints "requires a running daemon"
- [ ] `openfang pack install` with no source argument prints usage help

### Integration Tests (Required)

- [ ] With daemon: `openfang pack list --json` returns valid JSON
- [ ] With daemon: `openfang pack upgrade <id> --dry-run` returns effects
      report without actually upgrading
- [ ] With daemon: `openfang pack uninstall <nonexistent_id>` returns error

### Regression and Anti-Pattern Guards

- [ ] `--dry-run` must use the separate dry-run endpoint, not a query parameter
- [ ] `install` must not silently succeed on empty source string
- [ ] Existing CLI commands remain unchanged
- [ ] No `unwrap()` in handler code

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- `openfang pack list` displays installed packs with object counts
- `openfang pack install <source>` installs a pack and prints confirmation
- `openfang pack upgrade <id> --dry-run` shows effects preview
- `openfang pack objects <id>` shows managed objects
- `make fmt && make lint && make test` all pass at zero warnings and zero failures
