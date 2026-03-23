//! Public agent-definition schema plus validate/normalize/compile pipeline.
//!
//! This crate owns the product-facing `agent_definition` shape described in the
//! Compozy PRD. It stays synchronous and bounded: validation does not boot
//! providers, call the network, or execute templates.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use arky_claude_code::{ClaudeCompatibleProviderKind, CLAUDE_COMPATIBLE_PROVIDER_IDS};
use arky_config::{
    normalize_driver, validate_request_extra, ArkyConfigBuilder, ClaudeCodeBehaviorLayer,
    ClaudeCompatibleBehaviorLayer, CodexBehaviorLayer, ProviderBehaviorLayer,
    ProviderConfigBuilder, ProviderRequestDefaults, ResolvedAgentProviderConfig,
    ResolvedProviderBehaviorConfig,
};
use openfang_provider_binding::{
    compile_provider_binding, CompileError as ProviderBindingCompileError,
};
use openfang_types::{
    agent::{AgentManifest, AutonomousConfig, ManifestCapabilities},
    contract::{self, ContractKind, ContractNode, ContractValidationIssue},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{json, Value};
use thiserror::Error;

pub use openfang_provider_binding::ProviderBinding;

/// Portable provider request defaults reused across drivers.
pub type ProviderDefaults = ProviderRequestDefaults;

const PLACEHOLDER_PROVIDER_NAME: &str = "__agent_definition__";
const METADATA_KEY_COMPOZY: &str = "compozy";

macro_rules! string_enum_with_unknown {
    ($name:ident, default $default:ident, { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum $name {
            $($variant,)+
            Unknown(String),
        }

        impl $name {
            #[must_use]
            pub fn from_raw(value: impl Into<String>) -> Self {
                let raw = value.into();
                let normalized = raw.trim().to_ascii_lowercase();
                match normalized.as_str() {
                    $($value => Self::$variant,)+
                    _ => Self::Unknown(raw.trim().to_owned()),
                }
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $value,)+
                    Self::Unknown(value) => value.as_str(),
                }
            }

            #[must_use]
            pub const fn is_known(&self) -> bool {
                !matches!(self, Self::Unknown(_))
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::$default
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let raw = String::deserialize(deserializer)?;
                Ok(Self::from_raw(raw))
            }
        }
    };
}

string_enum_with_unknown!(DelegationKind, default Call, {
    Call => "call",
    Send => "send",
    Spawn => "spawn",
});

string_enum_with_unknown!(WorkspaceKind, default None, {
    None => "none",
    Workspace => "workspace",
    Worktree => "worktree",
});

string_enum_with_unknown!(MemoryPolicy, default Session, {
    None => "none",
    Session => "session",
    Shared => "shared",
});

string_enum_with_unknown!(HitlPolicy, default ExplicitOnly, {
    Never => "never",
    ExplicitOnly => "explicit_only",
    Always => "always",
});

/// Severity for machine-readable validation issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// The definition is invalid and cannot be compiled safely.
    Error,
    /// The definition is accepted, but authors should review the warning.
    Warning,
}

impl Severity {
    #[must_use]
    pub const fn is_error(self) -> bool {
        matches!(self, Self::Error)
    }
}

/// Machine-readable validation issue for the agent-definition pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    /// Severity of the issue.
    pub severity: Severity,
    /// Stable machine-readable issue code.
    pub code: String,
    /// Path to the invalid field.
    pub path: String,
    /// Human-readable message.
    pub message: String,
}

impl ValidationIssue {
    /// Creates one validation issue.
    #[must_use]
    pub fn new(
        severity: Severity,
        code: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            code: code.into(),
            path: path.into(),
            message: message.into(),
        }
    }

    /// Creates one blocking validation issue.
    #[must_use]
    pub fn error(
        code: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(Severity::Error, code, path, message)
    }

    /// Creates one non-blocking validation issue.
    #[must_use]
    pub fn warning(
        code: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(Severity::Warning, code, path, message)
    }
}

/// External names available to reference validation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ValidationContext {
    /// Known reusable profile names.
    pub known_profiles: BTreeSet<String>,
    /// Optional driver binding for known profiles.
    pub known_profile_drivers: BTreeMap<String, String>,
    /// Known skill names.
    pub known_skills: BTreeSet<String>,
    /// Known primitive names or wildcard namespaces.
    pub known_primitives: BTreeSet<String>,
    /// Snapshot of known agent names for future cross-agent checks.
    pub known_agents: BTreeSet<String>,
}

impl ValidationContext {
    /// Creates an empty validation context.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Seeds known profile names.
    #[must_use]
    pub fn with_profiles(mut self, profiles: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.known_profiles = profiles.into_iter().map(Into::into).collect();
        self
    }

    /// Registers one known profile and its driver.
    #[must_use]
    pub fn with_profile_driver(
        mut self,
        profile: impl Into<String>,
        driver: impl Into<String>,
    ) -> Self {
        let profile = profile.into();
        self.known_profiles.insert(profile.clone());
        self.known_profile_drivers.insert(profile, driver.into());
        self
    }

    /// Seeds known skill names.
    #[must_use]
    pub fn with_skills(mut self, skills: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.known_skills = skills.into_iter().map(Into::into).collect();
        self
    }

    /// Seeds known primitive names.
    #[must_use]
    pub fn with_primitives(
        mut self,
        primitives: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.known_primitives = primitives.into_iter().map(Into::into).collect();
        self
    }
}

/// Provider-specific config block accepted on the public authoring surface.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ProviderConfig {
    /// No provider-specific config was authored.
    #[default]
    Empty,
    /// Codex-specific config.
    Codex(CodexBehaviorLayer),
    /// Claude Code-specific config.
    ClaudeCode(ClaudeCodeBehaviorLayer),
    /// Claude-compatible wrapper config.
    ClaudeCompatible(ClaudeCompatibleBehaviorLayer),
    /// Raw config that did not match any known typed shape.
    Unknown(Value),
}

