//! Shell-backed hook implementations.

use std::{collections::BTreeMap, io::ErrorKind, path::PathBuf, time::Duration};

use arky_protocol::Message;
use async_trait::async_trait;
use regex::Regex;
use serde::{de::DeserializeOwned, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
};
use tokio_util::sync::CancellationToken;

use crate::{
    AfterToolCallContext, BeforeToolCallContext, FailureMode, HookError, HookEvent, Hooks,
    PromptSubmitContext, PromptUpdate, SessionEndContext, SessionStartContext, SessionStartUpdate,
    StopContext, StopDecision, ToolResultOverride, Verdict,
};

const DEFAULT_HOOK_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_HOOK_OUTPUT_BYTES: usize = 1_000_000;

#[derive(Debug, Serialize)]
struct ShellHookPayload<'a, T> {
    event: HookEvent,
    input: &'a T,
}

/// Filters which tool names a shell hook applies to.
#[derive(Debug, Clone, Default)]
pub struct ToolMatcher {
    exact_names: Vec<String>,
    patterns: Vec<Regex>,
}

impl ToolMatcher {
    /// Creates an empty matcher that matches all tools.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an exact tool-name match.
    #[must_use]
    pub fn with_tool_name(mut self, tool_name: impl Into<String>) -> Self {
        self.exact_names.push(tool_name.into());
        self
    }

    /// Adds a regex pattern match.
    pub fn with_pattern(mut self, pattern: &str) -> Result<Self, regex::Error> {
        self.patterns.push(Regex::new(pattern)?);
        Ok(self)
    }

    /// Returns whether the matcher applies to a tool name.
    #[must_use]
    pub fn matches(&self, tool_name: &str) -> bool {
        if self.exact_names.is_empty() && self.patterns.is_empty() {
            return true;
        }

        self.exact_names.iter().any(|value| value == tool_name)
            || self
                .patterns
                .iter()
                .any(|pattern| pattern.is_match(tool_name))
    }
}

/// Hook implementation that shells out to an executable.
#[derive(Debug, Clone)]
pub struct ShellCommandHook {
    event: HookEvent,
    command: String,
    args: Vec<String>,
    timeout: Duration,
    matcher: Option<ToolMatcher>,
    env: BTreeMap<String, String>,
    cwd: Option<PathBuf>,
    failure_mode: FailureMode,
}

impl ShellCommandHook {
    /// Creates a shell-command hook for a single lifecycle event.
    #[must_use]
    pub fn new(event: HookEvent, command: impl Into<String>) -> Self {
        Self {
            event,
            command: command.into(),
            args: Vec::new(),
            timeout: DEFAULT_HOOK_TIMEOUT,
            matcher: None,
            env: BTreeMap::new(),
            cwd: None,
            failure_mode: FailureMode::FailOpen,
        }
    }

    /// Adds command-line arguments.
    #[must_use]
    pub fn with_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the hook timeout.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Sets the tool matcher used by tool-related events.
    #[must_use]
    pub fn with_matcher(mut self, matcher: ToolMatcher) -> Self {
        self.matcher = Some(matcher);
        self
    }

