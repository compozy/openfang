//! Configuration loading, merging, and validation for Arky.
//!
//! Use this crate to load provider/runtime settings from disk, environment
//! variables, and explicit overrides before handing a finalized configuration
//! to the rest of the SDK.

mod error;
mod layered;
mod loader;
mod merge;
mod validate;
mod validation;

pub use crate::{
    error::{ConfigError, ValidationIssue},
    layered::{
        normalize_driver, ClaudeCodeBehaviorLayer, ClaudeCompatibleBehaviorLayer,
        CodexBehaviorLayer, PartialProviderBehaviorConfig, PartialProviderProfileConfig,
        ProviderBehaviorLayer, ProviderProfileConfig, ProviderRequestDefaults,
        ResolvedAgentProviderConfig, ResolvedClaudeCodeBehaviorConfig,
        ResolvedClaudeCompatibleBehaviorConfig, ResolvedCodexBehaviorConfig,
        ResolvedProviderBehaviorConfig,
    },
    loader::{
        AgentConfig, AgentConfigBuilder, ArkyConfig, ArkyConfigBuilder, ConfigFormat, ConfigLoader,
        ProviderConfig, ProviderConfigBuilder, ProviderProfileConfigBuilder, WorkspaceConfig,
        WorkspaceConfigBuilder,
    },
    validate::find_binary_on_path,
    validation::{validate_against_schema, RichValidationSchema},
};
