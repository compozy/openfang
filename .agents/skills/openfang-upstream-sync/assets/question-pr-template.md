## Summary

This draft PR records the questions that must be answered before integrating
`upstream/main` into the fork.

## Upstream Analysis

- Current branch: `{{current_branch}}`
- Upstream ref: `{{upstream_ref}}`
- Merge base: `{{merge_base}}`
- Upstream head: `{{upstream_head}}`
- Upstream commit count: `{{upstream_commit_count}}`
- Changed file count: `{{changed_file_count}}`
- Risk level: `{{risk_level}}`
- Draft question PR required: `{{question_pr_required}}`

## Risk Reasons

{{risk_reasons}}

## Potential Overlap With Fork Changes

{{overlap_list}}

## Upstream Commits Under Review

{{commit_list}}

## Changed Files Under Review

{{file_list}}

## Open Questions

{{open_questions}}

## Resolution Marker

Add exactly one of these markers in the PR body or a PR comment before the sync
continues:

{{resolution_markers}}

## Exit Criteria

- Compatibility and migration questions are answered.
- One resolution marker is recorded.
- If the outcome is to proceed, the sync will continue on a fresh execution
  branch.
