//! Shared subprocess lifecycle management.

use std::{collections::BTreeMap, path::PathBuf, process::Stdio, time::Duration};

use tokio::{
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command},
    time::{sleep, timeout},
};

use crate::ProviderError;

/// Restart policy applied by [`ProcessManager`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RestartPolicy {
    /// Never restart automatically.
    #[default]
    Never,
    /// Allow up to `max_restarts` restarts, waiting `backoff` before respawn.
    Limited {
        /// Maximum number of restarts permitted.
        max_restarts: u32,
        /// Delay before each respawn attempt.
        backoff: Duration,
    },
}

/// Subprocess spawn configuration.
#[derive(Debug, Clone)]
pub struct ProcessConfig {
    /// Executable path or binary name.
    pub program: String,
    /// Command-line arguments.
    pub args: Vec<String>,
    /// Optional working directory.
    pub cwd: Option<PathBuf>,
    /// Environment overrides.
    pub env: BTreeMap<String, String>,
    /// Whether to clear inherited environment variables before applying `env`.
    pub clear_env: bool,
    /// Maximum time allowed for graceful shutdown before kill fallback.
    pub shutdown_timeout: Duration,
    /// Whether a live child should be killed from `Drop`.
    pub kill_on_drop: bool,
}

impl ProcessConfig {
    /// Creates a process configuration for a program.
    #[must_use]
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
            clear_env: false,
            shutdown_timeout: Duration::from_secs(5),
            kill_on_drop: true,
        }
    }

    /// Replaces command-line arguments.
    #[must_use]
    pub fn with_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the working directory.
    #[must_use]
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Adds an environment override.
    #[must_use]
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Controls whether inherited environment variables are cleared.
    #[must_use]
    pub const fn with_clear_env(mut self, clear_env: bool) -> Self {
        self.clear_env = clear_env;
        self
    }

    /// Sets the graceful shutdown timeout.
    #[must_use]
    pub const fn with_shutdown_timeout(mut self, shutdown_timeout: Duration) -> Self {
        self.shutdown_timeout = shutdown_timeout;
        self
    }

    /// Controls kill-on-drop behavior.
    #[must_use]
    pub const fn with_kill_on_drop(mut self, kill_on_drop: bool) -> Self {
        self.kill_on_drop = kill_on_drop;
        self
    }

    fn display_command(&self) -> String {
        if self.args.is_empty() {
            return self.program.clone();
        }

        format!("{} {}", self.program, self.args.join(" "))
    }
}

/// Process manager used by provider implementations.
#[derive(Debug, Clone)]
pub struct ProcessManager {
    config: ProcessConfig,
    restart_policy: RestartPolicy,
}

impl ProcessManager {
    /// Creates a new process manager.
    #[must_use]
    pub const fn new(config: ProcessConfig) -> Self {
        Self {
            config,
            restart_policy: RestartPolicy::Never,
        }
    }

    /// Sets the restart policy.
    #[must_use]
    pub const fn with_restart_policy(mut self, restart_policy: RestartPolicy) -> Self {
        self.restart_policy = restart_policy;
        self
    }

    /// Returns the process configuration.
    #[must_use]
    pub const fn config(&self) -> &ProcessConfig {
        &self.config
    }

    /// Spawns a managed subprocess with piped stdio.
    pub fn spawn(&self) -> Result<ManagedProcess, ProviderError> {
        self.spawn_with_restart_count(0)
    }

    /// Restarts a managed subprocess if permitted by the restart policy.
    pub async fn restart(&self, process: &mut ManagedProcess) -> Result<(), ProviderError> {
        let next_restart_count = process.restart_count.saturating_add(1);

        match self.restart_policy {
            RestartPolicy::Never => {
                return Err(ProviderError::protocol_violation(
                    "restart was requested but the process manager is configured with RestartPolicy::Never",
                    None,
                ));
            }
            RestartPolicy::Limited { max_restarts, .. } if next_restart_count > max_restarts => {
                return Err(ProviderError::protocol_violation(
                    format!(
                        "restart budget exceeded for `{}`",
                        self.config.display_command()
                    ),
                    None,
                ));
            }
            RestartPolicy::Limited { backoff, .. } => {
                process.graceful_shutdown().await?;
                if !backoff.is_zero() {
                    sleep(backoff).await;
                }
            }
        }

        *process = self.spawn_with_restart_count(next_restart_count)?;
        Ok(())
    }

    fn spawn_with_restart_count(
        &self,
        restart_count: u32,
    ) -> Result<ManagedProcess, ProviderError> {
        let mut command = Command::new(&self.config.program);
        command.args(&self.config.args);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        if let Some(cwd) = &self.config.cwd {
            command.current_dir(cwd);
        }
        if self.config.clear_env {
            command.env_clear();
        }
        if !self.config.env.is_empty() {
            command.envs(self.config.env.clone());
        }

        let child = command.spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ProviderError::binary_not_found(self.config.program.clone())
            } else {
                ProviderError::process_crashed(
                    self.config.display_command(),
                    None,
                    Some(error.to_string()),
                )
            }
        })?;

        Ok(ManagedProcess {
            config: self.config.clone(),
            child: Some(child),
            restart_count,
        })
    }
}

