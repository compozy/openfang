---
name: upstream-sync
description: Synchronizes a fork repository with its upstream source while managing risk through clarification questions. Analyzes divergence between fork and upstream, classifies changes by risk level, and pauses to ask clarifying questions when high-risk changes are detected before proceeding. Use when syncing fork with upstream, pulling upstream changes, updating from upstream, or maintaining fork parity. Do not use for rebasing feature branches (use git-rebase instead) or for general git operations unrelated to upstream synchronization.
---

# Upstream Fork Synchronization

## Quick Start

For routine syncs with low divergence:

```bash
# Step 1: Backup
bash scripts/pre-sync-backup.sh

# Step 2: Fetch upstream
git fetch upstream --tags

# Step 3: Analyze and assess risk
bash scripts/analyze-upstream.sh
bash scripts/risk-assessment.sh

# Step 4: If low risk (exit code 0), merge
git checkout main
git merge upstream/main

# Step 5: Validate
make fmt && make lint && make test

# Step 6: Push
git push origin main
```

For anything beyond trivial changes, follow the full workflow below.

## Core Workflow

```
Upstream Sync Workflow:
- [ ] Step 1: Pre-flight checks
- [ ] Step 2: Create safety backup
- [ ] Step 3: Fetch upstream changes
- [ ] Step 4: Analyze divergence scope
- [ ] Step 5: Assess risk level
- [ ] Step 6: RISK GATE — clarify with user if needed
- [ ] Step 7: Choose sync strategy
- [ ] Step 8: Execute sync
- [ ] Step 9: Resolve conflicts
- [ ] Step 10: Validate and test
- [ ] Step 11: Push and document
```

### Step 1: Pre-flight Checks

Verify the environment before starting:

```bash
# Confirm upstream remote exists
git remote -v | grep upstream

# If missing, add it
git remote add upstream git@github.com:RightNow-AI/openfang.git

# Ensure working tree is clean
git status --porcelain
```

If uncommitted changes exist, STOP. Ask the user whether to stash or commit. Never sync on a dirty tree.

### Step 2: Create Safety Backup

ALWAYS create a backup before syncing. Execute:

```bash
bash scripts/pre-sync-backup.sh
```

This creates a timestamped backup branch and saves recovery info.

### Step 3: Fetch Upstream Changes

```bash
git fetch upstream --tags
```

Fetching only downloads objects. Nothing changes locally. If no `upstream` remote exists, Step 1 should have configured it.

### Step 4: Analyze Divergence Scope

```bash
bash scripts/analyze-upstream.sh
```

The script outputs:
- Commit counts: upstream-ahead and fork-ahead since last sync
- Files changed upstream, grouped by crate/directory
- Overlapping files (modified by BOTH fork and upstream) — conflict candidates
- New files added upstream, files deleted upstream (with dangling reference check)
- Cargo.toml and migration conflict indicators

If the divergence is large (50+ commits), read `references/risk-rules.md` for crate sensitivity rankings before proceeding.

### Step 5: Assess Risk Level

```bash
bash scripts/risk-assessment.sh
```

The script checks seven risk rules and exits with a risk-coded exit code:

| Exit Code | Level | Meaning |
|-----------|-------|---------|
| 0 | **LOW** | Safe to merge. No questions needed |
| 2 | **MEDIUM** | Partial overlap. Ask 1-2 targeted questions |
| 3 | **HIGH** | Core crate conflicts. Mandatory clarification |
| 1 | **ERROR** | Script failed (missing remote, no merge base) |

Risk rules evaluated: commit volume, file overlap count, critical path overlap (kernel/runtime/types/server), Cargo.toml conflicts, migration conflicts, public API/trait signature changes, dangling references to upstream-deleted files.

For the complete risk classification criteria and crate sensitivity ranking, read `references/risk-rules.md`.

#### Risk Level Summary

| Level | Meaning | Action |
|-------|---------|--------|
| LOW | Non-overlapping changes, docs, tests only | Proceed directly |
| MEDIUM | Changes in shared types, Cargo.toml deps, config structs | Ask 1-2 targeted questions |
| HIGH | Changes in kernel, runtime, or crates the fork heavily modified | Ask detailed questions, create question PR |
| CRITICAL | Breaking trait changes, migration conflicts, API removals | Full review required, create question PR |