    /// Sets the working directory for the command.
    #[must_use]
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Adds an environment variable override.
    #[must_use]
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let _ = self.env.insert(key.into(), value.into());
        self
    }

    /// Sets an explicit failure mode override.
    #[must_use]
    pub const fn with_failure_mode(mut self, failure_mode: FailureMode) -> Self {
        self.failure_mode = failure_mode;
        self
    }

    fn build_command(&self) -> Command {
        let mut command = Command::new(&self.command);
        command
            .args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .envs(
                self.env
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str())),
            );

        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }

        command
    }

    fn matches_tool(&self, tool_name: &str) -> bool {
        self.matcher
            .as_ref()
            .is_none_or(|matcher| matcher.matches(tool_name))
    }

    fn execution_error(&self, event: HookEvent, message: impl Into<String>) -> HookError {
        HookError::execution_failed(message, Some(event), Some(self.command.clone()))
    }

    async fn write_payload(&self, child: &mut Child, payload: &[u8]) -> Option<std::io::Error> {
        let mut stdin = child.stdin.take()?;

        if let Err(error) = stdin.write_all(payload).await {
            return Some(error);
        }

        stdin.shutdown().await.err()
    }

    async fn wait_for_exit(
        &self,
        child: &mut Child,
        event: HookEvent,
        cancel: CancellationToken,
    ) -> Result<std::process::ExitStatus, HookError> {
        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                Err(self.execution_error(event, "hook execution cancelled"))
            }
            () = tokio::time::sleep(self.timeout) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                Err(HookError::timeout(
                    "hook execution timed out",
                    Some(event),
                    Some(self.command.clone()),
                    Some(self.timeout),
                ))
            }
            result = child.wait() => result.map_err(|error| {
                self.execution_error(event, format!("failed to wait for hook command: {error}"))
            }),
        }
    }

    fn failure_message(
        stdin_error: Option<&std::io::Error>,
        stderr_text: &str,
        exit_status: std::process::ExitStatus,
    ) -> String {
        if let Some(error) = stdin_error {
            return format_stdin_error(error, stderr_text);
        }

        if stderr_text.is_empty() {
            format!("hook command exited with status {exit_status}")
        } else {
            stderr_text.to_owned()
        }
    }

    fn parse_stdout<R>(
        stdout: &[u8],
        stdout_text: String,
        plain_text: impl Fn(String) -> R,
    ) -> Option<R>
    where
        R: DeserializeOwned,
    {
        if stdout_text.is_empty() {
            return None;
        }

        if let Ok(result) = serde_json::from_slice::<R>(stdout) {
            return Some(result);
        }

        Some(plain_text(stdout_text))
    }

    async fn execute_for_event<T, R>(
        &self,
        event: HookEvent,
        input: &T,
        cancel: CancellationToken,
        plain_text: impl Fn(String) -> R,
    ) -> Result<Option<R>, HookError>
    where
        T: Serialize + Sync,
        R: DeserializeOwned,
    {
        let payload = serde_json::to_vec(&ShellHookPayload { event, input }).map_err(|error| {
            self.execution_error(
                event,
                format!("failed to serialize shell hook payload: {error}"),
            )
        })?;

        let mut child = self.build_command().spawn().map_err(|error| {
            self.execution_error(event, format!("failed to spawn hook command: {error}"))
        })?;

        let stdin_error = self.write_payload(&mut child, &payload).await;

        let stdout_task = child.stdout.take().map(|mut stdout| {
            tokio::spawn(async move {
                let mut buffer = Vec::new();
                stdout.read_to_end(&mut buffer).await.map(|_| buffer)
            })
        });
        let stderr_task = child.stderr.take().map(|mut stderr| {
            tokio::spawn(async move {
                let mut buffer = Vec::new();
                stderr.read_to_end(&mut buffer).await.map(|_| buffer)
            })
        });

        let exit_status = self.wait_for_exit(&mut child, event, cancel).await?;

        let stdout = read_joined_output(stdout_task, event, &self.command).await?;
        let stderr = read_joined_output(stderr_task, event, &self.command).await?;
        let stdout_text = trim_output(&stdout);
        let stderr_text = trim_output(&stderr);

        if !exit_status.success() {
            return Err(self.execution_error(
                event,
                Self::failure_message(stdin_error.as_ref(), &stderr_text, exit_status),
            ));
        }

        if let Some(error) = stdin_error {
            if !is_recoverable_stdin_error(&error) {
                return Err(self.execution_error(event, format_stdin_error(&error, &stderr_text)));
            }
        }

        Ok(Self::parse_stdout(&stdout, stdout_text, plain_text))
    }
}

#[async_trait]
impl Hooks for ShellCommandHook {
    fn failure_mode(&self) -> Option<FailureMode> {
        Some(self.failure_mode)
    }

    fn timeout(&self) -> Option<Duration> {
        Some(self.timeout)
    }

