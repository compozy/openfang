//! Shared Codex app-server lifecycle management.

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use arky_provider::{ManagedProcess, ProcessConfig, ProcessManager, ProviderError};
use serde_json::{Map, Value, json};
use tokio::{io::AsyncReadExt, sync::Mutex};

use crate::{
    ApprovalHandler, CodexModelDescriptor, CodexModelService, CodexProviderConfig,
    CompactThreadParams, NotificationRouter, RpcTransport, RpcTransportConfig, ThreadManager,
    ThreadOpenParams, TurnNotificationStream, TurnStartParams,
    rpc::{InitializeCapabilities, InitializeParams},
};

#[derive(Debug, Clone)]
struct ResolvedCommand {
    program: String,
    prefix_args: Vec<String>,
}

impl ResolvedCommand {
    fn args_with(&self, extra: &[String]) -> Vec<String> {
        let mut args = self.prefix_args.clone();
        args.extend(extra.iter().cloned());
        args
    }
}

/// Long-lived Codex app-server process.
pub struct CodexAppServer {
    config: CodexProviderConfig,
    command_label: String,
    rpc: Arc<RpcTransport>,
    router: NotificationRouter,
    threads: ThreadManager<RpcTransport>,
    model_service: CodexModelService<RpcTransport>,
    process: Mutex<Option<ManagedProcess>>,
    stderr_task: Mutex<Option<tokio::task::JoinHandle<String>>>,
    notification_worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
    approval_worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
    shutdown_started: AtomicBool,
}

impl std::fmt::Debug for CodexAppServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexAppServer")
            .field("config", &self.config)
            .field("command_label", &self.command_label)
            .finish_non_exhaustive()
    }
}

impl CodexAppServer {
    /// Spawns and initializes one Codex app-server process.
    pub async fn spawn(config: CodexProviderConfig) -> Result<Self, ProviderError> {
        let resolved = ensure_binary_validated(&config).await?;
        let manager = ProcessManager::new(app_server_process_config(&config, &resolved));
        let mut process = manager.spawn()?;
        let stdout = process.take_stdout()?;
        let stdin = process.take_stdin()?;
        let stderr = process.take_stderr()?;
        let stderr_task = tokio::spawn(async move {
            let mut stderr = stderr;
            let mut buffer = String::new();
            let _ = stderr.read_to_string(&mut buffer).await;
            buffer
        });
        let rpc = Arc::new(RpcTransport::new(
            stdout,
            stdin,
            RpcTransportConfig {
                request_timeout: config.request_timeout,
                ..RpcTransportConfig::default()
            },
        ));

        if let Err(error) = rpc.initialize(initialize_params(&config)).await {
            let mut process = process;
            let _ = process.graceful_shutdown().await;
            let _ = stderr_task.await;
            return Err(error);
        }

        let router = NotificationRouter::new();
        let notifications = rpc.take_notifications().ok_or_else(|| {
            ProviderError::protocol_violation(
                "rpc notification receiver has already been taken",
                None,
            )
        })?;
        let server_requests = rpc.take_server_requests().ok_or_else(|| {
            ProviderError::protocol_violation(
                "rpc server-request receiver has already been taken",
                None,
            )
        })?;
        let approval_handler = ApprovalHandler::new(config.approval_mode.clone());
        let notification_worker = spawn_notification_worker(router.clone(), notifications);
        let approval_worker = spawn_approval_worker(
            router.clone(),
            approval_handler,
            rpc.clone(),
            server_requests,
        );
        let threads = ThreadManager::new(rpc.clone(), router.clone());
        let model_service = CodexModelService::new(rpc.clone());
        let command_label = app_server_command_label(&config, &resolved);

        Ok(Self {
            config,
            command_label,
            rpc,
            router,
            threads,
            model_service,
            process: Mutex::new(Some(process)),
            stderr_task: Mutex::new(Some(stderr_task)),
            notification_worker: Mutex::new(Some(notification_worker)),
            approval_worker: Mutex::new(Some(approval_worker)),
            shutdown_started: AtomicBool::new(false),
        })
    }

