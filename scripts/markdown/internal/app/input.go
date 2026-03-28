package app

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
)

func resolveInputs(cfg *config) (string, string, string, error) {
	prValue := cfg.pr
	inputDir := cfg.issuesDir
	if prValue == "" && inputDir == "" {
		return "", "", "", errors.New("missing required flags: either --pr or --issues-dir must be provided")
	}

	var err error
	if prValue == "" && inputDir != "" {
		prValue, err = inferPrFromIssuesDir(inputDir)
		if err != nil {
			return "", "", "", err
		}
	}

	if inputDir == "" {
		if cfg.mode == ExecutionModePRDTasks {
			inputDir = fmt.Sprintf("tasks/prd-%s", prValue)
		} else {
			inputDir = fmt.Sprintf("ai-docs/reviews-pr-%s/issues", prValue)
		}
	}

	resolvedInputDir, err := filepath.Abs(inputDir)
	if err != nil {
		return "", "", "", fmt.Errorf("resolve issues dir: %w", err)
	}
	if st, statErr := os.Stat(resolvedInputDir); statErr != nil || !st.IsDir() {
		return "", "", "", fmt.Errorf("issues directory not found: %s", resolvedInputDir)
	}
	return prValue, inputDir, resolvedInputDir, nil
}

func ensureCLI(cfg *config) error {
	if cfg.dryRun {
		return nil
	}
	if err := assertIDEExists(cfg.ide); err != nil {
		return err
	}
	if err := assertExecSupported(cfg.ide); err != nil {
		return err
	}
	return nil
}

func validateAndFilterEntries(entries []issueEntry, mode executionMode) ([]issueEntry, error) {
	if len(entries) == 0 {
		if mode == ExecutionModePRDTasks {
			fmt.Println("No task files found.")
		} else {
			fmt.Println("No issue files found.")
		}
		return nil, errNoIssues
	}
	if mode == ExecutionModePRReview {
		entries = filterUnresolved(entries)
		if len(entries) == 0 {
			fmt.Println("All issues are already resolved. Nothing to do.")
			return nil, errNoIssues
		}
	}
	return entries, nil
}

func readIssueEntries(resolvedIssuesDir string, mode executionMode, includeCompleted bool) ([]issueEntry, error) {
	if mode == ExecutionModePRDTasks {
		return readTaskEntries(resolvedIssuesDir, includeCompleted)
	}
	return readCodeRabbitIssues(resolvedIssuesDir)
}

func parseTaskFile(content string) taskEntry {
	task := taskEntry{content: content}
	statusRe := regexp.MustCompile(`(?m)^##\s*status:\s*(\w+)`)
	if m := statusRe.FindStringSubmatch(content); len(m) > 1 {
		task.status = strings.TrimSpace(m[1])
	}

	contextStart := strings.Index(content, "<task_context>")
	contextEnd := strings.Index(content, "</task_context>")
	if contextStart > 0 && contextEnd > contextStart {
		contextBlock := content[contextStart : contextEnd+15]
		task.domain = extractXMLTag(contextBlock, "domain")
		task.taskType = extractXMLTag(contextBlock, "type")
		task.scope = extractXMLTag(contextBlock, "scope")
		task.complexity = extractXMLTag(contextBlock, "complexity")
		if deps := extractXMLTag(contextBlock, "dependencies"); deps != "none" {
			task.dependencies = strings.Split(deps, ",")
			for i := range task.dependencies {
				task.dependencies[i] = strings.TrimSpace(task.dependencies[i])
			}
		}
	}
	return task
}

func extractXMLTag(content, tag string) string {
	re := regexp.MustCompile(fmt.Sprintf(`<%s>(.*?)</%s>`, tag, tag))
	if m := re.FindStringSubmatch(content); len(m) > 1 {
		return strings.TrimSpace(m[1])
	}
	return ""
}

func isTaskCompleted(task *taskEntry) bool {
	status := strings.ToLower(task.status)
	return status == "completed" || status == "done" || status == "finished"
}

func extractTaskNumber(filename string) int {
	if !reTaskFile.MatchString(filename) {
		return 0
	}
	numStr := strings.TrimPrefix(filename, "_task_")
	numStr = strings.TrimSuffix(numStr, ".md")
	num, err := strconv.Atoi(numStr)
	if err != nil {
		return 0
	}
	return num
}

