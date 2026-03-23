//! Layered provider configuration for workspace installs, profiles, and agents.

use std::{collections::BTreeMap, path::PathBuf};

use arky_claude_code::{ClaudeCompatibleProviderKind, ClaudeFilesystemConfig, ClaudeSessionConfig};
use arky_codex::{CodexSandboxConfig, CodexSandboxExclusions, CodexWorkspaceConfig};
use arky_protocol::ReasoningEffort;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ValidationIssue;

const MAX_REQUEST_EXTRA_DEPTH: usize = 4;
const MAX_REQUEST_EXTRA_ENTRIES: usize = 32;
const FORBIDDEN_REQUEST_EXTRA_KEYS: &[&str] = &[
    "allow_npx",
    "api_key",
    "apikey",
    "app_server_args",
    "args",
    "auth_token",
    "binary",
    "bootstrap",
    "cache_dir",
    "client_name",
    "client_version",
    "credential",
    "credentials",
    "cwd",
    "env",
    "environment",
    "experimental_api",
    "headers",
    "idle_shutdown_timeout",
    "idle_shutdown_timeout_ms",
    "process",
    "request_timeout",
    "request_timeout_ms",
    "runtime_dir",
    "sanitize_environment",
    "scheduler_timeout",
    "scheduler_timeout_ms",
    "shared_app_server_key",
    "startup_timeout",
    "startup_timeout_ms",
    "transport",
    "version_args",
];

/// Portable request defaults shared across provider layers.
///
/// This intentionally reuses `ReasoningEffort` from `arky-protocol` because the
/// request-default surface should compile directly into provider requests later.
///
/// ```rust
/// use arky_config::ProviderRequestDefaults;
/// use arky_protocol::ReasoningEffort;
///
/// let base = ProviderRequestDefaults {
///     max_tokens: Some(512),
///     reasoning_effort: None,
/// };
/// let overlay = ProviderRequestDefaults {
///     max_tokens: Some(1024),
///     reasoning_effort: Some(ReasoningEffort::High),
/// };
///
/// let merged = base.merge(&overlay);
///
/// assert_eq!(merged.max_tokens, Some(1024));
/// assert_eq!(merged.reasoning_effort, Some(ReasoningEffort::High));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRequestDefaults {
    /// Optional maximum token budget override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Optional reasoning effort override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
}

impl ProviderRequestDefaults {
    #[must_use]
    pub fn merge(self, overlay: &Self) -> Self {
        Self {
            max_tokens: overlay.max_tokens.or(self.max_tokens),
            reasoning_effort: overlay.reasoning_effort.or(self.reasoning_effort),
        }
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.max_tokens.is_none() && self.reasoning_effort.is_none()
    }
}

/// Layered Codex behavior overrides. These remain partial so later layers can
/// override them before the config is compiled into concrete runtime settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexBehaviorLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_network_access: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_plan_tool: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_last: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_search: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmcp_client: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_verbosity: Option<String>,
}

impl CodexBehaviorLayer {
    #[must_use]
    pub fn merge(self, overlay: Self) -> Self {
        Self {
            sandbox_mode: overlay.sandbox_mode.or(self.sandbox_mode),
            sandbox_network_access: overlay
                .sandbox_network_access
                .or(self.sandbox_network_access),
            include_plan_tool: overlay.include_plan_tool.or(self.include_plan_tool),
            resume_last: overlay.resume_last.or(self.resume_last),
            web_search: overlay.web_search.or(self.web_search),
            rmcp_client: overlay.rmcp_client.or(self.rmcp_client),
            reasoning_summary: overlay.reasoning_summary.or(self.reasoning_summary),
            model_verbosity: overlay.model_verbosity.or(self.model_verbosity),
        }
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.sandbox_mode.is_none()
            && self.sandbox_network_access.is_none()
            && self.include_plan_tool.is_none()
            && self.resume_last.is_none()
            && self.web_search.is_none()
            && self.rmcp_client.is_none()
            && self.reasoning_summary.is_none()
            && self.model_verbosity.is_none()
    }
}