    /// Starts or resumes a thread and opens one turn stream.
    pub async fn open_turn(
        &self,
        thread_id: Option<&str>,
        model: String,
        config_overrides: Option<Map<String, Value>>,
        turn_params: TurnStartParams,
    ) -> Result<(String, TurnNotificationStream), ProviderError> {
        let thread_id = if let Some(thread_id) = thread_id {
            self.threads
                .resume_thread(
                    thread_id,
                    ThreadOpenParams {
                        model: Some(model.clone()),
                        config_overrides: config_overrides.clone(),
                    },
                )
                .await?
                .thread_id
        } else {
            self.threads
                .start_thread(ThreadOpenParams {
                    model: Some(model),
                    config_overrides,
                })
                .await?
                .thread_id
        };

        let turn_stream = self.threads.start_turn(&thread_id, turn_params).await?;
        Ok((thread_id, turn_stream))
    }

    /// Requests history compaction for an existing thread.
    pub async fn compact_thread(
        &self,
        thread_id: &str,
        params: CompactThreadParams,
    ) -> Result<(), ProviderError> {
        self.threads.compact_thread(thread_id, params).await
    }

    /// Lists every available model from the Codex app-server.
    pub async fn list_models(&self) -> Result<Vec<CodexModelDescriptor>, ProviderError> {
        self.model_service.list_all_models().await
    }

    /// Returns the operating-system process identifier when available.
    pub async fn process_id(&self) -> Option<u32> {
        self.process
            .lock()
            .await
            .as_ref()
            .and_then(ManagedProcess::id)
    }

    /// Returns whether the underlying process and transport are still healthy.
    pub async fn is_alive(&self) -> bool {
        if self.rpc.fatal_error().is_some() {
            return false;
        }

        self.process
            .lock()
            .await
            .as_mut()
            .is_some_and(|process| process.is_running().unwrap_or(false))
    }

    /// Maps a transport error into a richer process-crash error when possible.
    pub async fn map_stream_error(
        &self,
        error: ProviderError,
    ) -> Result<ProviderError, ProviderError> {
        if matches!(error, ProviderError::StreamInterrupted { .. }) && !self.is_alive().await {
            let stderr_excerpt = self.collect_stderr().await;
            return Ok(ProviderError::process_crashed(
                self.command_label.clone(),
                None,
                (!stderr_excerpt.trim().is_empty()).then_some(stderr_excerpt.trim().to_owned()),
            ));
        }

        Ok(error)
    }

    /// Shuts the app-server down gracefully.
    pub async fn shutdown(&self) -> Result<(), ProviderError> {
        if self.shutdown_started.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let notification_worker = self.notification_worker.lock().await.take();
        if let Some(worker) = notification_worker {
            worker.abort();
            let _ = worker.await;
        }
        let approval_worker = self.approval_worker.lock().await.take();
        if let Some(worker) = approval_worker {
            worker.abort();
            let _ = worker.await;
        }
        self.router
            .error_all(ProviderError::stream_interrupted(
                "codex app-server shutting down",
            ))
            .await;

        let process = self.process.lock().await.take();
        if let Some(mut process) = process {
            process.graceful_shutdown().await?;
        }
        let _ = self.collect_stderr().await;
        Ok(())
    }

    async fn collect_stderr(&self) -> String {
        let task = self.stderr_task.lock().await.take();
        if let Some(task) = task {
            task.await.unwrap_or_default()
        } else {
            String::new()
        }
    }
}

fn initialize_params(config: &CodexProviderConfig) -> InitializeParams {
    InitializeParams {
        protocol_version: Some(1),
        client_info: Some(crate::rpc::InitializeClientInfo {
            name: config.client_name.clone(),
            title: Some("Arky Codex Provider".to_owned()),
            version: config.client_version.clone(),
        }),
        capabilities: Some(InitializeCapabilities {
            experimental_api: Some(config.process.experimental_api),
            opt_out_notification_methods: Vec::new(),
        }),
    }
}

async fn ensure_binary_validated(
    config: &CodexProviderConfig,
) -> Result<ResolvedCommand, ProviderError> {
    let candidates = command_candidates(config);
    let mut last_error = None;

    for candidate in candidates {
        match validate_candidate(config, &candidate).await {
            Ok(_version) => {
                return Ok(ResolvedCommand {
                    program: candidate.program,
                    prefix_args: candidate.prefix_args,
                });
            }
            Err(error) => last_error = Some(error),
        }
    }

    Err(last_error.unwrap_or_else(|| ProviderError::binary_not_found(config.binary.clone())))
}

