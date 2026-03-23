# Upstream Sync Troubleshooting

## Missing `upstream` Remote

Symptoms:

- `git remote get-url upstream` fails
- analyzer exits before fetching

Recovery:

```bash
git remote add upstream https://github.com/RightNow-AI/openfang.git
git fetch upstream main
```

## Dirty Working Tree

Symptoms:

- branch-creation or PR scripts refuse to continue

Recovery:

- finish or isolate unrelated work first
- rerun the skill from a clean branch

Do not force the workflow forward with a dirty tree.

## Draft Question PR Cannot Resume

Symptoms:

- `check-question-pr.sh` returns `STATUS=blocked`

Recovery:

- add one exact resolution marker to the PR body or comments:
  - `Resolution: proceed`
  - `Resolution: proceed-with-followups`
  - `Resolution: do-not-sync`
- rerun the skill with `QUESTION_PR_NUMBER=<number>`

## Final PR Creation Fails With "No Commits Between"

Symptoms:

- `gh pr create` refuses to open the PR

Recovery:

- confirm the sync branch contains the merge commit
- confirm the sync branch was created from the expected base branch
- confirm the branch was pushed to `origin`

## Merge Conflicts Touch Fork-Specific Areas

Symptoms:

- conflicts appear in `openfang-*`, `arky-*`, config, runtime, or schema files

Recovery:

- inspect `.git/openfang-upstream-sync/overlap.txt`
- preserve Compozy-owned behavior intentionally
- document the local adaptation in `.git/openfang-upstream-sync/local-adaptations.txt`
- document remaining concerns in `.git/openfang-upstream-sync/residual-risks.txt`

## Validation Fails After Integration

Symptoms:

- `make fmt`, `make lint`, or `make test` fails after the merge

Recovery:

- fix the production or integration issue
- rerun the failed command
- rerun the full validation sequence before opening the final PR