/// Layered Claude behavior overrides used by direct Claude Code agents and as a
/// shared base for Claude-compatible wrappers.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaudeCodeBehaviorLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continue_conversation: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork_session: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additional_directories: Option<Vec<PathBuf>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enable_file_checkpointing: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disallowed_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<BTreeMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_budget_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_model: Option<String>,
}

impl ClaudeCodeBehaviorLayer {
    #[must_use]
    pub fn merge(self, overlay: Self) -> Self {
        Self {
            continue_conversation: overlay.continue_conversation.or(self.continue_conversation),
            fork_session: overlay.fork_session.or(self.fork_session),
            additional_directories: overlay
                .additional_directories
                .or(self.additional_directories),
            enable_file_checkpointing: overlay
                .enable_file_checkpointing
                .or(self.enable_file_checkpointing),
            allowed_tools: overlay.allowed_tools.or(self.allowed_tools),
            disallowed_tools: overlay.disallowed_tools.or(self.disallowed_tools),
            mcp_servers: overlay.mcp_servers.or(self.mcp_servers),
            max_budget_usd: overlay.max_budget_usd.or(self.max_budget_usd),
            fallback_model: overlay.fallback_model.or(self.fallback_model),
        }
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.continue_conversation.is_none()
            && self.fork_session.is_none()
            && self.additional_directories.is_none()
            && self.enable_file_checkpointing.is_none()
            && self.allowed_tools.is_none()
            && self.disallowed_tools.is_none()
            && self.mcp_servers.is_none()
            && self.max_budget_usd.is_none()
            && self.fallback_model.is_none()
    }
}

/// Layered wrapper-specific behavior for Claude-compatible gateway providers.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaudeCompatibleBehaviorLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<Box<ClaudeCodeBehaviorLayer>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

impl ClaudeCompatibleBehaviorLayer {
    #[must_use]
    pub fn merge(self, overlay: Self) -> Self {
        let base = match (self.base, overlay.base) {
            (Some(base), Some(overlay)) => Some(Box::new(base.merge(*overlay))),
            (Some(base), None) => Some(base),
            (None, Some(overlay)) => Some(overlay),
            (None, None) => None,
        };

        Self {
            base,
            selected_model: overlay.selected_model.or(self.selected_model),
            region: overlay.region.or(self.region),
            project_id: overlay.project_id.or(self.project_id),
        }
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.base.is_none()
            && self.selected_model.is_none()
            && self.region.is_none()
            && self.project_id.is_none()
    }
}

/// Typed behavior config attached to one provider layer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ProviderBehaviorLayer {
    Codex(CodexBehaviorLayer),
    ClaudeCode(ClaudeCodeBehaviorLayer),
    ClaudeCompatible(Box<ClaudeCompatibleBehaviorLayer>),
}

impl ProviderBehaviorLayer {
    #[must_use]
    pub fn merge(self, overlay: Self) -> Self {
        match (self, overlay) {
            (Self::Codex(base), Self::Codex(overlay)) => Self::Codex(base.merge(overlay)),
            (Self::ClaudeCode(base), Self::ClaudeCode(overlay)) => {
                Self::ClaudeCode(base.merge(overlay))
            }
            (Self::ClaudeCompatible(base), Self::ClaudeCompatible(overlay)) => {
                Self::ClaudeCompatible(Box::new(base.merge(*overlay)))
            }
            (_, overlay) => overlay,
        }
    }

    #[must_use]
    pub fn resolve(&self) -> ResolvedProviderBehaviorConfig {
        match self {
            Self::Codex(layer) => ResolvedProviderBehaviorConfig::Codex(layer.resolve()),
            Self::ClaudeCode(layer) => ResolvedProviderBehaviorConfig::ClaudeCode(layer.resolve()),
            Self::ClaudeCompatible(layer) => {
                ResolvedProviderBehaviorConfig::ClaudeCompatible(Box::new(layer.resolve()))
            }
        }
    }
}

/// Raw typed behavior namespaces accepted from TOML/YAML/env input.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialProviderBehaviorConfig {
    pub codex: Option<CodexBehaviorLayer>,
    #[serde(default, alias = "claude-code")]
    pub claude_code: Option<ClaudeCodeBehaviorLayer>,
    pub claude_compatible: Option<ClaudeCompatibleBehaviorLayer>,
}

