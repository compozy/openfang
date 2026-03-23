---
name: openfang-upstream-sync
description: Safely synchronizes a forked OpenFang repository with upstream/main by analyzing upstream risk, opening draft question pull requests when compatibility or migration concerns appear, integrating updates on a fresh branch, validating the repository, and creating the final GitHub pull request with GitHub CLI. Use when maintaining an OpenFang-derived fork against upstream/main with controlled review. Do not use for generic rebases, ad hoc cherry-picks, or sync flows that intentionally skip review.
---

# OpenFang Upstream Sync

Maintain an OpenFang-derived fork against `upstream/main` with a conservative,
PR-first workflow.

## Required Inputs

- `gh` authenticated for the target repository.
- `upstream` git remote pointing at the original OpenFang repository.
- A clean working tree before any branch creation.

Optional resume inputs:

- `QUESTION_PR_NUMBER=<number>` to resume a question-first sync after maintainers
  answer the draft PR on the same sync branch.

Run commands from the repository root.

## Workflow Checklist

Copy this checklist and mark progress:

```text
OpenFang Upstream Sync:
- [ ] Step 1: Verify repository safety and prerequisites
- [ ] Step 2: Analyze upstream/main
- [ ] Step 3: Classify risk and choose the path
- [ ] Step 4: Create the sync branch
- [ ] Step 5: Open a draft question PR when risk requires review
- [ ] Step 6: Integrate upstream/main conservatively
- [ ] Step 7: Validate with make fmt, make lint, and make test
- [ ] Step 8: Create or finalize the PR
```

## Step 1: Verify Repository Safety and Prerequisites

Run these checks first:

```bash
git rev-parse --is-inside-work-tree
git remote get-url upstream
gh auth status
git status --short
```

Require a clean working tree before branch creation:

```bash
if [ -n "$(git status --porcelain)" ]; then
  echo "Working tree must be clean before starting upstream sync."
  exit 1
fi
```

Capture the current branch as the base branch for both PR types:

```bash
BASE_BRANCH=$(git rev-parse --abbrev-ref HEAD)
```

If `BASE_BRANCH` is `HEAD`, stop and switch to a named branch first.

## Step 2: Analyze `upstream/main`

Run the bundled analyzer:

```bash
bash scripts/analyze-upstream-sync.sh
source .git/openfang-upstream-sync/analysis.env
```

If `STATUS=no_updates`, stop and report that no new upstream commits exist.

Read `references/risk-rules.md` when the reasons need interpretation.

Review the generated report:

```bash
sed -n '1,220p' "$ANALYSIS_REPORT"
```

## Step 3: Classify Risk and Choose the Path

Use the analyzer output:

- If `QUESTION_PR_REQUIRED=false`, continue directly to Step 4.
- If `QUESTION_PR_REQUIRED=true` and `QUESTION_PR_NUMBER` is unset, continue to
  Step 4 and then Step 5.
- If `QUESTION_PR_REQUIRED=true` and `QUESTION_PR_NUMBER` is set, verify that the
  prior draft PR is resolved before moving to Step 6.

For a resume run, verify the prior question PR:

```bash
bash scripts/check-question-pr.sh "$QUESTION_PR_NUMBER"
source .git/openfang-upstream-sync/question-pr.env
```

Decision rules:

- If `STATUS=ready`, switch back to the existing sync branch and continue to
  Step 6.
- If `STATUS=blocked`, stop and report the draft PR URL plus the missing
  resolution marker.
- If `STATUS=halt`, stop and do not integrate `upstream/main`.

## Step 4: Create the Sync Branch

Create a fresh sync branch from the base branch unless resuming an existing
draft PR:

```bash
TIMESTAMP=$(date +%Y%m%d-%H%M%S)
SYNC_BRANCH="upstream-sync/main-${TIMESTAMP}"
git switch -c "$SYNC_BRANCH"
```

If `QUESTION_PR_NUMBER` is set, reuse the existing PR head branch instead:

```bash
git fetch origin "$QUESTION_PR_HEAD_REF"
git switch "$QUESTION_PR_HEAD_REF"
SYNC_BRANCH="$QUESTION_PR_HEAD_REF"
```

## Step 5: Open a Draft Question PR

Run this step only when `QUESTION_PR_REQUIRED=true` and `QUESTION_PR_NUMBER` is
unset.

Create a committed planning artifact so GitHub has a diff to review on the same
sync branch:

```bash
QUESTION_NOTE="docs/plans/upstream-sync/${TIMESTAMP}-upstream-main-questions.md"
mkdir -p "$(dirname "$QUESTION_NOTE")"
python3 scripts/render-pr-body.py \
  --mode question-note \
  --analysis-env .git/openfang-upstream-sync/analysis.env \
  --output "$QUESTION_NOTE"

git add "$QUESTION_NOTE"
git commit -m "docs(upstream-sync): record upstream sync questions"
```