    async fn before_tool_call(
        &self,
        ctx: &BeforeToolCallContext,
        cancel: CancellationToken,
    ) -> Result<Verdict, HookError> {
        if self.event != HookEvent::BeforeToolCall || !self.matches_tool(&ctx.tool_call.name) {
            return Ok(Verdict::Allow);
        }

        self.execute_for_event(self.event, ctx, cancel, Verdict::block)
            .await
            .map(|result| result.unwrap_or(Verdict::Allow))
    }

    async fn after_tool_call(
        &self,
        ctx: &AfterToolCallContext,
        cancel: CancellationToken,
    ) -> Result<Option<ToolResultOverride>, HookError> {
        if self.event != HookEvent::AfterToolCall || !self.matches_tool(&ctx.tool_call.name) {
            return Ok(None);
        }

        self.execute_for_event(self.event, ctx, cancel, ToolResultOverride::from_text)
            .await
    }

    async fn session_start(
        &self,
        ctx: &SessionStartContext,
        cancel: CancellationToken,
    ) -> Result<Option<SessionStartUpdate>, HookError> {
        if self.event != HookEvent::SessionStart {
            return Ok(None);
        }

        self.execute_for_event(self.event, ctx, cancel, |text| {
            SessionStartUpdate::new().with_messages(vec![Message::system(text)])
        })
        .await
    }

    async fn session_end(&self, ctx: &SessionEndContext) -> Result<(), HookError> {
        if self.event != HookEvent::SessionEnd {
            return Ok(());
        }

        let _ = self
            .execute_for_event(self.event, ctx, CancellationToken::new(), |_text| ())
            .await?;
        Ok(())
    }

    async fn on_stop(
        &self,
        ctx: &StopContext,
        cancel: CancellationToken,
    ) -> Result<StopDecision, HookError> {
        if self.event != HookEvent::OnStop {
            return Ok(StopDecision::Stop);
        }

        self.execute_for_event(self.event, ctx, cancel, StopDecision::continue_with)
            .await
            .map(|result| result.unwrap_or(StopDecision::Stop))
    }

    async fn user_prompt_submit(
        &self,
        ctx: &PromptSubmitContext,
        cancel: CancellationToken,
    ) -> Result<Option<PromptUpdate>, HookError> {
        if self.event != HookEvent::UserPromptSubmit {
            return Ok(None);
        }

        self.execute_for_event(self.event, ctx, cancel, |text| {
            PromptUpdate::new().with_messages(vec![Message::system(text)])
        })
        .await
    }
}

async fn read_joined_output(
    task: Option<tokio::task::JoinHandle<std::io::Result<Vec<u8>>>>,
    event: HookEvent,
    command: &str,
) -> Result<Vec<u8>, HookError> {
    let Some(task) = task else {
        return Ok(Vec::new());
    };

    let bytes = task.await.map_err(|error| {
        HookError::execution_failed(
            format!("failed to join hook output reader: {error}"),
            Some(event),
            Some(command.to_owned()),
        )
    })?;

    bytes.map_err(|error| {
        HookError::execution_failed(
            format!("failed to read hook output: {error}"),
            Some(event),
            Some(command.to_owned()),
        )
    })
}

fn trim_output(output: &[u8]) -> String {
    let mut output = output.to_vec();
    if output.len() > MAX_HOOK_OUTPUT_BYTES {
        let keep_from = output.len() - MAX_HOOK_OUTPUT_BYTES;
        output.drain(..keep_from);
    }
    String::from_utf8_lossy(&output).trim().to_owned()
}

fn is_recoverable_stdin_error(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::BrokenPipe)
}

fn format_stdin_error(error: &std::io::Error, stderr_text: &str) -> String {
    if !stderr_text.is_empty() {
        return stderr_text.to_owned();
    }

    let code = error
        .raw_os_error()
        .into_iter()
        .map(|value| format!(" ({value})"))
        .next()
        .unwrap_or_default();
    if is_recoverable_stdin_error(error) {
        format!("hook command closed stdin before payload was fully written{code}")
    } else {
        format!("hook command stdin write failed{code}")
    }
}