### Step 6: RISK GATE — Clarification Questions

**This is the most important step.** If risk is MEDIUM or above, STOP and ask the user clarifying questions before writing any code. Never guess at intent for risky changes.

#### When to Ask Questions

Read `references/risk-rules.md` for the complete rule set. In summary, ask when:

1. **Shared type changes**: Upstream modified types/traits that the fork also modified
2. **Dependency conflicts**: Upstream changed Cargo.toml dependencies that conflict with fork additions
3. **Core crate changes**: Upstream changed `openfang-kernel`, `openfang-runtime`, or `openfang-types` in ways that touch fork-modified code
4. **Migration conflicts**: Upstream added database migrations that conflict with fork migrations
5. **API surface changes**: Upstream added/removed/changed API routes that the fork also changed
6. **Large scope**: Upstream has 50+ changed files or touches 5+ crates

#### How to Ask Questions

Use the templates in `assets/question-templates.md`. Structure questions as:

1. **State the fact**: "Upstream changed `KernelConfig` to add field X"
2. **State the conflict**: "The fork also modified `KernelConfig` to add field Y"
3. **Present options**: "Options: (a) keep both fields, (b) adopt upstream's field and rename ours, (c) skip this upstream change"
4. **Ask for decision**: "Which approach should be taken?"

For MEDIUM risk, ask questions inline in the conversation.

For HIGH or CRITICAL risk, present all questions at once, grouped by area, using the template in `assets/question-pr-template.md`. Wait for all answers before proceeding.

**NEVER proceed with HIGH or CRITICAL risk changes without explicit user confirmation.**

### Step 7: Choose Sync Strategy

After risk assessment (and user answers if questions were asked), choose the strategy:

#### Strategy A: Fast-Forward Merge (Preferred for LOW risk)

When the fork branch has no divergent commits from upstream:

```bash
git checkout main
git merge --ff-only upstream/main
```

If fast-forward fails, the branches have diverged. Use Strategy B or C.

#### Strategy B: Merge Commit (Preferred for MEDIUM risk)

Preserves both histories with a merge commit:

```bash
git checkout main
git merge upstream/main --no-edit
```

This is the safest default. The merge commit documents the sync point.

#### Strategy C: Cherry-Pick Selective (Preferred for HIGH/CRITICAL risk)

When only specific upstream commits should be adopted:

```bash
# List upstream commits not in fork
git log --oneline main..upstream/main

# Cherry-pick specific commits
git cherry-pick <commit-sha>

# Or cherry-pick a range
git cherry-pick <start-sha>..<end-sha>
```

Use this when the user explicitly chose to skip certain upstream changes.

#### Strategy D: Rebase (Use with caution)

Only when the user explicitly requests linear history:

```bash
git checkout main
git rebase upstream/main
```

**Warning**: This rewrites history. Only use on branches not shared with others. Invoke the `git-rebase` skill for conflict resolution guidance.

### Step 8: Execute Sync

Execute the chosen strategy. If no conflicts arise, skip to Step 10.

### Step 9: Resolve Conflicts

When conflicts occur, follow these fork-specific resolution defaults:

1. **Upstream infrastructure wins** — build configs, CI, linting, shared tooling → adopt upstream
2. **Fork-specific features win** — code the fork added that upstream does not have → preserve
3. **Shared logic conflicts** — both sides changed the same function → understand WHY, then merge or ask
4. **New upstream tests** — always adopt
5. **Upstream deleted code** — if fork still references it, keep. If unreferenced, remove

Apply the user's decisions from Step 6 when they were asked specific questions.

#### Cargo.toml Conflicts (most common in Rust fork syncs)

1. Accept upstream's dependency versions as the base
2. Re-add fork-specific dependencies using `cargo add`
3. Run `cargo check` to verify compatibility
4. Never hand-edit `Cargo.lock` — delete it and run `cargo generate-lockfile`