func readTaskEntries(tasksDir string, includeCompleted bool) ([]issueEntry, error) {
	entries := []issueEntry{}
	files, err := os.ReadDir(tasksDir)
	if err != nil {
		return nil, err
	}

	names := make([]string, 0, len(files))
	for _, f := range files {
		if !f.Type().IsRegular() || !strings.HasSuffix(f.Name(), ".md") {
			continue
		}
		if !reTaskFile.MatchString(f.Name()) {
			continue
		}
		names = append(names, f.Name())
	}

	sort.SliceStable(names, func(i, j int) bool {
		return extractTaskNumber(names[i]) < extractTaskNumber(names[j])
	})

	for _, name := range names {
		absPath := filepath.Join(tasksDir, name)
		b, err := os.ReadFile(absPath)
		if err != nil {
			return nil, err
		}
		content := string(b)
		task := parseTaskFile(content)
		if !includeCompleted && isTaskCompleted(&task) {
			continue
		}
		entries = append(entries, issueEntry{
			name:     name,
			absPath:  absPath,
			content:  content,
			codeFile: strings.TrimSuffix(name, ".md"),
		})
	}
	return entries, nil
}

func readCodeRabbitIssues(resolvedIssuesDir string) ([]issueEntry, error) {
	entries := []issueEntry{}
	files, err := os.ReadDir(resolvedIssuesDir)
	if err != nil {
		return nil, err
	}

	names := make([]string, 0, len(files))
	for _, f := range files {
		if !f.Type().IsRegular() {
			continue
		}
		if f.Name() == "_summary.md" {
			continue
		}
		if strings.HasSuffix(f.Name(), ".md") {
			names = append(names, f.Name())
		}
	}
	sort.Strings(names)

	for _, name := range names {
		absPath := filepath.Join(resolvedIssuesDir, name)
		b, err := os.ReadFile(absPath)
		if err != nil {
			return nil, err
		}
		content := string(b)
		cf := extractCodeFileFromIssue(content)
		if cf == "" {
			cf = "__unknown__:" + name
		}
		entries = append(entries, issueEntry{name: name, absPath: absPath, content: content, codeFile: cf})
	}
	return entries, nil
}

func filterUnresolved(all []issueEntry) []issueEntry {
	out := make([]issueEntry, 0, len(all))
	for _, e := range all {
		if !isIssueResolved(e.content) {
			out = append(out, e)
		}
	}
	return out
}

var (
	reResolvedStatus = regexp.MustCompile(`(?mi)^\s*(status|state)\s*:\s*resolved\b`)
	reResolvedTask   = regexp.MustCompile(`(?mi)^\s*-\s*\[(x|X)\]\s*resolved\b`)
	reTaskFile       = regexp.MustCompile(`^_task_\d+\.md$`)
)

func isIssueResolved(content string) bool {
	if strings.Contains(strings.ToUpper(content), "RESOLVED ✓") {
		return true
	}
	if reResolvedStatus.FindStringIndex(content) != nil {
		return true
	}
	if reResolvedTask.FindStringIndex(content) != nil {
		return true
	}
	return false
}

func groupIssues(entries []issueEntry) map[string][]issueEntry {
	groups := make(map[string][]issueEntry)
	for _, it := range entries {
		groups[it.codeFile] = append(groups[it.codeFile], it)
	}
	return groups
}

func writeGroupedSummaries(groupedDir string, groups map[string][]issueEntry) error {
	for codeFile, items := range groups {
		safeName := safeFileName(func() string {
			if strings.HasPrefix(codeFile, "__unknown__") {
				return unknownFileName
			}
			return codeFile
		}())
		groupFile := filepath.Join(groupedDir, fmt.Sprintf("%s.md", safeName))
		header := fmt.Sprintf("# Issue Group for %s\n\n", func() string {
			if strings.HasPrefix(codeFile, "__unknown__") {
				return "(unknown file)"
			}
			return codeFile
		}())

		var sb strings.Builder
		sb.WriteString(header)
		sb.WriteString(buildGroupedResolutionChecklist(items))
		sb.WriteString("## Included Issues\n\n")
		for _, it := range items {
			sb.WriteString("- ")
			sb.WriteString(it.name)
			sb.WriteString("\n")
		}
		for _, it := range items {
			sb.WriteString("\n---\n\n## ")
			sb.WriteString(it.name)
			sb.WriteString("\n\n")
			sb.WriteString(it.content)
		}
		sb.WriteString("\n")
		if err := os.WriteFile(groupFile, []byte(sb.String()), 0o600); err != nil {
			return err
		}
	}
	return nil
}