impl ProviderConfig {
    fn from_raw_value(raw: Value) -> Self {
        if raw.as_object().is_some_and(serde_json::Map::is_empty) {
            return Self::Empty;
        }

        if let Ok(config) = serde_json::from_value::<CodexBehaviorLayer>(raw.clone()) {
            return Self::Codex(config);
        }

        if let Ok(config) = serde_json::from_value::<ClaudeCodeBehaviorLayer>(raw.clone()) {
            return Self::ClaudeCode(config);
        }

        if let Ok(config) = serde_json::from_value::<ClaudeCompatibleBehaviorLayer>(raw.clone()) {
            return Self::ClaudeCompatible(config);
        }

        Self::Unknown(raw)
    }

    #[must_use]
    pub fn normalized_for_driver(&self, driver: &str) -> Self {
        if !matches!(self, Self::Empty) {
            return self.clone();
        }

        match expected_config_kind(driver) {
            Some(ConfigKind::Codex) => Self::Codex(CodexBehaviorLayer::default()),
            Some(ConfigKind::ClaudeCode) => Self::ClaudeCode(ClaudeCodeBehaviorLayer::default()),
            Some(ConfigKind::ClaudeCompatible) => {
                Self::ClaudeCompatible(ClaudeCompatibleBehaviorLayer::default())
            }
            None => Self::Empty,
        }
    }

    #[must_use]
    pub fn matches_driver(&self, driver: &str) -> bool {
        match (self, expected_config_kind(driver)) {
            (Self::Empty, Some(_)) => true,
            (Self::Codex(_), Some(ConfigKind::Codex)) => true,
            (Self::ClaudeCode(_), Some(ConfigKind::ClaudeCode)) => true,
            (Self::ClaudeCompatible(_), Some(ConfigKind::ClaudeCompatible)) => true,
            (Self::Unknown(_), _) => false,
            _ => false,
        }
    }

    fn into_resolved_behavior(self, driver: &str) -> Option<ResolvedProviderBehaviorConfig> {
        let behavior = match self.normalized_for_driver(driver) {
            Self::Codex(layer) => ProviderBehaviorLayer::Codex(layer),
            Self::ClaudeCode(layer) => ProviderBehaviorLayer::ClaudeCode(layer),
            Self::ClaudeCompatible(layer) => {
                ProviderBehaviorLayer::ClaudeCompatible(Box::new(layer))
            }
            Self::Empty | Self::Unknown(_) => return None,
        };

        Some(behavior.resolve())
    }
}

impl Serialize for ProviderConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Empty => BTreeMap::<String, Value>::new().serialize(serializer),
            Self::Codex(config) => config.serialize(serializer),
            Self::ClaudeCode(config) => config.serialize(serializer),
            Self::ClaudeCompatible(config) => config.serialize(serializer),
            Self::Unknown(raw) => raw.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ProviderConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Value::deserialize(deserializer)?;
        Ok(Self::from_raw_value(raw))
    }
}

/// Public provider block for one agent definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProviderBlock {
    /// Selected provider driver, such as `codex` or `claude_code`.
    pub driver: String,
    /// Selected model identifier.
    pub model: String,
    /// Optional reusable profile reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Small cross-provider request defaults.
    pub defaults: ProviderDefaults,
    /// Driver-specific behavior config.
    pub config: ProviderConfig,
    /// Optional bounded request-level escape hatch.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub request_extra: BTreeMap<String, Value>,
}

/// Prompt authoring block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PromptBlock {
    /// Optional system prompt prefix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Optional free-form instructions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Skill names referenced by the prompt.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
}

/// Capability envelope for one agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CapabilitiesBlock {
    /// Allowed tool names or patterns.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    /// Allowed primitive names or wildcard namespaces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub primitives: Vec<String>,
    /// Delegation styles permitted for the agent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delegation: Vec<DelegationKind>,
    /// Workspace automation mode.
    pub workspace: WorkspaceKind,
    /// Whether network access is available.
    pub network: bool,
}

/// Runtime behavior block for one agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RuntimeBlock {
    /// Whether the agent is autonomous by default.
    pub autonomous: bool,
    /// Memory access policy.
    pub memory_policy: MemoryPolicy,
    /// Human-in-the-loop policy.
    pub hitl: HitlPolicy,
}

impl Default for RuntimeBlock {
    fn default() -> Self {
        Self {
            autonomous: true,
            memory_policy: MemoryPolicy::default(),
            hitl: HitlPolicy::default(),
        }
    }
}

/// Public input authoring shape for Compozy agents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AgentDefinition {
    /// Stable user-controlled definition ID.
    pub id: String,
    /// Human-readable agent name.
    pub name: String,
    /// Product-facing semantic version.
    pub version: String,
    /// Human-readable description.
    pub description: String,
    /// Whether the definition is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Primary organization group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Secondary free-form tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Provider selection and behavior.
    pub provider: ProviderBlock,
    /// Prompt authoring block.
    pub prompt: PromptBlock,
    /// Operational capability envelope.
    pub capabilities: CapabilitiesBlock,
    /// Runtime behavior block.
    pub runtime: RuntimeBlock,
    /// Optional input contract.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<ContractNode>,
    /// Optional output contract.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<ContractNode>,
}

/// Product-layer metadata separated from the raw OpenFang manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentProductMetadata {
    /// Product-facing semantic version.
    pub version: String,
    /// Whether the definition is enabled.
    pub enabled: bool,
    /// Primary organization group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Secondary free-form tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Optional input contract.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<ContractNode>,
    /// Optional output contract.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<ContractNode>,
}

