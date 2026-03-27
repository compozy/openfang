## markdown

## status: completed

<task_context>
<domain>engine/packs/system</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task40</dependencies>
</task_context>

# Task 41.0: Pack System Install Upgrade And Bootstrap

## Overview

Implement the pack install, upgrade, upgrade dry-run, and uninstall endpoints, along with
built-in pack bootstrapping at startup. Packs are versioned distribution units following ADR-044.
Built-in packs (including SDLC) are bundled first-party packs, not a special case -- they are
installed via the same code path as external packs.

This task implements the operational pack endpoints defined in API-SPEC.md section 7:
`POST /api/v1/packs/install`, `POST /api/v1/packs/{id}/upgrade`,
`POST /api/v1/packs/{id}/upgrade/dry-run`, and `POST /api/v1/packs/{id}/uninstall`.
It depends on Task 40 for the pack list/detail/CRUD endpoints that define the pack resource model.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start -- **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Implement pack install, upgrade, upgrade dry-run, and uninstall endpoints matching API-SPEC.md
  section 7: `POST /api/v1/packs/install`, `POST /api/v1/packs/{id}/upgrade`,
  `POST /api/v1/packs/{id}/upgrade/dry-run`, `POST /api/v1/packs/{id}/uninstall`. Built-in packs
  (including SDLC) must be bootstrapped on first startup through the same pack system -- there is
  no special-case install path for bundled packs.
- The upgrade dry-run must return the `would_execute`, `resolved`, `effects`, and `explanation`
  shape from API-SPEC.md section 7, accurately reporting which managed objects would be added,
  updated, or removed without mutating anything.
- Pack state (installed packs, versions, managed object inventory) must be stored in `compozy.db`
  in a `pack` table. This table is introduced as part of this task if not yet present. Minimum
  columns: `pack_id`, `name`, `version`, `source_kind`, `installed`, `managed`, `installed_at`,
  `updated_at`, `objects_json` (counts by type).
- Built-in pack bootstrapping: the kernel boot sequence must check whether the SDLC pack is
  installed. If not, it calls the same `PackInstaller` logic that backs
  `POST /api/v1/packs/install`. This ensures the boot path and the API path use identical code.
- Upgrades must be explicit (ADR-044): managed definitions are updated in place; user forks
  survive upgrades untouched. The `forked_from` provenance field must remain intact after an
  upgrade of the upstream pack.
- Uninstall must remove managed definitions and the pack record. If user forks exist, the
  uninstall must error unless a `force` flag is provided.
- The SDLC built-in pack must include: a workflow definition, agent definitions, and trigger
  definitions that represent a minimal software development lifecycle automation.
</requirements>

## Subtasks

- [x] 41.1 Create the `pack` table migration in `migrations/compozy/` if not yet present.
      Columns: `pack_id TEXT PRIMARY KEY`, `name TEXT NOT NULL`, `version TEXT NOT NULL`,
      `source_kind TEXT NOT NULL` (bundled/external), `installed INTEGER NOT NULL DEFAULT 1`,
      `managed INTEGER NOT NULL DEFAULT 1`, `installed_at TEXT NOT NULL`, `updated_at TEXT NOT NULL`,
      `objects_json TEXT` (JSON counts by type).

- [x] 41.2 Implement the `PackInstaller` struct with methods: `install`, `upgrade`, `upgrade_dry_run`,
      `uninstall`. The `install` method accepts a source descriptor (`kind: "bundled"` or
      `kind: "external"`, `pack_id`, `version`), resolves the pack content, writes managed
      definitions to disk (using the shared `definition_store` from the design decisions), records
      the pack in the `pack` table, and returns the install result. The `upgrade` method compares
      the installed version with the target version, updates managed definitions, preserves user
      forks, and increments the version. The `upgrade_dry_run` method returns the diff without
      mutating. The `uninstall` method removes managed definitions and the pack record.

- [x] 41.3 Implement `POST /api/v1/packs/install` endpoint. The handler accepts:
      ```json
      {
        "source": {
          "kind": "bundled",
          "pack_id": "sdlc",
          "version": "1.2.0"
        }
      }
      ```
      and calls `PackInstaller::install`.

- [x] 41.4 Implement `POST /api/v1/packs/{id}/upgrade` and
      `POST /api/v1/packs/{id}/upgrade/dry-run` endpoints. The upgrade dry-run returns:
      ```json
      {
        "would_execute": true,
        "resolved": {
          "pack_id": "sdlc",
          "from_version": "1.2.0",
          "to_version": "1.3.0"
        },
        "effects": {
          "managed_objects_added": 1,
          "managed_objects_updated": 3,
          "managed_objects_removed": 0,
          "forks_untouched": 2
        },
        "explanation": {
          "managed_objects_only": true,
          "forks_remain_detached": true
        }
      }
      ```

- [x] 41.5 Implement `POST /api/v1/packs/{id}/uninstall` endpoint. The handler removes managed
      definitions and the pack record. If user forks exist without the `force` flag, return an
      error with the list of forked definition IDs.

- [x] 41.6 Implement built-in pack bootstrapping in the kernel boot sequence. On first kernel
      startup when the SDLC pack is not installed, call `PackInstaller::install` with the bundled
      SDLC pack descriptor. The SDLC pack content (workflow, agent, trigger definitions) must be
      embedded in the binary or loaded from a known resource path.

