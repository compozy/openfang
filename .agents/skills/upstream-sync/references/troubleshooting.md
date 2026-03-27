# Troubleshooting Upstream Sync Issues

## Problem: No `upstream` Remote Configured

**Symptom:** `git fetch upstream` returns "fatal: 'upstream' does not appear to be a git repository"

**Solution:**

```bash
git remote add upstream git@github.com:RightNow-AI/openfang.git
git fetch upstream
```

## Problem: Merge Conflicts in Cargo.lock

**Symptom:** Massive conflict in `Cargo.lock` with hundreds of conflicting lines.

**Solution:** Never manually merge `Cargo.lock`. Regenerate it:

```bash
# Accept either version first
git checkout --theirs Cargo.lock

# Then regenerate from Cargo.toml
cargo generate-lockfile

# Stage the result
git add Cargo.lock
```

## Problem: Upstream Changed Rust Edition or MSRV

**Symptom:** Build fails with edition-related errors after sync.

**Solution:**
1. Check upstream's `Cargo.toml` for edition change
2. Check `rust-toolchain.toml` for MSRV change
3. Update fork's toolchain to match
4. Run `cargo check` to verify

## Problem: Upstream Renamed a Crate

**Symptom:** Build fails with unresolved imports after sync.

**Solution:**
1. Identify the rename from upstream's commit messages
2. Update all fork references to the new crate name:

```bash
# Find all references to old crate name
grep -rn "old_crate_name" crates/ Cargo.toml
```

3. Update `Cargo.toml` workspace members if needed

## Problem: Merge Stopped Mid-Way

**Symptom:** `git status` shows "You have unmerged paths."

**Solution:**
1. Check which files still need resolution: `git diff --name-only --diff-filter=U`
2. Resolve remaining files
3. Stage all: `git add .`
4. Continue: `git merge --continue`

If the merge is unsalvageable:

```bash
git merge --abort
# Return to pre-merge state, try a different approach
```

## Problem: Fork Tests Fail After Sync

**Symptom:** `make test` fails on tests that passed before sync.

**Solution:**
1. Identify which tests fail: `cargo test --all-features 2>&1 | grep "FAILED"`
2. Determine if failures are from:
   - Upstream API changes (update fork code to match new API)
   - Missing imports (update `use` statements)
   - Trait contract changes (add new required methods)
   - Removed functionality (port to replacement)
3. Fix each failure category systematically

## Problem: `make lint` Fails After Sync

**Symptom:** Clippy warnings or errors after merging upstream code.

**Solution:**
1. Run `make lint 2>&1 | head -100` to see first errors
2. Common causes:
   - Upstream code uses patterns our clippy config disallows (fix the code)
   - New unused imports from conflict resolution (remove them)
   - Missing `#[allow]` attributes that upstream had (check upstream's clippy config)
3. Run `make lint-fix` for auto-fixable issues first

## Problem: Backup Branch Pollutes Branch List

**Symptom:** Many `backup-sync-*` branches accumulate over time.

**Solution:**

```bash
# List all backup branches
git branch | grep "backup-sync"

# Delete old backups (keep last 3)
git branch | grep "backup-sync" | head -n -3 | xargs git branch -d
```

## Problem: Upstream Force-Pushed to Main

**Symptom:** `git merge upstream/main` creates unexpected conflicts or duplicate commits.

**Solution:** This is rare but serious. Steps:
1. Abort current merge: `git merge --abort`
2. Find the last known good upstream commit from previous sync
3. Use `git log upstream/main` to identify the rewrite point
4. Ask the user before proceeding — force-pushed upstream is a CRITICAL risk event