/// Compiled representation returned by the agent-definition pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledAgentDefinition {
    /// OpenFang platform manifest projection.
    pub agent_manifest: AgentManifest,
    /// Typed provider binding projection.
    pub provider_binding: ProviderBinding,
    /// Product-layer metadata projection.
    pub product_metadata: AgentProductMetadata,
}

/// Compile-time failures while mapping a validated definition into IR.
#[derive(Debug, Error)]
pub enum CompileError {
    /// The caller skipped the earlier validation stages.
    #[error("agent definition failed validation and is not ready for compile")]
    DefinitionNotCompileReady,
    /// The caller skipped the normalization stage.
    #[error("agent definition must be normalized before compile")]
    DefinitionNotNormalized,
    /// The placeholder install layer required by the provider-binding compiler
    /// could not be built.
    #[error("could not build placeholder provider install config: {message}")]
    PlaceholderInstallConfig { message: String },
    /// The provider-binding compiler rejected the derived provider state.
    #[error(transparent)]
    ProviderBinding(#[from] ProviderBindingCompileError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigKind {
    Codex,
    ClaudeCode,
    ClaudeCompatible,
}

/// Runs the full validation surface except normalization.
#[must_use]
pub fn validate_definition(
    definition: &AgentDefinition,
    context: &ValidationContext,
) -> Vec<ValidationIssue> {
    let mut issues = stage1_schema_validate(definition);
    issues.extend(stage2_reference_validate(definition, context));
    issues.extend(stage3_semantic_validate(definition));
    issues
}

/// Stage 1: schema validation.
#[must_use]
pub fn stage1_schema_validate(definition: &AgentDefinition) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    push_missing_string_field(&mut issues, "id", definition.id.as_str());
    push_missing_string_field(&mut issues, "name", definition.name.as_str());
    push_missing_string_field(
        &mut issues,
        "provider.driver",
        definition.provider.driver.as_str(),
    );
    push_missing_string_field(
        &mut issues,
        "provider.model",
        definition.provider.model.as_str(),
    );

    let driver = normalize_runtime_driver(definition.provider.driver.as_str());
    if !driver.is_empty() && !is_known_driver(driver.as_str()) {
        issues.push(ValidationIssue::error(
            "unknown_driver",
            "provider.driver",
            format!(
                "driver `{}` is not supported",
                definition.provider.driver.trim()
            ),
        ));
    }

    if matches!(definition.provider.defaults.max_tokens, Some(0)) {
        issues.push(ValidationIssue::error(
            "invalid_field",
            "provider.defaults.max_tokens",
            "max_tokens must be greater than zero",
        ));
    }

    for (index, delegation) in definition.capabilities.delegation.iter().enumerate() {
        if !delegation.is_known() {
            issues.push(invalid_enum_issue(
                format!("capabilities.delegation[{index}]"),
                delegation.as_str(),
                &["call", "send", "spawn"],
            ));
        }
    }

    if !definition.capabilities.workspace.is_known() {
        issues.push(invalid_enum_issue(
            "capabilities.workspace",
            definition.capabilities.workspace.as_str(),
            &["none", "workspace", "worktree"],
        ));
    }

    if !definition.runtime.memory_policy.is_known() {
        issues.push(invalid_enum_issue(
            "runtime.memory_policy",
            definition.runtime.memory_policy.as_str(),
            &["none", "session", "shared"],
        ));
    }

    if !definition.runtime.hitl.is_known() {
        issues.push(invalid_enum_issue(
            "runtime.hitl",
            definition.runtime.hitl.as_str(),
            &["never", "explicit_only", "always"],
        ));
    }

    issues
}

/// Stage 2: reference validation.
#[must_use]
pub fn stage2_reference_validate(
    definition: &AgentDefinition,
    context: &ValidationContext,
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    if let Some(profile) = definition.provider.profile.as_deref() {
        if !context.known_profiles.is_empty() && !context.known_profiles.contains(profile) {
            issues.push(ValidationIssue::error(
                "unknown_profile",
                "provider.profile",
                format!("profile `{profile}` is not registered"),
            ));
        }

        if let Some(expected_driver) = context.known_profile_drivers.get(profile) {
            let profile_driver = normalize_public_driver(expected_driver.as_str());
            let definition_driver = normalize_public_driver(definition.provider.driver.as_str());
            if !profile_driver.is_empty()
                && !definition_driver.is_empty()
                && profile_driver != definition_driver
            {
                issues.push(ValidationIssue::error(
                    "profile_driver_mismatch",
                    "provider.profile",
                    format!(
                        "profile `{profile}` targets driver `{profile_driver}` but the definition uses `{definition_driver}`"
                    ),
                ));
            }
        }
    }

    if !context.known_skills.is_empty() {
        for (index, skill) in definition.prompt.skills.iter().enumerate() {
            if !context.known_skills.contains(skill.as_str()) {
                issues.push(ValidationIssue::error(
                    "unknown_skill",
                    format!("prompt.skills[{index}]"),
                    format!("skill `{skill}` is not registered"),
                ));
            }
        }
    }

    if !context.known_primitives.is_empty() {
        for (index, primitive) in definition.capabilities.primitives.iter().enumerate() {
            if !primitive_is_known(primitive.as_str(), &context.known_primitives) {
                issues.push(ValidationIssue::error(
                    "unknown_primitive",
                    format!("capabilities.primitives[{index}]"),
                    format!("primitive `{primitive}` is not registered"),
                ));
            }
        }
    }

    issues
}

