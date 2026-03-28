package app

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/spf13/cobra"
)

func TestShouldSuppressCodexRolloutStderrLine(t *testing.T) {
	t.Parallel()
	noiseLine := "\x1b[2m2026-02-11T22:55:19.818397Z\x1b[0m \x1b[31mERROR\x1b[0m " +
		"\x1b[2mcodex_core::rollout::list\x1b[0m\x1b[2m:\x1b[0m state db missing rollout path " +
		"for thread 019c4084-4858-7df3-84e1-b0873437aa64"
	if !shouldSuppressCodexRolloutStderrLine(noiseLine) {
		t.Fatalf("expected known rollout noise line to be suppressed")
	}

	realError := "2026-02-11T22:55:19.818397Z ERROR codex_core::network: request failed: EOF"
	if shouldSuppressCodexRolloutStderrLine(realError) {
		t.Fatalf("expected real codex error to be kept")
	}
}

func TestLineFilterWriterSuppressesOnlyKnownCodexNoise(t *testing.T) {
	t.Parallel()
	var out bytes.Buffer
	w := newLineFilterWriter(&out, nil, shouldSuppressCodexRolloutStderrLine)

	chunk1 := "2026-02-11T22:55:19.818397Z ERROR codex_core::rollout::list: state db missing rollout "
	chunk2 := "path for thread 019c4084-4858-7df3-84e1-b0873437aa64\nREAL ERROR: failed to open file\n"
	if _, err := w.Write([]byte(chunk1)); err != nil {
		t.Fatalf("unexpected write error: %v", err)
	}
	if _, err := w.Write([]byte(chunk2)); err != nil {
		t.Fatalf("unexpected write error: %v", err)
	}

	got := out.String()
	if strings.Contains(got, "state db missing rollout path for thread") {
		t.Fatalf("expected rollout noise to be filtered, got %q", got)
	}
	if !strings.Contains(got, "REAL ERROR: failed to open file") {
		t.Fatalf("expected real stderr line to remain, got %q", got)
	}
}

func TestBuildCodexCommandUsesExecJSONOrder(t *testing.T) {
	t.Parallel()
	cmd := buildCodexCommand("", nil, "medium")
	want := "codex --dangerously-bypass-approvals-and-sandbox -m " + defaultCodexModel +
		" -c model_reasoning_effort=medium exec --json -"
	if cmd != want {
		t.Fatalf("unexpected codex command string\nwant: %s\ngot:  %s", want, cmd)
	}
}

func TestBuildCodexCommandIncludesAddDirsBeforeExec(t *testing.T) {
	t.Parallel()

	cmd := buildCodexCommand("", []string{"../shared", "../docs"}, "medium")

	want := "codex --dangerously-bypass-approvals-and-sandbox -m " + defaultCodexModel + " " +
		"-c model_reasoning_effort=medium --add-dir ../shared --add-dir ../docs exec --json -"
	if cmd != want {
		t.Fatalf("unexpected codex command string\nwant: %s\ngot:  %s", want, cmd)
	}
}

func TestBuildClaudeCommandIncludesAddDirs(t *testing.T) {
	t.Parallel()

	cmd := buildClaudeCommand("", []string{"../shared", "../docs"}, "medium")

	if !strings.Contains(cmd, "--add-dir ../shared --add-dir ../docs") {
		t.Fatalf("expected claude command to include add-dir flags, got %q", cmd)
	}
}

func TestBuildCLIArgsIncludesAutoCommit(t *testing.T) {
	t.Parallel()
	origAutoCommit := autoCommit
	origTimeout := timeout
	origAddDirs := append([]string(nil), addDirs...)
	t.Cleanup(func() {
		autoCommit = origAutoCommit
		timeout = origTimeout
		addDirs = origAddDirs
	})

	autoCommit = false
	timeout = "10m"
	addDirs = []string{"../shared", "../docs", "../shared"}
	args := buildCLIArgs()
	if args.autoCommit {
		t.Fatalf("expected autoCommit=false in cli args")
	}
	if !reflect.DeepEqual(args.addDirs, []string{"../shared", "../docs"}) {
		t.Fatalf("expected normalized addDirs in cli args, got %#v", args.addDirs)
	}

	autoCommit = true
	args = buildCLIArgs()
	if !args.autoCommit {
		t.Fatalf("expected autoCommit=true in cli args")
	}
}

func TestApplyStringSliceInputParsesAddDirsFromFormValue(t *testing.T) {
	t.Parallel()

	origAddDirs := append([]string(nil), addDirs...)
	t.Cleanup(func() {
		addDirs = origAddDirs
	})

	cmd := &cobra.Command{Use: "test"}
	cmd.Flags().StringSlice("add-dir", nil, "test add-dir")

	fi := &formInputs{
		addDirs: " ../shared, ../docs ,, ../shared \n ../workspace ",
	}

	fi.apply(cmd)

	want := []string{"../shared", "../docs", "../workspace"}
	if !reflect.DeepEqual(addDirs, want) {
		t.Fatalf("unexpected addDirs from form\nwant: %#v\ngot:  %#v", want, addDirs)
	}
}

