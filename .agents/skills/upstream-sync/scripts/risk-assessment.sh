#!/bin/bash

# risk-assessment.sh
# Classifies the risk level of syncing fork with upstream
# Usage: bash risk-assessment.sh [upstream-branch]
# Exit codes: 0=LOW, 1=ERROR, 2=MEDIUM, 3=HIGH

set -e

UPSTREAM_BRANCH="${1:-upstream/main}"
RISK_LEVEL="LOW"
RISK_REASONS=()

echo "Upstream Sync Risk Assessment"
echo "======================================"
echo ""

# Check upstream remote
if ! git remote | grep -q "^upstream$"; then
    echo "ERROR: 'upstream' remote not found" >&2
    exit 1
fi

MERGE_BASE=$(git merge-base HEAD "$UPSTREAM_BRANCH" 2>/dev/null || echo "")
if [ -z "$MERGE_BASE" ]; then
    echo "ERROR: No common ancestor found" >&2
    exit 1
fi

# Gather data
UPSTREAM_AHEAD=$(git rev-list --count "$MERGE_BASE".."$UPSTREAM_BRANCH")
UPSTREAM_FILES=$(git diff --name-only "$MERGE_BASE".."$UPSTREAM_BRANCH")
FORK_FILES=$(git diff --name-only "$MERGE_BASE"..HEAD)
OVERLAP=$(comm -12 <(echo "$UPSTREAM_FILES" | sort) <(echo "$FORK_FILES" | sort))
OVERLAP_COUNT=$(echo "$OVERLAP" | grep -c . || echo 0)

# --- Rule 1: Commit volume ---
echo "Checking commit volume..."
if [ "$UPSTREAM_AHEAD" -gt 100 ]; then
    RISK_LEVEL="HIGH"
    RISK_REASONS+=("VOLUME: $UPSTREAM_AHEAD upstream commits (threshold: 100)")
elif [ "$UPSTREAM_AHEAD" -gt 30 ]; then
    if [ "$RISK_LEVEL" != "HIGH" ]; then RISK_LEVEL="MEDIUM"; fi
    RISK_REASONS+=("VOLUME: $UPSTREAM_AHEAD upstream commits (threshold: 30)")
fi
echo "  Upstream commits: $UPSTREAM_AHEAD"
echo ""

# --- Rule 2: Overlapping file count ---
echo "Checking file overlap..."
if [ "$OVERLAP_COUNT" -gt 20 ]; then
    RISK_LEVEL="HIGH"
    RISK_REASONS+=("OVERLAP: $OVERLAP_COUNT files modified by both fork and upstream (threshold: 20)")
elif [ "$OVERLAP_COUNT" -gt 5 ]; then
    if [ "$RISK_LEVEL" != "HIGH" ]; then RISK_LEVEL="MEDIUM"; fi
    RISK_REASONS+=("OVERLAP: $OVERLAP_COUNT files modified by both fork and upstream (threshold: 5)")
fi
echo "  Overlapping files: $OVERLAP_COUNT"
echo ""

# --- Rule 3: Critical path overlap ---
echo "Checking critical path overlap..."
CRITICAL_PATTERNS=(
    "src/lib.rs"
    "src/main.rs"
    "kernel"
    "openfang-types"
    "openfang-runtime"
    "openfang-server"
)

CRITICAL_HITS=()
for pattern in "${CRITICAL_PATTERNS[@]}"; do
    HITS=$(echo "$OVERLAP" | grep "$pattern" || true)
    if [ -n "$HITS" ]; then
        CRITICAL_HITS+=("$HITS")
    fi
done