/// Stage 3: semantic validation.
#[must_use]
pub fn stage3_semantic_validate(definition: &AgentDefinition) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let driver = normalize_runtime_driver(definition.provider.driver.as_str());

    if !driver.is_empty() && is_known_driver(driver.as_str()) {
        if !definition.provider.config.matches_driver(driver.as_str()) {
            issues.push(ValidationIssue::error(
                "invalid_provider_config",
                "provider.config",
                expected_provider_config_message(
                    definition.provider.driver.as_str(),
                    &definition.provider.config,
                ),
            ));
        }
    }

    let mut request_extra_issues = Vec::new();
    validate_request_extra(
        &definition.provider.request_extra,
        "provider.request_extra",
        &mut request_extra_issues,
    );
    issues.extend(request_extra_issues.into_iter().map(|issue| {
        ValidationIssue::error(
            "invalid_request_extra",
            issue.field().to_owned(),
            issue.message().to_owned(),
        )
    }));

    if let Some(input) = definition.input.as_ref() {
        issues.extend(prefixed_contract_issues("input", contract::validate(input)));
    }

    if let Some(output) = definition.output.as_ref() {
        issues.extend(prefixed_contract_issues(
            "output",
            contract::validate(output),
        ));
        issues.extend(validate_output_semantics(output));
    }

    issues
}

/// Stage 4: normalization.
#[must_use]
pub fn stage4_normalize(mut definition: AgentDefinition) -> AgentDefinition {
    definition.id = definition.id.trim().to_owned();
    definition.name = definition.name.trim().to_owned();
    definition.version = definition.version.trim().to_owned();
    definition.description = definition.description.trim().to_owned();
    definition.enabled = Some(definition.enabled.unwrap_or(true));
    definition.group = trimmed_option(definition.group);
    dedupe_and_sort_strings(&mut definition.tags);

    definition.provider.driver = normalize_public_driver(definition.provider.driver.as_str());
    definition.provider.model = definition.provider.model.trim().to_owned();
    definition.provider.profile = trimmed_option(definition.provider.profile);
    definition.provider.config = definition
        .provider
        .config
        .normalized_for_driver(definition.provider.driver.as_str());

    definition.prompt.system = trimmed_option(definition.prompt.system);
    definition.prompt.instructions = trimmed_option(definition.prompt.instructions);
    dedupe_and_sort_strings(&mut definition.prompt.skills);

    dedupe_and_sort_strings(&mut definition.capabilities.tools);
    dedupe_and_sort_strings(&mut definition.capabilities.primitives);
    dedupe_and_sort_delegation(&mut definition.capabilities.delegation);

    if let Some(input) = definition.input.take() {
        definition.input = Some(contract::normalize(input));
    }

    if let Some(output) = definition.output.take() {
        definition.output = Some(contract::normalize(output));
    }

    definition
}

/// Compiles one validated and normalized agent definition.
pub fn compile(definition: AgentDefinition) -> Result<CompiledAgentDefinition, CompileError> {
    if has_blocking_validation_issues(&definition) {
        return Err(CompileError::DefinitionNotCompileReady);
    }

    if stage4_normalize(definition.clone()) != definition {
        return Err(CompileError::DefinitionNotNormalized);
    }

    let driver = normalize_runtime_driver(definition.provider.driver.as_str());
    let placeholder_install = placeholder_install_config(driver.as_str())?;
    let provider_binding = compile_provider_binding(ResolvedAgentProviderConfig {
        provider: PLACEHOLDER_PROVIDER_NAME.to_owned(),
        driver,
        profile: definition.provider.profile.clone(),
        install: placeholder_install,
        model: Some(definition.provider.model.clone()),
        defaults: definition.provider.defaults.clone(),
        config: definition
            .provider
            .config
            .clone()
            .into_resolved_behavior(definition.provider.driver.as_str()),
        request_extra: definition.provider.request_extra.clone(),
    })?;

    let agent_manifest = compile_agent_manifest(&definition, &provider_binding);
    let product_metadata = AgentProductMetadata {
        version: definition.version.clone(),
        enabled: definition.enabled.unwrap_or(true),
        group: definition.group.clone(),
        tags: definition.tags.clone(),
        input: definition.input.clone(),
        output: definition.output.clone(),
    };

    Ok(CompiledAgentDefinition {
        agent_manifest,
        provider_binding,
        product_metadata,
    })
}

fn compile_agent_manifest(
    definition: &AgentDefinition,
    provider_binding: &ProviderBinding,
) -> AgentManifest {
    let mut model = openfang_types::agent::ModelConfig {
        provider: provider_binding.driver.clone(),
        model: definition.provider.model.clone(),
        system_prompt: joined_prompt(definition),
        ..openfang_types::agent::ModelConfig::default()
    };
    model.max_tokens = definition
        .provider
        .defaults
        .max_tokens
        .unwrap_or(model.max_tokens);

    AgentManifest {
        name: definition.name.clone(),
        description: definition.description.clone(),
        model,
        capabilities: compile_manifest_capabilities(definition),
        skills: definition.prompt.skills.clone(),
        metadata: compile_manifest_metadata(definition),
        autonomous: definition
            .runtime
            .autonomous
            .then(AutonomousConfig::default),
        ..AgentManifest::default()
    }
}

fn compile_manifest_capabilities(definition: &AgentDefinition) -> ManifestCapabilities {
    let mut capabilities = ManifestCapabilities::default();
    capabilities.tools = definition.capabilities.tools.clone();
    capabilities.network = if definition.capabilities.network {
        vec!["*".to_owned()]
    } else {
        Vec::new()
    };
    capabilities.agent_spawn = definition
        .capabilities
        .delegation
        .iter()
        .any(|kind| matches!(kind, DelegationKind::Spawn));
    capabilities.agent_message = if definition.capabilities.delegation.is_empty() {
        Vec::new()
    } else {
        vec!["*".to_owned()]
    };

    match definition.runtime.memory_policy {
        MemoryPolicy::None => {}
        MemoryPolicy::Session => {
            capabilities.memory_read = vec!["self.*".to_owned()];
            capabilities.memory_write = vec!["self.*".to_owned()];
        }
        MemoryPolicy::Shared => {
            capabilities.memory_read = vec!["*".to_owned()];
            capabilities.memory_write = vec!["*".to_owned()];
        }
        MemoryPolicy::Unknown(_) => {}
    }

    capabilities
}

