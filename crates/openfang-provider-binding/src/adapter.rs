//! Typed provider adapter layer for Compozy bindings.

use std::{path::PathBuf, time::Duration};

use arky_claude_code::{
    BedrockProviderConfig, ClaudeCodeProviderConfig, ClaudeCompatibleProviderKind,
    ClaudeProviderProfile, MinimaxProviderConfig, MoonshotProviderConfig, OllamaProviderConfig,
    OpenRouterProviderConfig, VercelProviderConfig, VertexProviderConfig, ZaiProviderConfig,
};
use arky_codex::CodexProviderConfig;
use arky_config::{
    normalize_driver, ProviderConfig, ProviderRequestDefaults, ResolvedClaudeCodeBehaviorConfig,
    ResolvedClaudeCompatibleBehaviorConfig, ResolvedCodexBehaviorConfig,
    ResolvedProviderBehaviorConfig,
};
use arky_error::ClassifiedError;
use arky_protocol::ReasoningEffort;
use thiserror::Error;
use tracing::warn;

use crate::ProviderBinding;

/// Typed Compozy provider config produced by the adapter layer.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CompozyProviderConfig {
    /// Direct Codex runtime config.
    Codex(CodexProviderConfig),
    /// Direct Claude Code runtime config.
    ClaudeCode(ClaudeCodeProviderConfig),
    /// Claude-compatible wrapper profile paired with its wrapper kind.
    ClaudeCompatible {
        /// Wrapper kind derived from the normalized provider driver.
        kind: ClaudeCompatibleProviderKind,
        /// Typed wrapper profile consumed by the Claude runtime.
        profile: ClaudeProviderProfile,
    },
}

/// Adapter failures while translating one compiled binding into typed runtime config.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum AdapterError {
    /// The binding driver does not map to one supported adapter path.
    #[error("provider driver `{driver}` is not supported by the typed adapter layer")]
    UnsupportedDriver {
        /// Normalized provider driver.
        driver: String,
    },
    /// The binding is missing the typed behavior block required by the target adapter.
    #[error("provider driver `{driver}` requires a typed behavior config")]
    MissingBehaviorConfig {
        /// Normalized provider driver.
        driver: String,
    },
    /// The binding carries the wrong behavior namespace for the chosen driver.
    #[error("expected `{expected}` behavior config but found `{found}`")]
    ConfigTypeMismatch {
        /// Expected typed behavior namespace or driver family.
        expected: String,
        /// Actual typed behavior namespace attached to the binding.
        found: String,
    },
}

impl ClassifiedError for AdapterError {
    fn error_code(&self) -> &'static str {
        match self {
            Self::UnsupportedDriver { .. } => "PROVIDER_ADAPTER_UNSUPPORTED_DRIVER",
            Self::MissingBehaviorConfig { .. } => "PROVIDER_ADAPTER_MISSING_BEHAVIOR_CONFIG",
            Self::ConfigTypeMismatch { .. } => "PROVIDER_ADAPTER_CONFIG_TYPE_MISMATCH",
        }
    }

    fn http_status(&self) -> u16 {
        422
    }
}

/// Builds a typed `CodexProviderConfig` from the compiled binding plus install-layer config.
///
/// Mapping:
/// - `ResolvedCodexBehaviorConfig.sandbox.sandbox_mode` ->
///   `CodexProviderConfig.sandbox.sandbox_mode`
/// - `ResolvedCodexBehaviorConfig.sandbox.sandbox_network_access` ->
///   `CodexProviderConfig.sandbox.sandbox_network_access`
/// - `ResolvedCodexBehaviorConfig.workspace.include_plan_tool` ->
///   `CodexProviderConfig.workspace.include_plan_tool`
/// - `ResolvedCodexBehaviorConfig.workspace.resume_last` ->
///   `CodexProviderConfig.workspace.resume_last`
/// - `ResolvedCodexBehaviorConfig.web_search` ->
///   `CodexProviderConfig.capability.web_search`
/// - `ResolvedCodexBehaviorConfig.rmcp_client` ->
///   `CodexProviderConfig.capability.rmcp_client`
/// - `ResolvedCodexBehaviorConfig.reasoning_summary` ->
///   `CodexProviderConfig.reasoning_summary`
/// - `ResolvedCodexBehaviorConfig.model_verbosity` ->
///   `CodexProviderConfig.model_verbosity`
/// - `ProviderBinding.defaults.reasoning_effort` ->
///   `CodexProviderConfig.reasoning_effort`
/// - `ProviderConfig.binary/env/args/cwd/shared_app_server_key/request_timeout_ms/
///   startup_timeout_ms/client_name/client_version` -> install/runtime fields on the config
///
/// `ProviderBinding.defaults.max_tokens` is intentionally left as a request-time concern
/// rather than being collapsed into the static Codex config.
pub fn binding_to_codex_config(
    binding: &ProviderBinding,
    install: &ProviderConfig,
) -> Result<CodexProviderConfig, AdapterError> {
    let behavior = expect_codex_behavior(binding)?;
    let mut config = codex_install_config(install);

    config.sandbox.sandbox_mode = behavior.sandbox.sandbox_mode.clone();
    config.sandbox.sandbox_network_access = behavior.sandbox.sandbox_network_access;
    config.workspace.include_plan_tool = behavior.workspace.include_plan_tool;
    config.workspace.resume_last = behavior.workspace.resume_last;
    config.capability.web_search = behavior.web_search;
    config.capability.rmcp_client = behavior.rmcp_client;
    config.reasoning_summary = behavior.reasoning_summary.clone();
    config.model_verbosity = behavior.model_verbosity.clone();
    config.reasoning_effort = reasoning_effort_to_string(&binding.defaults);

    Ok(config)
}

