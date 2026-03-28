package app

import (
	"fmt"
	"path/filepath"
	"strings"
)

func buildPRDTaskPrompt(task issueEntry, autoCommit bool) string {
	taskData := parseTaskFile(task.content)
	prdDir := filepath.Dir(task.absPath)
	tasksFile := filepath.Join(prdDir, "_tasks.md")
	header := fmt.Sprintf("# Implementation Task: %s\n\n", task.name)
	contextSection := buildTaskContextSection(&taskData)
	criticalSection := buildCriticalExecutionSection(autoCommit)
	postureSection := buildAutonomousExecutionPostureSection()
	explorationSection := buildExplorationPhaseSection(autoCommit)
	specSection := fmt.Sprintf("## Task Specification\n\n%s\n\n", task.content)
	implSection := buildImplementationInstructionsSection(prdDir)
	subagentSection := buildSubagentUsageSection(autoCommit)
	completionSection := buildCompletionCriteriaSection(task.absPath, tasksFile, task.name, autoCommit)
	return header + contextSection + criticalSection + postureSection + explorationSection +
		specSection + implSection + subagentSection + completionSection
}

func buildTaskContextSection(taskData *taskEntry) string {
	var sb strings.Builder
	sb.WriteString("## Task Context\n\n")
	sb.WriteString(fmt.Sprintf("- **Domain**: %s\n", taskData.domain))
	sb.WriteString(fmt.Sprintf("- **Type**: %s\n", taskData.taskType))
	sb.WriteString(fmt.Sprintf("- **Scope**: %s\n", taskData.scope))
	sb.WriteString(fmt.Sprintf("- **Complexity**: %s\n", taskData.complexity))
	if len(taskData.dependencies) > 0 {
		sb.WriteString(fmt.Sprintf("- **Dependencies**: %s\n", strings.Join(taskData.dependencies, ", ")))
	}
	sb.WriteString("\n")
	return sb.String()
}

func buildCriticalExecutionSection(autoCommit bool) string {
	autoCommitValidationRequirements := "- Self-review implementation before completion handoff (check quality, standards compliance)\n" +
		"- All blocking issues from self-review MUST be resolved before handoff\n" +
		"- Auto-commit is disabled (`--auto-commit=false`): do NOT create commits automatically\n"
	autoCommitInvalidationRequirements := "- Task completion steps are skipped (including validation and status updates)\n"
	if autoCommit {
		autoCommitValidationRequirements = "- Self-review implementation before committing (check quality, standards compliance)\n" +
			"- All blocking issues from self-review MUST be resolved before committing\n" +
			"- All changes MUST be committed with a descriptive commit message (only after review)\n"
		autoCommitInvalidationRequirements = "- Task completion steps are skipped (including commits)\n"
	}

	return renderPromptTemplate("prd-task-critical.txt", map[string]string{
		"AUTO_COMMIT_VALIDATION_REQUIREMENTS":   autoCommitValidationRequirements,
		"AUTO_COMMIT_INVALIDATION_REQUIREMENTS": autoCommitInvalidationRequirements,
	})
}

func buildAutonomousExecutionPostureSection() string {
	return renderPromptTemplate("prd-task-autonomous-execution-posture.txt", nil)
}

func buildExplorationPhaseSection(autoCommit bool) string {
	autoCommitStep := "9. **NO AUTO-COMMIT**: Since `--auto-commit=false`, do NOT create commits automatically\n\n"
	if autoCommit {
		autoCommitStep = "9. **COMMIT (ONLY AFTER REVIEW)**: Commit changes ONLY after self-review is clean\n\n"
	}
	return renderPromptTemplate("prd-task-exploration-phase.txt", map[string]string{
		"AUTO_COMMIT_STEP": autoCommitStep,
	})
}

func buildSubagentUsageSection(autoCommit bool) string {
	selfReviewStrategy := "- **Self-Review (Quality Gate)**: Before completion handoff, review your own implementation for quality and standards compliance.\n"
	if autoCommit {
		selfReviewStrategy = "- **Self-Review (Quality Gate)**: Before committing, review your own implementation for quality and standards compliance.\n"
	}
	return renderPromptTemplate("prd-task-execution-strategy.txt", map[string]string{
		"SELF_REVIEW_STRATEGY": selfReviewStrategy,
	})
}

func buildImplementationInstructionsSection(prdDir string) string {
	return renderPromptTemplate("prd-task-implementation-instructions.txt", map[string]string{
		"PRD_DIR": prdDir,
	})
}