impl PartialProviderBehaviorConfig {
    #[must_use]
    pub fn merge(self, overlay: Self) -> Self {
        Self {
            codex: merge_optional(self.codex, overlay.codex),
            claude_code: merge_optional(self.claude_code, overlay.claude_code),
            claude_compatible: merge_optional(self.claude_compatible, overlay.claude_compatible),
        }
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.codex.is_none() && self.claude_code.is_none() && self.claude_compatible.is_none()
    }

    pub fn finalize_for_driver(
        self,
        driver: &str,
        field_prefix: &str,
        issues: &mut Vec<ValidationIssue>,
    ) -> Option<ProviderBehaviorLayer> {
        let normalized_driver = normalize_driver(driver);
        let expected_namespace = expected_namespace(normalized_driver.as_str());
        let namespaces = [
            ("codex", self.codex.as_ref().map(|_| ())),
            ("claude_code", self.claude_code.as_ref().map(|_| ())),
            (
                "claude_compatible",
                self.claude_compatible.as_ref().map(|_| ()),
            ),
        ];
        let present = namespaces
            .iter()
            .filter_map(|(name, value)| value.map(|()| *name))
            .collect::<Vec<_>>();

        if present.len() > 1 {
            issues.push(ValidationIssue::new(
                field_prefix,
                format!(
                    "must only define one typed provider config block, found: {}",
                    present.join(", ")
                ),
            ));
            return None;
        }

        match expected_namespace {
            Some("codex") => resolve_expected_layer(
                self.codex,
                present.first().copied(),
                "codex",
                normalized_driver.as_str(),
                field_prefix,
                issues,
                ProviderBehaviorLayer::Codex,
            ),
            Some("claude_code") => resolve_expected_layer(
                self.claude_code,
                present.first().copied(),
                "claude_code",
                normalized_driver.as_str(),
                field_prefix,
                issues,
                ProviderBehaviorLayer::ClaudeCode,
            ),
            Some("claude_compatible") => resolve_expected_layer(
                self.claude_compatible,
                present.first().copied(),
                "claude_compatible",
                normalized_driver.as_str(),
                field_prefix,
                issues,
                |layer| ProviderBehaviorLayer::ClaudeCompatible(Box::new(layer)),
            ),
            None => {
                if let Some(namespace) = present.first() {
                    issues.push(ValidationIssue::new(
                        format!("{field_prefix}.{namespace}"),
                        format!(
                            "driver `{normalized_driver}` does not have a typed provider config namespace"
                        ),
                    ));
                }
                None
            }
            Some(_) => None,
        }
    }
}

/// Finalized reusable profile sitting between workspace installs and agents.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProviderProfileConfig {
    driver: String,
    model: Option<String>,
    defaults: ProviderRequestDefaults,
    config: Option<ProviderBehaviorLayer>,
}

impl ProviderProfileConfig {
    #[must_use]
    pub(super) const fn new(
        driver: String,
        model: Option<String>,
        defaults: ProviderRequestDefaults,
        config: Option<ProviderBehaviorLayer>,
    ) -> Self {
        Self {
            driver,
            model,
            defaults,
            config,
        }
    }

    #[must_use]
    pub const fn driver(&self) -> &str {
        self.driver.as_str()
    }

    #[must_use]
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    #[must_use]
    pub const fn defaults(&self) -> &ProviderRequestDefaults {
        &self.defaults
    }

    #[must_use]
    pub const fn config(&self) -> Option<&ProviderBehaviorLayer> {
        self.config.as_ref()
    }
}

/// Input shape for reusable provider profiles.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialProviderProfileConfig {
    pub driver: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub defaults: ProviderRequestDefaults,
    #[serde(default)]
    pub config: PartialProviderBehaviorConfig,
}

/// Concrete Codex behavior after profile + agent layering has been resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedCodexBehaviorConfig {
    #[serde(flatten)]
    pub sandbox: CodexSandboxConfig,
    #[serde(flatten)]
    pub workspace: CodexWorkspaceConfig,
    pub web_search: bool,
    pub rmcp_client: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_verbosity: Option<String>,
}