/// Builds a typed `ClaudeCodeProviderConfig` from the compiled binding plus install-layer config.
///
/// Mapping:
/// - `ResolvedClaudeCodeBehaviorConfig.session.continue_conversation` ->
///   `ClaudeCodeProviderConfig.session.continue_conversation`
/// - `ResolvedClaudeCodeBehaviorConfig.session.fork_session` ->
///   `ClaudeCodeProviderConfig.session.fork_session`
/// - `ResolvedClaudeCodeBehaviorConfig.filesystem.additional_directories` ->
///   `ClaudeCodeProviderConfig.filesystem.additional_directories`
/// - `ResolvedClaudeCodeBehaviorConfig.filesystem.enable_file_checkpointing` ->
///   `ClaudeCodeProviderConfig.filesystem.enable_file_checkpointing`
/// - `ResolvedClaudeCodeBehaviorConfig.allowed_tools` ->
///   `ClaudeCodeProviderConfig.allowed_tools`
/// - `ResolvedClaudeCodeBehaviorConfig.disallowed_tools` ->
///   `ClaudeCodeProviderConfig.disallowed_tools`
/// - `ResolvedClaudeCodeBehaviorConfig.mcp_servers` ->
///   `ClaudeCodeProviderConfig.mcp_servers`
/// - `ResolvedClaudeCodeBehaviorConfig.max_budget_usd` ->
///   `ClaudeCodeProviderConfig.max_budget_usd`
/// - `ResolvedClaudeCodeBehaviorConfig.fallback_model` ->
///   `ClaudeCodeProviderConfig.cli_behavior.fallback_model`
/// - `ProviderBinding.defaults.reasoning_effort` ->
///   `ClaudeCodeProviderConfig.reasoning_effort`
/// - `ProviderConfig.binary/env/args/cwd` -> install/runtime fields on the config
///
/// `ProviderBinding.defaults.max_tokens` remains a request-time concern instead of being
/// reinterpreted as a static CLI session budget.
pub fn binding_to_claude_code_config(
    binding: &ProviderBinding,
    install: &ProviderConfig,
) -> Result<ClaudeCodeProviderConfig, AdapterError> {
    let behavior = expect_claude_code_behavior(binding)?;
    Ok(claude_code_install_config(binding, install, behavior))
}

/// Builds a typed Claude-compatible wrapper profile from the compiled binding plus install config.
///
/// Mapping:
/// - `ResolvedClaudeCompatibleBehaviorConfig.base` ->
///   `ClaudeCompatibleProviderConfig` via the same field-by-field Claude Code mapping above
/// - `ResolvedClaudeCompatibleBehaviorConfig.selected_model` ->
///   wrapper-specific `selected_model` fields
/// - `ResolvedClaudeCompatibleBehaviorConfig.region` ->
///   `BedrockProviderConfig.region`
/// - `ResolvedClaudeCompatibleBehaviorConfig.project_id` ->
///   `VertexProviderConfig.project_id`
///
/// Fields that the selected wrapper cannot represent are not silently dropped; the adapter emits
/// `tracing::warn!()` before returning the profile.
pub fn binding_to_claude_compatible_config(
    binding: &ProviderBinding,
    kind: ClaudeCompatibleProviderKind,
    install: &ProviderConfig,
) -> Result<ClaudeProviderProfile, AdapterError> {
    let behavior = expect_claude_compatible_behavior(binding)?;
    warn_unsupported_wrapper_fields(kind, behavior);

    let base = claude_code_install_config(binding, install, &behavior.base);
    let selected_model = wrapper_selected_model(binding, behavior);

    let profile = match kind {
        ClaudeCompatibleProviderKind::ClaudeCode => ClaudeProviderProfile::ClaudeCode,
        ClaudeCompatibleProviderKind::Bedrock => {
            ClaudeProviderProfile::Bedrock(BedrockProviderConfig {
                base,
                selected_model: behavior.selected_model.clone(),
                region: behavior.region.clone(),
            })
        }
        ClaudeCompatibleProviderKind::Vertex => {
            ClaudeProviderProfile::Vertex(VertexProviderConfig {
                base,
                selected_model: behavior.selected_model.clone(),
                project_id: behavior.project_id.clone(),
            })
        }
        ClaudeCompatibleProviderKind::Zai => ClaudeProviderProfile::Zai(ZaiProviderConfig {
            base,
            api_key: wrapper_api_key(install, kind),
            selected_model,
        }),
        ClaudeCompatibleProviderKind::OpenRouter => {
            ClaudeProviderProfile::OpenRouter(OpenRouterProviderConfig {
                base,
                api_key: wrapper_api_key(install, kind),
                selected_model,
            })
        }
        ClaudeCompatibleProviderKind::Vercel => {
            ClaudeProviderProfile::Vercel(VercelProviderConfig {
                base,
                api_key: wrapper_api_key(install, kind),
                selected_model,
            })
        }
        ClaudeCompatibleProviderKind::Moonshot => {
            ClaudeProviderProfile::Moonshot(MoonshotProviderConfig {
                base,
                api_key: wrapper_api_key(install, kind),
                selected_model,
            })
        }
        ClaudeCompatibleProviderKind::Minimax => {
            ClaudeProviderProfile::Minimax(MinimaxProviderConfig {
                base,
                api_key: wrapper_api_key(install, kind),
                selected_model: behavior.selected_model.clone(),
            })
        }
        ClaudeCompatibleProviderKind::Ollama => {
            ClaudeProviderProfile::Ollama(OllamaProviderConfig {
                base,
                base_url: wrapper_base_url(install),
                selected_model,
            })
        }
    };

    Ok(profile)
}

