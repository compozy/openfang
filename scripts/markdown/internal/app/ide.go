package app

import (
	"bytes"
	"context"
	"embed"
	"fmt"
	"os"
	"os/exec"
	"strconv"
	"strings"
	"sync"
)

//go:embed prompts/*.txt
var promptTemplatesFS embed.FS

type ideSpec struct {
	id               string
	displayName      string
	defaultModel     string
	supportsAddDirs  bool
	formatsJSON      bool
	shellPreviewFunc func(model string, addDirs []string, reasoning string) string
	commandFunc      func(ctx context.Context, model string, addDirs []string, reasoning string) *exec.Cmd
}

var ideSpecs = map[string]ideSpec{
	ideCodex: {
		id:               ideCodex,
		displayName:      "Codex",
		defaultModel:     defaultCodexModel,
		supportsAddDirs:  true,
		formatsJSON:      false,
		shellPreviewFunc: buildCodexCommand,
		commandFunc:      codexCommand,
	},
	ideClaude: {
		id:               ideClaude,
		displayName:      "Claude",
		defaultModel:     defaultClaudeModel,
		supportsAddDirs:  true,
		formatsJSON:      true,
		shellPreviewFunc: buildClaudeCommand,
		commandFunc:      claudeCommand,
	},
	ideDroid: {
		id:              ideDroid,
		displayName:     "Droid",
		defaultModel:    defaultCodexModel,
		supportsAddDirs: false,
		formatsJSON:     true,
		shellPreviewFunc: func(model string, _ []string, reasoning string) string {
			return buildDroidCommand(model, reasoning)
		},
		commandFunc: func(ctx context.Context, model string, _ []string, reasoning string) *exec.Cmd {
			return droidCommand(ctx, model, reasoning)
		},
	},
	ideCursor: {
		id:              ideCursor,
		displayName:     "Cursor",
		defaultModel:    defaultCursorModel,
		supportsAddDirs: false,
		formatsJSON:     true,
		shellPreviewFunc: func(model string, _ []string, reasoning string) string {
			return buildCursorCommand(model, reasoning)
		},
		commandFunc: func(ctx context.Context, model string, _ []string, reasoning string) *exec.Cmd {
			return cursorCommand(ctx, model, reasoning)
		},
	},
}

func (cfg *config) validate() error {
	if cfg.mode != ExecutionModePRReview && cfg.mode != ExecutionModePRDTasks {
		return fmt.Errorf(
			"invalid --mode value %q: must be %q or %q",
			cfg.mode,
			modeCodeReview,
			modePRDTasks,
		)
	}
	if _, ok := ideSpecs[cfg.ide]; !ok {
		return fmt.Errorf(
			"invalid --ide value %q: must be %q, %q, %q, or %q",
			cfg.ide,
			ideClaude,
			ideCodex,
			ideDroid,
			ideCursor,
		)
	}
	if cfg.mode == ExecutionModePRDTasks && cfg.batchSize != 1 {
		return fmt.Errorf("batch size must be 1 for prd-tasks mode (got %d)", cfg.batchSize)
	}
	if cfg.maxRetries < 0 {
		return fmt.Errorf("max-retries cannot be negative (got %d)", cfg.maxRetries)
	}
	if cfg.retryBackoffMultiplier <= 0 {
		return fmt.Errorf("retry-backoff-multiplier must be positive (got %.2f)", cfg.retryBackoffMultiplier)
	}
	return nil
}

func notifyJobStart(
	useUI bool,
	uiCh chan uiMsg,
	index int,
	j *job,
	ide string,
	model string,
	addDirs []string,
	reasoningEffort string,
) {
	if useUI {
		uiCh <- jobStartedMsg{Index: index}
		return
	}

	shellCmdStr := buildShellCommandString(ide, model, addDirs, reasoningEffort)
	ideName := getIDEName(ide)
	totalIssues := countTotalIssues(j)
	codeFileLabel := formatCodeFileLabel(j.codeFiles)
	fmt.Printf(
		"\n=== Running %s (non-interactive) for batch: %s (%d issues)\n$ %s\n",
		ideName,
		codeFileLabel,
		totalIssues,
		shellCmdStr,
	)
}

func buildShellCommandString(ide string, model string, addDirs []string, reasoningEffort string) string {
	spec, ok := ideSpecs[ide]
	if !ok {
		return ""
	}
	dirs := addDirs
	if !spec.supportsAddDirs {
		dirs = nil
	}
	return spec.shellPreviewFunc(model, dirs, reasoningEffort)
}