fn compile_manifest_metadata(definition: &AgentDefinition) -> HashMap<String, Value> {
    let mut metadata = HashMap::new();
    metadata.insert(
        METADATA_KEY_COMPOZY.to_owned(),
        json!({
            "definition_id": definition.id,
            "capabilities": {
                "primitives": definition.capabilities.primitives,
                "delegation": definition.capabilities.delegation.iter().map(DelegationKind::as_str).collect::<Vec<_>>(),
                "workspace": definition.capabilities.workspace.as_str(),
                "network": definition.capabilities.network,
            },
            "runtime": {
                "memory_policy": definition.runtime.memory_policy.as_str(),
                "hitl": definition.runtime.hitl.as_str(),
            }
        }),
    );
    metadata
}

fn placeholder_install_config(driver: &str) -> Result<arky_config::ProviderConfig, CompileError> {
    let config = ArkyConfigBuilder::new()
        .provider(
            PLACEHOLDER_PROVIDER_NAME,
            ProviderConfigBuilder::new().driver(driver.to_owned()),
        )
        .build()
        .map_err(|error| CompileError::PlaceholderInstallConfig {
            message: error.to_string(),
        })?;

    config
        .provider(PLACEHOLDER_PROVIDER_NAME)
        .cloned()
        .ok_or_else(|| CompileError::PlaceholderInstallConfig {
            message: "placeholder provider config was not built".to_owned(),
        })
}

fn validate_output_semantics(output: &ContractNode) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    match output.kind {
        ContractKind::ArtifactRef => {
            if output
                .artifact_type
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
            {
                issues.push(ValidationIssue::error(
                    "missing_semantic_metadata",
                    "output.artifact_type",
                    "artifact_ref outputs require artifact_type metadata",
                ));
            }
        }
        ContractKind::DocRef => {
            if output
                .doc_type
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
            {
                issues.push(ValidationIssue::error(
                    "missing_semantic_metadata",
                    "output.doc_type",
                    "doc_ref outputs require doc_type metadata",
                ));
            }
        }
        _ => {}
    }

    issues
}

fn prefixed_contract_issues(
    prefix: &str,
    contract_issues: Vec<ContractValidationIssue>,
) -> Vec<ValidationIssue> {
    contract_issues
        .into_iter()
        .map(|issue| {
            let path = join_path(prefix, issue.path());
            ValidationIssue::error("invalid_contract", path, issue.message().to_owned())
        })
        .collect()
}

fn invalid_enum_issue(path: impl Into<String>, value: &str, allowed: &[&str]) -> ValidationIssue {
    ValidationIssue::error(
        "invalid_enum_value",
        path,
        format!(
            "value `{value}` is not supported; expected one of: {}",
            allowed.join(", ")
        ),
    )
}

fn expected_provider_config_message(driver: &str, config: &ProviderConfig) -> String {
    let expected = expected_config_label(driver).unwrap_or("unknown");
    match config {
        ProviderConfig::Unknown(_) => {
            format!(
                "driver `{}` expects `{expected}` config fields",
                normalize_public_driver(driver)
            )
        }
        ProviderConfig::Codex(_) => format!(
            "driver `{}` expects `{expected}` config fields, but received codex fields",
            normalize_public_driver(driver)
        ),
        ProviderConfig::ClaudeCode(_) => format!(
            "driver `{}` expects `{expected}` config fields, but received claude_code fields",
            normalize_public_driver(driver)
        ),
        ProviderConfig::ClaudeCompatible(_) => format!(
            "driver `{}` expects `{expected}` config fields, but received claude_compatible fields",
            normalize_public_driver(driver)
        ),
        ProviderConfig::Empty => format!(
            "driver `{}` expects `{expected}` config fields",
            normalize_public_driver(driver)
        ),
    }
}

fn has_blocking_validation_issues(definition: &AgentDefinition) -> bool {
    stage1_schema_validate(definition)
        .into_iter()
        .chain(stage3_semantic_validate(definition))
        .any(|issue| issue.severity.is_error())
}

fn expected_config_kind(driver: &str) -> Option<ConfigKind> {
    let normalized = normalize_runtime_driver(driver);
    if normalized == "codex" {
        return Some(ConfigKind::Codex);
    }

    match ClaudeCompatibleProviderKind::from_kind(normalized.as_str()) {
        Some(ClaudeCompatibleProviderKind::ClaudeCode) => Some(ConfigKind::ClaudeCode),
        Some(_) => Some(ConfigKind::ClaudeCompatible),
        None => None,
    }
}

fn expected_config_label(driver: &str) -> Option<&'static str> {
    match expected_config_kind(driver) {
        Some(ConfigKind::Codex) => Some("codex"),
        Some(ConfigKind::ClaudeCode) => Some("claude_code"),
        Some(ConfigKind::ClaudeCompatible) => Some("claude_compatible"),
        None => None,
    }
}

fn is_known_driver(driver: &str) -> bool {
    let normalized = normalize_runtime_driver(driver);
    normalized == "codex" || CLAUDE_COMPATIBLE_PROVIDER_IDS.contains(&normalized.as_str())
}

fn normalize_runtime_driver(driver: &str) -> String {
    let lowercase = driver.trim().to_ascii_lowercase();
    normalize_driver(lowercase.as_str())
}