CRITICAL_COUNT=${#CRITICAL_HITS[@]}
if [ "$CRITICAL_COUNT" -gt 0 ]; then
    if [ "$CRITICAL_COUNT" -gt 3 ]; then
        RISK_LEVEL="HIGH"
    elif [ "$RISK_LEVEL" != "HIGH" ]; then
        RISK_LEVEL="MEDIUM"
    fi
    RISK_REASONS+=("CRITICAL PATH: $CRITICAL_COUNT overlapping changes in core crates/files")
    echo "  Critical overlaps found:"
    for hit in "${CRITICAL_HITS[@]}"; do
        echo "    $hit"
    done
fi
echo ""

# --- Rule 4: Cargo.toml conflicts ---
echo "Checking Cargo.toml conflicts..."
CARGO_OVERLAP=$(echo "$OVERLAP" | grep "Cargo.toml" || true)
if [ -n "$CARGO_OVERLAP" ]; then
    CARGO_COUNT=$(echo "$CARGO_OVERLAP" | grep -c . || echo 0)
    if [ "$CARGO_COUNT" -gt 2 ]; then
        RISK_LEVEL="HIGH"
        RISK_REASONS+=("CARGO: $CARGO_COUNT Cargo.toml files modified by both sides")
    elif [ "$RISK_LEVEL" != "HIGH" ]; then
        RISK_LEVEL="MEDIUM"
        RISK_REASONS+=("CARGO: $CARGO_COUNT Cargo.toml files modified by both sides")
    fi
    echo "  Conflicting Cargo.toml files:"
    echo "$CARGO_OVERLAP" | sed 's/^/    /'
else
    echo "  No Cargo.toml conflicts."
fi
echo ""

# --- Rule 5: Migration conflicts ---
echo "Checking migration conflicts..."
UPSTREAM_MIGRATIONS=$(echo "$UPSTREAM_FILES" | grep -i "migration" || true)
FORK_MIGRATIONS=$(echo "$FORK_FILES" | grep -i "migration" || true)
if [ -n "$UPSTREAM_MIGRATIONS" ] && [ -n "$FORK_MIGRATIONS" ]; then
    RISK_LEVEL="HIGH"
    RISK_REASONS+=("MIGRATIONS: Both fork and upstream have migration changes")
    echo "  Upstream migrations:"
    echo "$UPSTREAM_MIGRATIONS" | sed 's/^/    /'
    echo "  Fork migrations:"
    echo "$FORK_MIGRATIONS" | sed 's/^/    /'
elif [ -n "$UPSTREAM_MIGRATIONS" ]; then
    echo "  Upstream has new migrations (no fork conflict)."
else
    echo "  No migration conflicts."
fi
echo ""

# --- Rule 6: Trait/API signature changes ---
echo "Checking trait/API signature changes..."
TRAIT_CHANGES=0
for f in $OVERLAP; do
    if echo "$f" | grep -q "\.rs$"; then
        UPSTREAM_DIFF=$(git diff "$MERGE_BASE".."$UPSTREAM_BRANCH" -- "$f" 2>/dev/null || true)
        if echo "$UPSTREAM_DIFF" | grep -qE "^[\+\-].*(pub fn |pub async fn |fn |trait |impl )"; then
            TRAIT_CHANGES=$((TRAIT_CHANGES + 1))
            if [ "$TRAIT_CHANGES" -le 5 ]; then
                echo "  Signature change detected in: $f"
            fi
        fi
    fi
done
if [ "$TRAIT_CHANGES" -gt 0 ]; then
    if [ "$TRAIT_CHANGES" -gt 3 ]; then
        RISK_LEVEL="HIGH"
    elif [ "$RISK_LEVEL" != "HIGH" ]; then
        RISK_LEVEL="MEDIUM"
    fi
    RISK_REASONS+=("SIGNATURES: $TRAIT_CHANGES files with public API/trait signature changes in overlapping code")
fi
echo ""

# --- Rule 7: Deleted files still referenced ---
echo "Checking for deleted file references..."
DELETED_UPSTREAM=$(git diff --name-only --diff-filter=D "$MERGE_BASE".."$UPSTREAM_BRANCH")
DANGLING=0
for f in $DELETED_UPSTREAM; do
    BASENAME=$(basename "$f" | sed 's/\.[^.]*$//')
    if git grep -q "$BASENAME" -- '*.rs' '*.toml' 2>/dev/null; then
        DANGLING=$((DANGLING + 1))
        echo "  Dangling reference: $f (still used in fork)"
    fi
done
if [ "$DANGLING" -gt 0 ]; then
    if [ "$RISK_LEVEL" != "HIGH" ]; then RISK_LEVEL="MEDIUM"; fi
    RISK_REASONS+=("DANGLING: $DANGLING upstream-deleted files still referenced by fork")
fi
echo ""

# --- Final Assessment ---
echo "======================================"
echo ""
echo "RISK LEVEL: $RISK_LEVEL"
echo ""

if [ ${#RISK_REASONS[@]} -gt 0 ]; then
    echo "Risk factors:"
    for reason in "${RISK_REASONS[@]}"; do
        echo "  - $reason"
    done
    echo ""
fi

case "$RISK_LEVEL" in
    LOW)
        echo "RECOMMENDATION: Proceed with sync. No clarification needed."
        echo "Suggested strategy: merge commit (git merge upstream/main)"
        exit 0
        ;;
    MEDIUM)
        echo "RECOMMENDATION: Ask 1-2 targeted clarification questions before proceeding."
        echo "Focus questions on the specific risk factors listed above."
        echo "Suggested strategy: merge commit with careful conflict review"
        exit 2
        ;;
    HIGH)
        echo "RECOMMENDATION: MANDATORY clarification before proceeding."
        echo "Present all risk factors to the user and ask for direction on each."
        echo "Suggested strategy: cherry-pick or incremental merge after user guidance"
        exit 3
        ;;
esac
