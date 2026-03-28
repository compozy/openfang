package app

import (
	"fmt"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
)

type buildBatchedIssuesParams struct {
	PR          string
	BatchGroups map[string][]issueEntry
	Grouped     bool
	AutoCommit  bool
	Mode        executionMode
}

func buildBatchedIssuesPrompt(p buildBatchedIssuesParams) string {
	if p.Mode == ExecutionModePRDTasks {
		return buildPRDTasksPrompt(p)
	}
	return buildCodeReviewPrompt(p)
}

func buildPRDTasksPrompt(p buildBatchedIssuesParams) string {
	var task issueEntry
	for _, items := range p.BatchGroups {
		if len(items) > 0 {
			task = items[0]
			break
		}
	}
	return buildPRDTaskPrompt(task, p.AutoCommit)
}

func batchIssueRange(batchIssues []issueEntry) (int, int, bool) {
	minIssue := 0
	maxIssue := 0
	hasIssueRange := false
	for _, issue := range batchIssues {
		issueNum, ok := parseIssueNumber(issue.name)
		if !ok {
			continue
		}
		if !hasIssueRange {
			minIssue = issueNum
			maxIssue = issueNum
			hasIssueRange = true
			continue
		}
		if issueNum < minIssue {
			minIssue = issueNum
		}
		if issueNum > maxIssue {
			maxIssue = issueNum
		}
	}
	return minIssue, maxIssue, hasIssueRange
}

func parseIssueNumber(name string) (int, bool) {
	base := filepath.Base(name)
	parts := strings.SplitN(base, "-", 2)
	if len(parts) == 0 || !isAllDigits(parts[0]) {
		return 0, false
	}
	issueNum, err := strconv.Atoi(parts[0])
	if err != nil {
		return 0, false
	}
	return issueNum, true
}

func sortCodeFiles(batchGroups map[string][]issueEntry) []string {
	codeFiles := make([]string, 0, len(batchGroups))
	for codeFile := range batchGroups {
		codeFiles = append(codeFiles, codeFile)
	}
	sort.Strings(codeFiles)
	return codeFiles
}

func buildBatchHeader(pr string, codeFiles []string, batchGroups map[string][]issueEntry) string {
	totalIssues := 0
	for _, items := range batchGroups {
		totalIssues += len(items)
	}
	return fmt.Sprintf(`<arguments>
  <type>batched-issues</type>
  <pr>%s</pr>
  <files>%d</files>
  <total-issues>%d</total-issues>
</arguments>`, pr, len(codeFiles), totalIssues)
}

func buildBatchChecklist(pr string, batchGroups map[string][]issueEntry, grouped bool) string {
	allIssues := make([]issueEntry, 0)
	for _, items := range batchGroups {
		allIssues = append(allIssues, items...)
	}
	sort.Slice(allIssues, func(i, j int) bool {
		return allIssues[i].name < allIssues[j].name
	})

	var checklistPaths []string
	if grouped {
		seenGrouped := make(map[string]bool)
		for _, issue := range allIssues {
			groupedPath := fmt.Sprintf("ai-docs/reviews-pr-%s/issues/grouped/%s.md", pr, safeFileName(issue.codeFile))
			if !seenGrouped[groupedPath] {
				checklistPaths = append(checklistPaths, groupedPath)
				seenGrouped[groupedPath] = true
			}
		}
	}
	for _, item := range allIssues {
		checklistPaths = append(checklistPaths, normalizeForPrompt(item.absPath))
	}

	var chk strings.Builder
	chk.WriteString("\n<checklist>\n  <title>Progress Files to Update</title>\n")
	for _, path := range checklistPaths {
		chk.WriteString("  <path>")
		chk.WriteString(path)
		chk.WriteString("</path>\n")
	}
	chk.WriteString("</checklist>\n")
	return chk.String()
}