fn normalize_public_driver(driver: &str) -> String {
    normalize_runtime_driver(driver).replace('-', "_")
}

fn primitive_is_known(primitive: &str, known: &BTreeSet<String>) -> bool {
    if known.contains(primitive) {
        return true;
    }

    known.iter().any(|candidate| {
        wildcard_matches(candidate.as_str(), primitive) || wildcard_matches(primitive, candidate)
    })
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    match pattern.strip_suffix(".*") {
        Some(prefix) => value == pattern || value.starts_with(prefix),
        None => pattern == value,
    }
}

fn push_missing_string_field(
    issues: &mut Vec<ValidationIssue>,
    path: impl Into<String>,
    value: &str,
) {
    if value.trim().is_empty() {
        issues.push(ValidationIssue::error(
            "missing_field",
            path,
            "field is required",
        ));
    }
}

fn trimmed_option(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    })
}

fn dedupe_and_sort_strings(values: &mut Vec<String>) {
    for value in values.iter_mut() {
        *value = value.trim().to_owned();
    }
    values.retain(|value| !value.is_empty());
    values.sort();
    values.dedup();
}

fn dedupe_and_sort_delegation(values: &mut Vec<DelegationKind>) {
    values.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    values.dedup_by(|left, right| left.as_str() == right.as_str());
}

