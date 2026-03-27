#!/bin/bash

# pre-sync-backup.sh
# Creates a safe backup before starting an upstream sync
# Usage: bash pre-sync-backup.sh

set -e

echo "Pre-Sync Safety Backup"
echo "======================================"
echo ""

# Get current branch
CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)

if [ "$CURRENT_BRANCH" = "HEAD" ]; then
    echo "ERROR: Detached HEAD state" >&2
    echo "Checkout a branch first: git checkout main" >&2
    exit 1
fi

# Check if clean
if [ -n "$(git status --porcelain)" ]; then
    echo "WARNING: Uncommitted changes detected" >&2
    echo "Commit or stash changes before syncing:" >&2
    echo "  git stash" >&2
    echo "  or" >&2
    echo "  git add . && git commit -m 'wip: save before upstream sync'" >&2
    exit 1
fi

echo "Current branch: $CURRENT_BRANCH"
echo ""

# Create timestamped backup
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_BRANCH="backup-sync-${CURRENT_BRANCH}-${TIMESTAMP}"

echo "Creating backup branch..."
git branch "$BACKUP_BRANCH"

# Get current commit
CURRENT_SHA=$(git rev-parse HEAD)
UPSTREAM_SHA=$(git rev-parse upstream/main 2>/dev/null || echo "unknown")

echo ""
echo "Backup created successfully."
echo ""
echo "Backup Details:"
echo "   Branch:       $BACKUP_BRANCH"
echo "   Commit SHA:   $CURRENT_SHA"
echo "   Upstream SHA: $UPSTREAM_SHA"
echo ""

# Save backup info
BACKUP_INFO_FILE=".sync-backup-info"
cat > "$BACKUP_INFO_FILE" << EOF
# Upstream Sync Backup Information
# Created: $(date)

BACKUP_BRANCH=$BACKUP_BRANCH
CURRENT_BRANCH=$CURRENT_BRANCH
COMMIT_SHA=$CURRENT_SHA
UPSTREAM_SHA=$UPSTREAM_SHA
TIMESTAMP=$TIMESTAMP

# Recovery:
# git reset --hard $BACKUP_BRANCH
EOF

echo "Backup info saved to: $BACKUP_INFO_FILE"
echo ""

echo "Recovery Instructions:"
echo ""
echo "  Option 1: Reset to backup branch"
echo "    git reset --hard $BACKUP_BRANCH"
echo ""
echo "  Option 2: Use git reflog"
echo "    git reflog"
echo "    git reset --hard <SHA>"
echo ""

# List recent backup branches
BACKUP_COUNT=$(git branch | grep -c "backup-sync" || echo 0)
if [ "$BACKUP_COUNT" -gt 0 ]; then
    echo "All Sync Backup Branches:"
    git branch | grep "backup-sync" | tail -5 | sed 's/^/  /'
    echo ""
fi

echo "Ready to sync. Next steps:"
echo "  1. git fetch upstream --tags"
echo "  2. bash scripts/analyze-upstream.sh"
echo "  3. bash scripts/risk-assessment.sh"