/// Spawned subprocess with kill-on-drop semantics.
pub struct ManagedProcess {
    config: ProcessConfig,
    child: Option<Child>,
    restart_count: u32,
}

impl ManagedProcess {
    /// Returns the operating-system process identifier when available.
    #[must_use]
    pub fn id(&self) -> Option<u32> {
        self.child.as_ref().and_then(Child::id)
    }

    /// Returns how many times the process has been restarted.
    #[must_use]
    pub const fn restart_count(&self) -> u32 {
        self.restart_count
    }

    /// Takes the child stdin handle.
    pub fn take_stdin(&mut self) -> Result<ChildStdin, ProviderError> {
        self.child_mut()?
            .stdin
            .take()
            .ok_or_else(|| ProviderError::protocol_violation("child stdin is not available", None))
    }

    /// Takes the child stdout handle.
    pub fn take_stdout(&mut self) -> Result<ChildStdout, ProviderError> {
        self.child_mut()?
            .stdout
            .take()
            .ok_or_else(|| ProviderError::protocol_violation("child stdout is not available", None))
    }

    /// Takes the child stderr handle.
    pub fn take_stderr(&mut self) -> Result<ChildStderr, ProviderError> {
        self.child_mut()?
            .stderr
            .take()
            .ok_or_else(|| ProviderError::protocol_violation("child stderr is not available", None))
    }

    /// Waits for the process to exit.
    pub async fn wait(&mut self) -> Result<std::process::ExitStatus, ProviderError> {
        let status = self.child_mut()?.wait().await.map_err(|error| {
            ProviderError::process_crashed(
                self.config.display_command(),
                None,
                Some(error.to_string()),
            )
        })?;

        if status.success() {
            Ok(status)
        } else {
            Err(ProviderError::process_crashed(
                self.config.display_command(),
                status.code(),
                None,
            ))
        }
    }

    /// Returns whether the process is still alive.
    pub fn is_running(&mut self) -> Result<bool, ProviderError> {
        let child = self.child_mut()?;
        child
            .try_wait()
            .map(|status| status.is_none())
            .map_err(|error| {
                ProviderError::process_crashed(
                    self.config.display_command(),
                    None,
                    Some(error.to_string()),
                )
            })
    }

    /// Closes stdin, waits for exit, and falls back to kill when needed.
    pub async fn graceful_shutdown(&mut self) -> Result<(), ProviderError> {
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };

        let _ = child.stdin.take();

        let wait_result = timeout(self.config.shutdown_timeout, child.wait()).await;
        match wait_result {
            Ok(Ok(_status)) => Ok(()),
            Ok(Err(error)) => Err(ProviderError::process_crashed(
                self.config.display_command(),
                None,
                Some(error.to_string()),
            )),
            Err(_) => {
                child.start_kill().map_err(|error| {
                    ProviderError::process_crashed(
                        self.config.display_command(),
                        None,
                        Some(error.to_string()),
                    )
                })?;
                let _status = child.wait().await.map_err(|error| {
                    ProviderError::process_crashed(
                        self.config.display_command(),
                        None,
                        Some(error.to_string()),
                    )
                })?;
                Ok(())
            }
        }
    }

    fn child_mut(&mut self) -> Result<&mut Child, ProviderError> {
        self.child.as_mut().ok_or_else(|| {
            ProviderError::protocol_violation(
                "process has already been consumed or shut down",
                None,
            )
        })
    }
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        if !self.config.kill_on_drop {
            return;
        }

        let Some(mut child) = self.child.take() else {
            return;
        };

        let _ = child.start_kill();

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = child.wait().await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pretty_assertions::assert_eq;

    use super::{ProcessConfig, ProcessManager, RestartPolicy};

    #[tokio::test]
    async fn process_manager_should_spawn_wait_and_restart_processes() {
        let manager =
            ProcessManager::new(ProcessConfig::new("sh").with_args(["-c", "printf first"]))
                .with_restart_policy(RestartPolicy::Limited {
                    max_restarts: 1,
                    backoff: Duration::from_millis(1),
                });
        let mut process = manager.spawn().expect("process should spawn");
        let mut stdout = process.take_stdout().expect("stdout should be available");
        let mut buffer = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut stdout, &mut buffer)
            .await
            .expect("stdout should read");
        process.wait().await.expect("process should exit cleanly");

        assert_eq!(buffer, "first");

        manager
            .restart(&mut process)
            .await
            .expect("restart should succeed");
        assert_eq!(process.restart_count(), 1);
    }

    #[tokio::test]
    async fn process_manager_should_gracefully_shutdown_stdin_bound_processes() {
        let manager = ProcessManager::new(
            ProcessConfig::new("sh")
                .with_args(["-c", "cat >/dev/null"])
                .with_shutdown_timeout(Duration::from_millis(100)),
        );
        let mut process = manager.spawn().expect("process should spawn");

        process
            .graceful_shutdown()
            .await
            .expect("shutdown should succeed");
    }
}