func TestCreateIDECommandAddsDirsOnlyForSupportedIDEs(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name    string
		ide     string
		wantAdd bool
	}{
		{name: "codex", ide: ideCodex, wantAdd: true},
		{name: "claude", ide: ideClaude, wantAdd: true},
		{name: "cursor", ide: ideCursor, wantAdd: false},
		{name: "droid", ide: ideDroid, wantAdd: false},
	}

	for _, tc := range cases {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			cmd := createIDECommand(context.Background(), &cliArgs{
				ide:             tc.ide,
				addDirs:         []string{"../shared", "../docs"},
				reasoningEffort: "medium",
			})
			if cmd == nil {
				t.Fatalf("expected command for ide %q", tc.ide)
			}

			got := strings.Join(cmd.Args, " ")
			hasAddDir := strings.Contains(got, "--add-dir ../shared --add-dir ../docs")
			if hasAddDir != tc.wantAdd {
				t.Fatalf("unexpected add-dir presence for %s: %q", tc.ide, got)
			}
		})
	}
}

func TestClaudePromptForEffortUsesEmbeddedTemplates(t *testing.T) {
	t.Parallel()

	cases := map[string]string{
		"low":    "Think concisely and act quickly. Prefer direct solutions.",
		"medium": "Think hard through problems carefully before acting. Balance speed with thoroughness.",
		"high":   "Ultrathink deeply and comprehensively before taking action.",
		"xhigh":  "Ultra-deep thinking mode: Exhaustively analyze every aspect of the problem.",
	}

	for reasoning, snippet := range cases {
		prompt := claudePromptForEffort(reasoning)
		if !strings.Contains(prompt, snippet) {
			t.Fatalf("expected prompt for %q to include %q, got %q", reasoning, snippet, prompt)
		}
	}
}

func TestBuildPRDTaskPromptRespectsAutoCommitFlag(t *testing.T) {
	t.Parallel()
	task := issueEntry{
		name:    "_task_1.md",
		absPath: "/tmp/tasks/prd-demo/_task_1.md",
		content: `## status: pending
<task_context>
  <domain>backend</domain>
  <type>feature</type>
  <scope>small</scope>
  <complexity>low</complexity>
</task_context>
`,
	}

	promptWithAutoCommit := buildPRDTaskPrompt(task, true)
	if !strings.Contains(promptWithAutoCommit, "MUST COMMIT Changes") {
		t.Fatalf("expected auto-commit prompt to include commit instructions")
	}

	promptWithoutAutoCommit := buildPRDTaskPrompt(task, false)
	if strings.Contains(promptWithoutAutoCommit, "MUST COMMIT Changes") {
		t.Fatalf("expected no-auto-commit prompt to omit mandatory commit step")
	}
	if !strings.Contains(promptWithoutAutoCommit, "--auto-commit=false") {
		t.Fatalf("expected no-auto-commit prompt to mention --auto-commit=false")
	}
}

func TestBuildPRDTaskPromptIncludesAutonomousExecutionGuardrails(t *testing.T) {
	t.Parallel()
	task := issueEntry{
		name:    "_task_2.md",
		absPath: "/tmp/tasks/prd-demo/_task_2.md",
		content: `## status: pending
<task_context>
  <domain>frontend</domain>
  <type>bugfix</type>
  <scope>medium</scope>
  <complexity>medium</complexity>
</task_context>
`,
	}

	prompt := buildPRDTaskPrompt(task, false)

	requiredSnippets := []string{
		"## Autonomous Execution Posture",
		"Resume from the current workspace state instead of restarting from scratch.",
		"Reproduce first: capture a concrete pre-change signal before modifying production code",
		"Treat any task-authored `Validation`, `Test Plan`, or `Testing` sections",
		"Only stop early for a real blocker such as missing required auth, permissions, secrets",
		"Every explicit `Validation`, `Test Plan`, or `Testing` requirement has been executed",
		"You rely on stale verification output that does not reflect the latest code",
	}

	for _, snippet := range requiredSnippets {
		if !strings.Contains(prompt, snippet) {
			t.Fatalf("expected prompt to include snippet %q", snippet)
		}
	}
}

func TestBuildAfterFinishBlockRespectsAutoCommitFlag(t *testing.T) {
	t.Parallel()
	withAutoCommit := buildAfterFinishBlock("123", true)
	if !strings.Contains(withAutoCommit, "MUST COMMIT") {
		t.Fatalf("expected auto-commit after-finish block to require commit")
	}

	withoutAutoCommit := buildAfterFinishBlock("123", false)
	if strings.Contains(withoutAutoCommit, "MUST COMMIT") {
		t.Fatalf("expected no-auto-commit after-finish block to omit commit requirement")
	}
	if !strings.Contains(withoutAutoCommit, "--auto-commit=false") {
		t.Fatalf("expected no-auto-commit after-finish block to mention --auto-commit=false")
	}
}