/// Dispatches one compiled `ProviderBinding` into the typed runtime config required by Arky.
pub fn build_provider_config(
    binding: &ProviderBinding,
    install: &ProviderConfig,
) -> Result<CompozyProviderConfig, AdapterError> {
    let driver = normalize_driver(binding.driver.as_str());
    if driver == "codex" {
        return binding_to_codex_config(binding, install).map(CompozyProviderConfig::Codex);
    }

    match ClaudeCompatibleProviderKind::from_kind(driver.as_str()) {
        Some(ClaudeCompatibleProviderKind::ClaudeCode) => {
            binding_to_claude_code_config(binding, install).map(CompozyProviderConfig::ClaudeCode)
        }
        Some(kind) => binding_to_claude_compatible_config(binding, kind, install)
            .map(|profile| CompozyProviderConfig::ClaudeCompatible { kind, profile }),
        None => Err(AdapterError::UnsupportedDriver { driver }),
    }
}

fn expect_codex_behavior(
    binding: &ProviderBinding,
) -> Result<&ResolvedCodexBehaviorConfig, AdapterError> {
    match binding.config.as_ref() {
        Some(ResolvedProviderBehaviorConfig::Codex(config)) => Ok(config),
        Some(other) => Err(AdapterError::ConfigTypeMismatch {
            expected: "codex".to_owned(),
            found: behavior_config_name(other).to_owned(),
        }),
        None => Err(AdapterError::MissingBehaviorConfig {
            driver: binding.driver.clone(),
        }),
    }
}

fn expect_claude_code_behavior(
    binding: &ProviderBinding,
) -> Result<&ResolvedClaudeCodeBehaviorConfig, AdapterError> {
    match binding.config.as_ref() {
        Some(ResolvedProviderBehaviorConfig::ClaudeCode(config)) => Ok(config),
        Some(other) => Err(AdapterError::ConfigTypeMismatch {
            expected: "claude-code".to_owned(),
            found: behavior_config_name(other).to_owned(),
        }),
        None => Err(AdapterError::MissingBehaviorConfig {
            driver: binding.driver.clone(),
        }),
    }
}

fn expect_claude_compatible_behavior(
    binding: &ProviderBinding,
) -> Result<&ResolvedClaudeCompatibleBehaviorConfig, AdapterError> {
    match binding.config.as_ref() {
        Some(ResolvedProviderBehaviorConfig::ClaudeCompatible(config)) => Ok(config),
        Some(other) => Err(AdapterError::ConfigTypeMismatch {
            expected: "claude-compatible".to_owned(),
            found: behavior_config_name(other).to_owned(),
        }),
        None => Err(AdapterError::MissingBehaviorConfig {
            driver: binding.driver.clone(),
        }),
    }
}

fn behavior_config_name(config: &ResolvedProviderBehaviorConfig) -> &'static str {
    match config {
        ResolvedProviderBehaviorConfig::Codex(_) => "codex",
        ResolvedProviderBehaviorConfig::ClaudeCode(_) => "claude-code",
        ResolvedProviderBehaviorConfig::ClaudeCompatible(_) => "claude-compatible",
    }
}

fn codex_install_config(install: &ProviderConfig) -> CodexProviderConfig {
    let mut config = CodexProviderConfig {
        binary: install_binary(install),
        env: install.env().clone(),
        app_server_args: install.args().to_vec(),
        cwd: install.cwd().map(PathBuf::from),
        shared_app_server_key: install.shared_app_server_key().map(ToOwned::to_owned),
        ..CodexProviderConfig::default()
    };

    if let Some(timeout_ms) = install.request_timeout_ms() {
        config.request_timeout = Duration::from_millis(timeout_ms);
    }
    if let Some(timeout_ms) = install.startup_timeout_ms() {
        config.startup_timeout = Duration::from_millis(timeout_ms);
    }
    if let Some(client_name) = install.client_name() {
        config.client_name = client_name.to_owned();
    }
    if let Some(client_version) = install.client_version() {
        config.client_version = client_version.to_owned();
    }

    config
}

