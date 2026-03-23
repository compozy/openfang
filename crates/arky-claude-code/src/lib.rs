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
        validate_claude_model_id, validate_prompt_length, validate_session_id_format,
        ClaudeCliBehaviorConfig, ClaudeCodeProviderConfig, ClaudeFilesystemConfig,
        ClaudePermissionConfig, ClaudePluginConfig, ClaudeSandboxConfig, ClaudeSessionConfig,
        ClaudeStderrCallback, KNOWN_CLAUDE_MODEL_IDS, MAX_PROMPT_WARNING_LENGTH,
    },
    conversion::{
        collect_runtime_warning_messages, collect_warning_messages, convert_messages,
        extract_structured_output, image_part_from_base64, image_part_from_bytes,
        image_part_from_data_url, image_source_payload, map_finish_reason, map_permission_mode,
        parse_image_string, structured_output_args, ClaudeInjectedPromptStream,
        ClaudeMessageConversion, ClaudeMessageDeliveryCallback, ClaudeMessageInjector,
    },
    cooldown::{SpawnAttemptStatus, SpawnFailurePolicy, SpawnFailureRecord, SpawnFailureTracker},
    dedup::TextDeduplicator,
    generate::generate_with_recovery,
    parser::{
        is_claude_truncation_error, ClaudeEventParser, ClaudeEventSource, ClaudeNormalizedEvent,
    },
    profile::{
        BedrockProvider, BedrockProviderConfig, ClaudeCompatibleProviderConfig,
        ClaudeCompatibleProviderKind, ClaudeProviderProfile, MinimaxProvider,
        MinimaxProviderConfig, MoonshotProvider, MoonshotProviderConfig, OllamaProvider,
        OllamaProviderConfig, OpenRouterProvider, OpenRouterProviderConfig, VercelProvider,
        VercelProviderConfig, VertexProvider, VertexProviderConfig, ZaiProvider, ZaiProviderConfig,
        CLAUDE_COMPATIBLE_PROVIDER_IDS,
    },
    provider::ClaudeCodeProvider,
    session::SessionManager,
    tool_bridge::{
        build_tool_bridge, serialize_tool_input, serialize_tool_input_with_metadata,
        ClaudeCombinedToolBridgeConfig, ClaudeToolBridgeConfig, ClaudeToolBridgeTool,
        SerializedToolInput, ToolInputLimits, DEFAULT_TOOL_INPUT_LIMITS,
    },
    tool_fsm::{ToolLifecycleState, ToolLifecycleTracker},
};