impl CodexBehaviorLayer {
    fn resolve(&self) -> ResolvedCodexBehaviorConfig {
        ResolvedCodexBehaviorConfig {
            sandbox: CodexSandboxConfig {
                sandbox_mode: self.sandbox_mode.clone(),
                sandbox_writable_roots: Vec::new(),
                sandbox_network_access: self.sandbox_network_access,
                full_auto: false,
                dangerously_bypass_approvals_and_sandbox: false,
                exclusions: CodexSandboxExclusions::default(),
            },
            workspace: CodexWorkspaceConfig {
                skip_git_repo_check: false,
                include_plan_tool: self.include_plan_tool.unwrap_or(false),
                resume_last: self.resume_last.unwrap_or(false),
            },
            web_search: self.web_search.unwrap_or(false),
            rmcp_client: self.rmcp_client.unwrap_or(false),
            reasoning_summary: self.reasoning_summary.clone(),
            model_verbosity: self.model_verbosity.clone(),
        }
    }
}

/// Concrete Claude behavior after profile + agent layering has been resolved.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedClaudeCodeBehaviorConfig {
    #[serde(flatten)]
    pub session: ClaudeSessionConfig,
    #[serde(flatten)]
    pub filesystem: ClaudeFilesystemConfig,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disallowed_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub mcp_servers: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_budget_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_model: Option<String>,
}

impl ClaudeCodeBehaviorLayer {
    fn resolve(&self) -> ResolvedClaudeCodeBehaviorConfig {
        ResolvedClaudeCodeBehaviorConfig {
            session: ClaudeSessionConfig {
                continue_conversation: self.continue_conversation.unwrap_or(false),
                resume: None,
                session_id: None,
                resume_session_at: None,
                persist_session: false,
                fork_session: self.fork_session.unwrap_or(false),
            },
            filesystem: ClaudeFilesystemConfig {
                additional_directories: self.additional_directories.clone().unwrap_or_default(),
                enable_file_checkpointing: self.enable_file_checkpointing.unwrap_or(false),
            },
            allowed_tools: self.allowed_tools.clone().unwrap_or_default(),
            disallowed_tools: self.disallowed_tools.clone().unwrap_or_default(),
            mcp_servers: self.mcp_servers.clone().unwrap_or_default(),
            max_budget_usd: self.max_budget_usd,
            fallback_model: self.fallback_model.clone(),
        }
    }
}

/// Concrete wrapper behavior after profile + agent layering has been resolved.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedClaudeCompatibleBehaviorConfig {
    pub base: ResolvedClaudeCodeBehaviorConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

impl ClaudeCompatibleBehaviorLayer {
    fn resolve(&self) -> ResolvedClaudeCompatibleBehaviorConfig {
        ResolvedClaudeCompatibleBehaviorConfig {
            base: self.base.as_deref().cloned().unwrap_or_default().resolve(),
            selected_model: self.selected_model.clone(),
            region: self.region.clone(),
            project_id: self.project_id.clone(),
        }
    }
}

/// Fully merged provider-specific behavior for one agent.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ResolvedProviderBehaviorConfig {
    Codex(ResolvedCodexBehaviorConfig),
    ClaudeCode(ResolvedClaudeCodeBehaviorConfig),
    ClaudeCompatible(Box<ResolvedClaudeCompatibleBehaviorConfig>),
}

/// Fully merged provider state for one agent.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedAgentProviderConfig<TInstall> {
    pub provider: String,
    pub driver: String,
    pub profile: Option<String>,
    pub install: TInstall,
    pub model: Option<String>,
    pub defaults: ProviderRequestDefaults,
    pub config: Option<ResolvedProviderBehaviorConfig>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub request_extra: BTreeMap<String, Value>,
}

pub fn normalize_driver(driver: &str) -> String {
    let trimmed = driver.trim().replace('_', "-");
    match trimmed.as_str() {
        "claude" => "claude-code".to_owned(),
        other => other.to_owned(),
    }
}

