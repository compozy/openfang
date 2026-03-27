# Risk Classification Rules

## Scoring System

Each upstream change is scored against these rules. The highest triggered rule determines the overall risk level.

## LOW Risk (Score 0-2)

Changes that do not overlap with fork modifications and are safe to merge automatically.

**Triggers:**
- Documentation-only changes (`.md`, comments, `//!` module docs)
- Test-only changes that do not modify public APIs
- New files in crates the fork has not modified
- Dependency version bumps with no breaking changes (patch/minor)
- Formatting or linting fixes
- New examples or benchmarks

**Action:** Proceed directly with merge. No questions needed.

## MEDIUM Risk (Score 3-5)

Changes that partially overlap with fork modifications or affect shared interfaces.

**Triggers:**
- Upstream modified `Cargo.toml` dependencies that the fork also depends on
- Changes to shared types in `openfang-types` that the fork uses but did not modify
- New fields added to config structs (`KernelConfig`, `AgentConfig`, etc.)
- Changes to error types or error handling patterns
- New public API methods on existing traits (additive, non-breaking)
- Changes to 3-5 crates simultaneously
- 20-50 changed files

**Action:** Ask 1-2 targeted questions about the specific overlap area. Example: "Upstream added field `max_retries` to `KernelConfig`. The fork has custom fields on the same struct. Keep both?"

## HIGH Risk (Score 6-8)

Changes that directly conflict with fork-specific modifications or alter core behavior.

**Triggers:**
- Upstream modified the same functions/methods the fork modified
- Breaking trait changes (method signature changes, removed methods)
- Upstream restructured a crate the fork heavily modified (moved modules, renamed files)
- Database migration conflicts (same migration number, conflicting schema changes)
- Changes to `openfang-kernel` boot sequence or lifecycle management
- Changes to `openfang-runtime` workflow execution that affect fork extensions
- Upstream removed or renamed public APIs that the fork calls
- 50-100 changed files or touches 5+ crates

**Action:** Create a structured question document. Present all conflicts grouped by area. Require explicit user confirmation for each area before proceeding.

## CRITICAL Risk (Score 9-10)

Changes that fundamentally alter architecture or could break the fork irreversibly.

**Triggers:**
- Upstream changed the `KernelHandle` trait contract
- Upstream migrated to a new async runtime or changed concurrency model
- Major crate restructuring (crate splits, merges, renames)
- Upstream changed the Rust edition or MSRV in a way that conflicts with fork code
- Upstream rewrote `server.rs` routing or `AppState` structure
- Upstream changed the CLI entry point or command structure
- Breaking changes to the provider/Arky trait system
- 100+ changed files

**Action:** Full review required. Create a detailed question document with architecture impact analysis. Do NOT proceed without explicit user approval for the overall approach.

## Fork-Specific Overlap Detection

To determine if a file is "fork-modified," compare the fork against the last sync point:

```bash
# Files the fork modified since forking (or last sync)
git diff --name-only upstream/main...HEAD

# Files upstream modified since last sync
git diff --name-only HEAD...upstream/main

# Overlap = files in both lists = conflict zones
comm -12 <(git diff --name-only upstream/main...HEAD | sort) \
         <(git diff --name-only HEAD...upstream/main | sort)
```

## Crate Sensitivity Ranking

Crates ordered by risk when upstream changes touch them:

| Rank | Crate | Reason |
|------|-------|--------|
| 1 | `openfang-kernel` | Core lifecycle, heavily extended by fork |
| 2 | `openfang-runtime` | Workflow engine, fork adds custom execution |
| 3 | `openfang-types` | Shared types, changes cascade everywhere |
| 4 | `openfang-server` | API routes, fork adds custom endpoints |
| 5 | `openfang-memory` | Persistence layer, migration conflicts |
| 6 | `openfang-provider` | Provider trait, fork may add custom providers |
| 7 | `openfang-cli` | CLI commands, user actively building |
| 8 | `openfang-config` | Config structs, additive changes usually safe |
| 9 | Other crates | Lower risk unless fork modified them |

## Question Escalation Matrix

| Risk | Question Format | Blocking? |
|------|----------------|-----------|
| LOW | No questions | No |
| MEDIUM | Inline in conversation, 1-2 questions | Soft block (proceed if no response after presenting options) |
| HIGH | Structured document, grouped by area | Hard block (must wait for answers) |
| CRITICAL | Full architecture review document | Hard block + recommend sync branch for testing |
