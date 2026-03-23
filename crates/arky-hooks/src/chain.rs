//! Hook composition and merge semantics.

use std::{
    ops::Add,
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex},
    time::Duration,
};

use arky_error::{ErrorLogEntry, classify_error};
use async_trait::async_trait;
use futures::{FutureExt, future::join_all};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::{
    AfterToolCallContext, BeforeToolCallContext, FailureMode, HookError, HookEvent, Hooks,
    PromptSubmitContext, PromptUpdate, SessionEndContext, SessionStartContext, SessionStartUpdate,
    StopContext, StopDecision, ToolResultOverride, Verdict,
};

const DEFAULT_CHAIN_TIMEOUT: Duration = Duration::from_secs(30);

/// Runtime configuration shared by a [`HookChain`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookChainConfig {
    /// Fallback failure mode applied when a hook does not override it.
    pub failure_mode: FailureMode,
    /// Fallback timeout applied when a hook does not override it.
    pub default_timeout: Duration,
}

impl Default for HookChainConfig {
    fn default() -> Self {
        Self {
            failure_mode: FailureMode::FailClosed,
            default_timeout: DEFAULT_CHAIN_TIMEOUT,
        }
    }
}

/// Structured diagnostic emitted for fail-open hook failures.
#[derive(Debug, Clone, PartialEq)]
pub struct HookDiagnostic {
    /// Lifecycle event being processed.
    pub event: HookEvent,
    /// Hook registration index.
    pub hook_index: usize,
    /// Stable hook name used for tracing.
    pub hook_name: String,
    /// Effective failure mode used for the failing hook.
    pub failure_mode: FailureMode,
    /// Structured error classification.
    pub error: ErrorLogEntry,
}

struct HookInvocation<T> {
    index: usize,
    hook_name: String,
    failure_mode: FailureMode,
    outcome: Result<T, HookError>,
}

/// Composes multiple hook implementations into a single hook surface.
#[derive(Default)]
pub struct HookChain {
    hooks: Vec<Arc<dyn Hooks>>,
    config: HookChainConfig,
    diagnostics: Mutex<Vec<HookDiagnostic>>,
}

impl HookChain {
    /// Creates an empty hook chain.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the chain-wide fallback failure mode.
    #[must_use]
    pub const fn with_failure_mode(mut self, failure_mode: FailureMode) -> Self {
        self.config.failure_mode = failure_mode;
        self
    }

    /// Sets the chain-wide fallback timeout.
    #[must_use]
    pub const fn with_default_timeout(mut self, default_timeout: Duration) -> Self {
        self.config.default_timeout = default_timeout;
        self
    }

    /// Adds a hook to the chain using builder syntax.
    #[must_use]
    pub fn with_hook<H>(mut self, hook: H) -> Self
    where
        H: Hooks + 'static,
    {
        self.hooks.push(Arc::new(hook));
        self
    }

    /// Pushes a hook onto the chain.
    pub fn push<H>(&mut self, hook: H)
    where
        H: Hooks + 'static,
    {
        self.hooks.push(Arc::new(hook));
    }

    /// Returns the number of registered hooks.
    #[must_use]
    pub fn len(&self) -> usize {
        self.hooks.len()
    }

    /// Returns whether the chain is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }

    /// Returns the recorded fail-open diagnostics.
    #[must_use]
    pub fn diagnostics(&self) -> Vec<HookDiagnostic> {
        self.diagnostics_guard().clone()
    }

    /// Clears accumulated fail-open diagnostics.
    pub fn clear_diagnostics(&self) {
        self.diagnostics_guard().clear();
    }

    fn effective_failure_mode(&self, hook: &dyn Hooks) -> FailureMode {
        hook.failure_mode().unwrap_or(self.config.failure_mode)
    }

    fn effective_timeout(&self, hook: &dyn Hooks) -> Duration {
        hook.timeout().unwrap_or(self.config.default_timeout)
    }