func buildCodexCommand(model string, addDirs []string, reasoningEffort string) string {
	modelToUse := defaultCodexModel
	if model != "" && model != defaultCodexModel {
		modelToUse = model
	}
	args := []string{
		ideCodex,
		"--dangerously-bypass-approvals-and-sandbox",
		"-m", modelToUse,
		"-c", fmt.Sprintf("model_reasoning_effort=%s", reasoningEffort),
	}
	args = appendRepeatedFlag(args, "--add-dir", addDirs)
	args = append(args, "exec", "--json", "-")
	return formatShellCommand(args)
}

func buildClaudeCommand(model string, addDirs []string, reasoningEffort string) string {
	thinkPrompt := getThinkPrompt(reasoningEffort)
	modelToUse := defaultClaudeModel
	if model != "" && model != defaultClaudeModel {
		modelToUse = model
	}
	args := []string{
		ideClaude,
		"--print",
		"--output-format", "stream-json",
		"--verbose",
		"--model", modelToUse,
	}
	args = appendRepeatedFlag(args, "--add-dir", addDirs)
	args = append(
		args,
		"--dangerously-skip-permissions",
		"--permission-mode", "bypassPermissions",
		"--append-system-prompt", thinkPrompt,
	)
	return "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1 " + formatShellCommand(args)
}

func buildDroidCommand(model string, reasoningEffort string) string {
	base := fmt.Sprintf(
		"droid exec --skip-permissions-unsafe --reasoning-effort %s --output-format stream-json",
		reasoningEffort,
	)
	if model != "" && model != defaultCodexModel {
		return fmt.Sprintf("%s --model %s", base, model)
	}
	if model == defaultCodexModel {
		return fmt.Sprintf("%s --model %s", base, defaultCodexModel)
	}
	return base
}

func buildCursorCommand(model string, _ string) string {
	modelToUse := defaultCursorModel
	if model != "" && model != defaultCursorModel {
		modelToUse = model
	}
	return fmt.Sprintf(
		"cursor-agent --print --output-format stream-json --model %s",
		modelToUse,
	)
}

func getThinkPrompt(reasoningEffort string) string {
	return reasoningPromptForEffort(reasoningEffort)
}

func getIDEName(ide string) string {
	if spec, ok := ideSpecs[ide]; ok {
		return spec.displayName
	}
	return ""
}

func countTotalIssues(j *job) int {
	total := 0
	for _, items := range j.groups {
		total += len(items)
	}
	return total
}

func formatCodeFileLabel(codeFiles []string) string {
	label := strings.Join(codeFiles, ", ")
	if len(codeFiles) > 1 {
		return fmt.Sprintf("%d files: %s", len(codeFiles), label)
	}
	return label
}

func createIDECommand(ctx context.Context, cfg *config) *exec.Cmd {
	spec, ok := ideSpecs[cfg.ide]
	if !ok {
		return nil
	}
	modelToUse := cfg.model
	if modelToUse == "" {
		modelToUse = spec.defaultModel
	}
	dirs := cfg.addDirs
	if !spec.supportsAddDirs {
		dirs = nil
	}
	return spec.commandFunc(ctx, modelToUse, dirs, cfg.reasoningEffort)
}

func appendRepeatedFlag(args []string, flag string, values []string) []string {
	for _, value := range normalizeAddDirs(values) {
		args = append(args, flag, value)
	}
	return args
}

func formatShellCommand(args []string) string {
	formatted := make([]string, len(args))
	for i, arg := range args {
		formatted[i] = formatShellArg(arg)
	}
	return strings.Join(formatted, " ")
}

func formatShellArg(arg string) string {
	if arg == "" {
		return `""`
	}
	if strings.ContainsAny(arg, " \t\n\"'\\$`|&;<>*?[]{}()") {
		return strconv.Quote(arg)
	}
	return arg
}

func claudePromptForEffort(reasoning string) string {
	return reasoningPromptForEffort(reasoning)
}

func mustReadPromptTemplate(name string) string {
	content, err := promptTemplatesFS.ReadFile("prompts/" + name)
	if err != nil {
		return ""
	}
	return string(content)
}

