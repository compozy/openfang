#!/usr/bin/env bash

set -euo pipefail

OUTPUT_DIR=".git/openfang-upstream-sync"
ENV_FILE="${OUTPUT_DIR}/analysis.env"
REPORT_FILE="${OUTPUT_DIR}/analysis.md"
COMMITS_FILE="${OUTPUT_DIR}/commits.txt"
FILES_FILE="${OUTPUT_DIR}/files.txt"
REASONS_FILE="${OUTPUT_DIR}/reasons.txt"
OVERLAP_FILE="${OUTPUT_DIR}/overlap.txt"

ensure_repo() {
  git rev-parse --is-inside-work-tree >/dev/null 2>&1 || {
    echo "ERROR: Run analyze-upstream-sync.sh inside a git repository." >&2
    exit 1
  }
}

ensure_upstream() {
  git remote get-url upstream >/dev/null 2>&1 || {
    echo "ERROR: Missing git remote 'upstream'." >&2
    exit 1
  }
}

fetch_upstream() {
  git fetch upstream main --quiet 2>/dev/null || {
    echo "ERROR: Failed to fetch upstream/main. Check remote access and authentication for 'upstream'." >&2
    exit 1
  }
}

bump_risk() {
  local candidate="$1"
  if [[ "$candidate" == "high" ]]; then
    RISK_LEVEL="high"
    return
  fi
  if [[ "$candidate" == "medium" && "$RISK_LEVEL" == "low" ]]; then
    RISK_LEVEL="medium"
  fi
}

add_reason() {
  local reason="$1"
  for existing in "${REASONS[@]:-}"; do
    if [[ "$existing" == "$reason" ]]; then
      return
    fi
  done
  REASONS+=("$reason")
}

