//! Claude Code CLI provider implementation for Arky.
//!
//! This crate owns the Claude-specific subprocess integration surface:
//! configuration, stream parsing, nested tool tracking, deduplication, and
//! session bookkeeping for the Claude CLI protocol.

mod classifier;
mod config;
mod conversion;
mod cooldown;
mod dedup;
mod generate;
mod nested;
mod parser;
mod profile;
mod provider;
mod session;
mod tool_bridge;
mod tool_fsm;

pub use crate::{
    classifier::ClaudeErrorClassifier,
    config::{
        ClaudeCliBehaviorConfig, ClaudeCodeProviderConfig, ClaudeFilesystemConfig,
        ClaudePermissionConfig, ClaudePluginConfig, ClaudeSandboxConfig, ClaudeSessionConfig,
        ClaudeStderrCallback, KNOWN_CLAUDE_MODEL_IDS, MAX_PROMPT_WARNING_LENGTH,
        validate_claude_model_id, validate_prompt_length, validate_session_id_format,
    },
    conversion::{
        ClaudeInjectedPromptStream, ClaudeMessageConversion, ClaudeMessageDeliveryCallback,
        ClaudeMessageInjector, collect_runtime_warning_messages, collect_warning_messages,
        convert_messages, extract_structured_output, image_part_from_base64, image_part_from_bytes,
        image_part_from_data_url, image_source_payload, map_finish_reason, map_permission_mode,
        parse_image_string, structured_output_args,
    },
    cooldown::{SpawnAttemptStatus, SpawnFailurePolicy, SpawnFailureRecord, SpawnFailureTracker},
    dedup::TextDeduplicator,
    generate::generate_with_recovery,
    parser::{
        ClaudeEventParser, ClaudeEventSource, ClaudeNormalizedEvent, is_claude_truncation_error,
    },
    profile::{
        BedrockProvider, BedrockProviderConfig, CLAUDE_COMPATIBLE_PROVIDER_IDS,
        ClaudeCompatibleProviderConfig, ClaudeCompatibleProviderKind, MinimaxProvider,
        MinimaxProviderConfig, MoonshotProvider, MoonshotProviderConfig, OllamaProvider,
        OllamaProviderConfig, OpenRouterProvider, OpenRouterProviderConfig, VercelProvider,
        VercelProviderConfig, VertexProvider, VertexProviderConfig, ZaiProvider, ZaiProviderConfig,
    },
    provider::ClaudeCodeProvider,
    session::SessionManager,
    tool_bridge::{
        ClaudeCombinedToolBridgeConfig, ClaudeToolBridgeConfig, ClaudeToolBridgeTool,
        DEFAULT_TOOL_INPUT_LIMITS, SerializedToolInput, ToolInputLimits, build_tool_bridge,
        serialize_tool_input, serialize_tool_input_with_metadata,
    },
    tool_fsm::{ToolLifecycleState, ToolLifecycleTracker},
};