async fn validate_candidate(
    config: &CodexProviderConfig,
    candidate: &CommandCandidate,
) -> Result<String, ProviderError> {
    let manager = ProcessManager::new(version_process_config(config, candidate));
    let mut process = manager.spawn()?;
    let mut stdout = process.take_stdout()?;
    let mut stderr = process.take_stderr()?;
    let mut stdout_buffer = String::new();
    let mut stderr_buffer = String::new();

    tokio::try_join!(
        stdout.read_to_string(&mut stdout_buffer),
        stderr.read_to_string(&mut stderr_buffer),
    )
    .map_err(|error| {
        ProviderError::stream_interrupted(format!("failed to read codex version output: {error}"))
    })?;
    process.wait().await?;

    let version = if stdout_buffer.trim().is_empty() {
        stderr_buffer.trim()
    } else {
        stdout_buffer.trim()
    };
    if version.is_empty() {
        return Err(ProviderError::protocol_violation(
            "codex version command returned no output",
            Some(json!({
                "stderr": stderr_buffer.trim(),
            })),
        ));
    }

    Ok(version.to_owned())
}

#[derive(Debug, Clone)]
struct CommandCandidate {
    program: String,
    prefix_args: Vec<String>,
}

impl CommandCandidate {
    fn args_with(&self, extra: &[String]) -> Vec<String> {
        let mut args = self.prefix_args.clone();
        args.extend(extra.iter().cloned());
        args
    }
}

fn command_candidates(config: &CodexProviderConfig) -> Vec<CommandCandidate> {
    let mut candidates = vec![resolve_command_candidate(&config.binary)];
    if config.process.allow_npx && config.binary != "npx" {
        candidates.push(CommandCandidate {
            program: "npx".to_owned(),
            prefix_args: vec!["-y".to_owned(), "@openai/codex".to_owned()],
        });
    }
    candidates
}

fn resolve_command_candidate(binary: &str) -> CommandCandidate {
    if has_javascript_extension(binary) {
        return CommandCandidate {
            program: "node".to_owned(),
            prefix_args: vec![binary.to_owned()],
        };
    }

    CommandCandidate {
        program: binary.to_owned(),
        prefix_args: Vec::new(),
    }
}

fn has_javascript_extension(binary: &str) -> bool {
    std::path::Path::new(binary)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "js" | "mjs" | "cjs"
            )
        })
}

fn version_process_config(
    config: &CodexProviderConfig,
    candidate: &CommandCandidate,
) -> ProcessConfig {
    let process = ProcessConfig::new(&candidate.program)
        .with_args(candidate.args_with(&config.version_args))
        .with_kill_on_drop(true);
    apply_process_overrides(process, config)
}

fn app_server_process_config(
    config: &CodexProviderConfig,
    resolved: &ResolvedCommand,
) -> ProcessConfig {
    let process = ProcessConfig::new(&resolved.program)
        .with_args(resolved.args_with(&config.app_server_args))
        .with_kill_on_drop(true);
    apply_process_overrides(process, config)
}

fn app_server_command_label(config: &CodexProviderConfig, resolved: &ResolvedCommand) -> String {
    let args = resolved.args_with(&config.app_server_args);
    if args.is_empty() {
        return resolved.program.clone();
    }

    format!("{} {}", resolved.program, args.join(" "))
}

fn apply_process_overrides(
    mut process: ProcessConfig,
    config: &CodexProviderConfig,
) -> ProcessConfig {
    if let Some(cwd) = &config.cwd {
        process = process.with_cwd(cwd.clone());
    }

    let mut merged_env = merged_spawn_env(config);
    if !merged_env.contains_key("RUST_LOG") {
        merged_env.insert("RUST_LOG".to_owned(), "error".to_owned());
    }

    process = process.with_clear_env(true);
    for (key, value) in merged_env {
        process = process.with_env(key, value);
    }

    process.with_shutdown_timeout(Duration::from_secs(5))
}

fn merged_spawn_env(config: &CodexProviderConfig) -> BTreeMap<String, String> {
    merged_spawn_env_from_iter(std::env::vars(), config)
}

