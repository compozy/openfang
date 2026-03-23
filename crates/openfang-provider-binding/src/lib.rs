//! Provider binding compile layer for Compozy agents.
//!
//! This crate owns the derived provider IR that sits between file-backed
//! `agent_definition` documents and the Arky runtime registry.

use std::collections::BTreeMap;

use arky_claude_code::{ClaudeCompatibleProviderKind, CLAUDE_COMPATIBLE_PROVIDER_IDS};
use arky_config::{
    normalize_driver, validate_request_extra, ProviderConfig, ProviderRequestDefaults,
    ResolvedAgentProviderConfig, ResolvedProviderBehaviorConfig, ValidationIssue,
};
use arky_error::ClassifiedError;
use arky_protocol::{
    Message, ModelRef, ProviderId, ProviderSettings, SessionRef, ToolContext, ToolDefinition,
    TurnContext, TurnId,
};
use arky_provider::{validate_capabilities, ProviderCapabilities, ProviderRequest};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

/// Runtime-safe compiled provider binding derived from layered config input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ProviderBinding {
    /// Normalized provider driver name.
    pub driver: String,
    /// Canonical provider identifier used for registry lookup.
    pub provider_id: ProviderId,
    /// Fully qualified model reference used when constructing requests.
    pub model: ModelRef,
    /// Optional reusable profile reference preserved for observability.
    pub profile: Option<String>,
    /// Portable request defaults copied from the resolved provider config.
    pub defaults: ProviderRequestDefaults,
    /// Resolved provider-specific behavior config.
    pub config: Option<ResolvedProviderBehaviorConfig>,
}

impl ProviderBinding {
    fn new(
        driver: String,
        provider_id: ProviderId,
        model: ModelRef,
        profile: Option<String>,
        defaults: ProviderRequestDefaults,
        config: Option<ResolvedProviderBehaviorConfig>,
    ) -> Self {
        Self {
            driver,
            provider_id,
            model,
            profile,
            defaults,
            config,
        }
    }
}

/// Compile-time failures when turning layered provider config into a binding.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CompileError {
    /// The resolved driver does not map to a known provider runtime.
    #[error("provider driver `{driver}` is not supported")]
    UnknownDriver {
        /// Normalized driver name.
        driver: String,
    },
    /// The resolved config requires a capability the target driver cannot satisfy.
    #[error("provider driver `{driver}` does not support capability `{capability}`: {message}")]
    CapabilityMismatch {
        /// Stable capability key.
        capability: String,
        /// Normalized driver name.
        driver: String,
        /// Human-readable mismatch detail.
        message: String,
    },
    /// The layered config did not resolve a runtime model.
    #[error("provider driver `{driver}` requires a resolved model")]
    MissingModel {
        /// Normalized driver name.
        driver: String,
    },
    /// The bounded `request_extra` payload still violates the install-layer boundary.
    #[error("invalid provider request_extra field `{field}`: {message}")]
    InvalidRequestExtra {
        /// Failing request-extra field path.
        field: String,
        /// Human-readable validation message.
        message: String,
    },
}

