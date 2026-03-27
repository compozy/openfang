#!/bin/bash

# analyze-upstream.sh
# Analyzes divergence between fork and upstream
# Usage: bash analyze-upstream.sh [upstream-branch]

set -e

UPSTREAM_BRANCH="${1:-upstream/main}"
LOCAL_BRANCH=$(git rev-parse --abbrev-ref HEAD)

echo "Upstream Divergence Analysis"
echo "======================================"
echo ""
echo "Local branch:    $LOCAL_BRANCH"
echo "Upstream branch: $UPSTREAM_BRANCH"
echo ""

# Check upstream remote exists
if ! git remote | grep -q "^upstream$"; then
    echo "ERROR: 'upstream' remote not found" >&2
    echo "Add it with: git remote add upstream <url>" >&2
    exit 1
fi

# Ensure upstream is fetched
UPSTREAM_HEAD=$(git rev-parse "$UPSTREAM_BRANCH" 2>/dev/null)
if [ -z "$UPSTREAM_HEAD" ]; then
    echo "ERROR: Cannot resolve $UPSTREAM_BRANCH" >&2
    echo "Run: git fetch upstream" >&2
    exit 1
fi

# Find merge base
MERGE_BASE=$(git merge-base HEAD "$UPSTREAM_BRANCH" 2>/dev/null || echo "")
if [ -z "$MERGE_BASE" ]; then
    echo "ERROR: No common ancestor found between HEAD and $UPSTREAM_BRANCH" >&2
    exit 1
fi

echo "Merge base: $(echo "$MERGE_BASE" | head -c 12)"
echo ""

# --- Commit counts ---
UPSTREAM_AHEAD=$(git rev-list --count "$MERGE_BASE".."$UPSTREAM_BRANCH")
FORK_AHEAD=$(git rev-list --count "$MERGE_BASE"..HEAD)

echo "=== Commit Divergence ==="
echo "  Upstream ahead by: $UPSTREAM_AHEAD commits"
echo "  Fork ahead by:     $FORK_AHEAD commits"
echo ""

if [ "$UPSTREAM_AHEAD" -eq 0 ]; then
    echo "Fork is already up-to-date with upstream."
    exit 0
fi

# --- Files changed upstream ---
echo "=== Upstream Changes (since last sync) ==="
UPSTREAM_FILES=$(git diff --name-only "$MERGE_BASE".."$UPSTREAM_BRANCH")
UPSTREAM_FILE_COUNT=$(echo "$UPSTREAM_FILES" | grep -c . || echo 0)
echo "  Files changed: $UPSTREAM_FILE_COUNT"
echo ""

# Group by top-level directory
echo "  By directory:"
echo "$UPSTREAM_FILES" | sed 's|/.*||' | sort | uniq -c | sort -rn | head -20 | sed 's/^/    /'
echo ""

# Show crate-level breakdown if in a Rust workspace
if echo "$UPSTREAM_FILES" | grep -q "^crates/"; then
    echo "  By crate:"
    echo "$UPSTREAM_FILES" | grep "^crates/" | sed 's|crates/||;s|/.*||' | sort | uniq -c | sort -rn | sed 's/^/    /'
    echo ""
fi

# --- Files changed in fork ---
echo "=== Fork Changes (since last sync) ==="
FORK_FILES=$(git diff --name-only "$MERGE_BASE"..HEAD)
FORK_FILE_COUNT=$(echo "$FORK_FILES" | grep -c . || echo 0)
echo "  Files changed: $FORK_FILE_COUNT"
echo ""

# --- Overlapping files (conflict candidates) ---
echo "=== Overlap Analysis (conflict candidates) ==="
OVERLAP=$(comm -12 <(echo "$UPSTREAM_FILES" | sort) <(echo "$FORK_FILES" | sort))
OVERLAP_COUNT=$(echo "$OVERLAP" | grep -c . || echo 0)