fn joined_prompt(definition: &AgentDefinition) -> String {
    let parts = [
        definition.prompt.system.as_deref(),
        definition.prompt.instructions.as_deref(),
    ];

    parts
        .into_iter()
        .flatten()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn join_path(prefix: &str, suffix: &str) -> String {
    if suffix.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix}.{suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compile, stage1_schema_validate, stage2_reference_validate, stage3_semantic_validate,
        stage4_normalize, validate_definition, AgentDefinition, CompileError, ProviderConfig,
        Severity, ValidationContext,
    };
    use arky_config::{ClaudeCodeBehaviorLayer, CodexBehaviorLayer};
    use openfang_types::{agent::AgentManifest, contract::ContractKind};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn stage1_schema_validate_should_accept_minimal_valid_definition() {
        let definition = minimal_definition();

        assert_eq!(stage1_schema_validate(&definition), Vec::new());
    }

    #[test]
    fn stage1_schema_validate_should_report_missing_driver() {
        let definition: AgentDefinition = serde_json::from_value(json!({
            "id": "writer",
            "name": "Writer",
            "provider": {
                "model": "gpt-5"
            }
        }))
        .expect("definition should deserialize");

        let issues = stage1_schema_validate(&definition);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "missing_field");
        assert_eq!(issues[0].path, "provider.driver");
    }

    #[test]
    fn stage1_schema_validate_should_report_unknown_driver() {
        let mut definition = minimal_definition();
        definition.provider.driver = "unknown_provider".to_owned();

        let issues = stage1_schema_validate(&definition);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "unknown_driver");
        assert_eq!(issues[0].path, "provider.driver");
    }

    #[test]
    fn stage1_schema_validate_should_report_invalid_hitl_policy() {
        let definition: AgentDefinition = serde_json::from_value(json!({
            "id": "writer",
            "name": "Writer",
            "provider": {
                "driver": "codex",
                "model": "gpt-5"
            },
            "runtime": {
                "hitl": "interactive"
            }
        }))
        .expect("definition should deserialize");

        let issues = stage1_schema_validate(&definition);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "invalid_enum_value");
        assert_eq!(issues[0].path, "runtime.hitl");
    }

    #[test]
    fn stage2_reference_validate_should_report_unknown_profile() {
        let definition = minimal_definition_with_profile();
        let context = ValidationContext::new().with_profiles(["other"]);

        let issues = stage2_reference_validate(&definition, &context);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "unknown_profile");
        assert_eq!(issues[0].path, "provider.profile");
    }

    #[test]
    fn stage2_reference_validate_should_skip_checks_when_registries_are_empty() {
        let definition = minimal_definition_with_profile_and_skill();

        assert_eq!(
            stage2_reference_validate(&definition, &ValidationContext::new()),
            Vec::new()
        );
    }

    #[test]
    fn stage3_semantic_validate_should_reject_forbidden_request_extra_keys() {
        let mut definition = minimal_definition();
        definition
            .provider
            .request_extra
            .insert("api_key".to_owned(), json!("secret"));

        let issues = stage3_semantic_validate(&definition);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "invalid_request_extra");
        assert_eq!(issues[0].path, "provider.request_extra.api_key");
    }

    #[test]
    fn stage3_semantic_validate_should_require_artifact_metadata_for_output() {
        let mut definition = minimal_definition();
        definition.output = Some(
            serde_json::from_value(json!({
                "kind": "artifact_ref"
            }))
            .expect("output contract should deserialize"),
        );

        let issues = stage3_semantic_validate(&definition);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "missing_semantic_metadata");
        assert_eq!(issues[0].path, "output.artifact_type");
    }

    #[test]
    fn stage3_semantic_validate_should_prefix_contract_validation_paths() {
        let mut definition = minimal_definition();
        definition.input = Some(
            serde_json::from_value(json!({
                "kind": "object",
                "required": ["title"]
            }))
            .expect("input contract should deserialize"),
        );

        let issues = stage3_semantic_validate(&definition);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "invalid_contract");
        assert_eq!(issues[0].path, "input.required[0]");
    }

    #[test]
    fn stage4_normalize_should_fill_enabled_default() {
        let normalized = stage4_normalize(minimal_definition());

        assert_eq!(normalized.enabled, Some(true));
    }

    #[test]
    fn stage4_normalize_should_canonicalize_contract_aliases() {
        let definition: AgentDefinition = serde_json::from_value(json!({
            "id": "writer",
            "name": "Writer",
            "provider": {
                "driver": "codex",
                "model": "gpt-5"
            },
            "input": {
                "kind": "text"
            }
        }))
        .expect("definition should deserialize");

        let normalized = stage4_normalize(definition);
        let input = normalized.input.expect("normalized input should exist");

        assert_eq!(input.kind, ContractKind::String);
        assert_eq!(input.declared_kind(), "string");
    }

    #[test]
    fn compile_should_keep_product_metadata_out_of_agent_manifest() {
        let compiled = compile(stage4_normalize(prd_writer_definition()))
            .expect("normalized definition should compile");

        assert_eq!(compiled.product_metadata.group.as_deref(), Some("sdlc"));
        assert_eq!(
            compiled.product_metadata.tags,
            vec!["docs".to_owned(), "planning".to_owned(), "prd".to_owned()]
        );
        assert_eq!(compiled.agent_manifest.tags, Vec::<String>::new());
        assert_eq!(
            compiled.agent_manifest.version,
            AgentManifest::default().version
        );
    }

    #[test]
    fn compile_should_map_provider_identity_without_loss() {
        let compiled = compile(stage4_normalize(minimal_definition()))
            .expect("normalized definition should compile");

        assert_eq!(compiled.provider_binding.driver, "codex");
        assert_eq!(compiled.provider_binding.model.model_id, "gpt-5");
    }

    #[test]
    fn full_pipeline_should_compile_prd_writer_example_without_issues() {
        let definition = prd_writer_definition();
        let context = ValidationContext::new();

        let mut issues = stage1_schema_validate(&definition);
        issues.extend(stage2_reference_validate(&definition, &context));
        issues.extend(stage3_semantic_validate(&definition));
        let normalized = stage4_normalize(definition);
        let compiled = compile(normalized.clone()).expect("normalized definition should compile");

        assert_eq!(issues, Vec::new());
        assert_eq!(compiled.agent_manifest.name, "PRD Writer");
        assert_eq!(compiled.product_metadata.version, "1.0.0");
        assert_eq!(normalized.provider.driver, "claude_code");
    }

    #[test]
    fn well_formed_toml_definition_should_validate_and_compile() {
        let definition: AgentDefinition = toml::from_str(
            r#"
id = "prd-writer"
name = "PRD Writer"
version = "1.0.0"
description = "Writes and iterates on product requirement documents"
enabled = true
group = "sdlc"
tags = ["docs", "prd", "planning"]

[provider]
driver = "claude_code"
model = "sonnet"
profile = "default"

[provider.defaults]
reasoning_effort = "high"
max_tokens = 8000

[provider.config]
continue_conversation = true
fork_session = false
allowed_tools = ["Read", "Write", "Bash"]
disallowed_tools = []
additional_directories = ["./docs"]
max_budget_usd = 5.0
fallback_model = "sonnet"

[prompt]
system = "You are a senior product writer."
instructions = "Write clear, implementation-ready PRDs."
skills = ["writing", "prd"]

[capabilities]
tools = ["*"]
primitives = ["issue.read", "artifact.*", "doc.*", "hitl.*"]
delegation = ["call", "send"]
workspace = "none"
network = true

[runtime]
autonomous = true
memory_policy = "session"
hitl = "explicit_only"

[input]
kind = "object"

[output]
kind = "artifact_ref"
artifact_type = "prd"
"#,
        )
        .expect("toml definition should deserialize");

        assert_eq!(
            validate_definition(&definition, &ValidationContext::new()),
            Vec::new()
        );

        let normalized = stage4_normalize(definition);
        let compiled = compile(normalized).expect("normalized definition should compile");

        assert_eq!(compiled.agent_manifest.name, "PRD Writer");
        assert_eq!(compiled.product_metadata.enabled, true);
    }

    #[test]
    fn malformed_driver_should_fail_in_stage1() {
        let definition: AgentDefinition = serde_json::from_value(json!({
            "id": "writer",
            "name": "Writer",
            "provider": {
                "driver": "",
                "model": "gpt-5"
            }
        }))
        .expect("definition should deserialize");

        let issues = stage1_schema_validate(&definition);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "missing_field");
        assert_eq!(issues[0].path, "provider.driver");
    }

    #[test]
    fn invalid_input_contract_should_fail_in_stage3() {
        let definition: AgentDefinition = serde_json::from_value(json!({
            "id": "writer",
            "name": "Writer",
            "provider": {
                "driver": "codex",
                "model": "gpt-5"
            },
            "input": {
                "kind": "object",
                "fields": {
                    "title": { "kind": "string" }
                },
                "required": ["missing"]
            }
        }))
        .expect("definition should deserialize");

        let issues = stage3_semantic_validate(&definition);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path, "input.required[0]");
        assert_eq!(issues[0].code, "invalid_contract");
    }

    #[test]
    fn multiple_independent_violations_should_all_be_collected() {
        let definition: AgentDefinition = serde_json::from_value(json!({
            "name": "Writer",
            "provider": {
                "driver": "unknown_provider",
                "model": "gpt-5",
                "request_extra": {
                    "api_key": "secret"
                }
            }
        }))
        .expect("definition should deserialize");

        let issues = validate_definition(&definition, &ValidationContext::new());
        let paths = issues
            .iter()
            .map(|issue| issue.path.as_str())
            .collect::<Vec<_>>();
        let codes = issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            paths,
            vec!["id", "provider.driver", "provider.request_extra.api_key"]
        );
        assert_eq!(
            codes,
            vec!["missing_field", "unknown_driver", "invalid_request_extra"]
        );
    }

    #[test]
    fn compile_should_reject_unnormalized_definitions() {
        let definition: AgentDefinition = serde_json::from_value(json!({
            "id": "writer",
            "name": "Writer",
            "provider": {
                "driver": "claude",
                "model": "sonnet"
            },
            "input": {
                "kind": "text"
            }
        }))
        .expect("definition should deserialize");

        let error = compile(definition).expect_err("unnormalized definition should fail");

        assert!(matches!(error, CompileError::DefinitionNotNormalized));
    }

    #[test]
    fn validation_issue_should_include_code_and_path() {
        let issue = stage1_schema_validate(
            &serde_json::from_value::<AgentDefinition>(json!({
                "name": "Writer",
                "provider": {
                    "model": "gpt-5"
                }
            }))
            .expect("definition should deserialize"),
        )
        .into_iter()
        .next()
        .expect("one issue should exist");

        assert_eq!(issue.severity, Severity::Error);
        assert_eq!(issue.code, "missing_field");
        assert_eq!(issue.path, "id");
    }

    fn minimal_definition() -> AgentDefinition {
        stage4_normalize(
            serde_json::from_value(json!({
                "id": "writer",
                "name": "Writer",
                "provider": {
                    "driver": "codex",
                    "model": "gpt-5",
                    "defaults": {
                        "max_tokens": 1024
                    },
                    "config": {
                        "web_search": true
                    }
                }
            }))
            .expect("definition should deserialize"),
        )
    }

    fn minimal_definition_with_profile() -> AgentDefinition {
        let mut definition = minimal_definition();
        definition.provider.profile = Some("default".to_owned());
        definition
    }

    fn minimal_definition_with_profile_and_skill() -> AgentDefinition {
        let mut definition = minimal_definition_with_profile();
        definition.prompt.skills = vec!["writing".to_owned()];
        definition
    }

    fn prd_writer_definition() -> AgentDefinition {
        serde_json::from_value(json!({
            "id": "prd-writer",
            "name": "PRD Writer",
            "version": "1.0.0",
            "description": "Writes and iterates on product requirement documents",
            "enabled": true,
            "group": "sdlc",
            "tags": ["docs", "prd", "planning"],
            "provider": {
                "driver": "claude_code",
                "model": "sonnet",
                "profile": "default",
                "defaults": {
                    "reasoning_effort": "high",
                    "max_tokens": 8000
                },
                "config": {
                    "continue_conversation": true,
                    "fork_session": false,
                    "allowed_tools": ["Read", "Write", "Bash"],
                    "disallowed_tools": [],
                    "additional_directories": ["./docs"],
                    "max_budget_usd": 5.0,
                    "fallback_model": "sonnet"
                }
            },
            "prompt": {
                "system": "You are a senior product writer.",
                "instructions": "Write clear, implementation-ready PRDs.",
                "skills": ["writing", "prd"]
            },
            "capabilities": {
                "tools": ["*"],
                "primitives": ["issue.read", "artifact.*", "doc.*", "hitl.*"],
                "delegation": ["call", "send"],
                "workspace": "none",
                "network": true
            },
            "runtime": {
                "autonomous": true,
                "memory_policy": "session",
                "hitl": "explicit_only"
            },
            "input": {
                "kind": "object"
            },
            "output": {
                "kind": "artifact_ref",
                "artifact_type": "prd"
            },
            "origin": {
                "kind": "user"
            },
            "forked_from": null,
            "created_at": "2026-03-21T12:00:00Z",
            "updated_at": "2026-03-21T14:00:00Z"
        }))
        .expect("definition should deserialize")
    }

    #[test]
    fn provider_config_should_preserve_unknown_payloads() {
        let config: ProviderConfig =
            serde_json::from_value(json!({"unsupported": true})).expect("config should parse");

        match config {
            ProviderConfig::Unknown(raw) => assert_eq!(raw, json!({"unsupported": true})),
            other => panic!("expected unknown config, got {other:?}"),
        }
    }

    #[test]
    fn provider_config_should_normalize_empty_blocks_to_driver_defaults() {
        let definition: AgentDefinition = serde_json::from_value(json!({
            "id": "writer",
            "name": "Writer",
            "provider": {
                "driver": "codex",
                "model": "gpt-5",
                "config": {}
            }
        }))
        .expect("definition should deserialize");

        let normalized = stage4_normalize(definition);

        assert!(matches!(
            normalized.provider.config,
            ProviderConfig::Codex(CodexBehaviorLayer {
                web_search: None,
                ..
            })
        ));
    }

    #[test]
    fn provider_config_should_accept_claude_code_blocks() {
        let config: ProviderConfig = serde_json::from_value(json!({
            "fork_session": true
        }))
        .expect("config should parse");

        assert!(matches!(
            config,
            ProviderConfig::ClaudeCode(ClaudeCodeBehaviorLayer {
                fork_session: Some(true),
                ..
            })
        ));
    }

    #[test]
    fn stage2_reference_validate_should_report_profile_driver_mismatch() {
        let definition = minimal_definition_with_profile();
        let context = ValidationContext::new().with_profile_driver("default", "claude_code");

        let issues = stage2_reference_validate(&definition, &context);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "profile_driver_mismatch");
        assert_eq!(issues[0].path, "provider.profile");
    }

    #[test]
    fn compile_should_reject_invalid_provider_config_even_if_normalized() {
        let definition: AgentDefinition = serde_json::from_value(json!({
            "id": "writer",
            "name": "Writer",
            "enabled": true,
            "provider": {
                "driver": "codex",
                "model": "gpt-5",
                "config": {
                    "fork_session": true
                }
            }
        }))
        .expect("definition should deserialize");
        let normalized = stage4_normalize(definition);

        let error = compile(normalized).expect_err("invalid config should not compile");

        assert!(matches!(error, CompileError::DefinitionNotCompileReady));
    }
}