pub fn validate_driver(
    driver: Option<String>,
    field: &str,
    issues: &mut Vec<ValidationIssue>,
) -> Option<String> {
    driver.and_then(|driver| {
        let normalized = normalize_driver(driver.as_str());
        if normalized.trim().is_empty() {
            issues.push(ValidationIssue::new(field, "must not be empty"));
            None
        } else {
            Some(normalized)
        }
    })
}

pub fn validate_defaults(
    defaults: ProviderRequestDefaults,
    field_prefix: &str,
    issues: &mut Vec<ValidationIssue>,
) -> ProviderRequestDefaults {
    if matches!(defaults.max_tokens, Some(0)) {
        issues.push(ValidationIssue::new(
            format!("{field_prefix}.max_tokens"),
            "must be greater than zero",
        ));
    }

    defaults
}

pub fn validate_request_extra(
    request_extra: &BTreeMap<String, Value>,
    field_prefix: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if request_extra.is_empty() {
        return;
    }

    let mut entry_count = 0usize;
    for (key, value) in request_extra {
        let field = format!("{field_prefix}.{key}");
        if is_forbidden_request_extra_key(key.as_str()) {
            issues.push(ValidationIssue::new(
                field.clone(),
                "is reserved for installation/workspace provider config and is not allowed in request_extra",
            ));
        }
        validate_request_extra_value(value, field.as_str(), 1, &mut entry_count, issues);
    }

    if entry_count > MAX_REQUEST_EXTRA_ENTRIES {
        issues.push(ValidationIssue::new(
            field_prefix,
            format!(
                "must not contain more than {MAX_REQUEST_EXTRA_ENTRIES} nested request_extra entries"
            ),
        ));
    }
}

fn validate_request_extra_value(
    value: &Value,
    field_prefix: &str,
    depth: usize,
    entry_count: &mut usize,
    issues: &mut Vec<ValidationIssue>,
) {
    if depth > MAX_REQUEST_EXTRA_DEPTH {
        issues.push(ValidationIssue::new(
            field_prefix,
            format!("must not exceed {MAX_REQUEST_EXTRA_DEPTH} levels of nesting"),
        ));
        return;
    }

    match value {
        Value::Object(map) => {
            for (key, nested) in map {
                *entry_count += 1;
                let field = format!("{field_prefix}.{key}");
                if is_forbidden_request_extra_key(key.as_str()) {
                    issues.push(ValidationIssue::new(
                        field.clone(),
                        "is reserved for installation/workspace provider config and is not allowed in request_extra",
                    ));
                }
                validate_request_extra_value(
                    nested,
                    field.as_str(),
                    depth + 1,
                    entry_count,
                    issues,
                );
            }
        }
        Value::Array(values) => {
            for (index, nested) in values.iter().enumerate() {
                *entry_count += 1;
                validate_request_extra_value(
                    nested,
                    format!("{field_prefix}[{index}]").as_str(),
                    depth + 1,
                    entry_count,
                    issues,
                );
            }
        }
        _ => {
            *entry_count += 1;
        }
    }
}

fn expected_namespace(driver: &str) -> Option<&'static str> {
    if driver == "codex" {
        return Some("codex");
    }

    match ClaudeCompatibleProviderKind::from_kind(driver) {
        Some(ClaudeCompatibleProviderKind::ClaudeCode) => Some("claude_code"),
        Some(_) => Some("claude_compatible"),
        None => None,
    }
}

fn is_forbidden_request_extra_key(key: &str) -> bool {
    let normalized = normalize_layer_key(key);
    FORBIDDEN_REQUEST_EXTRA_KEYS
        .iter()
        .any(|forbidden| normalized == *forbidden || normalized.ends_with(&format!("_{forbidden}")))
}

fn normalize_layer_key(key: &str) -> String {
    let mut normalized = String::with_capacity(key.len());
    let mut previous_was_separator = false;
    let mut previous_was_lower_or_digit = false;

    for character in key.chars() {
        if character == '-' || character == '_' || character == ' ' {
            if !normalized.is_empty() && !previous_was_separator {
                normalized.push('_');
            }
            previous_was_separator = true;
            previous_was_lower_or_digit = false;
            continue;
        }

        if character.is_ascii_uppercase() {
            if !normalized.is_empty() && !previous_was_separator && previous_was_lower_or_digit {
                normalized.push('_');
            }
            normalized.push(character.to_ascii_lowercase());
            previous_was_separator = false;
            previous_was_lower_or_digit = false;
            continue;
        }

        normalized.push(character.to_ascii_lowercase());
        previous_was_separator = false;
        previous_was_lower_or_digit = character.is_ascii_lowercase() || character.is_ascii_digit();
    }

    normalized
}