fn claude_code_install_config(
    binding: &ProviderBinding,
    install: &ProviderConfig,
    behavior: &ResolvedClaudeCodeBehaviorConfig,
) -> ClaudeCodeProviderConfig {
    let mut config = ClaudeCodeProviderConfig {
        binary: install_binary(install),
        cwd: install.cwd().map(PathBuf::from),
        extra_args: install.args().to_vec(),
        env: install.env().clone(),
        ..ClaudeCodeProviderConfig::default()
    };

    config.reasoning_effort = reasoning_effort_to_string(&binding.defaults);
    config.session.continue_conversation = behavior.session.continue_conversation;
    config.session.fork_session = behavior.session.fork_session;
    config.filesystem.additional_directories = behavior.filesystem.additional_directories.clone();
    config.filesystem.enable_file_checkpointing = behavior.filesystem.enable_file_checkpointing;
    config.allowed_tools = behavior.allowed_tools.clone();
    config.disallowed_tools = behavior.disallowed_tools.clone();
    config.mcp_servers = behavior.mcp_servers.clone();
    config.max_budget_usd = behavior.max_budget_usd;
    config.cli_behavior.fallback_model = behavior.fallback_model.clone();

    config
}

fn reasoning_effort_to_string(defaults: &ProviderRequestDefaults) -> Option<String> {
    defaults.reasoning_effort.map(reasoning_effort_label)
}

fn reasoning_effort_label(effort: ReasoningEffort) -> String {
    match effort {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::XHigh => "max",
    }
    .to_owned()
}

fn install_binary(install: &ProviderConfig) -> String {
    install
        .binary()
        .map(|binary| binary.to_string_lossy().into_owned())
        .unwrap_or_else(|| default_binary_for_kind(install.kind()).to_owned())
}

fn default_binary_for_kind(kind: &str) -> &str {
    match kind {
        "claude" | "claude-code" | "zai" | "openrouter" | "vercel" | "moonshot" | "minimax"
        | "bedrock" | "vertex" | "ollama" => "claude",
        "codex" => "codex",
        other => other,
    }
}

fn wrapper_selected_model(
    binding: &ProviderBinding,
    behavior: &ResolvedClaudeCompatibleBehaviorConfig,
) -> String {
    behavior
        .selected_model
        .clone()
        .or_else(|| binding.model.provider_model_id.clone())
        .unwrap_or_else(|| binding.model.model_id.clone())
}

fn wrapper_api_key(install: &ProviderConfig, kind: ClaudeCompatibleProviderKind) -> String {
    let candidate_keys = match kind {
        ClaudeCompatibleProviderKind::Zai => &["ZAI_API_KEY", "ANTHROPIC_API_KEY"][..],
        ClaudeCompatibleProviderKind::OpenRouter => {
            &["OPENROUTER_API_KEY", "ANTHROPIC_API_KEY"][..]
        }
        ClaudeCompatibleProviderKind::Vercel => &["VERCEL_API_KEY", "ANTHROPIC_API_KEY"][..],
        ClaudeCompatibleProviderKind::Moonshot => &["MOONSHOT_API_KEY", "ANTHROPIC_API_KEY"][..],
        ClaudeCompatibleProviderKind::Minimax => &["MINIMAX_API_KEY", "ANTHROPIC_API_KEY"][..],
        _ => &[][..],
    };

    let api_key = candidate_keys
        .iter()
        .find_map(|key| install.env().get(*key).cloned())
        .unwrap_or_default();
    if api_key.is_empty() && !candidate_keys.is_empty() {
        warn!(
            driver = kind.as_str(),
            env_keys = ?candidate_keys,
            "install-layer provider env does not include a recognized API key override"
        );
    }
    api_key
}

fn wrapper_base_url(install: &ProviderConfig) -> Option<String> {
    ["OLLAMA_BASE_URL", "ANTHROPIC_BASE_URL"]
        .iter()
        .find_map(|key| install.env().get(*key).cloned())
}

