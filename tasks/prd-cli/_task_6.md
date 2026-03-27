## markdown

## status: completed

<task_context>
<domain>cli/commands</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>low</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 6.0: CLI Artifact And Doc Commands

## Overview

Add `openfang artifact` and `openfang doc` command groups to the CLI, exposing
the immutable, versioned artifact/doc browsing API (PRD Tasks 37, 38) to
terminal users. These are read-only commands — the CLI provides listing,
detail inspection, and version history browsing. Artifacts and docs are created
by workflows and looper runs, not directly by users.

Both command groups are structurally identical (list, get, versions) and map to
parallel API endpoints under `/api/v1/artifacts` and `/api/v1/docs`.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-cli/techspec.md` for the full CLI architecture specification
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add `ArtifactCommands` subcommand enum with 3 variants: `List`, `Get`,
  `Versions`
- Add `DocCommands` subcommand enum with 3 variants: `List`, `Get`, `Versions`
- `list` commands must support `--type`, `--task_id`, and `--json` flags
- `get` commands must support `--json` flag
- `versions` commands must support `--json` flag and show version history
  with: `VERSION | HASH | CREATED_BY | CREATED_AT`
- Register both as `#[command(subcommand)]` variants in the `Commands` enum:
  `Artifact(ArtifactCommands)` and `Doc(DocCommands)`
- Table output for `artifact list`: `ID | TYPE | TITLE | VERSION | TASK_ID | CREATED`
- Table output for `doc list`: `ID | TYPE | TITLE | VERSION | TASK_ID | CREATED`
- The `get` detail view must show the current version content inlined, plus
  provenance fields (`created_by_kind`, `created_by_ref`)
</requirements>

## Subtasks

- [x] 6.1 Define `ArtifactCommands` enum with clap `#[derive(Subcommand)]` and
      all 3 variants
- [x] 6.2 Define `DocCommands` enum with clap `#[derive(Subcommand)]` and all
      3 variants
- [x] 6.3 Add `Artifact(ArtifactCommands)` and `Doc(DocCommands)` to the
      `Commands` enum and wire dispatch
- [x] 6.4 Implement `cmd_artifact_list`, `cmd_artifact_get`,
      `cmd_artifact_versions` handlers
- [x] 6.5 Implement `cmd_doc_list`, `cmd_doc_get`, `cmd_doc_versions` handlers
- [x] 6.6 Run `make fmt && make lint && make test` — all must pass with zero
      warnings before marking done

## Implementation Details

The artifact and doc commands are structurally identical. The handlers can
share a pattern — the only differences are the API path prefix and column
labels. Consider extracting the common logic or using a straightforward
copy-and-adjust approach (both are acceptable given the small surface area).

The `versions` subcommand shows the immutable version history:

```
VERSION  HASH                              CREATED_BY       CREATED_AT
1        sha256:a1b2c3d4e5f6...            agent:solver_1   2026-03-25T10:00:00Z
2        sha256:f6e5d4c3b2a1...            workflow:wf_001  2026-03-25T11:30:00Z
```

The `get` detail view for both artifacts and docs should print a structured
summary including the current version content and provenance metadata.

### Relevant Files

- `crates/openfang-cli/src/main.rs` — all changes go here
- `crates/openfang-cli/src/ui.rs` — error/success helpers

### Dependent Files

- `crates/openfang-api/src/server.rs` — route registration (read-only)
- `tasks/prd-cli/techspec.md` — full API mapping

## Deliverables

- `ArtifactCommands` and `DocCommands` enums registered in `Commands`
- 6 handler functions (3 artifact + 3 doc)
- Version history display with hash and provenance
- `--type` and `--task_id` filter flags on list commands
- All lint and test checks passing

## Tests

### Unit Tests (Required)

- [x] `openfang artifact --help` exits 0 and output contains all 3 subcommands
- [x] `openfang doc --help` exits 0 and output contains all 3 subcommands
- [x] `openfang artifact list` without a daemon prints "requires a running daemon"

### Integration Tests (Required)

- [x] With daemon: `openfang artifact list --json` returns valid JSON
- [x] With daemon: `openfang doc list --task_id <id> --json` filters by task
- [x] With daemon: `openfang artifact versions <nonexistent_id>` returns error

### Regression and Anti-Pattern Guards

- [x] SHA-256 hash display must be the full hash, not truncated
- [x] Existing CLI commands remain unchanged
- [x] No `unwrap()` in handler code

### Verification Commands

- [x] `make fmt`
- [x] `make lint`
- [x] `make test`

## Success Criteria

- `openfang artifact list` and `openfang doc list` display formatted tables
- `openfang artifact versions <id>` shows immutable version history
- `--type` and `--task_id` filters work correctly
- `make fmt && make lint && make test` all pass at zero warnings and zero failures