- [x] 41.7 Define the SDLC built-in pack content: a minimal workflow definition, agent definitions,
      and trigger definitions representing a basic software development lifecycle automation.

- [x] 41.8 Add tests for pack operations. See the Tests section below.

- [x] 41.9 Confirm that `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
      and `cargo test --workspace` all pass at zero warnings before marking this task complete.

## Implementation Details

The pack system follows ADR-044: packs are versioned distribution units; built-in packs are
bundled first-party packs, not a separate special case; upgrades are explicit; managed definitions
are immutable in place; user forks survive upgrades.

The `POST /api/v1/packs/install` install request shape from API-SPEC.md section 7:

```json
{
  "source": {
    "kind": "bundled",
    "pack_id": "sdlc",
    "version": "1.2.0"
  }
}
```

Pack state is stored in `compozy.db`. Managed definitions are written to the file-backed
definition directories (`~/.compozy/workflows/`, `~/.compozy/agents/`, `~/.compozy/triggers/`)
using the shared `definition_store` infrastructure. Each managed definition carries provenance
metadata (`managed_by_pack`, `pack_version`) so the pack system can identify which definitions
belong to which pack.

Built-in pack bootstrapping uses the same `PackInstaller` code path as the API endpoint. The
kernel boot sequence checks the `pack` table; if the SDLC pack is not present, it triggers an
install. This ensures there is no special-case install logic that could diverge from the API path.

### Relevant Files

- `crates/openfang-api/src/routes.rs` -- handler implementations
- `crates/openfang-api/src/server.rs` -- route registration
- `crates/openfang-kernel/src/kernel.rs` -- boot sequence, background task spawning
- `crates/openfang-memory/src/migration.rs` -- migration runner pattern
- `migrations/compozy/` -- migration sequence to extend
- `tasks/prd-compozy/docs/API-SPEC.md` section 7 -- pack endpoints
- `tasks/prd-compozy/docs/adrs/044-versioned-packs-explicit-upgrades-and-safe-forks.md`
- `tasks/prd-compozy/docs/DESIGN.md` section 16 -- packs

### Dependent Files

- Task 40 -- pack list/detail/CRUD endpoints (direct dependency)
- Task 43 -- E2E integration test will verify pack bootstrapping

## Deliverables

- `pack` table migration in `compozy.db`
- `PackInstaller` with install, upgrade, upgrade_dry_run, uninstall methods
- `POST /api/v1/packs/install` endpoint
- `POST /api/v1/packs/{id}/upgrade` and `POST /api/v1/packs/{id}/upgrade/dry-run` endpoints
- `POST /api/v1/packs/{id}/uninstall` endpoint
- Built-in pack bootstrap in kernel boot sequence
- SDLC built-in pack content (workflow, agent, trigger definitions)

## Tests

### Unit Tests (Required)

- [x] `POST /api/v1/packs/install` with a `bundled` source descriptor for a known built-in pack
      creates the correct `pack` record in `compozy.db` and installs managed definitions under the
      pack namespace.
- [x] `POST /api/v1/packs/{id}/upgrade/dry-run` returns the correct `effects` object without
      mutating any managed object; querying the pack after the dry-run confirms the version has not
      changed.
- [x] `POST /api/v1/packs/{id}/upgrade` mutates only managed pack objects; user-forked definitions
      with the same IDs are not overwritten, and their `forked_from` provenance is preserved.
- [x] `POST /api/v1/packs/{id}/uninstall` removes managed definitions and the pack record.
- [x] `POST /api/v1/packs/{id}/uninstall` without `force` flag errors when user forks exist.

### Integration Tests (Required)

- [x] Pack install from a bundled source creates managed definitions; pack upgrade correctly
      increments the version and updates managed objects without touching user forks; pack uninstall
      removes managed definitions and the pack record.
- [x] Built-in pack bootstrapping: start the kernel with an empty `compozy.db`; verify the SDLC
      pack is installed automatically; verify the pack record exists with correct version and
      source_kind.

### Regression and Anti-Pattern Guards

- [x] Pack upgrade must never overwrite user-forked definitions; the `forked_from` provenance
      field must remain intact after an upgrade of the upstream pack.
- [x] Built-in pack bootstrapping must use the same code path as `POST /api/v1/packs/install`;
      there must be no special-case install logic for bundled packs that bypasses the pack system.
- [x] Pack install must not create duplicate managed definitions if called twice with the same
      pack and version.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- Pack install, upgrade, upgrade dry-run, and uninstall work correctly for bundled packs.
- Built-in packs are bootstrapped via the pack system on first start using the same code path
  as the API endpoint.
- The upgrade dry-run accurately reports managed object changes without mutating anything.
- User forks survive pack upgrades with provenance intact.
- The SDLC built-in pack is installed automatically on first startup.
- All `cargo fmt`, `cargo clippy`, and `cargo test` checks pass at zero warnings.

---

## Notes

- CLI commands for pack management are deferred to future work (do not touch openfang-cli).
- This task implements DESIGN.md section 16 (packs) and API-SPEC.md section 7 (pack endpoints).