type commitInstructionsOpts struct {
	commitMsg           string
	includePRDDirs      bool
	includeVerification bool
	localCommitNote     bool
	indentPrefix        string
}

func buildCommitInstructions(opts commitInstructionsOpts) string {
	var sb strings.Builder
	prefix := opts.indentPrefix
	if prefix == "" {
		prefix = "   - "
	}
	indent := strings.Repeat(" ", len(prefix))
	commitMsgForMarkdown := strings.ReplaceAll(opts.commitMsg, "`", "'")

	sb.WriteString(prefix + "First, verify what will be committed: `git status`\n")
	sb.WriteString(fmt.Sprintf(prefix+"Commit code changes with a descriptive message like: `%s`\n", commitMsgForMarkdown))

	exclusions := "`ai-docs/reviews-pr-*`"
	if opts.includePRDDirs {
		exclusions += " or `tasks/prd-*`"
	}
	sb.WriteString(fmt.Sprintf(prefix+"**CRITICAL:** Do NOT commit files from %s directories.\n", exclusions))
	sb.WriteString(indent + "These are tracking files only and are ignored by `.gitignore`.\n")

	if opts.includeVerification {
		sb.WriteString(prefix + "Verify no tracking files are staged: `git diff --cached --name-only | grep -E 'ai-docs/reviews-pr-|tasks/prd-'`\n")
		sb.WriteString(indent + "(This command should return nothing. If it shows files, reset with `git reset`)\n")
	}

	sb.WriteString(fmt.Sprintf(prefix+"Use: `git add . && git commit -m %q`\n", opts.commitMsg))
	sb.WriteString(indent + "(Note: `git add .` respects `.gitignore`")
	if opts.includeVerification {
		sb.WriteString(", but always verify first)\n")
	} else {
		sb.WriteString(" and will automatically exclude tracking files)\n")
	}

	if opts.localCommitNote {
		sb.WriteString("\n" + indent)
		sb.WriteString("Note: Commit locally only - do NOT push. Multiple batches will be committed separately.\n")
	}

	return sb.String()
}

func buildSelfReviewStep(autoCommit bool) string {
	var sb strings.Builder
	if autoCommit {
		sb.WriteString("2. **Self-Review Implementation (MANDATORY BEFORE COMMIT)**:\n")
		sb.WriteString("   - **CRITICAL**: You MUST review your own implementation BEFORE committing\n")
	} else {
		sb.WriteString("2. **Self-Review Implementation (MANDATORY BEFORE COMPLETION HANDOFF)**:\n")
		sb.WriteString("   - **CRITICAL**: You MUST review your own implementation BEFORE completion handoff\n")
	}
	sb.WriteString("   - Check that all requirements from the task specification are met\n")
	sb.WriteString("   - Verify code follows project standards and coding conventions\n")
	sb.WriteString("   - Ensure no workarounds, hacks, or shortcuts were used\n")
	sb.WriteString("   - If you find blocking issues:\n")
	sb.WriteString("     - Resolve ALL blocking issues directly using tools\n")
	sb.WriteString("     - Re-run tests and linting after fixes\n")
	sb.WriteString("     - Review again until all issues are resolved\n")
	if autoCommit {
		sb.WriteString("   - **DO NOT skip self-review** - this is a mandatory gate before commits\n\n")
	} else {
		sb.WriteString("   - **DO NOT skip self-review** - this is a mandatory gate before completion handoff\n\n")
	}
	return sb.String()
}

func buildCommitStep(taskName string, autoCommit bool) string {
	var sb strings.Builder
	if !autoCommit {
		sb.WriteString("6. **No Auto-Commit for This Run**:\n")
		sb.WriteString("   - Auto-commit is disabled via `--auto-commit=false`\n")
		sb.WriteString("   - Do NOT create a commit automatically at task completion\n")
		sb.WriteString("   - Leave the changes ready for manual review/commit\n\n")
		return sb.String()
	}

	sb.WriteString("6. **MUST COMMIT Changes (ONLY AFTER REVIEW)**:\n")
	sb.WriteString("   - **MANDATORY**: Commit ALL changes with a descriptive message following project conventions\n")
	sb.WriteString("   - **CRITICAL**: Commits MUST happen AFTER self-review with zero blocking issues\n")
	sb.WriteString(buildCommitInstructions(commitInstructionsOpts{
		commitMsg:           fmt.Sprintf("fix(repo): complete %s", taskName),
		includePRDDirs:      true,
		includeVerification: true,
		localCommitNote:     false,
		indentPrefix:        "   - ",
	}))
	sb.WriteString("   - ⚠️ **DO NOT commit before self-review** - commits without review invalidate the task\n\n")
	return sb.String()
}