#### Migration Conflicts

1. Keep upstream's migrations as-is (preserve timestamps)
2. Renumber fork-specific migrations to come AFTER upstream's latest
3. Verify with `make test` that migrations apply cleanly

For detailed resolution patterns with code examples, read `references/conflict-resolution.md`.

After resolving all conflicts:

```bash
git add <resolved-files>
git merge --continue   # or git rebase --continue
```

### Step 10: Validate and Test

Run the validation script:

```bash
bash scripts/validate-sync.sh
```

This runs all checks: conflict markers, `cargo check`, `make fmt`, `make lint`, `make test`.

If the script is unavailable, run manually — **all three MUST pass**:

```bash
make fmt
make lint
make test
```

If any fail, fix the issues and re-run. Do NOT consider the sync complete until all three pass with zero errors and zero warnings.

### Step 11: Push and Document

```bash
git push origin main
```

If force push is needed (rebase strategy), use `--force-with-lease` and **confirm with the user first**:

```bash
git push origin main --force-with-lease
```

If using a sync branch instead of direct main:

```bash
git push origin sync/upstream-YYYY-MM-DD
```

Create a PR using `assets/sync-pr-template.md` with:
- List of upstream commits included
- Risk assessment summary
- Conflicts resolved and how
- Any skipped upstream changes and why

## Handling Partial Syncs

Sometimes the user wants to sync only specific upstream changes. In that case:

1. Run `bash scripts/analyze-upstream.sh` to see all changes
2. Ask the user which changes to include
3. Use Strategy C (cherry-pick) to apply only selected commits
4. Document skipped commits in the PR description

## Emergency Abort

If sync goes wrong at any point:

```bash
# Abort merge in progress
git merge --abort

# Or abort rebase in progress
git rebase --abort

# Recover from backup
git reset --hard backup-sync-<timestamp>
```

The backup from Step 2 ensures no work is lost.

## Bundled Scripts

- **`scripts/pre-sync-backup.sh`**: Create safety backup before syncing
- **`scripts/analyze-upstream.sh`**: Analyze upstream divergence (commits, files, overlaps)
- **`scripts/risk-assessment.sh`**: Classify risk level with scored rules (exit code reflects risk)
- **`scripts/validate-sync.sh`**: Validate post-sync code integrity (conflict markers, compilation, formatting, linting, tests)

## Reference Files

- **Risk Rules**: `references/risk-rules.md` — Complete risk classification criteria and scoring
- **Conflict Resolution**: `references/conflict-resolution.md` — Fork-specific conflict resolution patterns
- **Troubleshooting**: `references/troubleshooting.md` — Common upstream sync issues and fixes

## Asset Files

- **Question Templates**: `assets/question-templates.md` — Templates for clarification questions by risk area
- **Sync PR Template**: `assets/sync-pr-template.md` — PR template for upstream sync PRs
- **Question PR Template**: `assets/question-pr-template.md` — Summary template for HIGH/CRITICAL risk reviews

## When NOT to Sync

- Uncommitted changes in working directory — commit or stash first
- Active rebase or merge in progress — finish or abort first
- Fork CI/CD is broken — fix the build first
- Middle of a large feature implementation — finish the feature first
- Upstream has known broken commits — wait for upstream fix
- Multiple team members actively pushing to fork's main — coordinate first

**Default to caution.** A delayed sync is better than a broken fork.

## Error Handling

- **`upstream` remote not found**: Run `git remote add upstream <url>` and retry
- **Dirty working tree**: Stash (`git stash`) or commit before retrying
- **Cargo.lock conflicts**: Delete `Cargo.lock`, run `cargo generate-lockfile`, verify with `cargo check`
- **Risk assessment script fails**: Fall back to manual — run `git diff --stat upstream/main...HEAD` and classify by reading `references/risk-rules.md`
- **Tests fail after sync**: Isolate failing tests, determine cause (upstream API change vs resolution error), fix, re-validate
- **Upstream force-pushed main**: CRITICAL risk event — abort sync, read `references/troubleshooting.md`, ask user before proceeding
