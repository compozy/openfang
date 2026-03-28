package app

import "time"

const (
	unknownFileName               = "unknown"
	ideCodex                      = "codex"
	ideClaude                     = "claude"
	ideDroid                      = "droid"
	ideCursor                     = "cursor-agent"
	defaultCodexModel             = "gpt-5.4"
	defaultClaudeModel            = "opus"
	defaultCursorModel            = "composer-1"
	defaultActivityTimeout        = 10 * time.Minute
	exitCodeTimeout               = -2
	exitCodeCanceled              = -1
	activityCheckInterval         = 5 * time.Second
	processTerminationGracePeriod = 5 * time.Second
	gracefulShutdownTimeout       = 30 * time.Second
	uiMessageDrainDelay           = 80 * time.Millisecond
	uiTickInterval                = 120 * time.Millisecond
	modeCodeReview                = "pr-review"
	modePRDTasks                  = "prd-tasks"
)

type executionMode string

const (
	ExecutionModePRReview executionMode = modeCodeReview
	ExecutionModePRDTasks executionMode = modePRDTasks
)

type config struct {
	pr                     string
	issuesDir              string
	dryRun                 bool
	autoCommit             bool
	concurrent             int
	batchSize              int
	ide                    string
	model                  string
	addDirs                []string
	grouped                bool
	tailLines              int
	reasoningEffort        string
	mode                   executionMode
	includeCompleted       bool
	timeout                time.Duration
	maxRetries             int
	retryBackoffMultiplier float64
}

type cliArgs = config

type issueEntry struct {
	name     string
	absPath  string
	content  string
	codeFile string
}

type taskEntry struct {
	content      string
	status       string
	domain       string
	taskType     string
	scope        string
	complexity   string
	dependencies []string
}

type solvePreparation struct {
	jobs              []job
	issuesDir         string
	resolvedPr        string
	issuesDirPath     string
	groupedSummarized bool
}

type job struct {
	codeFiles     []string
	groups        map[string][]issueEntry
	safeName      string
	prompt        []byte
	outPromptPath string
	outLog        string
	errLog        string
}

type failInfo struct {
	codeFile string
	exitCode int
	outLog   string
	errLog   string
	err      error
}

type jobPhase string

const (
	jobPhaseQueued    jobPhase = "queued"
	jobPhaseScheduled jobPhase = "scheduled"
	jobPhaseRunning   jobPhase = "running"
	jobPhaseRetrying  jobPhase = "retrying"
	jobPhaseSucceeded jobPhase = "succeeded"
	jobPhaseFailed    jobPhase = "failed"
	jobPhaseCanceled  jobPhase = "canceled"
)

type jobAttemptStatus string

const (
	attemptStatusSuccess     jobAttemptStatus = "success"
	attemptStatusFailure     jobAttemptStatus = "failure"
	attemptStatusTimeout     jobAttemptStatus = "timeout"
	attemptStatusCanceled    jobAttemptStatus = "canceled"
	attemptStatusSetupFailed jobAttemptStatus = "setup_failed"
)

type jobAttemptResult struct {
	status   jobAttemptStatus
	exitCode int
	failure  *failInfo
}

func (r jobAttemptResult) Successful() bool {
	return r.status == attemptStatusSuccess
}

func (r jobAttemptResult) NeedsRetry() bool {
	return r.status == attemptStatusFailure || r.status == attemptStatusTimeout
}

func (r jobAttemptResult) IsCanceled() bool {
	return r.status == attemptStatusCanceled
}

type jobState int

const (
	jobPending jobState = iota
	jobRunning
	jobSuccess
	jobFailed
)

const (
	sidebarWidthRatio      = 0.25
	sidebarMinWidth        = 30
	sidebarMaxWidth        = 50
	mainMinWidth           = 60
	minContentHeight       = 10
	sidebarChromeWidth     = 4
	sidebarChromeHeight    = 2
	mainHorizontalPadding  = 2
	logViewportMinHeight   = 6
	sidebarViewportMinRows = 5
	headerSectionHeight    = 3
	helpSectionHeight      = 2
	separatorSectionHeight = 1
	chromeHeight           = headerSectionHeight + helpSectionHeight + separatorSectionHeight
)

type uiJob struct {
	codeFile    string
	codeFiles   []string
	issues      int
	safeName    string
	outLog      string
	errLog      string
	state       jobState
	exitCode    int
	lastOut     []string
	lastErr     []string
	startedAt   time.Time
	completedAt time.Time
	duration    time.Duration
	tokenUsage  *TokenUsage
}

type tickMsg struct{}

type jobQueuedMsg struct {
	Index     int
	CodeFile  string
	CodeFiles []string
	Issues    int
	SafeName  string
	OutLog    string
	ErrLog    string
}

type jobStartedMsg struct{ Index int }

type jobFinishedMsg struct {
	Index    int
	Success  bool
	ExitCode int
}

type jobLogUpdateMsg struct {
	Index int
	Out   []string
	Err   []string
}

type drainMsg struct{}

type tokenUsageUpdateMsg struct {
	Index int
	Usage TokenUsage
}

type jobFailureMsg struct {
	Failure failInfo
}

type TokenUsage struct {
	InputTokens         int
	CacheCreationTokens int
	CacheReadTokens     int
	OutputTokens        int
	Ephemeral5mTokens   int
	Ephemeral1hTokens   int
}

func (u *TokenUsage) Add(other TokenUsage) {
	u.InputTokens += other.InputTokens
	u.CacheCreationTokens += other.CacheCreationTokens
	u.CacheReadTokens += other.CacheReadTokens
	u.OutputTokens += other.OutputTokens
	u.Ephemeral5mTokens += other.Ephemeral5mTokens
	u.Ephemeral1hTokens += other.Ephemeral1hTokens
}

func (u *TokenUsage) Total() int {
	return u.InputTokens + u.OutputTokens
}

type ClaudeMessage struct {
	Type    string `json:"type"`
	Message struct {
		Role    string `json:"role"`
		Content []struct {
			Type    string `json:"type"`
			Text    string `json:"text"`
			Content string `json:"content"`
		} `json:"content"`
		Usage struct {
			InputTokens         int `json:"input_tokens"`
			CacheCreationTokens int `json:"cache_creation_input_tokens"`
			CacheReadTokens     int `json:"cache_read_input_tokens"`
			OutputTokens        int `json:"output_tokens"`
			CacheCreation       struct {
				Ephemeral5mTokens int `json:"ephemeral_5m_input_tokens"`
				Ephemeral1hTokens int `json:"ephemeral_1h_input_tokens"`
			} `json:"cache_creation"`
		} `json:"usage"`
	} `json:"message"`
}

type uiViewState string

const (
	uiViewJobs     uiViewState = "jobs"
	uiViewSummary  uiViewState = "summary"
	uiViewFailures uiViewState = "failures"
)

type uiMsg any