func buildCompletionSteps(taskAbsPath, tasksFile, taskName string, autoCommit bool) string {
	var sb strings.Builder
	sb.WriteString("1. **Verify Implementation and Evidence**:\n")
	sb.WriteString("   - All subtasks in this task file are completed\n")
	sb.WriteString("   - All deliverables specified are produced\n")
	sb.WriteString("   - Every explicit acceptance criterion from the task/PRD docs is satisfied\n")
	sb.WriteString("   - Every explicit `Validation`, `Test Plan`, or `Testing` requirement has been executed\n")
	sb.WriteString("   - The pre-change reproduction signal is resolved or superseded by concrete after-change proof\n")
	sb.WriteString("   - All tests pass: `pnpm run test`\n")
	sb.WriteString("   - Code passes linting: `pnpm run lint`\n")
	sb.WriteString("   - Code passes type checking: `pnpm run typecheck`\n\n")
	sb.WriteString(buildSelfReviewStep(autoCommit))
	sb.WriteString("3. **Mark Subtasks Complete**:\n")
	sb.WriteString(fmt.Sprintf("   - In `%s`, check all `[ ]` boxes to `[x]` for completed subtasks\n\n", taskAbsPath))
	sb.WriteString("4. **Update Task Status**:\n")
	sb.WriteString(fmt.Sprintf("   - In `%s`, change the status line from:\n", taskAbsPath))
	sb.WriteString("     ```\n     ## status: pending\n     ```\n")
	sb.WriteString("     to:\n")
	sb.WriteString("     ```\n     ## status: completed\n     ```\n\n")
	sb.WriteString("5. **Update Master Tasks List**:\n")
	sb.WriteString(fmt.Sprintf("   - In `%s`, check the corresponding task checkbox for `%s`\n\n", tasksFile, taskName))
	sb.WriteString(buildCommitStep(taskName, autoCommit))
	return sb.String()
}

func buildCompletionChecklist(autoCommit bool) string {
	blockingIssuesRequirement := "- **ALL blocking issues resolved before completion handoff**\n"
	commitCompletionRequirement := "- **No automatic commit created** (`--auto-commit=false`)\n\n"
	autoCommitOrderRequirement := "6. **Do NOT auto-commit for this run**\n7. Update task status to 'completed'\n\n"
	autoCommitInvalidationChecklist := "- You skip the self-review before completion handoff\n" +
		"- You create an automatic commit despite `--auto-commit=false`\n" +
		"- You fail to resolve blocking issues before completion handoff\n"
	if autoCommit {
		blockingIssuesRequirement = "- **ALL blocking issues resolved before committing**\n"
		commitCompletionRequirement = "- **ALL changes committed** with a descriptive commit message (ONLY after review)\n\n"
		autoCommitOrderRequirement = "6. **ONLY THEN: Commit changes**\n7. Update task status to 'completed'\n\n"
		autoCommitInvalidationChecklist = "- You skip the self-review before committing\n" +
			"- You commit before resolving blocking issues\n" +
			"- You skip the commit step after review\n" +
			"- You fail to resolve blocking issues before committing\n"
	}

	return renderPromptTemplate("prd-task-completion-checklist.txt", map[string]string{
		"BLOCKING_ISSUES_REQUIREMENT":        blockingIssuesRequirement,
		"COMMIT_COMPLETION_REQUIREMENT":      commitCompletionRequirement,
		"AUTO_COMMIT_ORDER_REQUIREMENT":      autoCommitOrderRequirement,
		"AUTO_COMMIT_INVALIDATION_CHECKLIST": autoCommitInvalidationChecklist,
	})
}

func buildCompletionCriteriaSection(taskAbsPath, tasksFile, taskName string, autoCommit bool) string {
	var sb strings.Builder
	sb.WriteString("## Completion Criteria\n\n")
	sb.WriteString("After implementation, you MUST complete ALL of the following steps:\n\n")
	sb.WriteString(buildCompletionSteps(taskAbsPath, tasksFile, taskName, autoCommit))
	sb.WriteString(buildCompletionChecklist(autoCommit))
	return sb.String()
}