fn merged_spawn_env_from_iter<I>(
    base_env: I,
    config: &CodexProviderConfig,
) -> BTreeMap<String, String>
where
    I: IntoIterator<Item = (String, String)>,
{
    let sanitize = config.process.sanitize_environment;
    let mut merged_env = base_env
        .into_iter()
        .filter(|(key, _)| !sanitize || !is_sanitized_spawn_key(key))
        .collect::<BTreeMap<_, _>>();
    merged_env.extend(
        config
            .env
            .iter()
            .filter(|(key, _)| !sanitize || !is_sanitized_spawn_key(key))
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    merged_env
}

fn is_sanitized_spawn_key(key: &str) -> bool {
    key.starts_with("LD_") || key.starts_with("DYLD_") || key == "NODE_OPTIONS"
}

fn spawn_notification_worker(
    notification_router: NotificationRouter,
    mut notifications: tokio::sync::mpsc::UnboundedReceiver<
        Result<crate::CodexNotification, ProviderError>,
    >,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(item) = notifications.recv().await {
            match item {
                Ok(notification) => {
                    if let Err(error) = notification_router.dispatch(notification).await {
                        notification_router.error_all(error).await;
                        break;
                    }
                }
                Err(error) => {
                    notification_router.error_all(error).await;
                    break;
                }
            }
        }
    })
}

fn spawn_approval_worker(
    approval_router: NotificationRouter,
    approval_handler: ApprovalHandler,
    approval_transport: Arc<RpcTransport>,
    mut server_requests: tokio::sync::mpsc::UnboundedReceiver<
        Result<crate::CodexServerRequest, ProviderError>,
    >,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(item) = server_requests.recv().await {
            match item {
                Ok(request) => {
                    if let Err(error) = approval_handler
                        .handle(request, approval_transport.as_ref())
                        .await
                    {
                        approval_router.error_all(error).await;
                        break;
                    }
                }
                Err(error) => {
                    approval_router.error_all(error).await;
                    break;
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::merged_spawn_env_from_iter;
    use crate::CodexProviderConfig;

    #[test]
    fn merged_spawn_env_should_filter_preload_variables_when_enabled() {
        let mut config = CodexProviderConfig::default();
        config.process.sanitize_environment = true;
        config
            .env
            .insert("LD_PRELOAD".to_owned(), "/tmp/override.so".to_owned());
        config.env.insert(
            "DYLD_INSERT_LIBRARIES".to_owned(),
            "/tmp/override.dylib".to_owned(),
        );
        config
            .env
            .insert("CUSTOM_ENV".to_owned(), "custom".to_owned());

        let merged = merged_spawn_env_from_iter(
            [
                ("LD_PRELOAD".to_owned(), "/tmp/lib.so".to_owned()),
                (
                    "DYLD_INSERT_LIBRARIES".to_owned(),
                    "/tmp/lib.dylib".to_owned(),
                ),
                ("NODE_OPTIONS".to_owned(), "--require ./hook.js".to_owned()),
                ("PATH".to_owned(), "/usr/bin".to_owned()),
            ],
            &config,
        );

        assert_eq!(merged.contains_key("LD_PRELOAD"), false);
        assert_eq!(merged.contains_key("DYLD_INSERT_LIBRARIES"), false);
        assert_eq!(merged.contains_key("NODE_OPTIONS"), false);
        assert_eq!(merged.get("PATH").map(String::as_str), Some("/usr/bin"));
        assert_eq!(merged.get("CUSTOM_ENV").map(String::as_str), Some("custom"));
    }

    #[test]
    fn merged_spawn_env_should_preserve_preload_variables_when_disabled() {
        let mut config = CodexProviderConfig::default();
        config.process.sanitize_environment = false;

        let merged = merged_spawn_env_from_iter(
            [
                ("LD_PRELOAD".to_owned(), "/tmp/lib.so".to_owned()),
                ("NODE_OPTIONS".to_owned(), "--require ./hook.js".to_owned()),
            ],
            &config,
        );

        assert_eq!(
            merged.get("LD_PRELOAD").map(String::as_str),
            Some("/tmp/lib.so")
        );
        assert_eq!(
            merged.get("NODE_OPTIONS").map(String::as_str),
            Some("--require ./hook.js")
        );
    }
}