classify_file() {
  local file="$1"
  case "$file" in
    Cargo.toml|Cargo.lock|Makefile|xtask/*|\
    crates/openfang-api/*|crates/openfang-cli/*|crates/openfang-kernel/*|\
    crates/openfang-memory/*|crates/openfang-runtime/*|crates/openfang-types/*|\
    crates/arky-config/*|crates/arky-provider/*|crates/arky-protocol/*|\
    crates/arky-session/*|crates/openfang-migrate/*)
      bump_risk "high"
      add_reason "Sensitive runtime, API, configuration, or schema code changed."
      ;;
    */migration*|*/migrations/*|*.sql|docs/api-reference.md|docs/cli-reference.md)
      bump_risk "high"
      add_reason "Migration, schema, or public contract files changed."
      ;;
    crates/openfang-channels/*|crates/openfang-wire/*|crates/openfang-skills/*|\
    crates/openfang-extensions/*|crates/openfang-hands/*|.github/*|docs/*)
      bump_risk "medium"
      add_reason "Shared integration, documentation, or platform surfaces changed."
      ;;
    *)
      ;;
  esac
}

ensure_repo
ensure_upstream
mkdir -p "$OUTPUT_DIR"

fetch_upstream

CURRENT_BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [[ "$CURRENT_BRANCH" == "HEAD" ]]; then
  echo "ERROR: Detached HEAD is not supported for upstream sync." >&2
  exit 1
fi

MERGE_BASE="$(git merge-base HEAD upstream/main)"
UPSTREAM_HEAD="$(git rev-parse upstream/main)"

mapfile -t COMMITS < <(git log --reverse --format="%H %s" "${MERGE_BASE}..upstream/main")
mapfile -t CHANGED_FILES < <(git diff --name-only "${MERGE_BASE}..upstream/main")
mapfile -t LOCAL_FILES < <(git diff --name-only "${MERGE_BASE}..HEAD")

printf "%s\n" "${COMMITS[@]}" > "$COMMITS_FILE"
printf "%s\n" "${CHANGED_FILES[@]}" > "$FILES_FILE"

if [[ "${#CHANGED_FILES[@]}" -eq 0 ]]; then
  : > "$REASONS_FILE"
  : > "$OVERLAP_FILE"
  cat > "$ENV_FILE" <<EOF
STATUS=no_updates
CURRENT_BRANCH=${CURRENT_BRANCH}
UPSTREAM_REF=upstream/main
MERGE_BASE=${MERGE_BASE}
UPSTREAM_HEAD=${UPSTREAM_HEAD}
UPSTREAM_COMMIT_COUNT=0
CHANGED_FILE_COUNT=0
RISK_LEVEL=none
QUESTION_PR_REQUIRED=false
ANALYSIS_REPORT=${REPORT_FILE}
COMMITS_FILE=${COMMITS_FILE}
FILES_FILE=${FILES_FILE}
REASONS_FILE=${REASONS_FILE}
OVERLAP_FILE=${OVERLAP_FILE}
EOF
  cat > "$REPORT_FILE" <<EOF
# Upstream Sync Analysis

- Status: no updates
- Current branch: \`${CURRENT_BRANCH}\`
- Upstream ref: \`upstream/main\`

No new commits exist on \`upstream/main\` beyond the current merge base.
EOF
  echo "SUCCESS: No upstream updates detected."
  echo "ANALYSIS_REPORT=$REPORT_FILE"
  exit 0
fi

RISK_LEVEL="low"
REASONS=()

for file in "${CHANGED_FILES[@]}"; do
  classify_file "$file"
done

if [[ "${#COMMITS[@]}" -ge 20 ]]; then
  bump_risk "medium"
  add_reason "Upstream commit volume is high and requires review."
fi

if [[ "${#CHANGED_FILES[@]}" -ge 40 ]]; then
  bump_risk "medium"
  add_reason "Upstream file volume is high and requires review."
fi

OVERLAP=()
if [[ "${#LOCAL_FILES[@]}" -gt 0 && "${#CHANGED_FILES[@]}" -gt 0 ]]; then
  while IFS= read -r overlap_file; do
    [[ -n "$overlap_file" ]] || continue
    OVERLAP+=("$overlap_file")
  done < <(
    comm -12 \
      <(printf "%s\n" "${LOCAL_FILES[@]}" | sort -u) \
      <(printf "%s\n" "${CHANGED_FILES[@]}" | sort -u)
  )
fi

printf "%s\n" "${OVERLAP[@]}" > "$OVERLAP_FILE"

if [[ "${#OVERLAP[@]}" -gt 0 ]]; then
  bump_risk "medium"
  add_reason "Upstream changes overlap files already modified in the fork branch."
fi

if grep -Eq '^(Cargo\.toml|Cargo\.lock|Makefile|xtask/|crates/openfang-api/|crates/openfang-cli/|crates/openfang-kernel/|crates/openfang-memory/|crates/openfang-runtime/|crates/openfang-types/|crates/arky-config/)' "$OVERLAP_FILE"; then
  bump_risk "high"
  add_reason "Upstream changes overlap sensitive fork-owned infrastructure files."
fi

printf "%s\n" "${REASONS[@]}" > "$REASONS_FILE"

QUESTION_PR_REQUIRED="false"
if [[ "$RISK_LEVEL" != "low" ]]; then
  QUESTION_PR_REQUIRED="true"
fi

cat > "$ENV_FILE" <<EOF
STATUS=updates
CURRENT_BRANCH=${CURRENT_BRANCH}
UPSTREAM_REF=upstream/main
MERGE_BASE=${MERGE_BASE}
UPSTREAM_HEAD=${UPSTREAM_HEAD}
UPSTREAM_COMMIT_COUNT=${#COMMITS[@]}
CHANGED_FILE_COUNT=${#CHANGED_FILES[@]}
RISK_LEVEL=${RISK_LEVEL}
QUESTION_PR_REQUIRED=${QUESTION_PR_REQUIRED}
ANALYSIS_REPORT=${REPORT_FILE}
COMMITS_FILE=${COMMITS_FILE}
FILES_FILE=${FILES_FILE}
REASONS_FILE=${REASONS_FILE}
OVERLAP_FILE=${OVERLAP_FILE}
EOF

{
  echo "# Upstream Sync Analysis"
  echo
  echo "- Status: updates"
  echo "- Current branch: \`${CURRENT_BRANCH}\`"
  echo "- Upstream ref: \`upstream/main\`"
  echo "- Merge base: \`${MERGE_BASE}\`"
  echo "- Upstream head: \`${UPSTREAM_HEAD}\`"
  echo "- Upstream commit count: ${#COMMITS[@]}"
  echo "- Changed file count: ${#CHANGED_FILES[@]}"
  echo "- Risk level: \`${RISK_LEVEL}\`"
  echo "- Question PR required: \`${QUESTION_PR_REQUIRED}\`"
  echo
  echo "## Risk Reasons"
  if [[ "${#REASONS[@]}" -eq 0 ]]; then
    echo "- No explicit risk reasons matched."
  else
    for reason in "${REASONS[@]}"; do
      echo "- ${reason}"
    done
  fi
  echo
  echo "## Upstream Commits"
  for commit in "${COMMITS[@]}"; do
    echo "- \`${commit%% *}\` ${commit#* }"
  done
  echo
  echo "## Changed Files"
  for file in "${CHANGED_FILES[@]}"; do
    echo "- \`${file}\`"
  done
  echo
  echo "## Overlap With Fork Changes"
  if [[ "${#OVERLAP[@]}" -eq 0 ]]; then
    echo "- None"
  else
    for file in "${OVERLAP[@]}"; do
      echo "- \`${file}\`"
    done
  fi
} > "$REPORT_FILE"

echo "SUCCESS: Upstream analysis completed."
echo "RISK_LEVEL=$RISK_LEVEL"
echo "QUESTION_PR_REQUIRED=$QUESTION_PR_REQUIRED"
echo "ANALYSIS_REPORT=$REPORT_FILE"