fn merge_optional<T>(base: Option<T>, overlay: Option<T>) -> Option<T>
where
    T: MergeLayer,
{
    match (base, overlay) {
        (Some(base), Some(overlay)) => Some(base.merge_layer(overlay)),
        (Some(base), None) => Some(base),
        (None, Some(overlay)) => Some(overlay),
        (None, None) => None,
    }
}

fn resolve_expected_layer<T>(
    layer: Option<T>,
    namespace: Option<&str>,
    expected_namespace: &str,
    normalized_driver: &str,
    field_prefix: &str,
    issues: &mut Vec<ValidationIssue>,
    wrap: impl FnOnce(T) -> ProviderBehaviorLayer,
) -> Option<ProviderBehaviorLayer> {
    layer.map(wrap).or_else(|| {
        if let Some(namespace) = namespace {
            issues.push(ValidationIssue::new(
                format!("{field_prefix}.{namespace}"),
                format!(
                    "is not supported for driver `{normalized_driver}`; use `{field_prefix}.{expected_namespace}`"
                ),
            ));
        }
        None
    })
}

trait MergeLayer {
    fn merge_layer(self, overlay: Self) -> Self;
}

impl MergeLayer for CodexBehaviorLayer {
    fn merge_layer(self, overlay: Self) -> Self {
        self.merge(overlay)
    }
}

impl MergeLayer for ClaudeCodeBehaviorLayer {
    fn merge_layer(self, overlay: Self) -> Self {
        self.merge(overlay)
    }
}