func buildGroupedResolutionChecklist(items []issueEntry) string {
	var checklist strings.Builder
	checklist.WriteString("## Resolution Checklist\n\n")
	checklist.WriteString(
		"> ⚠️ This grouped issue contains multiple unresolved review tasks for the same source file.\n",
	)
	checklist.WriteString("> Resolve **every** task below before treating this file as complete.\n")
	checklist.WriteString(
		"> After resolving a task, update the original issue file with `RESOLVED ✓` and run any provided gh command.\n\n",
	)
	for _, it := range items {
		checklist.WriteString("- [ ] Resolve `")
		checklist.WriteString(it.name)
		checklist.WriteString("` (source issue: `")
		checklist.WriteString(normalizeForPrompt(it.absPath))
		checklist.WriteString("`)\n")
		checklist.WriteString("      - Apply the requested code changes and update the issue status to `RESOLVED ✓`.\n")
		checklist.WriteString("      - Run the review thread command if a Thread ID is provided.\n")
	}
	checklist.WriteString("- [ ] Document the fixes in this grouped file and tick every checklist item above.\n\n")
	return checklist.String()
}

func normalizeForPrompt(absPath string) string {
	resolvedPath, err := filepath.Abs(absPath)
	if err != nil {
		return absPath
	}
	cwd, err := os.Getwd()
	if err != nil {
		return resolvedPath
	}
	cwd = filepath.Clean(cwd)
	resolvedPath = filepath.Clean(resolvedPath)
	prefix := cwd + string(os.PathSeparator)
	if strings.HasPrefix(resolvedPath, prefix) {
		return resolvedPath[len(prefix):]
	}
	return resolvedPath
}

func inferPrFromIssuesDir(dir string) (string, error) {
	re := regexp.MustCompile(`reviews-pr-(\d+)`)
	m := re.FindStringSubmatch(dir)
	if len(m) < 2 {
		return "", errors.New("unable to infer PR number from issues dir")
	}
	return m[1], nil
}

func extractCodeFileFromIssue(content string) string {
	re := regexp.MustCompile(`\*\*File:\*\*\s*` + "`" + `([^` + "`" + `]+)` + "`")
	m := re.FindStringSubmatch(content)
	if len(m) < 2 {
		return ""
	}
	raw := strings.TrimSpace(m[1])
	if idx := strings.LastIndex(raw, ":"); idx != -1 {
		tail := raw[idx+1:]
		if tail != "" && isAllDigits(tail) {
			raw = strings.TrimSpace(raw[:idx])
		}
	}
	return raw
}

func isAllDigits(s string) bool {
	for i := 0; i < len(s); i++ {
		if s[i] < '0' || s[i] > '9' {
			return false
		}
	}
	return true
}

func sanitizePath(p string) string {
	b := make([]rune, 0, len(p))
	for _, r := range p {
		if (r >= 'a' && r <= 'z') || (r >= 'A' && r <= 'Z') || (r >= '0' && r <= '9') || r == '.' || r == '_' ||
			r == '-' {
			b = append(b, r)
			continue
		}
		b = append(b, '_')
	}
	return string(b)
}

func safeFileName(p string) string {
	norm := strings.ReplaceAll(p, "\\", "/")
	base := sanitizePath(norm)
	sum := sha256.Sum256([]byte(norm))
	hash := hex.EncodeToString(sum[:])[:6]
	return fmt.Sprintf("%s-%s", base, hash)
}

func writeSummaries(resolvedIssuesDir string, groups map[string][]issueEntry) error {
	groupedDir := filepath.Join(resolvedIssuesDir, "grouped")
	if err := os.MkdirAll(groupedDir, 0o755); err != nil {
		return fmt.Errorf("mkdir grouped dir: %w", err)
	}
	return writeGroupedSummaries(groupedDir, groups)
}

func initPromptRoot(pr string) (string, error) {
	promptRoot, err := filepath.Abs(filepath.Join(".tmp", "codex-prompts", fmt.Sprintf("pr-%s", pr)))
	if err != nil {
		return "", err
	}
	if err := os.MkdirAll(promptRoot, 0o755); err != nil {
		return "", fmt.Errorf("mkdir prompt root: %w", err)
	}
	return promptRoot, nil
}

func assertIDEExists(ide string) error {
	if _, err := exec.LookPath(ide); err != nil {
		return fmt.Errorf("%s CLI not found on PATH", ide)
	}
	return nil
}

func assertExecSupported(ide string) error {
	cmd := exec.CommandContext(context.Background(), ide, "--help")
	cmd.Stdout = io.Discard
	cmd.Stderr = io.Discard
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("%s CLI does not appear to be properly installed or configured", ide)
	}
	return nil
}
