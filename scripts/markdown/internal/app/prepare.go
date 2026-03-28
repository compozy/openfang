package app

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

var errNoIssues = errors.New("no issues to process")

func executeSolveIssues(ctx context.Context, cfg *config) error {
	prepared, err := prepareSolveIssues(ctx, cfg)
	if err != nil {
		if errors.Is(err, errNoIssues) {
			return nil
		}
		return err
	}

	failed, failures, total, shutdownErr := executeJobsWithGracefulShutdown(ctx, prepared.jobs, cfg)
	summarizeResults(failed, failures, total)
	if shutdownErr != nil {
		fmt.Fprintf(os.Stderr, "\nShutdown interrupted: %v\n", shutdownErr)
		return shutdownErr
	}
	if len(failures) > 0 {
		return errors.New("one or more groups failed; see logs above")
	}
	return nil
}

func prepareSolveIssues(_ context.Context, cfg *config) (*solvePreparation, error) {
	prep := &solvePreparation{}

	var err error
	prep.resolvedPr, prep.issuesDir, prep.issuesDirPath, err = resolveInputs(cfg)
	if err != nil {
		return nil, err
	}
	if err := ensureCLI(cfg); err != nil {
		return nil, err
	}

	entries, err := readIssueEntries(prep.issuesDirPath, cfg.mode, cfg.includeCompleted)
	if err != nil {
		return nil, err
	}
	entries, err = validateAndFilterEntries(entries, cfg.mode)
	if err != nil {
		return nil, err
	}

	groups := groupIssues(entries)
	promptRoot, err := initPromptRoot(prep.resolvedPr)
	if err != nil {
		return nil, err
	}

	prep.jobs, prep.groupedSummarized, err = prepareJobs(
		prep.resolvedPr,
		groups,
		promptRoot,
		prep.issuesDirPath,
		cfg.batchSize,
		cfg.grouped,
		cfg.autoCommit,
		cfg.mode,
	)
	if err != nil {
		return nil, err
	}
	return prep, nil
}

func prepareJobs(
	pr string,
	groups map[string][]issueEntry,
	promptRoot string,
	issuesDir string,
	batchSize int,
	grouped bool,
	autoCommit bool,
	mode executionMode,
) ([]job, bool, error) {
	effectiveBatchSize := batchSize
	effectiveGrouped := grouped
	if mode == ExecutionModePRDTasks {
		effectiveBatchSize = 1
		effectiveGrouped = false
	}
	if effectiveBatchSize <= 0 {
		effectiveBatchSize = 1
	}

	collected := flattenAndSortIssues(groups, mode)
	batches := createIssueBatches(collected, effectiveBatchSize)
	if len(batches) == 0 {
		return nil, false, errors.New("no batches created for prompt preparation")
	}

	groupedWritten := false
	if effectiveGrouped {
		if err := writeSummaries(issuesDir, groups); err != nil {
			return nil, false, fmt.Errorf("write grouped summaries: %w", err)
		}
		groupedWritten = true
	}

	jobs := make([]job, 0, len(batches))
	for idx, batchIssues := range batches {
		jb, err := buildBatchJob(
			pr,
			promptRoot,
			effectiveGrouped,
			autoCommit,
			idx,
			batchIssues,
			mode,
		)
		if err != nil {
			return nil, groupedWritten, err
		}
		jobs = append(jobs, jb)
	}
	if len(jobs) == 0 {
		return nil, groupedWritten, errors.New("no jobs finalized")
	}
	return jobs, groupedWritten, nil
}

func buildBatchJob(
	pr string,
	promptRoot string,
	grouped bool,
	autoCommit bool,
	batchIdx int,
	batchIssues []issueEntry,
	mode executionMode,
) (job, error) {
	batchGroups, batchFiles := groupIssuesByCodeFile(batchIssues)
	safeName := determineBatchName(batchIdx, batchFiles, mode)
	promptStr := buildBatchedIssuesPrompt(buildBatchedIssuesParams{
		PR:          pr,
		BatchGroups: batchGroups,
		Grouped:     grouped,
		AutoCommit:  autoCommit,
		Mode:        mode,
	})
	outPromptPath, outLog, errLog, err := writeBatchArtifacts(promptRoot, safeName, promptStr)
	if err != nil {
		return job{}, err
	}
	return job{
		codeFiles:     batchFiles,
		groups:        batchGroups,
		safeName:      safeName,
		prompt:        []byte(promptStr),
		outPromptPath: outPromptPath,
		outLog:        outLog,
		errLog:        errLog,
	}, nil
}

func determineBatchName(batchIdx int, batchFiles []string, mode executionMode) string {
	if mode == ExecutionModePRDTasks {
		if len(batchFiles) == 1 {
			return safeFileName(batchFiles[0])
		}
		return fmt.Sprintf("task_%03d", batchIdx+1)
	}
	if len(batchFiles) == 1 {
		filename := batchFiles[0]
		if strings.HasPrefix(filename, "__unknown__") {
			filename = unknownFileName
		}
		return safeFileName(filename)
	}
	return fmt.Sprintf("batch_%03d", batchIdx+1)
}

func writeBatchArtifacts(promptRoot, safeName, promptStr string) (string, string, string, error) {
	outPromptPath := filepath.Join(promptRoot, fmt.Sprintf("%s.prompt.md", safeName))
	if err := os.WriteFile(outPromptPath, []byte(promptStr), 0o600); err != nil {
		return "", "", "", fmt.Errorf("write prompt: %w", err)
	}
	outLog := filepath.Join(promptRoot, fmt.Sprintf("%s.out.log", safeName))
	errLog := filepath.Join(promptRoot, fmt.Sprintf("%s.err.log", safeName))
	return outPromptPath, outLog, errLog, nil
}

func flattenAndSortIssues(groups map[string][]issueEntry, mode executionMode) []issueEntry {
	allIssues := make([]issueEntry, 0)
	for _, items := range groups {
		allIssues = append(allIssues, items...)
	}
	if mode == ExecutionModePRDTasks {
		sort.SliceStable(allIssues, func(i, j int) bool {
			numI := extractTaskNumber(allIssues[i].name)
			numJ := extractTaskNumber(allIssues[j].name)
			if numI != numJ {
				return numI < numJ
			}
			return allIssues[i].name < allIssues[j].name
		})
		return allIssues
	}

	sort.SliceStable(allIssues, func(i, j int) bool {
		return allIssues[i].name < allIssues[j].name
	})
	return allIssues
}

func createIssueBatches(allIssues []issueEntry, batchSize int) [][]issueEntry {
	batches := make([][]issueEntry, 0)
	for i := 0; i < len(allIssues); i += batchSize {
		end := i + batchSize
		if end > len(allIssues) {
			end = len(allIssues)
		}
		batches = append(batches, allIssues[i:end])
	}
	return batches
}

func groupIssuesByCodeFile(issues []issueEntry) (map[string][]issueEntry, []string) {
	batchGroups := make(map[string][]issueEntry)
	for _, issue := range issues {
		batchGroups[issue.codeFile] = append(batchGroups[issue.codeFile], issue)
	}
	batchFiles := make([]string, 0, len(batchGroups))
	for codeFile := range batchGroups {
		batchFiles = append(batchFiles, codeFile)
	}
	sort.Strings(batchFiles)
	return batchGroups, batchFiles
}
