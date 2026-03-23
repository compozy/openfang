//! Shared protocol types for the Arky SDK.
//!
//! The protocol crate defines the durable message, event, identifier, request,
//! session, and tool data structures shared across every other workspace crate.

mod event;
mod id;
mod message;
mod request;
mod session;
mod tool;
mod utils;

pub use crate::{
    event::{AgentEvent, EventMetadata, StreamDelta},
    id::{ProviderId, SessionId, TurnId},
    message::{ContentBlock, Message, MessageBuilder, MessageMetadata, Role},
    request::{
        AgentResponse, ErrorPayload, FinishReason, GenerateResponse, HookContext,
        InputTokenDetails, ModelRef, OutputTokenDetails, ProviderRequest, ProviderSettings,
        ReasoningEffort, SessionRef, ToolContext, ToolDefinition, TurnContext, Usage,
    },
    session::{PersistedEvent, ReplayCursor, TurnCheckpoint},
    tool::{ToolCall, ToolContent, ToolResult},
    utils::{extract_text_from_events, extract_tool_results, extract_tool_uses, extract_usage},
};
