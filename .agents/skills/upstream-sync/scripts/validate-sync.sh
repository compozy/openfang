#!/bin/bash

# validate-sync.sh
# Validates that an upstream sync is clean and ready for testing
# Usage: bash validate-sync.sh

set -e

echo "Upstream Sync Validation"
echo "======================================"
echo ""

EXIT_CODE=0

# 1. Check for conflict markers
echo "1. Checking for conflict markers..."
MARKER_FILES=$(git grep -rl '<<<<<<\|======\|>>>>>>' -- '*.rs' '*.toml' '*.md' '*.sql' 2>/dev/null || true)
if [ -z "$MARKER_FILES" ]; then
    echo "   PASS: No conflict markers found"
else
    echo "   FAIL: Conflict markers found in:" >&2
    echo "$MARKER_FILES" | sed 's/^/     /'
    EXIT_CODE=1
fi
echo ""

# 2. Check for unresolved git conflicts
echo "2. Checking git conflict status..."
UNRESOLVED=$(git diff --name-only --diff-filter=U 2>/dev/null || true)
if [ -z "$UNRESOLVED" ]; then
    echo "   PASS: No unresolved conflicts in git status"
else
    echo "   FAIL: Unresolved conflicts:" >&2
    echo "$UNRESOLVED" | sed 's/^/     /'
    EXIT_CODE=1
fi
echo ""

# 3. Check working directory state
echo "3. Checking working directory..."
UNSTAGED=$(git status --porcelain | grep '^ M\| ??' || true)
if [ -z "$UNSTAGED" ]; then
    echo "   PASS: Working directory clean"
else
    echo "   WARN: Unstaged or untracked files:"
    git status --short | head -20 | sed 's/^/     /'
fi
echo ""

# 4. Check Cargo.lock is consistent
echo "4. Checking Cargo.lock consistency..."
if cargo check --quiet 2>/dev/null; then
    echo "   PASS: Cargo.lock is consistent"
else
    echo "   FAIL: cargo check failed (dependency issues)" >&2
    EXIT_CODE=1
fi
echo ""

# 5. Check for duplicate dependencies
echo "5. Checking for duplicate crate entries..."
if [ -f "Cargo.lock" ]; then
    DUPES=$(grep '^name = ' Cargo.lock | sort | uniq -d || true)
    if [ -z "$DUPES" ]; then
        echo "   PASS: No duplicate crate names"
    else
        echo "   WARN: Duplicate crate versions (may be intentional):"
        echo "$DUPES" | head -10 | sed 's/^/     /'
    fi
else
    echo "   SKIP: No Cargo.lock found"
fi
echo ""

# 6. Format check
echo "6. Checking code formatting..."
if make fmt-check 2>/dev/null; then
    echo "   PASS: Code is formatted"
else
    echo "   FAIL: Code formatting issues detected" >&2
    echo "   Run: make fmt" >&2
    EXIT_CODE=1
fi
echo ""

# 7. Lint check
echo "7. Running clippy..."
if make lint-clippy 2>/dev/null; then
    echo "   PASS: No clippy warnings"
else
    echo "   FAIL: Clippy warnings detected" >&2
    EXIT_CODE=1
fi
echo ""

# 8. Test suite
echo "8. Running tests..."
if make test 2>/dev/null; then
    echo "   PASS: All tests pass"
else
    echo "   FAIL: Test failures detected" >&2
    EXIT_CODE=1
fi
echo ""

# --- Final ---
echo "======================================"
if [ $EXIT_CODE -eq 0 ]; then
    echo "VALIDATION PASSED"
    echo ""
    echo "Next steps:"
    echo "  git push origin $(git rev-parse --abbrev-ref HEAD)"
else
    echo "VALIDATION FAILED" >&2
    echo "" >&2
    echo "Fix all failures above, then re-run: bash scripts/validate-sync.sh" >&2
fi

exit $EXIT_CODE