    fn record_diagnostic(
        &self,
        event: HookEvent,
        hook_index: usize,
        hook_name: &str,
        failure_mode: FailureMode,
        error: &HookError,
    ) {
        let entry = classify_error(error);
        self.diagnostics_guard().push(HookDiagnostic {
            event,
            hook_index,
            hook_name: hook_name.to_owned(),
            failure_mode,
            error: entry.clone(),
        });

        warn!(
            hook_event = event.as_str(),
            hook_index,
            hook_name,
            failure_mode = ?failure_mode,
            error_code = entry.error_code,
            message = %entry.message,
            "hook failed in fail-open mode"
        );
    }

    fn handle_failure<T>(
        &self,
        event: HookEvent,
        invocation: &HookInvocation<T>,
        error: &HookError,
    ) -> Result<(), HookError> {
        match invocation.failure_mode {
            FailureMode::FailOpen => {
                self.record_diagnostic(
                    event,
                    invocation.index,
                    &invocation.hook_name,
                    invocation.failure_mode,
                    error,
                );
                Ok(())
            }
            FailureMode::FailClosed => Err(error.clone()),
        }
    }

    fn diagnostics_guard(&self) -> std::sync::MutexGuard<'_, Vec<HookDiagnostic>> {
        match self.diagnostics.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    async fn invoke_before_tool_call(
        &self,
        ctx: &BeforeToolCallContext,
        cancel: CancellationToken,
    ) -> Vec<HookInvocation<Verdict>> {
        let futures = self.hooks.iter().enumerate().map(|(index, hook)| {
            let hook = Arc::clone(hook);
            let hook_name = hook.hook_name().to_owned();
            let failure_mode = self.effective_failure_mode(hook.as_ref());
            let timeout = self.effective_timeout(hook.as_ref());
            let cancel = cancel.child_token();

            async move {
                let hook_cancel = cancel.clone();
                let outcome = execute_invocation(
                    hook_name.clone(),
                    HookEvent::BeforeToolCall,
                    timeout,
                    cancel,
                    async move { hook.before_tool_call(ctx, hook_cancel).await },
                )
                .await;

                HookInvocation {
                    index,
                    hook_name,
                    failure_mode,
                    outcome,
                }
            }
        });

        join_all(futures).await
    }

    async fn invoke_after_tool_call(
        &self,
        ctx: &AfterToolCallContext,
        cancel: CancellationToken,
    ) -> Vec<HookInvocation<Option<ToolResultOverride>>> {
        let futures = self.hooks.iter().enumerate().map(|(index, hook)| {
            let hook = Arc::clone(hook);
            let hook_name = hook.hook_name().to_owned();
            let failure_mode = self.effective_failure_mode(hook.as_ref());
            let timeout = self.effective_timeout(hook.as_ref());
            let cancel = cancel.child_token();

            async move {
                let hook_cancel = cancel.clone();
                let outcome = execute_invocation(
                    hook_name.clone(),
                    HookEvent::AfterToolCall,
                    timeout,
                    cancel,
                    async move { hook.after_tool_call(ctx, hook_cancel).await },
                )
                .await;

                HookInvocation {
                    index,
                    hook_name,
                    failure_mode,
                    outcome,
                }
            }
        });

        join_all(futures).await
    }

    async fn invoke_session_start(
        &self,
        ctx: &SessionStartContext,
        cancel: CancellationToken,
    ) -> Vec<HookInvocation<Option<SessionStartUpdate>>> {
        let futures = self.hooks.iter().enumerate().map(|(index, hook)| {
            let hook = Arc::clone(hook);
            let hook_name = hook.hook_name().to_owned();
            let failure_mode = self.effective_failure_mode(hook.as_ref());
            let timeout = self.effective_timeout(hook.as_ref());
            let cancel = cancel.child_token();

            async move {
                let hook_cancel = cancel.clone();
                let outcome = execute_invocation(
                    hook_name.clone(),
                    HookEvent::SessionStart,
                    timeout,
                    cancel,
                    async move { hook.session_start(ctx, hook_cancel).await },
                )
                .await;

                HookInvocation {
                    index,
                    hook_name,
                    failure_mode,
                    outcome,
                }
            }
        });

        join_all(futures).await
    }