Render the draft PR body:

```bash
QUESTION_PR_BODY=".git/openfang-upstream-sync/question-pr.md"
python3 scripts/render-pr-body.py \
  --mode question-pr \
  --analysis-env .git/openfang-upstream-sync/analysis.env \
  --output "$QUESTION_PR_BODY"
```

Open the draft PR:

```bash
QUESTION_TITLE="draft: assess upstream/main sync ${TIMESTAMP}"
bash scripts/create-question-pr.sh \
  --base "$BASE_BRANCH" \
  --head "$SYNC_BRANCH" \
  --title "$QUESTION_TITLE" \
  --body-file "$QUESTION_PR_BODY"
```

Stop after opening the draft PR. Report:

- sync branch
- PR URL
- risk level
- reasons
- required resolution marker

The draft PR template instructs maintainers to add one of these exact markers in
the PR body or comments:

- `Resolution: proceed`
- `Resolution: proceed-with-followups`
- `Resolution: do-not-sync`

To continue later, rerun the skill with `QUESTION_PR_NUMBER=<number>`.

## Step 6: Integrate `upstream/main` Conservatively

Fetch once more before merging:

```bash
git fetch upstream main
```

Use a no-fast-forward merge so the sync stays auditable:

```bash
git merge --no-ff --no-commit upstream/main
```

If conflicts appear:

- Read the analyzer overlap list in `.git/openfang-upstream-sync/overlap.txt`.
- Preserve fork-specific behavior intentionally.
- Prefer explicit manual conflict resolution over aggressive automation.
- Stage resolved files and finish the merge:

```bash
git add <resolved-files>
git commit -m "merge: sync upstream/main into ${SYNC_BRANCH}"
```

If no conflicts appear, create the merge commit with a descriptive message:

```bash
git commit -m "merge: sync upstream/main into ${SYNC_BRANCH}"
```

## Step 7: Validate the Integrated Branch

Run the mandatory repository checks:

```bash
make fmt
make lint
make test
```

If any command fails:

- Fix the production issue.
- Re-run the failing command.
- Re-run the full sequence until all three pass.

Capture a concise validation summary for the final PR:

```bash
VALIDATION_SUMMARY=".git/openfang-upstream-sync/validation-summary.txt"
cat > "$VALIDATION_SUMMARY" <<'EOF'
make fmt: passed
make lint: passed
make test: passed
EOF
```

If conflicts required non-obvious local adjustments, record them in optional
artifacts:

```bash
LOCAL_ADAPTATIONS=".git/openfang-upstream-sync/local-adaptations.txt"
RESIDUAL_RISKS=".git/openfang-upstream-sync/residual-risks.txt"
CONFLICT_SUMMARY=".git/openfang-upstream-sync/conflict-summary.txt"
```

## Step 8: Create or Finalize the PR

Render the final PR body:

```bash
FINAL_PR_BODY=".git/openfang-upstream-sync/final-pr.md"
python3 scripts/render-pr-body.py \
  --mode final-pr \
  --analysis-env .git/openfang-upstream-sync/analysis.env \
  --output "$FINAL_PR_BODY" \
  --strategy "merge --no-ff --no-commit upstream/main" \
  --validation-file "$VALIDATION_SUMMARY" \
  --conflict-summary-file "$CONFLICT_SUMMARY" \
  --local-adaptations-file "$LOCAL_ADAPTATIONS" \
  --residual-risks-file "$RESIDUAL_RISKS"
```

Create or finalize the PR:

```bash
FINAL_TITLE="sync: merge upstream/main ${TIMESTAMP}"
bash scripts/create-sync-pr.sh \
  --base "$BASE_BRANCH" \
  --head "$SYNC_BRANCH" \
  --title "$FINAL_TITLE" \
  --body-file "$FINAL_PR_BODY" \
  ${QUESTION_PR_NUMBER:+--pr-number "$QUESTION_PR_NUMBER"}
```

Report the final output contract:

- sync branch
- PR URL
- integrated upstream commit count
- conflict summary
- validation summary
- residual risks

## Error Handling

- If `upstream` is missing, stop and add the remote before retrying.
- If `gh auth status` fails, authenticate GitHub CLI before retrying.
- If `scripts/analyze-upstream-sync.sh` reports `QUESTION_PR_REQUIRED=true`,
  do not bypass Step 5.
- If the draft PR lacks a resolution marker, stop and wait for maintainer input.
- If the resolution marker is `Resolution: do-not-sync`, stop and report that
  the sync was intentionally blocked.
- If `git merge` produces conflicts, resolve them manually and preserve the fork
  intentionally; do not use aggressive auto-resolution flags.
- If any validation step fails, fix the code and rerun the full validation
  sequence before opening the final PR.
- If `gh pr create` fails because no diff exists, confirm that the question-note
  planning artifact or sync merge commit was committed before retrying.

Read `references/troubleshooting.md` for common recovery paths.