fn warn_unsupported_wrapper_fields(
    kind: ClaudeCompatibleProviderKind,
    behavior: &ResolvedClaudeCompatibleBehaviorConfig,
) {
    if behavior.base.session.fork_session && kind == ClaudeCompatibleProviderKind::Bedrock {
        warn!(
            driver = kind.as_str(),
            field = "fork_session",
            "claude-compatible behavior field is not supported by this wrapper and will be ignored"
        );
    }

    if behavior.region.is_some() && kind != ClaudeCompatibleProviderKind::Bedrock {
        warn!(
            driver = kind.as_str(),
            field = "region",
            "claude-compatible behavior field is not supported by this wrapper and will be ignored"
        );
    }

    if behavior.project_id.is_some() && kind != ClaudeCompatibleProviderKind::Vertex {
        warn!(
            driver = kind.as_str(),
            field = "project_id",
            "claude-compatible behavior field is not supported by this wrapper and will be ignored"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use arky_claude_code::{
        ClaudeCompatibleProviderKind, ClaudeFilesystemConfig, ClaudeProviderProfile,
        ClaudeSessionConfig,
    };
    use arky_codex::{CodexProvider, CodexSandboxConfig, CodexWorkspaceConfig};
    use arky_config::{
        AgentConfigBuilder, ArkyConfigBuilder, ConfigLoader, ProviderConfigBuilder,
        ProviderRequestDefaults, ResolvedClaudeCodeBehaviorConfig,
        ResolvedClaudeCompatibleBehaviorConfig, ResolvedCodexBehaviorConfig,
        ResolvedProviderBehaviorConfig,
    };
    use arky_protocol::{ModelRef, ProviderId, ReasoningEffort};
    use arky_provider::Provider;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use tempfile::tempdir;
    use tracing::{Event, Subscriber};
    use tracing_subscriber::{
        layer::{Context, SubscriberExt},
        registry::LookupSpan,
        Layer,
    };

    use super::{
        binding_to_claude_code_config, binding_to_claude_compatible_config,
        binding_to_codex_config, build_provider_config, AdapterError, CompozyProviderConfig,
    };
    use crate::{compile_provider_binding, ProviderBinding};

    #[test]
    fn codex_binding_should_map_sandbox_mode_to_codex_provider_config() {
        let binding = codex_binding(ReasoningEffort::Medium);

        let config =
            binding_to_codex_config(&binding, &install_config("codex")).expect("config should map");

        assert_eq!(
            config.sandbox.sandbox_mode.as_deref(),
            Some("workspace-write")
        );
        assert_eq!(config.sandbox.sandbox_network_access, Some(true));
    }

    #[test]
    fn codex_binding_should_map_web_search_and_reasoning_summary() {
        let binding = codex_binding(ReasoningEffort::Medium);

        let config =
            binding_to_codex_config(&binding, &install_config("codex")).expect("config should map");

        assert_eq!(config.capability.web_search, true);
        assert_eq!(config.capability.rmcp_client, true);
        assert_eq!(config.reasoning_summary.as_deref(), Some("detailed"));
    }

    #[test]
    fn claude_code_binding_should_map_session_and_filesystem_fields() {
        let binding = claude_code_binding(ReasoningEffort::Medium);

        let config = binding_to_claude_code_config(&binding, &install_config("claude-code"))
            .expect("config should map");

        assert_eq!(
            config.session,
            ClaudeSessionConfig {
                continue_conversation: true,
                resume: None,
                session_id: None,
                resume_session_at: None,
                persist_session: false,
                fork_session: true,
            }
        );
        assert_eq!(
            config.filesystem,
            ClaudeFilesystemConfig {
                additional_directories: vec![
                    PathBuf::from("/workspace/docs"),
                    PathBuf::from("/workspace/notes"),
                ],
                enable_file_checkpointing: true,
            }
        );
    }

    #[test]
    fn claude_code_binding_should_map_allowed_and_disallowed_tools() {
        let binding = claude_code_binding(ReasoningEffort::Medium);

        let config = binding_to_claude_code_config(&binding, &install_config("claude-code"))
            .expect("config should map");

        assert_eq!(
            config.allowed_tools,
            vec!["read_file".to_owned(), "edit_file".to_owned()]
        );
        assert_eq!(config.disallowed_tools, vec!["bash".to_owned()]);
    }

    #[test]
    fn claude_code_binding_should_map_mcp_servers_and_budget() {
        let binding = claude_code_binding(ReasoningEffort::Medium);

        let config = binding_to_claude_code_config(&binding, &install_config("claude-code"))
            .expect("config should map");

        assert_eq!(
            config.mcp_servers,
            BTreeMap::from([("filesystem".to_owned(), json!({ "transport": "stdio" }),)])
        );
        assert_eq!(config.max_budget_usd, Some(12.5));
    }

    #[test]
    fn claude_code_cli_args_should_reflect_binding_fields() {
        let binding = claude_code_binding(ReasoningEffort::High);

        let config = binding_to_claude_code_config(&binding, &install_config("claude-code"))
            .expect("config should map");
        let args = config
            .cli_args("Summarize the repo".to_owned(), "sonnet".to_owned(), None)
            .expect("cli args should build");

        assert_flag_value(&args, "--allowed-tools", "read_file,edit_file");
        assert_flag_present(&args, "--fork-session");
        assert_flag_value(&args, "--max-budget-usd", "12.5");
        assert_flag_value(&args, "--effort", "high");
    }

    #[test]
    fn claude_compatible_bedrock_binding_should_set_region() {
        let binding = bedrock_binding();

        let profile = binding_to_claude_compatible_config(
            &binding,
            ClaudeCompatibleProviderKind::Bedrock,
            &install_config("bedrock"),
        )
        .expect("profile should map");

        match profile {
            ClaudeProviderProfile::Bedrock(config) => {
                assert_eq!(config.region.as_deref(), Some("us-east-1"));
            }
            other => panic!("expected bedrock profile, got {other:?}"),
        }
    }

    #[test]
    fn adapter_should_warn_when_wrapper_specific_fields_are_ignored() {
        let binding = openrouter_binding_with_region();
        let install = install_config("openrouter");
        let warnings = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(CaptureWarningsLayer {
            warnings: Arc::clone(&warnings),
        });

        tracing::subscriber::with_default(subscriber, || {
            let _ = binding_to_claude_compatible_config(
                &binding,
                ClaudeCompatibleProviderKind::OpenRouter,
                &install,
            )
            .expect("profile should map");
        });

        let warnings = warnings.lock().expect("warnings should be readable");
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].as_str(), "region");
    }

    #[test]
    fn adapter_should_warn_when_bedrock_ignores_fork_session() {
        let binding = bedrock_binding_with_fork_session();
        let install = install_config("bedrock");
        let warnings = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(CaptureWarningsLayer {
            warnings: Arc::clone(&warnings),
        });

        tracing::subscriber::with_default(subscriber, || {
            let _ = binding_to_claude_compatible_config(
                &binding,
                ClaudeCompatibleProviderKind::Bedrock,
                &install,
            )
            .expect("profile should map");
        });

        let warnings = warnings.lock().expect("warnings should be readable");
        assert_eq!(warnings.as_slice(), ["fork_session"]);
    }

    #[test]
    fn adapter_should_reject_codex_config_for_claude_code_driver() {
        let binding = ProviderBinding {
            driver: "claude-code".to_owned(),
            provider_id: ProviderId::new("claude-code"),
            model: ModelRef::new("sonnet").with_provider_id(ProviderId::new("claude-code")),
            profile: None,
            defaults: ProviderRequestDefaults::default(),
            config: Some(codex_behavior()),
        };

        let error = binding_to_claude_code_config(&binding, &install_config("claude-code"))
            .expect_err("config mismatch should fail");

        assert_eq!(
            error,
            AdapterError::ConfigTypeMismatch {
                expected: "claude-code".to_owned(),
                found: "codex".to_owned(),
            }
        );
    }

    #[test]
    fn adapter_should_reject_binding_with_no_behavior_config_when_required() {
        let binding = ProviderBinding {
            driver: "codex".to_owned(),
            provider_id: ProviderId::new("codex"),
            model: ModelRef::new("gpt-5-mini").with_provider_id(ProviderId::new("codex")),
            profile: None,
            defaults: ProviderRequestDefaults::default(),
            config: None,
        };

        let error = binding_to_codex_config(&binding, &install_config("codex"))
            .expect_err("missing config should fail");

        assert_eq!(
            error,
            AdapterError::MissingBehaviorConfig {
                driver: "codex".to_owned(),
            }
        );
    }

    #[test]
    fn reasoning_effort_should_serialize_to_string_in_adapter() {
        let codex = codex_binding(ReasoningEffort::High);
        let claude = claude_code_binding(ReasoningEffort::High);

        let codex_config =
            binding_to_codex_config(&codex, &install_config("codex")).expect("codex should map");
        let claude_config = binding_to_claude_code_config(&claude, &install_config("claude-code"))
            .expect("claude should map");

        assert_eq!(codex_config.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(claude_config.reasoning_effort.as_deref(), Some("high"));
    }

    #[test]
    fn codex_agent_toml_should_compile_and_adapt_to_typed_config() {
        let directory = tempdir().expect("tempdir should exist");
        let path = directory.path().join("codex-agent.toml");
        fs::write(
            &path,
            r#"
                [providers.default]
                driver = "codex"
                binary = "missing-codex"

                [agents.writer]
                provider = "default"
                model = "gpt-4o"

                [agents.writer.config.codex]
                include_plan_tool = true
                web_search = true
            "#,
        )
        .expect("config should be written");

        let resolved = ConfigLoader::from_path(&path)
            .load()
            .expect("config should load")
            .resolve_agent_provider("writer")
            .expect("agent should resolve");
        let install = resolved.install.clone();
        let binding = compile_provider_binding(resolved).expect("binding should compile");
        let adapted = build_provider_config(&binding, &install).expect("config should adapt");

        match adapted {
            CompozyProviderConfig::Codex(config) => {
                assert_eq!(config.workspace.include_plan_tool, true);
                assert_eq!(config.capability.web_search, true);
            }
            other => panic!("expected codex config, got {other:?}"),
        }
    }

    #[test]
    fn claude_code_agent_toml_should_compile_and_adapt_allowed_tools() {
        let directory = tempdir().expect("tempdir should exist");
        let path = directory.path().join("claude-agent.toml");
        fs::write(
            &path,
            r#"
                [providers.default]
                driver = "claude-code"
                binary = "missing-claude"

                [agents.writer]
                provider = "default"
                model = "sonnet"

                [agents.writer.config.claude_code]
                fork_session = true
                allowed_tools = ["read_file", "edit_file"]
            "#,
        )
        .expect("config should be written");

        let resolved = ConfigLoader::from_path(&path)
            .load()
            .expect("config should load")
            .resolve_agent_provider("writer")
            .expect("agent should resolve");
        let install = resolved.install.clone();
        let binding = compile_provider_binding(resolved).expect("binding should compile");
        let adapted = build_provider_config(&binding, &install).expect("config should adapt");

        match adapted {
            CompozyProviderConfig::ClaudeCode(config) => {
                assert_eq!(
                    config.allowed_tools,
                    vec!["read_file".to_owned(), "edit_file".to_owned()]
                );
                assert_eq!(config.session.fork_session, true);
            }
            other => panic!("expected claude-code config, got {other:?}"),
        }
    }

    #[test]
    fn bedrock_agent_toml_should_compile_and_adapt_region() {
        let directory = tempdir().expect("tempdir should exist");
        let path = directory.path().join("bedrock-agent.toml");
        fs::write(
            &path,
            r#"
                [providers.default]
                driver = "bedrock"
                binary = "missing-claude"

                [agents.writer]
                provider = "default"
                model = "sonnet"

                [agents.writer.config.claude_compatible]
                region = "eu-west-1"
            "#,
        )
        .expect("config should be written");

        let resolved = ConfigLoader::from_path(&path)
            .load()
            .expect("config should load")
            .resolve_agent_provider("writer")
            .expect("agent should resolve");
        let install = resolved.install.clone();
        let binding = compile_provider_binding(resolved).expect("binding should compile");
        let adapted = build_provider_config(&binding, &install).expect("config should adapt");

        match adapted {
            CompozyProviderConfig::ClaudeCompatible {
                kind: ClaudeCompatibleProviderKind::Bedrock,
                profile: ClaudeProviderProfile::Bedrock(config),
            } => {
                assert_eq!(config.region.as_deref(), Some("eu-west-1"));
            }
            other => panic!("expected bedrock config, got {other:?}"),
        }
    }

    #[test]
    fn compozy_codex_provider_config_should_construct_a_codex_provider() {
        let binding = codex_binding(ReasoningEffort::Medium);

        let adapted =
            build_provider_config(&binding, &install_config("codex")).expect("config should map");

        match adapted {
            CompozyProviderConfig::Codex(config) => {
                let provider = CodexProvider::with_config(config.clone());
                assert_eq!(provider.descriptor().id.as_str(), "codex");
            }
            other => panic!("expected codex config, got {other:?}"),
        }
    }

    fn assert_flag_present(args: &[String], flag: &str) {
        assert_eq!(args.iter().any(|arg| arg == flag), true);
    }

    fn assert_flag_value(args: &[String], flag: &str, value: &str) {
        assert_eq!(
            args.windows(2)
                .any(|window| window == [flag.to_owned(), value.to_owned()]),
            true
        );
    }

    fn codex_binding(reasoning_effort: ReasoningEffort) -> ProviderBinding {
        ProviderBinding {
            driver: "codex".to_owned(),
            provider_id: ProviderId::new("codex"),
            model: ModelRef::new("gpt-5-mini").with_provider_id(ProviderId::new("codex")),
            profile: Some("fast-research".to_owned()),
            defaults: ProviderRequestDefaults {
                max_tokens: Some(2_048),
                reasoning_effort: Some(reasoning_effort),
            },
            config: Some(codex_behavior()),
        }
    }

    fn claude_code_binding(reasoning_effort: ReasoningEffort) -> ProviderBinding {
        ProviderBinding {
            driver: "claude-code".to_owned(),
            provider_id: ProviderId::new("claude-code"),
            model: ModelRef::new("sonnet").with_provider_id(ProviderId::new("claude-code")),
            profile: Some("safe-doc-writer".to_owned()),
            defaults: ProviderRequestDefaults {
                max_tokens: Some(1_024),
                reasoning_effort: Some(reasoning_effort),
            },
            config: Some(claude_code_behavior()),
        }
    }

    fn bedrock_binding() -> ProviderBinding {
        ProviderBinding {
            driver: "bedrock".to_owned(),
            provider_id: ProviderId::new("bedrock"),
            model: ModelRef::new("sonnet").with_provider_id(ProviderId::new("bedrock")),
            profile: Some("bedrock-safe".to_owned()),
            defaults: ProviderRequestDefaults {
                max_tokens: Some(1_024),
                reasoning_effort: Some(ReasoningEffort::Medium),
            },
            config: Some(bedrock_behavior()),
        }
    }

    fn bedrock_binding_with_fork_session() -> ProviderBinding {
        ProviderBinding {
            config: Some(ResolvedProviderBehaviorConfig::ClaudeCompatible(Box::new(
                ResolvedClaudeCompatibleBehaviorConfig {
                    base: ResolvedClaudeCodeBehaviorConfig {
                        session: ClaudeSessionConfig {
                            continue_conversation: false,
                            resume: None,
                            session_id: None,
                            resume_session_at: None,
                            persist_session: false,
                            fork_session: true,
                        },
                        filesystem: ClaudeFilesystemConfig::default(),
                        allowed_tools: Vec::new(),
                        disallowed_tools: Vec::new(),
                        mcp_servers: BTreeMap::new(),
                        max_budget_usd: None,
                        fallback_model: None,
                    },
                    selected_model: Some("anthropic.claude-3-5-sonnet-v1:0".to_owned()),
                    region: Some("us-east-1".to_owned()),
                    project_id: None,
                },
            ))),
            ..bedrock_binding()
        }
    }

    fn openrouter_binding_with_region() -> ProviderBinding {
        ProviderBinding {
            driver: "openrouter".to_owned(),
            provider_id: ProviderId::new("openrouter"),
            model: ModelRef::new("sonnet").with_provider_id(ProviderId::new("openrouter")),
            profile: None,
            defaults: ProviderRequestDefaults::default(),
            config: Some(ResolvedProviderBehaviorConfig::ClaudeCompatible(Box::new(
                ResolvedClaudeCompatibleBehaviorConfig {
                    base: ResolvedClaudeCodeBehaviorConfig {
                        session: ClaudeSessionConfig::default(),
                        filesystem: ClaudeFilesystemConfig::default(),
                        allowed_tools: Vec::new(),
                        disallowed_tools: Vec::new(),
                        mcp_servers: BTreeMap::new(),
                        max_budget_usd: None,
                        fallback_model: None,
                    },
                    selected_model: Some("openrouter/anthropic/claude-sonnet".to_owned()),
                    region: Some("us-east-1".to_owned()),
                    project_id: None,
                },
            ))),
        }
    }

    fn install_config(driver: &str) -> arky_config::ProviderConfig {
        ArkyConfigBuilder::new()
            .provider(
                "default",
                ProviderConfigBuilder::new()
                    .driver(driver)
                    .binary(format!("missing-{driver}"))
                    .args(["--stdio"])
                    .env("ANTHROPIC_API_KEY", "install-anthropic-key")
                    .env("OPENROUTER_API_KEY", "install-openrouter-key")
                    .env("OLLAMA_BASE_URL", "http://localhost:11434")
                    .env("FROM_PROVIDER", "provider"),
            )
            .agent(
                "writer",
                AgentConfigBuilder::new()
                    .provider("default")
                    .model("fixture-model"),
            )
            .build()
            .expect("fixture config should build")
            .resolve_agent_provider("writer")
            .expect("fixture provider should resolve")
            .install
    }

    fn codex_behavior() -> ResolvedProviderBehaviorConfig {
        ResolvedProviderBehaviorConfig::Codex(ResolvedCodexBehaviorConfig {
            sandbox: CodexSandboxConfig {
                sandbox_mode: Some("workspace-write".to_owned()),
                sandbox_writable_roots: Vec::new(),
                sandbox_network_access: Some(true),
                full_auto: false,
                dangerously_bypass_approvals_and_sandbox: false,
                exclusions: Default::default(),
            },
            workspace: CodexWorkspaceConfig {
                skip_git_repo_check: false,
                include_plan_tool: true,
                resume_last: true,
            },
            web_search: true,
            rmcp_client: true,
            reasoning_summary: Some("detailed".to_owned()),
            model_verbosity: Some("high".to_owned()),
        })
    }

    fn claude_code_behavior() -> ResolvedProviderBehaviorConfig {
        ResolvedProviderBehaviorConfig::ClaudeCode(ResolvedClaudeCodeBehaviorConfig {
            session: ClaudeSessionConfig {
                continue_conversation: true,
                resume: None,
                session_id: None,
                resume_session_at: None,
                persist_session: false,
                fork_session: true,
            },
            filesystem: ClaudeFilesystemConfig {
                additional_directories: vec![
                    PathBuf::from("/workspace/docs"),
                    PathBuf::from("/workspace/notes"),
                ],
                enable_file_checkpointing: true,
            },
            allowed_tools: vec!["read_file".to_owned(), "edit_file".to_owned()],
            disallowed_tools: vec!["bash".to_owned()],
            mcp_servers: BTreeMap::from([(
                "filesystem".to_owned(),
                json!({ "transport": "stdio" }),
            )]),
            max_budget_usd: Some(12.5),
            fallback_model: Some("haiku".to_owned()),
        })
    }

    fn bedrock_behavior() -> ResolvedProviderBehaviorConfig {
        ResolvedProviderBehaviorConfig::ClaudeCompatible(Box::new(
            ResolvedClaudeCompatibleBehaviorConfig {
                base: ResolvedClaudeCodeBehaviorConfig {
                    session: ClaudeSessionConfig {
                        continue_conversation: false,
                        resume: None,
                        session_id: None,
                        resume_session_at: None,
                        persist_session: false,
                        fork_session: false,
                    },
                    filesystem: ClaudeFilesystemConfig::default(),
                    allowed_tools: vec!["read_file".to_owned()],
                    disallowed_tools: Vec::new(),
                    mcp_servers: BTreeMap::new(),
                    max_budget_usd: Some(5.0),
                    fallback_model: Some("haiku".to_owned()),
                },
                selected_model: Some("anthropic.claude-3-5-sonnet-v1:0".to_owned()),
                region: Some("us-east-1".to_owned()),
                project_id: None,
            },
        ))
    }

    #[derive(Clone, Debug)]
    struct CaptureWarningsLayer {
        warnings: Arc<Mutex<Vec<String>>>,
    }

    impl<S> Layer<S> for CaptureWarningsLayer
    where
        S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    {
        fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
            struct Visitor {
                field: Option<String>,
            }

            impl tracing::field::Visit for Visitor {
                fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                    if field.name() == "field" {
                        self.field = Some(value.to_owned());
                    }
                }

                fn record_debug(
                    &mut self,
                    _field: &tracing::field::Field,
                    _value: &dyn std::fmt::Debug,
                ) {
                }
            }

            let mut visitor = Visitor { field: None };
            event.record(&mut visitor);

            if let Some(field) = visitor.field {
                self.warnings
                    .lock()
                    .expect("warnings should be writable")
                    .push(field);
            }
        }
    }
}