    async fn invoke_session_end(&self, ctx: &SessionEndContext) -> Vec<HookInvocation<()>> {
        let futures = self.hooks.iter().enumerate().map(|(index, hook)| {
            let hook = Arc::clone(hook);
            let hook_name = hook.hook_name().to_owned();
            let failure_mode = self.effective_failure_mode(hook.as_ref());
            let timeout = self.effective_timeout(hook.as_ref());

            async move {
                let outcome = execute_invocation(
                    hook_name.clone(),
                    HookEvent::SessionEnd,
                    timeout,
                    CancellationToken::new(),
                    async move { hook.session_end(ctx).await },
                )
                .await;

                HookInvocation {
                    index,
                    hook_name,
                    failure_mode,
                    outcome,
                }
            }
        });

        join_all(futures).await
    }

    async fn invoke_on_stop(
        &self,
        ctx: &StopContext,
        cancel: CancellationToken,
    ) -> Vec<HookInvocation<StopDecision>> {
        let futures = self.hooks.iter().enumerate().map(|(index, hook)| {
            let hook = Arc::clone(hook);
            let hook_name = hook.hook_name().to_owned();
            let failure_mode = self.effective_failure_mode(hook.as_ref());
            let timeout = self.effective_timeout(hook.as_ref());
            let cancel = cancel.child_token();

            async move {
                let hook_cancel = cancel.clone();
                let outcome = execute_invocation(
                    hook_name.clone(),
                    HookEvent::OnStop,
                    timeout,
                    cancel,
                    async move { hook.on_stop(ctx, hook_cancel).await },
                )
                .await;

                HookInvocation {
                    index,
                    hook_name,
                    failure_mode,
                    outcome,
                }
            }
        });

        join_all(futures).await
    }

    async fn invoke_prompt_submit(
        &self,
        ctx: &PromptSubmitContext,
        cancel: CancellationToken,
    ) -> Vec<HookInvocation<Option<PromptUpdate>>> {
        let futures = self.hooks.iter().enumerate().map(|(index, hook)| {
            let hook = Arc::clone(hook);
            let hook_name = hook.hook_name().to_owned();
            let failure_mode = self.effective_failure_mode(hook.as_ref());
            let timeout = self.effective_timeout(hook.as_ref());
            let cancel = cancel.child_token();

            async move {
                let hook_cancel = cancel.clone();
                let outcome = execute_invocation(
                    hook_name.clone(),
                    HookEvent::UserPromptSubmit,
                    timeout,
                    cancel,
                    async move { hook.user_prompt_submit(ctx, hook_cancel).await },
                )
                .await;

                HookInvocation {
                    index,
                    hook_name,
                    failure_mode,
                    outcome,
                }
            }
        });

        join_all(futures).await
    }
}

impl<H> Add<H> for HookChain
where
    H: Hooks + 'static,
{
    type Output = Self;

    fn add(self, rhs: H) -> Self::Output {
        self.with_hook(rhs)
    }
}

#[async_trait]
impl Hooks for HookChain {
    async fn before_tool_call(
        &self,
        ctx: &BeforeToolCallContext,
        cancel: CancellationToken,
    ) -> Result<Verdict, HookError> {
        let mut invocations = self.invoke_before_tool_call(ctx, cancel).await;
        invocations.sort_by_key(|invocation| invocation.index);

        let mut merged = Verdict::Allow;
        for invocation in invocations {
            match invocation.outcome {
                Ok(Verdict::Allow) => {}
                Ok(candidate @ Verdict::Block { .. }) => {
                    if matches!(merged, Verdict::Allow) {
                        merged = candidate;
                    }
                }
                Err(ref error) => {
                    self.handle_failure(HookEvent::BeforeToolCall, &invocation, error)?;
                }
            }
        }

        Ok(merged)
    }