if [ "$OVERLAP_COUNT" -gt 0 ]; then
    echo "  Overlapping files: $OVERLAP_COUNT"
    echo ""
    echo "  Files modified by BOTH fork and upstream:"
    echo "$OVERLAP" | sed 's/^/    /'
else
    echo "  No overlapping file changes detected."
fi
echo ""

# --- New files upstream ---
echo "=== New Files Added Upstream ==="
NEW_UPSTREAM=$(git diff --name-only --diff-filter=A "$MERGE_BASE".."$UPSTREAM_BRANCH")
NEW_COUNT=$(echo "$NEW_UPSTREAM" | grep -c . || echo 0)
if [ "$NEW_COUNT" -gt 0 ]; then
    echo "  New files: $NEW_COUNT"
    echo "$NEW_UPSTREAM" | head -30 | sed 's/^/    /'
    if [ "$NEW_COUNT" -gt 30 ]; then
        echo "    ... and $((NEW_COUNT - 30)) more"
    fi
else
    echo "  No new files."
fi
echo ""

# --- Deleted files upstream ---
echo "=== Files Deleted Upstream ==="
DELETED_UPSTREAM=$(git diff --name-only --diff-filter=D "$MERGE_BASE".."$UPSTREAM_BRANCH")
DELETED_COUNT=$(echo "$DELETED_UPSTREAM" | grep -c . || echo 0)
if [ "$DELETED_COUNT" -gt 0 ]; then
    echo "  Deleted files: $DELETED_COUNT"
    echo "$DELETED_UPSTREAM" | sed 's/^/    /'

    # Check if fork still references any deleted files
    echo ""
    echo "  Fork references to deleted files:"
    for f in $DELETED_UPSTREAM; do
        BASENAME=$(basename "$f" | sed 's/\.[^.]*$//')
        REFS=$(git grep -l "$BASENAME" -- '*.rs' '*.toml' 2>/dev/null | head -5 || true)
        if [ -n "$REFS" ]; then
            echo "    $f -> referenced in:"
            echo "$REFS" | sed 's/^/      /'
        fi
    done
else
    echo "  No deleted files."
fi
echo ""

# --- Cargo.toml changes ---
echo "=== Cargo.toml Changes ==="
CARGO_CHANGES=$(echo "$UPSTREAM_FILES" | grep "Cargo.toml" || true)
if [ -n "$CARGO_CHANGES" ]; then
    echo "  Upstream modified:"
    echo "$CARGO_CHANGES" | sed 's/^/    /'
    CARGO_OVERLAP=$(echo "$OVERLAP" | grep "Cargo.toml" || true)
    if [ -n "$CARGO_OVERLAP" ]; then
        echo ""
        echo "  CONFLICT RISK: Fork also modified:"
        echo "$CARGO_OVERLAP" | sed 's/^/    /'
    fi
else
    echo "  No Cargo.toml changes upstream."
fi
echo ""

# --- Migration file changes ---
echo "=== Migration Changes ==="
MIGRATION_CHANGES=$(echo "$UPSTREAM_FILES" | grep -i "migration" || true)
if [ -n "$MIGRATION_CHANGES" ]; then
    echo "  Upstream migration files:"
    echo "$MIGRATION_CHANGES" | sed 's/^/    /'
else
    echo "  No migration changes upstream."
fi
echo ""

# --- Summary ---
echo "======================================"
echo "SUMMARY"
echo "  Upstream commits:    $UPSTREAM_AHEAD"
echo "  Fork commits:        $FORK_AHEAD"
echo "  Upstream files:      $UPSTREAM_FILE_COUNT"
echo "  Fork files:          $FORK_FILE_COUNT"
echo "  Overlapping files:   $OVERLAP_COUNT"
echo "  New upstream files:  $NEW_COUNT"
echo "  Deleted upstream:    $DELETED_COUNT"
echo ""
echo "Next step: bash scripts/risk-assessment.sh"
