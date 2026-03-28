package app

import (
	"fmt"
	"strings"
)

type skillDrivenBatchPromptParams struct {
	PR            string
	CodeFiles     []string
	BatchIssues   []issueEntry
	Grouped       bool
	AutoCommit    bool
	MinIssue      int
	MaxIssue      int
	HasIssueRange bool
}

func buildCodeReviewPrompt(p buildBatchedIssuesParams) string {
	codeFiles := sortCodeFiles(p.BatchGroups)
	batchIssues := flattenAndSortIssues(p.BatchGroups, ExecutionModePRReview)
	minIssue, maxIssue, hasIssueRange := batchIssueRange(batchIssues)
	params := skillDrivenBatchPromptParams{
		PR:            p.PR,
		CodeFiles:     codeFiles,
		BatchIssues:   batchIssues,
		Grouped:       p.Grouped,
		AutoCommit:    p.AutoCommit,
		MinIssue:      minIssue,
		MaxIssue:      maxIssue,
		HasIssueRange: hasIssueRange,
	}

	sections := []string{
		buildBatchHeader(p.PR, codeFiles, p.BatchGroups),
		buildSkillDrivenHelperCommands(params),
		buildSkillDrivenCriticalSection(params),
		buildBatchIssueFilesSection(batchIssues),
		buildSkillDrivenExecutionSection(params),
		buildBatchChecklist(p.PR, p.BatchGroups, p.Grouped),
	}
	return strings.Join(sections, "\n\n")
}

func buildSkillDrivenHelperCommands(p skillDrivenBatchPromptParams) string {
	var sb strings.Builder
	sb.WriteString("## Helper Commands\n\n```bash\n")
	if p.HasIssueRange {
		sb.WriteString(fmt.Sprintf("# Review only this selected batch (%03d-%03d)\n", p.MinIssue, p.MaxIssue))
		sb.WriteString(fmt.Sprintf(
			"scripts/read_pr_issues.sh --pr %s --type issue --from %d --to %d\n\n",
			p.PR,
			p.MinIssue,
			p.MaxIssue,
		))
	} else {
		sb.WriteString("# Batch issue range could not be inferred; inspect files listed in <batch_issue_files>\n")
		sb.WriteString(fmt.Sprintf("scripts/read_pr_issues.sh --pr %s --type issue --all\n\n", p.PR))
	}

	if p.HasIssueRange {
		sb.WriteString("# After finishing this batch, resolve only this batch range\n")
		sb.WriteString(fmt.Sprintf(
			"bash .claude/skills/fix-coderabbit-review/scripts/resolve_pr_issues.sh --pr-dir ai-docs/reviews-pr-%s --from %d --to %d\n",
			p.PR,
			p.MinIssue,
			p.MaxIssue,
		))
	} else {
		sb.WriteString("# Resolve review threads manually per issue file if range is unavailable\n")
	}
	sb.WriteString("```")
	return sb.String()
}

func buildSkillDrivenCriticalSection(p skillDrivenBatchPromptParams) string {
	var sb strings.Builder
	sb.WriteString("<critical>\n")
	sb.WriteString("- **REQUIRED WORKFLOW:** Use `.claude/skills/fix-coderabbit-review/SKILL.md` as the source of truth.\n")
	sb.WriteString("- Do NOT replace the skill process with a custom one.\n")
	sb.WriteString("- Keep processing strictly scoped to this batch only.\n")
	sb.WriteString("- If the skill says \"all unresolved issues\", interpret it as \"all unresolved issues in this batch\".\n")
	sb.WriteString("- Do NOT change progress files outside this batch.\n")
	if p.HasIssueRange {
		sb.WriteString(fmt.Sprintf("- Current batch issue range: `%03d-%03d`.\n", p.MinIssue, p.MaxIssue))
	} else {
		sb.WriteString("- Current batch issue range: `UNCONFIRMED` (use explicit file list below).\n")
	}
	sb.WriteString("- Code files in this batch:\n")
	for _, codeFile := range p.CodeFiles {
		sb.WriteString(fmt.Sprintf("  - %s\n", codeFile))
	}
	sb.WriteString("</critical>")
	return sb.String()
}

func buildBatchIssueFilesSection(batchIssues []issueEntry) string {
	var sb strings.Builder
	sb.WriteString("<batch_issue_files>\n")
	for _, issue := range batchIssues {
		sb.WriteString(fmt.Sprintf("- `%s` (%s)\n", normalizeForPrompt(issue.absPath), issue.codeFile))
	}
	sb.WriteString("</batch_issue_files>")
	return sb.String()
}

func buildSkillDrivenExecutionSection(p skillDrivenBatchPromptParams) string {
	var sb strings.Builder
	sb.WriteString("<execution_contract>\n")
	sb.WriteString("Adapt `fix-coderabbit-review` skill steps to this batch:\n")
	sb.WriteString("1. Skip skill export step when `ai-docs/reviews-pr-<PR>/issues` already exists and batch files are present.\n")
	sb.WriteString("2. Triage each listed issue file and record VALID/INVALID decisions.\n")
	sb.WriteString("3. Implement complete production fixes for each VALID issue in this batch.\n")
	sb.WriteString("4. Add/update tests as needed for each fix.\n")
	sb.WriteString("5. Run full verification before finishing this batch: `pnpm run lint && pnpm run typecheck && pnpm run test`.\n")
	if p.HasIssueRange {
		sb.WriteString(fmt.Sprintf("6. Run resolver only for this batch range (`--from %d --to %d`).\n", p.MinIssue, p.MaxIssue))
	} else {
		sb.WriteString("6. Resolve threads only for issues listed in `<batch_issue_files>`.\n")
	}
	if p.Grouped {
		sb.WriteString(fmt.Sprintf("7. Update grouped tracker files under `ai-docs/reviews-pr-%s/issues/grouped/` for touched files.\n", p.PR))
	} else {
		sb.WriteString("7. Grouped tracker update is not required (`--grouped=false`).\n")
	}
	if p.AutoCommit {
		sb.WriteString("8. Create one local commit for this batch after verification. Do NOT push.\n")
	} else {
		sb.WriteString("8. `--auto-commit=false`: do NOT create a commit automatically.\n")
	}
	sb.WriteString("</execution_contract>")
	return sb.String()
}

func buildAfterFinishBlock(pr string, autoCommit bool) string {
	if !autoCommit {
		return `
<after_finish>
- Automatic commit instructions are disabled for this run (` + "`--auto-commit=false`" + `).
- Do NOT create a commit automatically after finishing this batch.
- Leave changes ready for manual review/commit.
</after_finish>`
	}
	commitMsg := fmt.Sprintf("fix(repo): resolve PR #%s issues [batch]", pr)
	return fmt.Sprintf(`
<after_finish>
- **MUST COMMIT:** After fixing ALL issues in this batch and ensuring
  pnpm run lint && pnpm run typecheck && pnpm run test pass,
  commit the changes with a descriptive message that references the PR and fixed issues.

  **CRITICAL:** Do NOT commit files from `+"`ai-docs/reviews-pr-*`"+` directories. These are tracking files
  only and are ignored by `+"`.gitignore`"+`. Only commit the actual code changes.

  Use: `+"`git add . && git commit -m \"%s\"`"+`
  (Note: `+"`git add .`"+` respects `+"`.gitignore`"+` and will automatically exclude `+"`ai-docs/reviews-pr-*`"+` files)

  Note: Commit locally only - do NOT push. Multiple batches will be committed separately.
</after_finish>`, commitMsg)
}