    async fn after_tool_call(
        &self,
        ctx: &AfterToolCallContext,
        cancel: CancellationToken,
    ) -> Result<Option<ToolResultOverride>, HookError> {
        let mut invocations = self.invoke_after_tool_call(ctx, cancel).await;
        invocations.sort_by_key(|invocation| invocation.index);

        let mut merged = ToolResultOverride::new();
        for invocation in invocations {
            match invocation.outcome {
                Ok(Some(update)) => merged.merge_from(update),
                Ok(None) => {}
                Err(ref error) => {
                    self.handle_failure(HookEvent::AfterToolCall, &invocation, error)?;
                }
            }
        }

        Ok((!merged.is_empty()).then_some(merged))
    }

    async fn session_start(
        &self,
        ctx: &SessionStartContext,
        cancel: CancellationToken,
    ) -> Result<Option<SessionStartUpdate>, HookError> {
        let mut invocations = self.invoke_session_start(ctx, cancel).await;
        invocations.sort_by_key(|invocation| invocation.index);

        let mut merged = SessionStartUpdate::new();
        for invocation in invocations {
            match invocation.outcome {
                Ok(Some(update)) => merged.merge_from(update),
                Ok(None) => {}
                Err(ref error) => {
                    self.handle_failure(HookEvent::SessionStart, &invocation, error)?;
                }
            }
        }

        Ok((!merged.is_empty()).then_some(merged))
    }

    async fn session_end(&self, ctx: &SessionEndContext) -> Result<(), HookError> {
        let mut invocations = self.invoke_session_end(ctx).await;
        invocations.sort_by_key(|invocation| invocation.index);

        for invocation in invocations {
            if let Err(ref error) = invocation.outcome {
                self.handle_failure(HookEvent::SessionEnd, &invocation, error)?;
            }
        }

        Ok(())
    }

    async fn on_stop(
        &self,
        ctx: &StopContext,
        cancel: CancellationToken,
    ) -> Result<StopDecision, HookError> {
        let mut invocations = self.invoke_on_stop(ctx, cancel).await;
        invocations.sort_by_key(|invocation| invocation.index);

        let mut merged = StopDecision::Stop;
        for invocation in invocations {
            match invocation.outcome {
                Ok(candidate @ StopDecision::Continue { .. }) => {
                    if matches!(merged, StopDecision::Stop) {
                        merged = candidate;
                    }
                }
                Ok(StopDecision::Stop) => {}
                Err(ref error) => {
                    self.handle_failure(HookEvent::OnStop, &invocation, error)?;
                }
            }
        }

        Ok(merged)
    }

    async fn user_prompt_submit(
        &self,
        ctx: &PromptSubmitContext,
        cancel: CancellationToken,
    ) -> Result<Option<PromptUpdate>, HookError> {
        let mut invocations = self.invoke_prompt_submit(ctx, cancel).await;
        invocations.sort_by_key(|invocation| invocation.index);

        let mut merged = PromptUpdate::new();
        for invocation in invocations {
            match invocation.outcome {
                Ok(Some(update)) => merged.merge_from(update),
                Ok(None) => {}
                Err(ref error) => {
                    self.handle_failure(HookEvent::UserPromptSubmit, &invocation, error)?;
                }
            }
        }

        Ok((!merged.is_empty()).then_some(merged))
    }
}

async fn execute_invocation<T, Fut>(
    hook_name: String,
    event: HookEvent,
    timeout: Duration,
    cancel: CancellationToken,
    future: Fut,
) -> Result<T, HookError>
where
    Fut: std::future::Future<Output = Result<T, HookError>>,
{
    let hook_name_for_future = hook_name.clone();
    let guarded = AssertUnwindSafe(async move {
        tokio::select! {
            biased;
            result = future => result,
            () = cancel.cancelled() => Err(HookError::execution_failed(
                "hook execution cancelled",
                Some(event),
                Some(hook_name_for_future.clone()),
            )),
            () = tokio::time::sleep(timeout) => Err(HookError::timeout(
                "hook execution timed out",
                Some(event),
                Some(hook_name_for_future.clone()),
                Some(timeout),
            )),
        }
    })
    .catch_unwind();

    match guarded.await {
        Ok(result) => result,
        Err(payload) => Err(HookError::panic_isolated(
            panic_payload_to_string(payload),
            Some(event),
            Some(hook_name),
        )),
    }
}

fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    let payload = match payload.downcast::<String>() {
        Ok(message) => return format!("hook panicked: {}", *message),
        Err(payload) => payload,
    };

    if let Ok(message) = payload.downcast::<&str>() {
        return format!("hook panicked: {}", *message);
    }

    "hook panicked".to_owned()
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };

    use arky_protocol::{Message, SessionRef, ToolCall, ToolContent, ToolResult};
    use async_trait::async_trait;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use tokio::sync::Mutex as AsyncMutex;
    use tokio_util::sync::CancellationToken;

    use crate::{
        AfterToolCallContext, BeforeToolCallContext, FailureMode, HookChain, HookError, HookEvent,
        HookExecutionScope, Hooks, PromptSubmitContext, PromptUpdate, SessionStartContext,
        SessionStartSource, SessionStartUpdate, StopContext, StopDecision, ToolResultOverride,
        Verdict,
    };

    #[derive(Clone)]
    struct BeforeHook {
        delay: Duration,
        verdict: Verdict,
    }

    #[async_trait]
    impl Hooks for BeforeHook {
        async fn before_tool_call(
            &self,
            _ctx: &BeforeToolCallContext,
            _cancel: CancellationToken,
        ) -> Result<Verdict, HookError> {
            tokio::time::sleep(self.delay).await;
            Ok(self.verdict.clone())
        }
    }

    #[derive(Clone)]
    struct AfterHook {
        override_value: Option<ToolResultOverride>,
    }

    #[async_trait]
    impl Hooks for AfterHook {
        async fn after_tool_call(
            &self,
            _ctx: &AfterToolCallContext,
            _cancel: CancellationToken,
        ) -> Result<Option<ToolResultOverride>, HookError> {
            Ok(self.override_value.clone())
        }
    }

    #[derive(Clone)]
    struct SessionStartHook {
        delay: Duration,
        update: Option<SessionStartUpdate>,
        completions: Option<Arc<AsyncMutex<Vec<&'static str>>>>,
        label: &'static str,
    }

    #[async_trait]
    impl Hooks for SessionStartHook {
        async fn session_start(
            &self,
            _ctx: &SessionStartContext,
            _cancel: CancellationToken,
        ) -> Result<Option<SessionStartUpdate>, HookError> {
            tokio::time::sleep(self.delay).await;
            if let Some(completions) = &self.completions {
                completions.lock().await.push(self.label);
            }
            Ok(self.update.clone())
        }
    }

    #[derive(Clone)]
    struct PromptHook {
        update: Option<PromptUpdate>,
    }

    #[async_trait]
    impl Hooks for PromptHook {
        async fn user_prompt_submit(
            &self,
            _ctx: &PromptSubmitContext,
            _cancel: CancellationToken,
        ) -> Result<Option<PromptUpdate>, HookError> {
            Ok(self.update.clone())
        }
    }

    #[derive(Clone)]
    struct StopHook {
        decision: StopDecision,
    }

    #[async_trait]
    impl Hooks for StopHook {
        async fn on_stop(
            &self,
            _ctx: &StopContext,
            _cancel: CancellationToken,
        ) -> Result<StopDecision, HookError> {
            Ok(self.decision.clone())
        }
    }

    #[derive(Clone)]
    struct ErrorHook {
        error: HookError,
        failure_mode: Option<FailureMode>,
    }

    #[async_trait]
    impl Hooks for ErrorHook {
        fn failure_mode(&self) -> Option<FailureMode> {
            self.failure_mode
        }

        async fn before_tool_call(
            &self,
            _ctx: &BeforeToolCallContext,
            _cancel: CancellationToken,
        ) -> Result<Verdict, HookError> {
            Err(self.error.clone())
        }
    }

    #[derive(Clone)]
    struct TimeoutHook;

    #[async_trait]
    impl Hooks for TimeoutHook {
        async fn session_start(
            &self,
            _ctx: &SessionStartContext,
            _cancel: CancellationToken,
        ) -> Result<Option<SessionStartUpdate>, HookError> {
            tokio::time::sleep(Duration::from_secs(5)).await;
            Ok(None)
        }
    }

    #[derive(Clone)]
    struct CancellationAwareHook {
        observed_cancel: Arc<AtomicBool>,
    }

    #[async_trait]
    impl Hooks for CancellationAwareHook {
        async fn before_tool_call(
            &self,
            _ctx: &BeforeToolCallContext,
            cancel: CancellationToken,
        ) -> Result<Verdict, HookError> {
            cancel.cancelled().await;
            self.observed_cancel.store(true, Ordering::SeqCst);
            Ok(Verdict::Allow)
        }
    }

    #[derive(Clone)]
    struct PanicHook;

    #[async_trait]
    impl Hooks for PanicHook {
        async fn before_tool_call(
            &self,
            _ctx: &BeforeToolCallContext,
            _cancel: CancellationToken,
        ) -> Result<Verdict, HookError> {
            panic!("literal panic message");
        }
    }

    fn before_context() -> BeforeToolCallContext {
        BeforeToolCallContext::new(
            SessionRef::default(),
            ToolCall::new(
                "call-1",
                "mcp/local/read_file",
                json!({ "path": "Cargo.toml" }),
            ),
        )
    }

    fn after_context() -> AfterToolCallContext {
        let call = ToolCall::new(
            "call-1",
            "mcp/local/read_file",
            json!({ "path": "Cargo.toml" }),
        );
        let result = ToolResult::success(
            "call-1",
            "mcp/local/read_file",
            vec![ToolContent::text("ok")],
        );
        AfterToolCallContext::new(SessionRef::default(), call, result)
    }

    fn session_start_context() -> SessionStartContext {
        SessionStartContext::new(SessionRef::default(), SessionStartSource::Startup)
            .with_scope(HookExecutionScope::new(SessionRef::default()))
    }

    fn prompt_context() -> PromptSubmitContext {
        PromptSubmitContext::new(SessionRef::default(), "original prompt")
    }

    fn stop_context() -> StopContext {
        StopContext::new(SessionRef::default())
    }

    #[tokio::test]
    async fn before_tool_call_should_use_first_block_in_registration_order() {
        let chain = HookChain::new()
            .with_hook(BeforeHook {
                delay: Duration::from_millis(60),
                verdict: Verdict::Allow,
            })
            .with_hook(BeforeHook {
                delay: Duration::from_millis(10),
                verdict: Verdict::block("first block"),
            })
            .with_hook(BeforeHook {
                delay: Duration::from_millis(20),
                verdict: Verdict::block("later block"),
            });

        let actual = chain
            .before_tool_call(&before_context(), CancellationToken::new())
            .await
            .expect("before tool call should merge");

        assert_eq!(actual, Verdict::block("first block"));
    }

    #[tokio::test]
    async fn after_tool_call_should_apply_last_write_wins_per_field() {
        let chain = HookChain::new()
            .with_hook(AfterHook {
                override_value: Some(
                    ToolResultOverride::new().with_content(vec![ToolContent::text("first")]),
                ),
            })
            .with_hook(AfterHook {
                override_value: Some(ToolResultOverride::new().with_is_error(true)),
            })
            .with_hook(AfterHook {
                override_value: Some(
                    ToolResultOverride::new().with_content(vec![ToolContent::text("last")]),
                ),
            });

        let actual = chain
            .after_tool_call(&after_context(), CancellationToken::new())
            .await
            .expect("after tool call should merge")
            .expect("override should exist");

        let expected = ToolResultOverride::new()
            .with_content(vec![ToolContent::text("last")])
            .with_is_error(true);

        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn session_start_should_merge_maps_and_append_messages_in_order() {
        let mut env_one = BTreeMap::new();
        let _ = env_one.insert("A".to_owned(), "1".to_owned());
        let mut env_two = BTreeMap::new();
        let _ = env_two.insert("B".to_owned(), "2".to_owned());

        let mut settings_one = BTreeMap::new();
        let _ = settings_one.insert("temperature".to_owned(), json!(0.5));
        let mut settings_two = BTreeMap::new();
        let _ = settings_two.insert("max_tokens".to_owned(), json!(64));

        let chain = HookChain::new()
            .with_hook(SessionStartHook {
                delay: Duration::ZERO,
                update: Some(
                    SessionStartUpdate::new()
                        .with_env(env_one)
                        .with_settings(settings_one)
                        .with_messages(vec![Message::system("one")]),
                ),
                completions: None,
                label: "one",
            })
            .with_hook(SessionStartHook {
                delay: Duration::ZERO,
                update: Some(
                    SessionStartUpdate::new()
                        .with_env(env_two)
                        .with_settings(settings_two)
                        .with_messages(vec![Message::system("two")]),
                ),
                completions: None,
                label: "two",
            });

        let actual = chain
            .session_start(&session_start_context(), CancellationToken::new())
            .await
            .expect("session start should merge")
            .expect("session start update should exist");

        let expected_messages = vec![Message::system("one"), Message::system("two")];
        assert_eq!(actual.messages, expected_messages);
        assert_eq!(actual.env.get("A"), Some(&"1".to_owned()));
        assert_eq!(actual.env.get("B"), Some(&"2".to_owned()));
        assert_eq!(actual.settings.get("temperature"), Some(&json!(0.5)));
        assert_eq!(actual.settings.get("max_tokens"), Some(&json!(64)));
    }

    #[tokio::test]
    async fn user_prompt_submit_should_use_last_prompt_and_append_messages() {
        let chain = HookChain::new()
            .with_hook(PromptHook {
                update: Some(
                    PromptUpdate::new()
                        .rewrite("first rewrite")
                        .with_messages(vec![Message::system("one")]),
                ),
            })
            .with_hook(PromptHook {
                update: Some(
                    PromptUpdate::new()
                        .rewrite("last rewrite")
                        .with_messages(vec![Message::system("two")]),
                ),
            });

        let actual = chain
            .user_prompt_submit(&prompt_context(), CancellationToken::new())
            .await
            .expect("prompt update should merge")
            .expect("prompt update should exist");

        let expected = PromptUpdate::new()
            .rewrite("last rewrite")
            .with_messages(vec![Message::system("one"), Message::system("two")]);

        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn on_stop_should_continue_when_any_hook_requests_it() {
        let chain = HookChain::new()
            .with_hook(StopHook {
                decision: StopDecision::Stop,
            })
            .with_hook(StopHook {
                decision: StopDecision::continue_with("keep going"),
            })
            .with_hook(StopHook {
                decision: StopDecision::continue_with("ignored later"),
            });

        let actual = chain
            .on_stop(&stop_context(), CancellationToken::new())
            .await
            .expect("stop decision should merge");

        assert_eq!(actual, StopDecision::continue_with("keep going"));
    }

    #[tokio::test]
    async fn hooks_should_run_concurrently_and_merge_in_registration_order() {
        let completions = Arc::new(AsyncMutex::new(Vec::new()));
        let chain = HookChain::new()
            .with_hook(SessionStartHook {
                delay: Duration::from_millis(90),
                update: Some(
                    SessionStartUpdate::new().with_messages(vec![Message::system("first")]),
                ),
                completions: Some(Arc::clone(&completions)),
                label: "first",
            })
            .with_hook(SessionStartHook {
                delay: Duration::from_millis(10),
                update: Some(
                    SessionStartUpdate::new().with_messages(vec![Message::system("second")]),
                ),
                completions: Some(Arc::clone(&completions)),
                label: "second",
            })
            .with_hook(SessionStartHook {
                delay: Duration::from_millis(50),
                update: Some(
                    SessionStartUpdate::new().with_messages(vec![Message::system("third")]),
                ),
                completions: Some(Arc::clone(&completions)),
                label: "third",
            });

        let start = Instant::now();
        let actual = chain
            .session_start(&session_start_context(), CancellationToken::new())
            .await
            .expect("session start should merge")
            .expect("update should exist");
        let elapsed = start.elapsed();
        let completion_order = completions.lock().await.clone();

        assert!(elapsed < Duration::from_millis(140));
        assert_eq!(completion_order, vec!["second", "third", "first"],);
        assert_eq!(
            actual.messages,
            vec![
                Message::system("first"),
                Message::system("second"),
                Message::system("third"),
            ],
        );
    }

    #[tokio::test]
    async fn timeout_should_return_hook_timeout() {
        let chain = HookChain::new()
            .with_default_timeout(Duration::from_millis(25))
            .with_hook(TimeoutHook);

        let error = chain
            .session_start(&session_start_context(), CancellationToken::new())
            .await
            .expect_err("timeout should fail");

        assert!(matches!(error, HookError::Timeout { .. }));
    }

    #[tokio::test]
    async fn cancellation_should_reach_running_hooks() {
        let observed_cancel = Arc::new(AtomicBool::new(false));
        let chain = HookChain::new().with_hook(CancellationAwareHook {
            observed_cancel: Arc::clone(&observed_cancel),
        });
        let token = CancellationToken::new();
        let delayed_cancel = token.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            delayed_cancel.cancel();
        });

        let actual = chain
            .before_tool_call(&before_context(), token)
            .await
            .expect("hook should observe cancellation cooperatively");

        assert_eq!(actual, Verdict::Allow);
        assert!(observed_cancel.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn fail_open_should_record_diagnostic_and_continue() {
        let chain = HookChain::new()
            .with_failure_mode(FailureMode::FailClosed)
            .with_hook(ErrorHook {
                error: HookError::execution_failed(
                    "boom",
                    Some(HookEvent::BeforeToolCall),
                    Some("error-hook".to_owned()),
                ),
                failure_mode: Some(FailureMode::FailOpen),
            })
            .with_hook(BeforeHook {
                delay: Duration::ZERO,
                verdict: Verdict::Allow,
            });

        let actual = chain
            .before_tool_call(&before_context(), CancellationToken::new())
            .await
            .expect("fail-open should continue");
        let diagnostics = chain.diagnostics();

        assert_eq!(actual, Verdict::Allow);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].error.error_code, "HOOK_EXECUTION_FAILED");
    }

    #[tokio::test]
    async fn fail_closed_should_return_the_hook_error() {
        let chain = HookChain::new().with_hook(ErrorHook {
            error: HookError::execution_failed(
                "boom",
                Some(HookEvent::BeforeToolCall),
                Some("error-hook".to_owned()),
            ),
            failure_mode: None,
        });

        let error = chain
            .before_tool_call(&before_context(), CancellationToken::new())
            .await
            .expect_err("fail-closed should propagate");

        assert!(matches!(error, HookError::ExecutionFailed { .. }));
    }

    #[tokio::test]
    async fn panic_isolation_should_preserve_literal_panic_messages() {
        let chain = HookChain::new().with_hook(PanicHook);

        let error = chain
            .before_tool_call(&before_context(), CancellationToken::new())
            .await
            .expect_err("panics should be isolated");

        let HookError::PanicIsolated {
            message,
            event,
            hook_name,
        } = error
        else {
            panic!("expected panic isolation error");
        };

        assert_eq!(message, "hook panicked: literal panic message");
        assert_eq!(event, Some(HookEvent::BeforeToolCall));
        assert_eq!(
            hook_name,
            Some(std::any::type_name::<PanicHook>().to_owned()),
        );
    }

    #[test]
    fn public_hook_types_should_be_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<HookChain>();
        assert_send_sync::<crate::ShellCommandHook>();
    }
}
