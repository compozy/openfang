#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: bash scripts/check-question-pr.sh <pr-number>" >&2
  exit 1
fi

PR_NUMBER="$1"
OUTPUT_DIR=".git/openfang-upstream-sync"
ENV_FILE="${OUTPUT_DIR}/question-pr.env"
mkdir -p "$OUTPUT_DIR"

command -v gh >/dev/null 2>&1 || {
  echo "ERROR: gh CLI is required." >&2
  exit 1
}

gh auth status >/dev/null 2>&1 || {
  echo "ERROR: gh auth status failed." >&2
  exit 1
}

JSON_OUTPUT="$(gh pr view "$PR_NUMBER" --json number,url,state,isDraft,body,comments,headRefName,baseRefName)"

python3 - "$JSON_OUTPUT" "$ENV_FILE" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
env_file = sys.argv[2]

markers = {
    "Resolution: do-not-sync": ("halt", "do-not-sync"),
    "Resolution: proceed-with-followups": ("ready", "proceed-with-followups"),
    "Resolution: proceed": ("ready", "proceed"),
}

search_space = [payload.get("body", "")]
search_space.extend(comment.get("body", "") for comment in payload.get("comments", []))

status = "blocked"
resolution = "missing"
for marker, values in markers.items():
    if any(marker in text for text in search_space):
        status, resolution = values
        break

if payload.get("state") != "OPEN":
    status = "halt"
    if resolution == "missing":
        resolution = "closed-without-resolution"

with open(env_file, "w", encoding="utf-8") as handle:
    handle.write(f"STATUS={status}\n")
    handle.write(f"QUESTION_PR_NUMBER={payload['number']}\n")
    handle.write(f"QUESTION_PR_URL={payload['url']}\n")
    handle.write(f"QUESTION_PR_STATE={payload['state']}\n")
    handle.write(f"QUESTION_PR_IS_DRAFT={str(payload['isDraft']).lower()}\n")
    handle.write(f"QUESTION_PR_RESOLUTION={resolution}\n")
    handle.write(f"QUESTION_PR_HEAD_REF={payload['headRefName']}\n")
    handle.write(f"QUESTION_PR_BASE_REF={payload['baseRefName']}\n")

print("SUCCESS: Question PR inspected.")
print(f"STATUS={status}")
print(f"QUESTION_PR_RESOLUTION={resolution}")
print(f"QUESTION_PR_URL={payload['url']}")
print(f"QUESTION_PR_HEAD_REF={payload['headRefName']}")
PY