impl ClassifiedError for CompileError {
    fn error_code(&self) -> &'static str {
        match self {
            Self::UnknownDriver { .. } => "PROVIDER_BINDING_UNKNOWN_DRIVER",
            Self::CapabilityMismatch { .. } => "PROVIDER_BINDING_CAPABILITY_MISMATCH",
            Self::MissingModel { .. } => "PROVIDER_BINDING_MISSING_MODEL",
            Self::InvalidRequestExtra { .. } => "PROVIDER_BINDING_INVALID_REQUEST_EXTRA",
        }
    }

    fn http_status(&self) -> u16 {
        422
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DriverKind {
    Codex,
    ClaudeCode,
    ClaudeCompatible,
}

/// Compiles the resolved provider layer into a runtime-safe provider binding.
pub fn compile_provider_binding(
    resolved: ResolvedAgentProviderConfig<ProviderConfig>,
) -> Result<ProviderBinding, CompileError> {
    let ResolvedAgentProviderConfig {
        driver,
        profile,
        model,
        defaults,
        config,
        request_extra,
        ..
    } = resolved;

    validate_request_extra_fields(&request_extra)?;

    let driver = normalize_driver(driver.as_str());
    let provider_id = resolve_provider_id(driver.as_str())?;
    validate_behavior_namespace(driver.as_str(), config.as_ref())?;

    let model = build_model_ref(
        driver.as_str(),
        &provider_id,
        model.ok_or_else(|| CompileError::MissingModel {
            driver: driver.clone(),
        })?,
        config.as_ref(),
    )?;

    validate_driver_capabilities(driver.as_str(), &model, &defaults, config.as_ref())?;

    Ok(ProviderBinding::new(
        driver,
        provider_id,
        model,
        profile,
        defaults,
        config,
    ))
}

/// Resolves the canonical provider identifier used by the runtime registry.
pub fn resolve_provider_id(driver: &str) -> Result<ProviderId, CompileError> {
    let normalized_driver = normalize_driver(driver);
    let _ = resolve_driver_kind(normalized_driver.as_str())?;

    Ok(ProviderId::new(normalized_driver))
}

fn resolve_driver_kind(driver: &str) -> Result<DriverKind, CompileError> {
    let normalized_driver = normalize_driver(driver);
    if normalized_driver == "codex" {
        return Ok(DriverKind::Codex);
    }

    if !CLAUDE_COMPATIBLE_PROVIDER_IDS.contains(&normalized_driver.as_str()) {
        return Err(CompileError::UnknownDriver {
            driver: normalized_driver,
        });
    }

    match ClaudeCompatibleProviderKind::from_kind(normalized_driver.as_str()) {
        Some(ClaudeCompatibleProviderKind::ClaudeCode) => Ok(DriverKind::ClaudeCode),
        Some(_) => Ok(DriverKind::ClaudeCompatible),
        None => Err(CompileError::UnknownDriver {
            driver: normalized_driver,
        }),
    }
}

fn validate_request_extra_fields(
    request_extra: &BTreeMap<String, Value>,
) -> Result<(), CompileError> {
    let mut issues = Vec::new();
    validate_request_extra(request_extra, "provider.request_extra", &mut issues);
    if let Some(issue) = issues.into_iter().next() {
        return Err(issue_to_compile_error(issue));
    }
    Ok(())
}

fn issue_to_compile_error(issue: ValidationIssue) -> CompileError {
    CompileError::InvalidRequestExtra {
        field: issue.field().to_owned(),
        message: issue.message().to_owned(),
    }
}

fn validate_behavior_namespace(
    driver: &str,
    config: Option<&ResolvedProviderBehaviorConfig>,
) -> Result<(), CompileError> {
    let Some(config) = config else {
        return Ok(());
    };

    let expected = resolve_driver_kind(driver)?;
    let actual = match config {
        ResolvedProviderBehaviorConfig::Codex(_) => DriverKind::Codex,
        ResolvedProviderBehaviorConfig::ClaudeCode(_) => DriverKind::ClaudeCode,
        ResolvedProviderBehaviorConfig::ClaudeCompatible(_) => DriverKind::ClaudeCompatible,
    };

    if actual == expected {
        return Ok(());
    }

    Err(CompileError::CapabilityMismatch {
        capability: "config_namespace".to_owned(),
        driver: driver.to_owned(),
        message: format!(
            "resolved config namespace `{}` does not match driver `{driver}`",
            driver_kind_label(actual)
        ),
    })
}

fn build_model_ref(
    driver: &str,
    provider_id: &ProviderId,
    model_id: String,
    config: Option<&ResolvedProviderBehaviorConfig>,
) -> Result<ModelRef, CompileError> {
    let mut model = ModelRef::new(model_id).with_provider_id(provider_id.clone());

    if matches!(resolve_driver_kind(driver)?, DriverKind::ClaudeCompatible) {
        if let Some(ResolvedProviderBehaviorConfig::ClaudeCompatible(config)) = config {
            if let Some(provider_model_id) = &config.selected_model {
                model = model.with_provider_model_id(provider_model_id.clone());
            }
        }
    }

    Ok(model)
}

fn validate_driver_capabilities(
    driver: &str,
    model: &ModelRef,
    defaults: &ProviderRequestDefaults,
    config: Option<&ResolvedProviderBehaviorConfig>,
) -> Result<(), CompileError> {
    let capabilities = driver_capabilities(driver)?;
    let request = capability_probe_request(model, defaults, config);

    if let Some(warning) = validate_capabilities(&request, &capabilities)
        .into_iter()
        .next()
    {
        return Err(CompileError::CapabilityMismatch {
            capability: warning.capability.into_owned(),
            driver: driver.to_owned(),
            message: warning.message,
        });
    }

    if requests_mcp_passthrough(config) && !capabilities.mcp_passthrough {
        return Err(CompileError::CapabilityMismatch {
            capability: "mcp_passthrough".to_owned(),
            driver: driver.to_owned(),
            message: "MCP server passthrough is not supported by this provider".to_owned(),
        });
    }

    Ok(())
}

fn capability_probe_request(
    model: &ModelRef,
    defaults: &ProviderRequestDefaults,
    config: Option<&ResolvedProviderBehaviorConfig>,
) -> ProviderRequest {
    let mut request = ProviderRequest::new(
        SessionRef::new(None),
        TurnContext::new(TurnId::new(), 1),
        model.clone(),
        vec![Message::user("provider capability probe")],
    );

    let mut settings = ProviderSettings::new();
    settings.reasoning_effort = defaults.reasoning_effort;
    request = request.with_settings(settings);

    if requests_tool_calls(config) {
        request =
            request.with_tools(
                ToolContext::new().with_definitions(vec![ToolDefinition::new(
                    "compile_probe_tool",
                    "capability validation probe",
                    Value::Object(Map::new()),
                )]),
            );
    }

    if requests_session_resume(config) {
        request.session = request
            .session
            .with_provider_session_id("compile-probe-session");
    }

    request
}

fn requests_tool_calls(config: Option<&ResolvedProviderBehaviorConfig>) -> bool {
    match config {
        Some(ResolvedProviderBehaviorConfig::ClaudeCode(config)) => {
            !config.allowed_tools.is_empty()
                || !config.disallowed_tools.is_empty()
                || !config.mcp_servers.is_empty()
        }
        Some(ResolvedProviderBehaviorConfig::ClaudeCompatible(config)) => {
            !config.base.allowed_tools.is_empty()
                || !config.base.disallowed_tools.is_empty()
                || !config.base.mcp_servers.is_empty()
        }
        _ => false,
    }
}

fn requests_mcp_passthrough(config: Option<&ResolvedProviderBehaviorConfig>) -> bool {
    match config {
        Some(ResolvedProviderBehaviorConfig::ClaudeCode(config)) => !config.mcp_servers.is_empty(),
        Some(ResolvedProviderBehaviorConfig::ClaudeCompatible(config)) => {
            !config.base.mcp_servers.is_empty()
        }
        _ => false,
    }
}

fn requests_session_resume(config: Option<&ResolvedProviderBehaviorConfig>) -> bool {
    match config {
        Some(ResolvedProviderBehaviorConfig::Codex(config)) => config.workspace.resume_last,
        Some(ResolvedProviderBehaviorConfig::ClaudeCode(config)) => {
            config.session.continue_conversation || config.session.fork_session
        }
        Some(ResolvedProviderBehaviorConfig::ClaudeCompatible(config)) => {
            config.base.session.continue_conversation || config.base.session.fork_session
        }
        None => false,
    }
}

fn driver_capabilities(driver: &str) -> Result<ProviderCapabilities, CompileError> {
    match resolve_driver_kind(driver)? {
        DriverKind::Codex => Ok(ProviderCapabilities::new()
            .with_streaming(true)
            .with_generate(true)
            .with_tool_calls(true)
            .with_mcp_passthrough(true)
            .with_session_resume(true)
            .with_steering(true)
            .with_follow_up(true)),
        DriverKind::ClaudeCode | DriverKind::ClaudeCompatible => Ok(ProviderCapabilities::new()
            .with_streaming(true)
            .with_generate(true)
            .with_tool_calls(true)
            .with_mcp_passthrough(true)
            .with_session_resume(true)),
    }
}

fn driver_kind_label(kind: DriverKind) -> &'static str {
    match kind {
        DriverKind::Codex => "codex",
        DriverKind::ClaudeCode => "claude_code",
        DriverKind::ClaudeCompatible => "claude_compatible",
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs};

    use arky_claude_code::{ClaudeCodeProvider, ClaudeFilesystemConfig, ClaudeSessionConfig};
    use arky_codex::CodexProvider;
    use arky_config::{
        AgentConfigBuilder, ArkyConfigBuilder, ClaudeCodeBehaviorLayer,
        ClaudeCompatibleBehaviorLayer, CodexBehaviorLayer, ConfigLoader, ProviderBehaviorLayer,
        ProviderConfigBuilder, ProviderRequestDefaults, ResolvedAgentProviderConfig,
        ResolvedProviderBehaviorConfig,
    };
    use arky_error::ClassifiedError;
    use arky_protocol::ReasoningEffort;
    use arky_provider::{Provider, ProviderRegistry};
    use pretty_assertions::assert_eq;
    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::{compile_provider_binding, driver_capabilities, resolve_provider_id, CompileError};

    #[test]
    fn provider_binding_should_capture_driver_model_profile_defaults_and_config() {
        let defaults = ProviderRequestDefaults {
            max_tokens: Some(2_048),
            reasoning_effort: None,
        };
        let config = Some(resolved_codex_behavior(true));

        let binding = compile_provider_binding(resolved_config(
            "codex",
            Some("gpt-5-mini"),
            Some("fast-research"),
            defaults.clone(),
            config.clone(),
            BTreeMap::new(),
        ))
        .expect("binding should compile");

        assert_eq!(binding.driver, "codex");
        assert_eq!(binding.provider_id.as_str(), "codex");
        assert_eq!(binding.model.model_id, "gpt-5-mini");
        assert_eq!(binding.model.provider_id, Some(binding.provider_id.clone()));
        assert_eq!(binding.profile.as_deref(), Some("fast-research"));
        assert_eq!(binding.defaults, defaults);
        assert_eq!(binding.config, config);
    }

    #[test]
    fn provider_binding_should_normalize_claude_alias_to_claude_code() {
        let binding = compile_provider_binding(resolved_config(
            "claude",
            Some("sonnet"),
            None,
            ProviderRequestDefaults::default(),
            Some(resolved_claude_code_behavior(true)),
            BTreeMap::new(),
        ))
        .expect("binding should compile");

        assert_eq!(binding.driver, "claude-code");
        assert_eq!(binding.provider_id.as_str(), "claude-code");
        assert_eq!(binding.model.provider_id, Some(binding.provider_id.clone()));
    }

    #[test]
    fn provider_binding_should_reject_missing_model() {
        let error = compile_provider_binding(resolved_config(
            "codex",
            None,
            None,
            ProviderRequestDefaults::default(),
            Some(resolved_codex_behavior(false)),
            BTreeMap::new(),
        ))
        .expect_err("missing model should fail");

        assert_eq!(
            error,
            CompileError::MissingModel {
                driver: "codex".to_owned(),
            }
        );
    }

    #[test]
    fn provider_binding_should_reject_unknown_driver() {
        let error = compile_provider_binding(resolved_config(
            "some-unknown-llm",
            Some("mystery-model"),
            None,
            ProviderRequestDefaults::default(),
            None,
            BTreeMap::new(),
        ))
        .expect_err("unknown driver should fail");

        assert_eq!(
            error,
            CompileError::UnknownDriver {
                driver: "some-unknown-llm".to_owned(),
            }
        );
    }

    #[test]
    fn provider_binding_should_enforce_capability_mismatch_for_session_resume_on_codex() {
        // The task text uses session resume as the example mismatch. The current
        // Codex runtime descriptor advertises session-resume support, so this
        // test follows the live contract and falls back to the current
        // unsupported capability when session resume is available.
        let defaults = ProviderRequestDefaults {
            max_tokens: None,
            reasoning_effort: Some(ReasoningEffort::High),
        };
        let config = Some(resolved_codex_behavior(true));
        let expected_capability = if driver_capabilities("codex")
            .expect("codex capabilities should resolve")
            .session_resume
        {
            "extended_thinking"
        } else {
            "session_resume"
        };

        let error = compile_provider_binding(resolved_config(
            "codex",
            Some("gpt-5"),
            None,
            defaults,
            config,
            BTreeMap::new(),
        ))
        .expect_err("unsupported capability should fail");

        assert_eq!(
            error,
            CompileError::CapabilityMismatch {
                capability: expected_capability.to_owned(),
                driver: "codex".to_owned(),
                message: if expected_capability == "extended_thinking" {
                    "reasoning effort is not supported by this provider".to_owned()
                } else {
                    "session resume is not supported by this provider".to_owned()
                },
            }
        );
    }

    #[test]
    fn provider_binding_should_reject_forbidden_request_extra_key() {
        let error = compile_provider_binding(resolved_config(
            "codex",
            Some("gpt-5"),
            None,
            ProviderRequestDefaults::default(),
            Some(resolved_codex_behavior(false)),
            BTreeMap::from([("api_key".to_owned(), json!("secret"))]),
        ))
        .expect_err("forbidden request_extra should fail");

        assert_eq!(
            error,
            CompileError::InvalidRequestExtra {
                field: "provider.request_extra.api_key".to_owned(),
                message:
                    "is reserved for installation/workspace provider config and is not allowed in request_extra"
                        .to_owned(),
            }
        );
    }

    #[test]
    fn provider_binding_should_serialize_and_deserialize_round_trip() {
        let binding = compile_provider_binding(resolved_config(
            "claude-code",
            Some("sonnet"),
            Some("safe-doc-writer"),
            ProviderRequestDefaults {
                max_tokens: Some(1_024),
                reasoning_effort: None,
            },
            Some(resolved_claude_code_behavior(true)),
            BTreeMap::new(),
        ))
        .expect("binding should compile");

        let json = serde_json::to_string(&binding).expect("binding should serialize");
        let round_trip: super::ProviderBinding =
            serde_json::from_str(&json).expect("binding should deserialize");

        assert_eq!(round_trip, binding);
    }

    #[test]
    fn compile_error_should_implement_classified_error() {
        let error = CompileError::UnknownDriver {
            driver: "mystery".to_owned(),
        };

        assert_eq!(error.error_code(), "PROVIDER_BINDING_UNKNOWN_DRIVER");
        assert_eq!(error.http_status(), 422);
    }

    #[test]
    fn fully_specified_claude_code_config_should_compile_to_binding() {
        let directory = tempdir().expect("tempdir should exist");
        let path = directory.path().join("claude-config.toml");
        fs::write(
            &path,
            r#"
                [providers.default]
                driver = "claude-code"
                binary = "missing-claude"

                [agents.writer]
                provider = "default"
                model = "sonnet"

                [agents.writer.defaults]
                max_tokens = 2048

                [agents.writer.config.claude_code]
                continue_conversation = true
                allowed_tools = ["Read"]
            "#,
        )
        .expect("config should be written");

        let resolved = ConfigLoader::from_path(&path)
            .load()
            .expect("config should load")
            .resolve_agent_provider("writer")
            .expect("agent should resolve");
        let binding = compile_provider_binding(resolved).expect("binding should compile");

        assert_eq!(binding.provider_id.as_str(), "claude-code");
        assert!(binding.config.is_some());
    }

    #[test]
    fn fully_specified_codex_config_should_compile_to_binding() {
        let directory = tempdir().expect("tempdir should exist");
        let path = directory.path().join("codex-config.toml");
        fs::write(
            &path,
            r#"
                [providers.default]
                driver = "codex"
                binary = "missing-codex"

                [agents.writer]
                provider = "default"
                model = "gpt-5-mini"

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
        let binding = compile_provider_binding(resolved).expect("binding should compile");

        assert_eq!(binding.provider_id.as_str(), "codex");
        assert!(binding.config.is_some());
    }

    #[test]
    fn claude_code_binding_should_resolve_via_provider_registry() {
        let registry = ProviderRegistry::new();
        registry
            .register(ClaudeCodeProvider::new())
            .expect("provider should register");
        let binding = compile_provider_binding(resolved_config(
            "claude-code",
            Some("sonnet"),
            None,
            ProviderRequestDefaults::default(),
            Some(resolved_claude_code_behavior(false)),
            BTreeMap::new(),
        ))
        .expect("binding should compile");

        let provider = registry
            .get(&binding.provider_id)
            .expect("provider should resolve");

        assert_eq!(provider.descriptor().id, binding.provider_id);
    }

    #[test]
    fn mismatched_driver_and_config_namespace_should_fail_at_compile_time() {
        let error = compile_provider_binding(resolved_config(
            "codex",
            Some("gpt-5"),
            None,
            ProviderRequestDefaults::default(),
            Some(resolved_claude_code_behavior(false)),
            BTreeMap::new(),
        ))
        .expect_err("config namespace mismatch should fail");

        assert_eq!(
            error,
            CompileError::CapabilityMismatch {
                capability: "config_namespace".to_owned(),
                driver: "codex".to_owned(),
                message: "resolved config namespace `claude_code` does not match driver `codex`"
                    .to_owned(),
            }
        );
    }

    #[test]
    fn compile_should_not_require_provider_binary_on_path() {
        let binding = compile_provider_binding(resolved_config(
            "claude-code",
            Some("sonnet"),
            None,
            ProviderRequestDefaults::default(),
            Some(resolved_claude_code_behavior(false)),
            BTreeMap::new(),
        ))
        .expect("compile should not check the filesystem for provider binaries");

        assert_eq!(binding.provider_id.as_str(), "claude-code");
    }

    #[test]
    fn provider_binding_should_apply_selected_model_to_provider_model_id() {
        let binding = compile_provider_binding(resolved_config(
            "bedrock",
            Some("claude-3-7-sonnet"),
            None,
            ProviderRequestDefaults::default(),
            Some(resolved_claude_compatible_behavior(
                "bedrock/claude-3-5-haiku",
            )),
            BTreeMap::new(),
        ))
        .expect("binding should compile");

        assert_eq!(binding.provider_id.as_str(), "bedrock");
        assert_eq!(
            binding.model.provider_model_id.as_deref(),
            Some("bedrock/claude-3-5-haiku")
        );
    }

    #[test]
    fn resolve_provider_id_should_map_claude_compatible_wrappers() {
        let provider_id = resolve_provider_id("openrouter").expect("provider id should resolve");

        assert_eq!(provider_id.as_str(), "openrouter");
    }

    #[test]
    fn codex_driver_capabilities_should_match_runtime_descriptor() {
        let provider = CodexProvider::new();

        assert_eq!(
            driver_capabilities("codex").expect("codex capabilities should resolve"),
            provider.descriptor().capabilities,
        );
    }

    #[test]
    fn claude_code_driver_capabilities_should_match_runtime_descriptor() {
        let provider = ClaudeCodeProvider::new();

        assert_eq!(
            driver_capabilities("claude-code").expect("claude-code capabilities should resolve"),
            provider.descriptor().capabilities,
        );
    }

    fn resolved_config(
        driver: &str,
        model: Option<&str>,
        profile: Option<&str>,
        defaults: ProviderRequestDefaults,
        config: Option<ResolvedProviderBehaviorConfig>,
        request_extra: BTreeMap<String, Value>,
    ) -> ResolvedAgentProviderConfig<arky_config::ProviderConfig> {
        ResolvedAgentProviderConfig {
            provider: "default".to_owned(),
            driver: driver.to_owned(),
            profile: profile.map(ToOwned::to_owned),
            install: install_config(driver),
            model: model.map(ToOwned::to_owned),
            defaults,
            config,
            request_extra,
        }
    }

    fn install_config(driver: &str) -> arky_config::ProviderConfig {
        ArkyConfigBuilder::new()
            .provider(
                "default",
                ProviderConfigBuilder::new()
                    .driver(driver)
                    .binary(format!("missing-{driver}")),
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

    fn resolved_codex_behavior(resume_last: bool) -> ResolvedProviderBehaviorConfig {
        ProviderBehaviorLayer::Codex(CodexBehaviorLayer {
            sandbox_mode: Some("workspace-write".to_owned()),
            sandbox_network_access: Some(true),
            include_plan_tool: Some(true),
            resume_last: Some(resume_last),
            web_search: Some(true),
            rmcp_client: Some(true),
            reasoning_summary: Some("detailed".to_owned()),
            model_verbosity: Some("high".to_owned()),
        })
        .resolve()
    }

    fn resolved_claude_code_behavior(
        continue_conversation: bool,
    ) -> ResolvedProviderBehaviorConfig {
        ProviderBehaviorLayer::ClaudeCode(ClaudeCodeBehaviorLayer {
            continue_conversation: Some(continue_conversation),
            fork_session: Some(false),
            additional_directories: None,
            enable_file_checkpointing: Some(true),
            allowed_tools: Some(vec!["Read".to_owned()]),
            disallowed_tools: Some(vec!["Bash".to_owned()]),
            mcp_servers: Some(BTreeMap::from([(
                "filesystem".to_owned(),
                json!({"transport": "stdio"}),
            )])),
            max_budget_usd: Some(50.0),
            fallback_model: Some("haiku".to_owned()),
        })
        .resolve()
    }

    fn resolved_claude_compatible_behavior(selected_model: &str) -> ResolvedProviderBehaviorConfig {
        ProviderBehaviorLayer::ClaudeCompatible(Box::new(ClaudeCompatibleBehaviorLayer {
            base: Some(Box::new(ClaudeCodeBehaviorLayer {
                continue_conversation: Some(true),
                fork_session: Some(false),
                additional_directories: None,
                enable_file_checkpointing: Some(true),
                allowed_tools: Some(vec!["Read".to_owned()]),
                disallowed_tools: Some(vec![]),
                mcp_servers: Some(BTreeMap::from([(
                    "gateway".to_owned(),
                    json!({"transport": "http"}),
                )])),
                max_budget_usd: Some(25.0),
                fallback_model: Some("haiku".to_owned()),
            })),
            selected_model: Some(selected_model.to_owned()),
            region: Some("us-east-1".to_owned()),
            project_id: Some("project-123".to_owned()),
        }))
        .resolve()
    }

    #[test]
    fn resolved_behavior_fixture_should_match_expected_shapes() {
        let claude = resolved_claude_code_behavior(true);
        let compatible = resolved_claude_compatible_behavior("bedrock/sonnet");

        match claude {
            ResolvedProviderBehaviorConfig::ClaudeCode(config) => {
                assert_eq!(
                    config.session,
                    ClaudeSessionConfig {
                        continue_conversation: true,
                        resume: None,
                        session_id: None,
                        resume_session_at: None,
                        persist_session: false,
                        fork_session: false,
                    }
                );
                assert_eq!(
                    config.filesystem,
                    ClaudeFilesystemConfig {
                        additional_directories: Vec::new(),
                        enable_file_checkpointing: true,
                    }
                );
            }
            other => panic!("expected claude_code config, got {other:?}"),
        }

        match compatible {
            ResolvedProviderBehaviorConfig::ClaudeCompatible(config) => {
                assert_eq!(config.selected_model.as_deref(), Some("bedrock/sonnet"));
            }
            other => panic!("expected claude_compatible config, got {other:?}"),
        }
    }
}