impl MergeLayer for ClaudeCompatibleBehaviorLayer {
    fn merge_layer(self, overlay: Self) -> Self {
        self.merge(overlay)
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use arky_protocol::ReasoningEffort;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{
        validate_request_extra, ClaudeCodeBehaviorLayer, CodexBehaviorLayer,
        ProviderRequestDefaults, FORBIDDEN_REQUEST_EXTRA_KEYS,
    };
    use crate::ValidationIssue;

    #[test]
    fn provider_request_defaults_merge_should_prefer_overlay() {
        let base = ProviderRequestDefaults {
            max_tokens: Some(256),
            reasoning_effort: Some(ReasoningEffort::Low),
        };
        let overlay = ProviderRequestDefaults {
            max_tokens: Some(1_024),
            reasoning_effort: None,
        };

        let actual = base.merge(&overlay);

        let expected = ProviderRequestDefaults {
            max_tokens: Some(1_024),
            reasoning_effort: Some(ReasoningEffort::Low),
        };

        assert_eq!(actual, expected);
    }

    #[test]
    fn codex_behavior_layer_merge_should_prefer_overlay_fields() {
        let base = CodexBehaviorLayer {
            sandbox_mode: Some("read-only".to_owned()),
            sandbox_network_access: Some(false),
            include_plan_tool: Some(false),
            resume_last: Some(false),
            web_search: Some(false),
            rmcp_client: Some(false),
            reasoning_summary: Some("auto".to_owned()),
            model_verbosity: Some("low".to_owned()),
        };
        let overlay = CodexBehaviorLayer {
            sandbox_mode: None,
            sandbox_network_access: Some(true),
            include_plan_tool: Some(true),
            resume_last: Some(true),
            web_search: Some(true),
            rmcp_client: Some(true),
            reasoning_summary: Some("detailed".to_owned()),
            model_verbosity: None,
        };

        let actual = base.merge(overlay);

        let expected = CodexBehaviorLayer {
            sandbox_mode: Some("read-only".to_owned()),
            sandbox_network_access: Some(true),
            include_plan_tool: Some(true),
            resume_last: Some(true),
            web_search: Some(true),
            rmcp_client: Some(true),
            reasoning_summary: Some("detailed".to_owned()),
            model_verbosity: Some("low".to_owned()),
        };

        assert_eq!(actual, expected);
    }

    #[test]
    fn claude_code_behavior_layer_merge_should_prefer_overlay_fields() {
        let base = ClaudeCodeBehaviorLayer {
            continue_conversation: Some(false),
            fork_session: Some(false),
            additional_directories: Some(vec![PathBuf::from("/base")]),
            enable_file_checkpointing: Some(false),
            allowed_tools: Some(vec!["Read".to_owned()]),
            disallowed_tools: Some(vec!["Bash".to_owned()]),
            mcp_servers: Some(BTreeMap::from([(
                "base".to_owned(),
                json!({"url": "stdio://"}),
            )])),
            max_budget_usd: Some(10.0),
            fallback_model: Some("claude-3".to_owned()),
        };
        let overlay = ClaudeCodeBehaviorLayer {
            continue_conversation: Some(true),
            fork_session: Some(true),
            additional_directories: None,
            enable_file_checkpointing: Some(true),
            allowed_tools: Some(vec!["Edit".to_owned()]),
            disallowed_tools: None,
            mcp_servers: Some(BTreeMap::from([(
                "overlay".to_owned(),
                json!({"url": "http://localhost"}),
            )])),
            max_budget_usd: Some(25.0),
            fallback_model: None,
        };

        let actual = base.merge(overlay);

        let expected = ClaudeCodeBehaviorLayer {
            continue_conversation: Some(true),
            fork_session: Some(true),
            additional_directories: Some(vec![PathBuf::from("/base")]),
            enable_file_checkpointing: Some(true),
            allowed_tools: Some(vec!["Edit".to_owned()]),
            disallowed_tools: Some(vec!["Bash".to_owned()]),
            mcp_servers: Some(BTreeMap::from([(
                "overlay".to_owned(),
                json!({"url": "http://localhost"}),
            )])),
            max_budget_usd: Some(25.0),
            fallback_model: Some("claude-3".to_owned()),
        };

        assert_eq!(actual, expected);
    }

    #[test]
    fn forbidden_request_extra_key_should_produce_validation_issue() {
        for forbidden_key in FORBIDDEN_REQUEST_EXTRA_KEYS {
            let mut issues = Vec::new();
            let request_extra = BTreeMap::from([((*forbidden_key).to_owned(), json!("blocked"))]);

            validate_request_extra(&request_extra, "request_extra", &mut issues);

            let actual = issues
                .iter()
                .map(|issue| (issue.field().to_owned(), issue.message().to_owned()))
                .collect::<Vec<_>>();
            let expected = vec![(
                format!("request_extra.{forbidden_key}"),
                "is reserved for installation/workspace provider config and is not allowed in request_extra"
                    .to_owned(),
            )];

            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn request_extra_nesting_beyond_limit_should_produce_validation_issue() {
        let request_extra = BTreeMap::from([(
            "nested".to_owned(),
            json!({
                "level1": {
                    "level2": {
                        "level3": {
                            "level4": "too deep"
                        }
                    }
                }
            }),
        )]);
        let mut issues = Vec::new();

        validate_request_extra(&request_extra, "request_extra", &mut issues);

        let actual = issue_fields(&issues);
        let expected = vec!["request_extra.nested.level1.level2.level3.level4".to_owned()];

        assert_eq!(actual, expected);
    }

    #[test]
    fn request_extra_entry_count_beyond_limit_should_produce_validation_issue() {
        let request_extra = (0..33)
            .map(|index| (format!("key_{index}"), json!(index)))
            .collect::<BTreeMap<_, _>>();
        let mut issues = Vec::new();

        validate_request_extra(&request_extra, "request_extra", &mut issues);

        let actual = issues
            .iter()
            .map(|issue| (issue.field().to_owned(), issue.message().to_owned()))
            .collect::<Vec<_>>();
        let expected = vec![(
            "request_extra".to_owned(),
            "must not contain more than 32 nested request_extra entries".to_owned(),
        )];

        assert_eq!(actual, expected);
    }

    fn issue_fields(issues: &[ValidationIssue]) -> Vec<String> {
        issues
            .iter()
            .map(|issue| issue.field().to_owned())
            .collect()
    }
}
