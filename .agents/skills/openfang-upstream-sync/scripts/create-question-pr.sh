#!/usr/bin/env bash

set -euo pipefail

BASE_BRANCH=""
HEAD_BRANCH=""
TITLE=""
BODY_FILE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --base)
      BASE_BRANCH="$2"
      shift 2
      ;;
    --head)
      HEAD_BRANCH="$2"
      shift 2
      ;;
    --title)
      TITLE="$2"
      shift 2
      ;;
    --body-file)
      BODY_FILE="$2"
      shift 2
      ;;
    *)
      echo "ERROR: Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$BASE_BRANCH" || -z "$HEAD_BRANCH" || -z "$TITLE" || -z "$BODY_FILE" ]]; then
  echo "Usage: bash scripts/create-question-pr.sh --base <branch> --head <branch> --title <title> --body-file <path>" >&2
  exit 1
fi

command -v gh >/dev/null 2>&1 || {
  echo "ERROR: gh CLI is required." >&2
  exit 1
}

gh auth status >/dev/null 2>&1 || {
  echo "ERROR: gh auth status failed." >&2
  exit 1
}

CURRENT_BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [[ "$CURRENT_BRANCH" != "$HEAD_BRANCH" ]]; then
  echo "ERROR: Current branch '$CURRENT_BRANCH' does not match expected head '$HEAD_BRANCH'." >&2
  exit 1
fi

if [[ -n "$(git status --porcelain)" ]]; then
  echo "ERROR: Working tree must be clean before opening the question PR." >&2
  exit 1
fi

if [[ ! -f "$BODY_FILE" ]]; then
  echo "ERROR: Missing PR body file '$BODY_FILE'." >&2
  exit 1
fi

git push -u origin "$HEAD_BRANCH"
PR_URL="$(gh pr create --draft --base "$BASE_BRANCH" --head "$HEAD_BRANCH" --title "$TITLE" --body-file "$BODY_FILE")"

echo "SUCCESS: Draft question PR created."
echo "PR_URL=$PR_URL"