func TestConfigValidateRejectsPRDTaskBatching(t *testing.T) {
	t.Parallel()

	cfg := &config{
		mode:      ExecutionModePRDTasks,
		ide:       ideCodex,
		batchSize: 2,
	}

	err := cfg.validate()
	if err == nil {
		t.Fatalf("expected validation error for prd-tasks batch size > 1")
	}
	if !strings.Contains(err.Error(), "batch size must be 1") {
		t.Fatalf("unexpected validation error: %v", err)
	}
}

func TestReadTaskEntriesSortsNumericallyAndFiltersCompleted(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	files := map[string]string{
		"_task_10.md": "## status: pending\n<task_context><domain>x</domain><type>feature</type><scope>s</scope><complexity>low</complexity></task_context>\n",
		"_task_2.md":  "## status: pending\n<task_context><domain>x</domain><type>feature</type><scope>s</scope><complexity>low</complexity></task_context>\n",
		"_task_3.md":  "## status: completed\n<task_context><domain>x</domain><type>feature</type><scope>s</scope><complexity>low</complexity></task_context>\n",
		"notes.md":    "ignored\n",
	}
	for name, content := range files {
		if err := os.WriteFile(filepath.Join(dir, name), []byte(content), 0o600); err != nil {
			t.Fatalf("write %s: %v", name, err)
		}
	}

	entries, err := readTaskEntries(dir, false)
	if err != nil {
		t.Fatalf("readTaskEntries: %v", err)
	}

	gotNames := []string{entries[0].name, entries[1].name}
	wantNames := []string{"_task_2.md", "_task_10.md"}
	if !reflect.DeepEqual(gotNames, wantNames) {
		t.Fatalf("unexpected task order\nwant: %#v\ngot:  %#v", wantNames, gotNames)
	}
	if len(entries) != 2 {
		t.Fatalf("expected completed tasks to be filtered, got %d entries", len(entries))
	}
}

func TestResolveInputsUsesDefaultPRDDirectory(t *testing.T) {
	tmp := t.TempDir()
	t.Chdir(tmp)
	if err := os.MkdirAll(filepath.Join("tasks", "prd-demo"), 0o755); err != nil {
		t.Fatalf("mkdir prd dir: %v", err)
	}

	prValue, inputDir, resolved, err := resolveInputs(&config{
		pr:   "demo",
		mode: ExecutionModePRDTasks,
	})
	if err != nil {
		t.Fatalf("resolveInputs: %v", err)
	}
	if prValue != "demo" {
		t.Fatalf("unexpected pr value: %q", prValue)
	}
	if inputDir != "tasks/prd-demo" {
		t.Fatalf("unexpected input dir: %q", inputDir)
	}
	wantResolved := filepath.Join(tmp, "tasks", "prd-demo")
	if resolved != wantResolved {
		t.Fatalf("unexpected resolved dir\nwant: %q\ngot:  %q", wantResolved, resolved)
	}
}

func TestPrepareJobsForPRDTasksForcesSingleBatchWithoutGroupedSummaries(t *testing.T) {
	t.Parallel()

	promptRoot := t.TempDir()
	issuesDir := t.TempDir()
	groups := map[string][]issueEntry{
		"_task_1": {
			{
				name:     "_task_1.md",
				absPath:  filepath.Join(issuesDir, "_task_1.md"),
				content:  "## status: pending\n<task_context><domain>backend</domain><type>feature</type><scope>small</scope><complexity>low</complexity></task_context>\n",
				codeFile: "_task_1",
			},
		},
		"_task_2": {
			{
				name:     "_task_2.md",
				absPath:  filepath.Join(issuesDir, "_task_2.md"),
				content:  "## status: pending\n<task_context><domain>backend</domain><type>feature</type><scope>small</scope><complexity>low</complexity></task_context>\n",
				codeFile: "_task_2",
			},
		},
	}

	jobs, groupedWritten, err := prepareJobs(
		"demo",
		groups,
		promptRoot,
		issuesDir,
		5,
		true,
		false,
		ExecutionModePRDTasks,
	)
	if err != nil {
		t.Fatalf("prepareJobs: %v", err)
	}
	if groupedWritten {
		t.Fatalf("expected grouped summaries to be disabled in prd mode")
	}
	if len(jobs) != 2 {
		t.Fatalf("expected one batch per task in prd mode, got %d", len(jobs))
	}
	for _, job := range jobs {
		if len(job.codeFiles) != 1 {
			t.Fatalf("expected single-file jobs in prd mode, got %#v", job.codeFiles)
		}
		if _, err := os.Stat(job.outPromptPath); err != nil {
			t.Fatalf("expected prompt artifact to be written: %v", err)
		}
	}
}
