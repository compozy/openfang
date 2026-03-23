//! Hook contracts, composition, and shell-backed lifecycle handlers for Arky.
//!
//! Hooks let applications inject policy and side effects around sessions,
//! prompts, tool calls, and completion handling without modifying provider or
//! orchestration code directly.

mod chain;
mod context;
mod error;
mod result;
mod shell;

use std::time::Duration;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

pub use crate::{
    chain::{HookChain, HookChainConfig, HookDiagnostic},
    context::{
        AfterToolCallContext, BeforeToolCallContext, HookEvent, HookExecutionScope,
        PromptSubmitContext, SessionEndContext, SessionStartContext, SessionStartSource,
        StopContext,
    },
    error::HookError,
    result::{PromptUpdate, SessionStartUpdate, StopDecision, ToolResultOverride, Verdict},
    shell::{ShellCommandHook, ToolMatcher},
};

/// Error-handling mode used when an individual hook fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureMode {
    /// Record the failure as a diagnostic and continue merging other hooks.
    FailOpen,
    /// Stop the lifecycle event and return the hook error to the caller.
    FailClosed,
}

/// Lifecycle hook contract shared by the provider and core layers.
#[async_trait]
pub trait Hooks: Send + Sync {
    /// Returns a stable name for diagnostics and tracing.
    fn hook_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Returns a hook-specific failure mode override, when needed.
    fn failure_mode(&self) -> Option<FailureMode> {
        None
    }

    /// Returns a hook-specific timeout override, when needed.
    fn timeout(&self) -> Option<Duration> {
        None
    }

    /// Runs before a tool call executes.
    async fn before_tool_call(
        &self,
        ctx: &BeforeToolCallContext,
        cancel: CancellationToken,
    ) -> Result<Verdict, HookError> {
        let _ = (ctx, cancel);
        Ok(Verdict::Allow)
    }

    /// Runs after a tool call finishes.
    async fn after_tool_call(
        &self,
        ctx: &AfterToolCallContext,
        cancel: CancellationToken,
    ) -> Result<Option<ToolResultOverride>, HookError> {
        let _ = (ctx, cancel);
        Ok(None)
    }

    /// Runs when a session starts.
    async fn session_start(
        &self,
        ctx: &SessionStartContext,
        cancel: CancellationToken,
    ) -> Result<Option<SessionStartUpdate>, HookError> {
        let _ = (ctx, cancel);
        Ok(None)
    }

    /// Runs when a session ends.
    async fn session_end(&self, ctx: &SessionEndContext) -> Result<(), HookError> {
        let _ = ctx;
        Ok(())
    }

    /// Runs when the agent is deciding whether to stop.
    async fn on_stop(
        &self,
        ctx: &StopContext,
        cancel: CancellationToken,
    ) -> Result<StopDecision, HookError> {
        let _ = (ctx, cancel);
        Ok(StopDecision::Stop)
    }

    /// Runs when a user submits a prompt.
    async fn user_prompt_submit(
        &self,
        ctx: &PromptSubmitContext,
        cancel: CancellationToken,
    ) -> Result<Option<PromptUpdate>, HookError> {
        let _ = (ctx, cancel);
        Ok(None)
    }
}
