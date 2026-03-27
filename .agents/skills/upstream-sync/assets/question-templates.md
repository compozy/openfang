# Clarification Question Templates

Use these templates when the risk assessment requires asking the user before proceeding.

## Template: Shared Type Change

```
**Upstream changed `{TypeName}` in `{crate_name}`**

Upstream commit: {sha} — {commit_message}
Change: {description of what upstream changed}

Fork status: The fork also modified `{TypeName}` to {description of fork changes}.

**Options:**
(a) Merge both changes (keep upstream additions + fork additions)
(b) Adopt upstream version, port fork changes to new structure
(c) Keep fork version, skip this upstream change

**Recommendation:** {agent's recommendation based on analysis}

Which approach?
```

## Template: Dependency Conflict

```
**Dependency conflict in `{Cargo.toml path}`**

Upstream changed: `{dep_name}` from {old_version} to {new_version}
Fork changed: `{dep_name}` from {old_version} to {fork_version}
  (or: Fork added `{new_dep}` which conflicts with upstream's `{dep_name}` {new_version})

**Options:**
(a) Use upstream's version ({new_version})
(b) Use fork's version ({fork_version})
(c) Find compatible version that satisfies both

Which version?
```

## Template: Core Crate Restructuring

```
**Upstream restructured `{crate_name}`**

Changes:
- {list of moved/renamed/deleted modules}

Fork impact:
- {list of fork files that import from affected modules}
- {number} fork-specific files need import updates

**Options:**
(a) Update all fork imports to match new upstream structure
(b) Partially sync — adopt restructuring but keep fork modules in place
(c) Skip this upstream change entirely (will increase future sync difficulty)

**Recommendation:** {recommendation}

How to proceed?
```

## Template: API Route Change

```
**Upstream changed API routes in `openfang-server`**

Added routes: {list}
Removed routes: {list}
Modified routes: {list}

Fork conflict: The fork has custom routes that {description of overlap}.

**Options:**
(a) Adopt all upstream route changes, merge with fork routes
(b) Cherry-pick only non-conflicting route changes
(c) Skip route changes, sync other crates only

Which approach?
```

## Template: Migration Conflict

```
**Database migration conflict**

Upstream added: `{migration_file}` — {description}
Fork has: `{fork_migration_file}` — {description}

Conflict: {same number / conflicting schema / etc.}

**Options:**
(a) Keep both — renumber fork migration to run after upstream's
(b) Merge migration SQL into a single migration
(c) Skip upstream migration (may break upstream compatibility)

Which approach?
```

## Template: Breaking Trait Change

```
**Upstream changed trait `{TraitName}` in `{crate_name}`**

Changes:
- {added/removed/modified methods}
- New signature: `{new_signature}`

Fork impact:
- {number} fork implementations of this trait must be updated
- Files affected: {list}

This is a **breaking change** that requires updating fork implementations.

**Options:**
(a) Update all fork implementations to match new trait contract
(b) Wrap upstream trait with fork-specific adapter trait
(c) Skip this upstream change (fork will diverge from upstream trait)

**Recommendation:** {recommendation}

How to proceed?
```

## Template: Large Scope Confirmation

```
**Large upstream sync: {count} commits, {file_count} files across {crate_count} crates**

Summary by area:
- {crate_1}: {change_summary} ({risk_level})
- {crate_2}: {change_summary} ({risk_level})
- ...

Fork overlap: {number} files modified by both fork and upstream.

**Questions:**
1. Sync everything at once, or break into multiple smaller syncs?
2. Any upstream changes to explicitly skip?
3. Preferred strategy: merge commit or cherry-pick?

Waiting for guidance before proceeding.
```