func renderPromptTemplate(name string, replacements map[string]string) string {
	content := mustReadPromptTemplate(name)
	if len(replacements) == 0 {
		return content
	}

	replacerArgs := make([]string, 0, len(replacements)*2)
	for key, value := range replacements {
		replacerArgs = append(replacerArgs, "{{"+key+"}}", value)
	}
	return strings.NewReplacer(replacerArgs...).Replace(content)
}

func reasoningPromptForEffort(reasoning string) string {
	switch reasoning {
	case "low":
		return mustReadPromptTemplate("claude-reasoning-low.txt")
	case "high":
		return mustReadPromptTemplate("claude-reasoning-high.txt")
	case "xhigh":
		return mustReadPromptTemplate("claude-reasoning-xhigh.txt")
	default:
		return mustReadPromptTemplate("claude-reasoning-medium.txt")
	}
}

func codexCommand(ctx context.Context, model string, addDirs []string, reasoning string) *exec.Cmd {
	args := []string{"--dangerously-bypass-approvals-and-sandbox"}
	if model != "" {
		args = append(args, "-m", model)
	}
	args = append(args, "-c", fmt.Sprintf("model_reasoning_effort=%s", reasoning))
	args = appendRepeatedFlag(args, "--add-dir", addDirs)
	args = append(args, "exec", "--json", "-")
	return exec.CommandContext(ctx, ideCodex, args...)
}

func claudeCommand(ctx context.Context, model string, addDirs []string, reasoning string) *exec.Cmd {
	prompt := claudePromptForEffort(reasoning)
	systemPrompt := prompt + "\n\n<critical>YOU SHOULD use a team of agents to handle properly the job and avoid do workaround to get it done</critical>"
	args := []string{
		"--print",
		"--output-format", "stream-json",
		"--verbose",
		"--model", model,
	}
	args = appendRepeatedFlag(args, "--add-dir", addDirs)
	args = append(
		args,
		"--permission-mode", "bypassPermissions",
		"--dangerously-skip-permissions",
		"--append-system-prompt", systemPrompt,
	)
	return exec.CommandContext(ctx, ideClaude, args...)
}

func droidCommand(ctx context.Context, model, reasoning string) *exec.Cmd {
	droidArgs := []string{
		"exec",
		"--skip-permissions-unsafe",
		"--reasoning-effort", reasoning,
		"--output-format", "stream-json",
	}
	if model != "" {
		droidArgs = append(droidArgs, "--model", model)
	}
	return exec.CommandContext(ctx, ideDroid, droidArgs...)
}

func cursorCommand(ctx context.Context, model, _ string) *exec.Cmd {
	cursorArgs := []string{
		"--print",
		"--output-format", "stream-json",
	}
	if model != "" {
		cursorArgs = append(cursorArgs, "--model", model)
	} else {
		cursorArgs = append(cursorArgs, "--model", defaultCursorModel)
	}
	return exec.CommandContext(ctx, ideCursor, cursorArgs...)
}

func setupCommandIO(
	cmd *exec.Cmd,
	j *job,
	cwd string,
	useUI bool,
	uiCh chan uiMsg,
	index int,
	tailLines int,
	ideType string,
	aggregateUsage *TokenUsage,
	aggregateMu *sync.Mutex,
) (*os.File, *os.File, *activityMonitor, error) {
	configureCommandEnvironment(cmd, cwd, j.prompt, ideType)
	outF, err := createLogFile(j.outLog)
	if err != nil {
		return nil, nil, nil, fmt.Errorf("create out log: %w", err)
	}
	errF, err := createLogFile(j.errLog)
	if err != nil {
		outF.Close()
		return nil, nil, nil, fmt.Errorf("create err log: %w", err)
	}

	monitor := newActivityMonitor()
	outTap, errTap := buildCommandTaps(
		outF,
		errF,
		tailLines,
		useUI,
		uiCh,
		index,
		ideType,
		aggregateUsage,
		aggregateMu,
		monitor,
	)
	cmd.Stdout = outTap
	cmd.Stderr = errTap
	return outF, errF, monitor, nil
}

func configureCommandEnvironment(cmd *exec.Cmd, cwd string, prompt []byte, ideType string) {
	cmd.Dir = cwd
	cmd.Stdin = bytes.NewReader(prompt)
	cmd.Env = append(os.Environ(),
		"FORCE_COLOR=1",
		"CLICOLOR_FORCE=1",
		"TERM=xterm-256color",
	)
	if ideType == ideClaude {
		cmd.Env = append(cmd.Env, "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1")
	}
}
