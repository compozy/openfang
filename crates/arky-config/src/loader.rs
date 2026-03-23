//! Configuration types, builders, and loading entry points.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    error::ConfigError,
    layered::{
        PartialProviderBehaviorConfig, PartialProviderProfileConfig, ProviderBehaviorLayer,
        ProviderProfileConfig, ProviderRequestDefaults, ResolvedAgentProviderConfig,
    },
    merge::{merge_agent, merge_config, merge_profile, merge_provider, merge_workspace},
    validate::{check_provider_prerequisites, validate_config},
};

const DEFAULT_ENV_PREFIX: &str = "ARKY";

/// Supported on-disk configuration formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    /// TOML configuration.
    Toml,
    /// YAML configuration.
    Yaml,
}

impl ConfigFormat {
    /// Infers a config format from a file extension.
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "toml" => Some(Self::Toml),
            "yaml" | "yml" => Some(Self::Yaml),
            _ => None,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Toml => "toml",
            Self::Yaml => "yaml",
        }
    }
}

/// Fully merged and validated Arky configuration.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ArkyConfig {
    workspace: WorkspaceConfig,
    providers: BTreeMap<String, ProviderConfig>,
    profiles: BTreeMap<String, ProviderProfileConfig>,
    agents: BTreeMap<String, AgentConfig>,
}

impl ArkyConfig {
    pub(super) const fn new(
        workspace: WorkspaceConfig,
        providers: BTreeMap<String, ProviderConfig>,
        profiles: BTreeMap<String, ProviderProfileConfig>,
        agents: BTreeMap<String, AgentConfig>,
    ) -> Self {
        Self {
            workspace,
            providers,
            profiles,
            agents,
        }
    }

    /// Creates a programmatic builder for Arky configuration.
    #[must_use]
    pub fn builder() -> ArkyConfigBuilder {
        ArkyConfigBuilder::new()
    }

    /// Loads configuration from a file plus current `ARKY_*` environment overrides.
    pub fn from_path(path: impl Into<PathBuf>) -> Result<Self, ConfigError> {
        ConfigLoader::from_path(path).load()
    }

    /// Returns workspace-level settings.
    #[must_use]
    pub const fn workspace(&self) -> &WorkspaceConfig {
        &self.workspace
    }

    /// Returns all configured providers.
    #[must_use]
    pub const fn providers(&self) -> &BTreeMap<String, ProviderConfig> {
        &self.providers
    }

    /// Returns one provider by name.
    #[must_use]
    pub fn provider(&self, name: &str) -> Option<&ProviderConfig> {
        self.providers.get(name)
    }

    /// Returns all configured reusable provider profiles.
    #[must_use]
    pub const fn profiles(&self) -> &BTreeMap<String, ProviderProfileConfig> {
        &self.profiles
    }

    /// Returns one reusable profile by name.
    #[must_use]
    pub fn profile(&self, name: &str) -> Option<&ProviderProfileConfig> {
        self.profiles.get(name)
    }

    /// Returns all configured agents.
    #[must_use]
    pub const fn agents(&self) -> &BTreeMap<String, AgentConfig> {
        &self.agents
    }

    /// Returns one agent by name.
    #[must_use]
    pub fn agent(&self, name: &str) -> Option<&AgentConfig> {
        self.agents.get(name)
    }

    /// Verifies that every configured provider prerequisite binary exists.
    pub fn check_prerequisites(&self) -> Result<BTreeMap<String, PathBuf>, ConfigError> {
        check_provider_prerequisites(self)
    }

    /// Resolves the fully layered provider config for one agent.
    ///
    /// ```rust
    /// use arky_config::ConfigLoader;
    /// use tempfile::tempdir;
    ///
    /// let directory = tempdir().expect("temp directory should exist");
    /// let path = directory.path().join("arky.toml");
    /// std::fs::write(
    ///     &path,
    ///     r#"
    ///         [providers.default]
    ///         driver = "codex"
    ///         model = "gpt-5"
    ///
    ///         [agents.writer]
    ///         provider = "default"
    ///         model = "gpt-5-mini"
    ///     "#,
    /// )
    /// .expect("config file should be written");
    ///
    /// let config = ConfigLoader::from_path(&path)
    ///     .load()
    ///     .expect("config should load");
    /// let resolved = config
    ///     .resolve_agent_provider("writer")
    ///     .expect("writer config should resolve");
    ///
    /// assert_eq!(resolved.driver, "codex");
    /// assert_eq!(resolved.model.as_deref(), Some("gpt-5-mini"));
    /// ```
    #[must_use]
    pub fn resolve_agent_provider(
        &self,
        name: &str,
    ) -> Option<ResolvedAgentProviderConfig<ProviderConfig>> {
        let agent = self.agent(name)?;
        let install = self.provider(agent.provider())?.clone();
        let profile = agent.profile().and_then(|value| self.profile(value));
        let driver = agent
            .driver()
            .or_else(|| profile.map(ProviderProfileConfig::driver))
            .unwrap_or_else(|| install.driver());

        let model = agent
            .model()
            .or_else(|| profile.and_then(ProviderProfileConfig::model))
            .or_else(|| install.model())
            .map(ToOwned::to_owned);
        let defaults = profile
            .map(ProviderProfileConfig::defaults)
            .cloned()
            .unwrap_or_default()
            .merge(agent.defaults());
        let config = match (
            profile.and_then(ProviderProfileConfig::config),
            agent.config(),
        ) {
            (Some(base), Some(overlay)) => Some(base.clone().merge(overlay.clone()).resolve()),
            (Some(base), None) => Some(base.clone().resolve()),
            (None, Some(overlay)) => Some(overlay.clone().resolve()),
            (None, None) => None,
        };

        Some(ResolvedAgentProviderConfig {
            provider: agent.provider().to_owned(),
            driver: driver.to_owned(),
            profile: agent.profile().map(ToOwned::to_owned),
            install,
            model,
            defaults,
            config,
            request_extra: agent.request_extra().clone(),
        })
    }
}

/// Workspace-scoped defaults shared across providers and agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceConfig {
    name: Option<String>,
    default_provider: Option<String>,
    data_dir: Option<PathBuf>,
    env: BTreeMap<String, String>,
}

impl WorkspaceConfig {
    pub(super) const fn new(
        name: Option<String>,
        default_provider: Option<String>,
        data_dir: Option<PathBuf>,
        env: BTreeMap<String, String>,
    ) -> Self {
        Self {
            name,
            default_provider,
            data_dir,
            env,
        }
    }

    /// Creates a builder for workspace-level settings.
    #[must_use]
    pub fn builder() -> WorkspaceConfigBuilder {
        WorkspaceConfigBuilder::new()
    }

    /// Returns the workspace name.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns the default provider name, if configured.
    #[must_use]
    pub fn default_provider(&self) -> Option<&str> {
        self.default_provider.as_deref()
    }

    /// Returns the configured data directory.
    #[must_use]
    pub fn data_dir(&self) -> Option<&Path> {
        self.data_dir.as_deref()
    }

    /// Returns injected workspace environment variables.
    #[must_use]
    pub const fn env(&self) -> &BTreeMap<String, String> {
        &self.env
    }
}

/// Provider-level runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderConfig {
    pub(crate) driver: String,
    pub(crate) binary: Option<PathBuf>,
    pub(crate) model: Option<String>,
    pub(crate) args: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) shared_app_server_key: Option<String>,
    pub(crate) request_timeout_ms: Option<u64>,
    pub(crate) startup_timeout_ms: Option<u64>,
    pub(crate) cache_dir: Option<PathBuf>,
    pub(crate) runtime_dir: Option<PathBuf>,
    pub(crate) client_name: Option<String>,
    pub(crate) client_version: Option<String>,
}

impl ProviderConfig {
    /// Creates a provider builder.
    #[must_use]
    pub fn builder() -> ProviderConfigBuilder {
        ProviderConfigBuilder::new()
    }

    /// Returns the provider family or adapter kind.
    #[must_use]
    pub const fn driver(&self) -> &str {
        self.driver.as_str()
    }

    /// Returns the canonical provider family or adapter kind.
    #[must_use]
    pub const fn kind(&self) -> &str {
        self.driver()
    }

    /// Returns an explicit binary override, if configured.
    #[must_use]
    pub fn binary(&self) -> Option<&Path> {
        self.binary.as_deref()
    }

    /// Returns the provider model override.
    #[must_use]
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    /// Returns provider command-line arguments.
    #[must_use]
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Returns provider-specific environment variables.
    #[must_use]
    pub const fn env(&self) -> &BTreeMap<String, String> {
        &self.env
    }

    /// Returns the working directory used when spawning provider processes.
    #[must_use]
    pub fn cwd(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    /// Returns the shared app-server key used for this installation.
    #[must_use]
    pub fn shared_app_server_key(&self) -> Option<&str> {
        self.shared_app_server_key.as_deref()
    }

    /// Returns the request timeout override in milliseconds.
    #[must_use]
    pub const fn request_timeout_ms(&self) -> Option<u64> {
        self.request_timeout_ms
    }

    /// Returns the startup timeout override in milliseconds.
    #[must_use]
    pub const fn startup_timeout_ms(&self) -> Option<u64> {
        self.startup_timeout_ms
    }

    /// Returns the provider cache directory override.
    #[must_use]
    pub fn cache_dir(&self) -> Option<&Path> {
        self.cache_dir.as_deref()
    }

    /// Returns the provider runtime directory override.
    #[must_use]
    pub fn runtime_dir(&self) -> Option<&Path> {
        self.runtime_dir.as_deref()
    }

    /// Returns the client identity used by this installation.
    #[must_use]
    pub fn client_name(&self) -> Option<&str> {
        self.client_name.as_deref()
    }

    /// Returns the client version used by this installation.
    #[must_use]
    pub fn client_version(&self) -> Option<&str> {
        self.client_version.as_deref()
    }

    pub(super) fn prerequisite_binary(&self) -> String {
        let Some(binary) = self.binary.as_ref() else {
            return default_binary_for_kind(self.driver.as_str());
        };

        binary.to_string_lossy().into_owned()
    }
}

/// Agent-level defaults used by the orchestrator.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentConfig {
    pub(crate) provider: String,
    pub(crate) driver: Option<String>,
    pub(crate) profile: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) defaults: ProviderRequestDefaults,
    pub(crate) config: Option<ProviderBehaviorLayer>,
    pub(crate) request_extra: BTreeMap<String, Value>,
    pub(crate) instructions: Option<String>,
    pub(crate) max_turns: Option<u16>,
    pub(crate) tools: Vec<String>,
}

impl AgentConfig {
    /// Creates an agent builder.
    #[must_use]
    pub fn builder() -> AgentConfigBuilder {
        AgentConfigBuilder::new()
    }

    /// Returns the provider entry used by this agent.
    #[must_use]
    pub const fn provider(&self) -> &str {
        self.provider.as_str()
    }

    /// Returns the explicit driver override for the agent, if set.
    #[must_use]
    pub fn driver(&self) -> Option<&str> {
        self.driver.as_deref()
    }

    /// Returns the reusable provider profile selected by this agent.
    #[must_use]
    pub fn profile(&self) -> Option<&str> {
        self.profile.as_deref()
    }

    /// Returns the per-agent model override.
    #[must_use]
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    /// Returns the per-agent request defaults.
    #[must_use]
    pub const fn defaults(&self) -> &ProviderRequestDefaults {
        &self.defaults
    }

    /// Returns the per-agent typed provider behavior block.
    #[must_use]
    pub const fn config(&self) -> Option<&ProviderBehaviorLayer> {
        self.config.as_ref()
    }

    /// Returns bounded request-level provider overrides.
    #[must_use]
    pub const fn request_extra(&self) -> &BTreeMap<String, Value> {
        &self.request_extra
    }

    /// Returns per-agent instructions.
    #[must_use]
    pub fn instructions(&self) -> Option<&str> {
        self.instructions.as_deref()
    }

    /// Returns the optional maximum turn budget.
    #[must_use]
    pub const fn max_turns(&self) -> Option<u16> {
        self.max_turns
    }

    /// Returns the tool allow-list for the agent.
    #[must_use]
    pub fn tools(&self) -> &[String] {
        &self.tools
    }
}

/// Builder for programmatic `ArkyConfig` creation.
#[derive(Debug, Clone, Default)]
pub struct ArkyConfigBuilder {
    partial: PartialArkyConfig,
}

impl ArkyConfigBuilder {
    /// Creates an empty config builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Merges workspace-level settings into the builder.
    #[must_use]
    pub fn workspace(mut self, workspace: WorkspaceConfigBuilder) -> Self {
        self.partial.workspace = merge_workspace(self.partial.workspace, workspace.into_partial());
        self
    }

    /// Merges or creates a provider entry.
    #[must_use]
    pub fn provider(mut self, name: impl Into<String>, provider: ProviderConfigBuilder) -> Self {
        let name = name.into();
        let merged = match self.partial.providers.remove(&name) {
            Some(existing) => merge_provider(existing, provider.into_partial()),
            None => provider.into_partial(),
        };
        self.partial.providers.insert(name, merged);
        self
    }

    /// Merges or creates a reusable provider profile.
    #[must_use]
    pub fn profile(
        mut self,
        name: impl Into<String>,
        profile: ProviderProfileConfigBuilder,
    ) -> Self {
        let name = name.into();
        let merged = match self.partial.profiles.remove(&name) {
            Some(existing) => merge_profile(existing, profile.into_partial()),
            None => profile.into_partial(),
        };
        self.partial.profiles.insert(name, merged);
        self
    }

    /// Merges or creates an agent entry.
    #[must_use]
    pub fn agent(mut self, name: impl Into<String>, agent: AgentConfigBuilder) -> Self {
        let name = name.into();
        let merged = match self.partial.agents.remove(&name) {
            Some(existing) => merge_agent(existing, agent.into_partial()),
            None => agent.into_partial(),
        };
        self.partial.agents.insert(name, merged);
        self
    }

    pub(crate) fn into_partial(self) -> PartialArkyConfig {
        self.partial
    }

    /// Builds the final merged and validated config.
    pub fn build(self) -> Result<ArkyConfig, ConfigError> {
        validate_config(self.partial)
    }
}

/// Builder for `WorkspaceConfig`.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceConfigBuilder {
    partial: PartialWorkspaceConfig,
}

impl WorkspaceConfigBuilder {
    /// Creates an empty workspace builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the workspace name.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.partial.name = Some(name.into());
        self
    }

    /// Sets the default provider name.
    #[must_use]
    pub fn default_provider(mut self, default_provider: impl Into<String>) -> Self {
        self.partial.default_provider = Some(default_provider.into());
        self
    }

    /// Sets the workspace data directory.
    #[must_use]
    pub fn data_dir(mut self, data_dir: impl Into<PathBuf>) -> Self {
        self.partial.data_dir = Some(data_dir.into());
        self
    }

    /// Adds or replaces one workspace environment variable.
    #[must_use]
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let env = self.partial.env.get_or_insert_with(BTreeMap::new);
        env.insert(key.into(), value.into());
        self
    }

    fn into_partial(self) -> PartialWorkspaceConfig {
        self.partial
    }
}

/// Builder for `ProviderConfig`.
#[derive(Debug, Clone, Default)]
pub struct ProviderConfigBuilder {
    partial: PartialProviderConfig,
}

impl ProviderConfigBuilder {
    /// Creates an empty provider builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the provider kind.
    #[must_use]
    pub fn kind(mut self, kind: impl Into<String>) -> Self {
        self.partial.driver = Some(kind.into());
        self
    }

    /// Sets the canonical provider driver name.
    #[must_use]
    pub fn driver(mut self, driver: impl Into<String>) -> Self {
        self.partial.driver = Some(driver.into());
        self
    }

    /// Sets an explicit provider binary path or command name.
    #[must_use]
    pub fn binary(mut self, binary: impl Into<PathBuf>) -> Self {
        self.partial.binary = Some(binary.into());
        self
    }

    /// Sets a provider model override.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.partial.model = Some(model.into());
        self
    }

    /// Replaces provider arguments.
    #[must_use]
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.partial.args = Some(args.into_iter().map(Into::into).collect());
        self
    }

    /// Adds or replaces one provider environment variable.
    #[must_use]
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let env = self.partial.env.get_or_insert_with(BTreeMap::new);
        env.insert(key.into(), value.into());
        self
    }

    /// Sets the provider working directory.
    #[must_use]
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.partial.cwd = Some(cwd.into());
        self
    }

    /// Sets the shared app-server key for this provider installation.
    #[must_use]
    pub fn shared_app_server_key(mut self, key: impl Into<String>) -> Self {
        self.partial.shared_app_server_key = Some(key.into());
        self
    }

    /// Sets the request timeout override in milliseconds.
    #[must_use]
    pub const fn request_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.partial.request_timeout_ms = Some(timeout_ms);
        self
    }

    /// Sets the startup timeout override in milliseconds.
    #[must_use]
    pub const fn startup_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.partial.startup_timeout_ms = Some(timeout_ms);
        self
    }

    /// Sets the provider cache directory.
    #[must_use]
    pub fn cache_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.partial.cache_dir = Some(path.into());
        self
    }

    /// Sets the provider runtime directory.
    #[must_use]
    pub fn runtime_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.partial.runtime_dir = Some(path.into());
        self
    }

    /// Sets the client name used by the installation.
    #[must_use]
    pub fn client_name(mut self, client_name: impl Into<String>) -> Self {
        self.partial.client_name = Some(client_name.into());
        self
    }

    /// Sets the client version used by the installation.
    #[must_use]
    pub fn client_version(mut self, client_version: impl Into<String>) -> Self {
        self.partial.client_version = Some(client_version.into());
        self
    }

    fn into_partial(self) -> PartialProviderConfig {
        self.partial
    }
}

/// Builder for reusable provider profiles.
#[derive(Debug, Clone, Default)]
pub struct ProviderProfileConfigBuilder {
    partial: PartialProviderProfileConfig,
}

impl ProviderProfileConfigBuilder {
    /// Creates an empty profile builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the driver targeted by this profile.
    #[must_use]
    pub fn driver(mut self, driver: impl Into<String>) -> Self {
        self.partial.driver = Some(driver.into());
        self
    }

    /// Sets the model default for the profile.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.partial.model = Some(model.into());
        self
    }

    /// Replaces the profile request defaults.
    #[must_use]
    pub fn defaults(mut self, defaults: ProviderRequestDefaults) -> Self {
        self.partial.defaults = self.partial.defaults.merge(&defaults);
        self
    }

    /// Replaces the typed provider behavior config.
    #[must_use]
    pub fn config(mut self, config: PartialProviderBehaviorConfig) -> Self {
        self.partial.config = self.partial.config.merge(config);
        self
    }

    fn into_partial(self) -> PartialProviderProfileConfig {
        self.partial
    }
}

/// Builder for `AgentConfig`.
#[derive(Debug, Clone, Default)]
pub struct AgentConfigBuilder {
    partial: PartialAgentConfig,
}

impl AgentConfigBuilder {
    /// Creates an empty agent builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the provider entry name for the agent.
    #[must_use]
    pub fn provider(mut self, provider: impl Into<String>) -> Self {
        self.partial.provider = Some(provider.into());
        self
    }

    /// Sets the explicit provider driver for the agent.
    #[must_use]
    pub fn driver(mut self, driver: impl Into<String>) -> Self {
        self.partial.driver = Some(driver.into());
        self
    }

    /// Sets the model override for the agent.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.partial.model = Some(model.into());
        self
    }

    /// Selects a reusable provider profile for the agent.
    #[must_use]
    pub fn profile(mut self, profile: impl Into<String>) -> Self {
        self.partial.profile = Some(profile.into());
        self
    }

    /// Sets the maximum provider token budget for the agent.
    #[must_use]
    pub const fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.partial.defaults.max_tokens = Some(max_tokens);
        self
    }

    /// Sets the provider reasoning effort for the agent.
    #[must_use]
    pub const fn reasoning_effort(
        mut self,
        reasoning_effort: arky_protocol::ReasoningEffort,
    ) -> Self {
        self.partial.defaults.reasoning_effort = Some(reasoning_effort);
        self
    }

    /// Adds or replaces one bounded request-level extra field.
    #[must_use]
    pub fn request_extra(mut self, key: impl Into<String>, value: Value) -> Self {
        self.partial.request_extra.insert(key.into(), value);
        self
    }

    /// Sets per-agent instructions.
    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.partial.instructions = Some(instructions.into());
        self
    }

    /// Sets the maximum turn budget.
    #[must_use]
    pub const fn max_turns(mut self, max_turns: u16) -> Self {
        self.partial.max_turns = Some(max_turns);
        self
    }

    /// Replaces the tool allow-list.
    #[must_use]
    pub fn tools(mut self, tools: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.partial.tools = Some(tools.into_iter().map(Into::into).collect());
        self
    }

    fn into_partial(self) -> PartialAgentConfig {
        self.partial
    }
}

/// Loads config from files, environment variables, and builder overrides.
#[derive(Debug, Clone)]
pub struct ConfigLoader {
    file_path: Option<PathBuf>,
    env_prefix: String,
    env_overrides: Option<Vec<(String, String)>>,
    builder_overrides: PartialArkyConfig,
}

impl Default for ConfigLoader {
    fn default() -> Self {
        Self {
            file_path: None,
            env_prefix: DEFAULT_ENV_PREFIX.to_owned(),
            env_overrides: None,
            builder_overrides: PartialArkyConfig::default(),
        }
    }
}

impl ConfigLoader {
    /// Creates a loader using the current process environment.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a loader that starts from one config file.
    #[must_use]
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self::new().with_file(path)
    }

    /// Sets the source config file.
    #[must_use]
    pub fn with_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.file_path = Some(path.into());
        self
    }

    /// Replaces the environment variable prefix used for overrides.
    #[must_use]
    pub fn with_env_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.env_prefix = prefix.into();
        self
    }

    /// Injects explicit environment overrides in `KEY=value` form.
    ///
    /// This is primarily useful for deterministic tests or hosts that want to
    /// avoid reading from the global process environment.
    #[must_use]
    pub fn with_env_overrides<I, K, V>(mut self, overrides: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.env_overrides = Some(
            overrides
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect(),
        );
        self
    }

    /// Merges builder overrides after file and environment values.
    #[must_use]
    pub fn with_builder_overrides(mut self, builder: ArkyConfigBuilder) -> Self {
        self.builder_overrides = merge_config(self.builder_overrides, builder.into_partial());
        self
    }

    /// Loads, merges, and validates the final config.
    pub fn load(self) -> Result<ArkyConfig, ConfigError> {
        let mut merged = match self.file_path.as_deref() {
            Some(path) => load_file(path)?,
            None => PartialArkyConfig::default(),
        };

        let env_overrides = match self.env_overrides {
            Some(overrides) => load_env_overrides(self.env_prefix.as_str(), overrides)?,
            None => load_env_overrides(self.env_prefix.as_str(), std::env::vars())?,
        };

        merged = merge_config(merged, env_overrides);
        merged = merge_config(merged, self.builder_overrides);

        validate_config(merged)
    }
}

pub fn load_file(path: &Path) -> Result<PartialArkyConfig, ConfigError> {
    if !path.exists() {
        return Err(ConfigError::NotFound {
            path: path.to_path_buf(),
        });
    }

    let format = ConfigFormat::from_path(path).ok_or_else(|| {
        ConfigError::parse(
            format!(
                "unsupported config file format for `{}`",
                path.to_string_lossy()
            ),
            Some(path.to_path_buf()),
            None,
        )
    })?;

    let contents = fs::read_to_string(path).map_err(|error| {
        ConfigError::parse(
            format!("failed to read config file: {error}"),
            Some(path.to_path_buf()),
            Some(format.name()),
        )
    })?;

    match format {
        ConfigFormat::Toml => toml::from_str(&contents).map_err(|error| {
            ConfigError::parse(
                format!("failed to parse TOML config: {error}"),
                Some(path.to_path_buf()),
                Some(format.name()),
            )
        }),
        ConfigFormat::Yaml => serde_norway::from_str(&contents).map_err(|error| {
            ConfigError::parse(
                format!("failed to parse YAML config: {error}"),
                Some(path.to_path_buf()),
                Some(format.name()),
            )
        }),
    }
}

fn load_env_overrides<I>(prefix: &str, overrides: I) -> Result<PartialArkyConfig, ConfigError>
where
    I: IntoIterator<Item = (String, String)>,
{
    let prefix = format!("{prefix}_");
    let mut root = Map::new();

    for (key, value) in overrides {
        let Some(rest) = key.strip_prefix(prefix.as_str()) else {
            continue;
        };

        let path = normalize_env_path(rest);
        if path.is_empty() {
            continue;
        }

        insert_path_value(&mut root, &path, parse_env_value(value.as_str()));
    }

    if root.is_empty() {
        return Ok(PartialArkyConfig::default());
    }

    serde_json::from_value(Value::Object(root)).map_err(|error| {
        ConfigError::parse(
            format!("failed to parse environment overrides: {error}"),
            None,
            Some("env"),
        )
    })
}

fn normalize_env_path(key: &str) -> Vec<String> {
    let raw_segments = key
        .split("__")
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let mut segments = Vec::with_capacity(raw_segments.len());
    let mut index = 0;

    while index < raw_segments.len() {
        if matches!(segments.last(), Some(last) if last == "env") {
            segments.push(raw_segments[index..].join("__"));
            break;
        }

        segments.push(raw_segments[index].to_ascii_lowercase());
        index += 1;
    }

    segments
}

fn insert_path_value(current: &mut Map<String, Value>, path: &[String], value: Value) {
    if path.len() == 1 {
        current.insert(path[0].clone(), value);
        return;
    }

    let key = path[0].clone();
    let entry = current
        .entry(key)
        .or_insert_with(|| Value::Object(Map::new()));
    let next = ensure_object(entry);

    insert_path_value(next, &path[1..], value);
}

fn ensure_object(value: &mut Value) -> &mut Map<String, Value> {
    if !matches!(value, Value::Object(_)) {
        *value = Value::Object(Map::new());
    }

    match value {
        Value::Object(map) => map,
        _ => unreachable!("value was normalized to an object"),
    }
}

fn parse_env_value(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_owned()))
}

fn default_binary_for_kind(kind: &str) -> String {
    match kind {
        "claude" | "claude-code" | "zai" | "openrouter" | "vercel" | "moonshot" | "minimax"
        | "bedrock" | "vertex" | "ollama" => "claude".to_owned(),
        "codex" => "codex".to_owned(),
        other => other.to_owned(),
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialArkyConfig {
    #[serde(default)]
    pub workspace: PartialWorkspaceConfig,
    #[serde(default)]
    pub providers: BTreeMap<String, PartialProviderConfig>,
    #[serde(default)]
    pub profiles: BTreeMap<String, PartialProviderProfileConfig>,
    #[serde(default)]
    pub agents: BTreeMap<String, PartialAgentConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialWorkspaceConfig {
    pub name: Option<String>,
    pub default_provider: Option<String>,
    pub data_dir: Option<PathBuf>,
    pub env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub profiles: BTreeMap<String, PartialProviderProfileConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialProviderConfig {
    #[serde(default, alias = "kind")]
    pub driver: Option<String>,
    pub binary: Option<PathBuf>,
    pub model: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<BTreeMap<String, String>>,
    pub cwd: Option<PathBuf>,
    pub shared_app_server_key: Option<String>,
    pub request_timeout_ms: Option<u64>,
    pub startup_timeout_ms: Option<u64>,
    pub cache_dir: Option<PathBuf>,
    pub runtime_dir: Option<PathBuf>,
    pub client_name: Option<String>,
    pub client_version: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialAgentConfig {
    pub provider: Option<String>,
    pub driver: Option<String>,
    pub profile: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub defaults: ProviderRequestDefaults,
    #[serde(default)]
    pub config: PartialProviderBehaviorConfig,
    #[serde(default)]
    pub request_extra: BTreeMap<String, Value>,
    pub instructions: Option<String>,
    pub max_turns: Option<u16>,
    pub tools: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
    };

    use arky_protocol::ReasoningEffort;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use super::{
        AgentConfigBuilder, ArkyConfig, ArkyConfigBuilder, ConfigLoader, PartialAgentConfig,
        PartialArkyConfig, PartialProviderConfig, PartialWorkspaceConfig, ProviderConfigBuilder,
        ProviderProfileConfigBuilder, WorkspaceConfigBuilder,
    };
    use crate::{
        validate::validate_config, ClaudeCodeBehaviorLayer, CodexBehaviorLayer, ConfigError,
        PartialProviderBehaviorConfig, PartialProviderProfileConfig, ProviderRequestDefaults,
        ResolvedProviderBehaviorConfig,
    };

    #[test]
    fn file_loading_should_parse_valid_toml() {
        let directory = tempdir().expect("temp directory should be created");
        let path = directory.path().join("arky.toml");
        fs::write(
            &path,
            r#"
                [workspace]
                name = "demo"
                default_provider = "default"

                [providers.default]
                kind = "claude-code"
                model = "claude-sonnet-4"

                [agents.writer]
                provider = "default"
                max_turns = 8
            "#,
        )
        .expect("config file should be written");

        let config = ArkyConfig::from_path(&path).expect("config should load");

        let actual = (
            config.workspace().name(),
            config.workspace().default_provider(),
            config.provider("default").and_then(|value| value.model()),
            config
                .agent("writer")
                .and_then(super::AgentConfig::max_turns),
        );

        let expected = (
            Some("demo"),
            Some("default"),
            Some("claude-sonnet-4"),
            Some(8),
        );

        assert_eq!(actual, expected);
    }

    #[test]
    fn file_loading_should_return_parse_failed_for_invalid_toml() {
        let directory = tempdir().expect("temp directory should be created");
        let path = directory.path().join("arky.toml");
        fs::write(&path, "not = [valid").expect("config file should be written");

        let error = ArkyConfig::from_path(&path).expect_err("invalid TOML should fail");

        let actual = match error {
            ConfigError::ParseFailed { path, format, .. } => (
                path.map(|value| value.to_string_lossy().into_owned()),
                format,
            ),
            other => panic!("expected parse error, got {other:?}"),
        };

        let expected = (Some(path.to_string_lossy().into_owned()), Some("toml"));

        assert_eq!(actual, expected);
    }

    #[test]
    fn env_overrides_should_override_file_values() {
        let directory = tempdir().expect("temp directory should be created");
        let path = directory.path().join("arky.toml");
        fs::write(
            &path,
            r#"
                [workspace]
                default_provider = "default"

                [providers.default]
                kind = "claude-code"
                model = "file-model"

                [agents.writer]
                provider = "default"
                model = "file-model"
            "#,
        )
        .expect("config file should be written");

        let config = ConfigLoader::from_path(&path)
            .with_env_overrides([
                (
                    "ARKY_PROVIDERS__DEFAULT__MODEL".to_owned(),
                    "env-model".to_owned(),
                ),
                (
                    "ARKY_AGENTS__WRITER__MODEL".to_owned(),
                    "env-agent-model".to_owned(),
                ),
            ])
            .load()
            .expect("config should load");

        let actual = (
            config.provider("default").and_then(|value| value.model()),
            config.agent("writer").and_then(|value| value.model()),
        );

        let expected = (Some("env-model"), Some("env-agent-model"));

        assert_eq!(actual, expected);
    }

    #[test]
    fn builder_should_override_environment_values() {
        let config = ConfigLoader::new()
            .with_env_overrides([(
                "ARKY_PROVIDERS__DEFAULT__MODEL".to_owned(),
                "env-model".to_owned(),
            )])
            .with_builder_overrides(
                ArkyConfig::builder()
                    .workspace(WorkspaceConfigBuilder::new().default_provider("default"))
                    .provider(
                        "default",
                        ProviderConfigBuilder::new()
                            .kind("claude-code")
                            .model("builder-model"),
                    ),
            )
            .load()
            .expect("config should build");

        let actual = config.provider("default").and_then(|value| value.model());

        let expected = Some("builder-model");

        assert_eq!(actual, expected);
    }

    #[test]
    fn yaml_loading_should_parse_valid_config() {
        let directory = tempdir().expect("temp directory should be created");
        let path = directory.path().join("arky.yaml");
        fs::write(
            &path,
            r"
workspace:
  default_provider: default
providers:
  default:
    kind: codex
    binary: cargo
agents:
  reviewer:
    provider: default
    tools:
      - read_file
",
        )
        .expect("config file should be written");

        let config = ArkyConfig::from_path(&path).expect("config should load");

        let actual = (
            config.workspace().default_provider(),
            config.provider("default").map(super::ProviderConfig::kind),
            config.agent("reviewer").map(|value| value.tools().to_vec()),
        );

        let expected = (
            Some("default"),
            Some("codex"),
            Some(vec!["read_file".to_owned()]),
        );

        assert_eq!(actual, expected);
    }

    #[test]
    fn builder_should_support_programmatic_configuration() {
        let config = ArkyConfigBuilder::new()
            .workspace(
                WorkspaceConfigBuilder::new()
                    .name("workspace")
                    .default_provider("default")
                    .data_dir(PathBuf::from("/tmp/arky"))
                    .env("RUST_LOG", "debug"),
            )
            .provider(
                "default",
                ProviderConfigBuilder::new()
                    .kind("claude-code")
                    .binary("claude")
                    .model("claude-sonnet-4")
                    .args(["--json"])
                    .env("API_KEY", "secret"),
            )
            .agent(
                "writer",
                crate::AgentConfigBuilder::new()
                    .provider("default")
                    .model("claude-sonnet-4")
                    .instructions("write clearly")
                    .max_turns(6)
                    .tools(["search", "edit"]),
            )
            .build()
            .expect("config should build");

        let actual = (
            config.workspace().name(),
            config.workspace().data_dir(),
            config
                .provider("default")
                .map(|value| value.args().to_vec()),
            config
                .agent("writer")
                .and_then(|value| value.instructions()),
            config.agent("writer").map(|value| value.tools().to_vec()),
        );

        let expected = (
            Some("workspace"),
            Some(Path::new("/tmp/arky")),
            Some(vec!["--json".to_owned()]),
            Some("write clearly"),
            Some(vec!["search".to_owned(), "edit".to_owned()]),
        );

        assert_eq!(actual, expected);
    }

    #[test]
    fn provider_profile_config_builder_should_expose_defaults_and_config() {
        let config = ArkyConfig::builder()
            .provider("default", ProviderConfigBuilder::new().driver("codex"))
            .profile(
                "fast-research",
                ProviderProfileConfigBuilder::new()
                    .driver("codex")
                    .model("gpt-4o")
                    .defaults(ProviderRequestDefaults {
                        max_tokens: Some(700),
                        reasoning_effort: Some(ReasoningEffort::Medium),
                    })
                    .config(PartialProviderBehaviorConfig {
                        codex: Some(CodexBehaviorLayer {
                            web_search: Some(true),
                            ..CodexBehaviorLayer::default()
                        }),
                        ..PartialProviderBehaviorConfig::default()
                    }),
            )
            .agent(
                "writer",
                AgentConfigBuilder::new()
                    .provider("default")
                    .profile("fast-research"),
            )
            .build()
            .expect("config should build");

        let profile = config
            .profile("fast-research")
            .expect("profile should be present");

        assert_eq!(profile.driver(), "codex");
        assert_eq!(profile.model(), Some("gpt-4o"));
        assert_eq!(profile.defaults().max_tokens, Some(700));
        assert_eq!(
            profile.defaults().reasoning_effort,
            Some(ReasoningEffort::Medium)
        );
    }

    #[test]
    fn profile_driver_mismatch_should_produce_validation_issue() {
        let error = validate_config(PartialArkyConfig {
            workspace: PartialWorkspaceConfig::default(),
            providers: BTreeMap::from([(
                "default".to_owned(),
                PartialProviderConfig {
                    driver: Some("codex".to_owned()),
                    ..PartialProviderConfig::default()
                },
            )]),
            profiles: BTreeMap::from([(
                "default".to_owned(),
                PartialProviderProfileConfig {
                    driver: Some("codex".to_owned()),
                    config: PartialProviderBehaviorConfig {
                        claude_code: Some(ClaudeCodeBehaviorLayer {
                            continue_conversation: Some(true),
                            ..ClaudeCodeBehaviorLayer::default()
                        }),
                        ..PartialProviderBehaviorConfig::default()
                    },
                    ..PartialProviderProfileConfig::default()
                },
            )]),
            agents: BTreeMap::new(),
        })
        .expect_err("profile typed namespace mismatch should fail");

        let actual = collect_validation_messages(error);
        let expected = vec![(
            "profiles.default.config.claude_code".to_owned(),
            "is not supported for driver `codex`; use `profiles.default.config.codex`".to_owned(),
        )];

        assert_eq!(actual, expected);
    }

    #[test]
    fn agent_driver_mismatch_should_produce_validation_issue() {
        let error = validate_config(PartialArkyConfig {
            workspace: PartialWorkspaceConfig::default(),
            providers: BTreeMap::from([(
                "default".to_owned(),
                PartialProviderConfig {
                    driver: Some("claude-code".to_owned()),
                    ..PartialProviderConfig::default()
                },
            )]),
            profiles: BTreeMap::new(),
            agents: BTreeMap::from([(
                "writer".to_owned(),
                PartialAgentConfig {
                    provider: Some("default".to_owned()),
                    driver: Some("claude-code".to_owned()),
                    config: PartialProviderBehaviorConfig {
                        codex: Some(CodexBehaviorLayer {
                            web_search: Some(true),
                            ..CodexBehaviorLayer::default()
                        }),
                        ..PartialProviderBehaviorConfig::default()
                    },
                    ..PartialAgentConfig::default()
                },
            )]),
        })
        .expect_err("agent typed namespace mismatch should fail");

        let actual = collect_validation_messages(error);
        let expected = vec![(
            "agents.writer.config.codex".to_owned(),
            "is not supported for driver `claude-code`; use `agents.writer.config.claude_code`"
                .to_owned(),
        )];

        assert_eq!(actual, expected);
    }

    #[test]
    fn resolve_agent_provider_should_merge_workspace_profile_agent_in_order() {
        let directory = tempdir().expect("temp directory should be created");
        let path = directory.path().join("layered.toml");
        fs::write(
            &path,
            r#"
                [workspace]
                default_provider = "default"

                [workspace.profiles.fast-research]
                driver = "codex"
                model = "gpt-4o"

                [workspace.profiles.fast-research.defaults]
                reasoning_effort = "medium"

                [workspace.profiles.fast-research.config.codex]
                include_plan_tool = true

                [providers.default]
                driver = "codex"
                binary = "cargo"
                model = "install-model"

                [agents.writer]
                provider = "default"
                profile = "fast-research"

                [agents.writer.defaults]
                max_tokens = 1200

                [agents.writer.config.codex]
                resume_last = true
            "#,
        )
        .expect("config file should be written");

        let config = ConfigLoader::from_path(&path)
            .load()
            .expect("config should load");
        let resolved = config
            .resolve_agent_provider("writer")
            .expect("writer provider should resolve");

        assert_eq!(resolved.install.binary(), Some(Path::new("cargo")));
        assert_eq!(resolved.model.as_deref(), Some("gpt-4o"));
        assert_eq!(resolved.defaults.max_tokens, Some(1_200));
        assert_eq!(
            resolved.defaults.reasoning_effort,
            Some(ReasoningEffort::Medium)
        );

        match resolved.config.expect("config should resolve") {
            ResolvedProviderBehaviorConfig::Codex(config) => {
                assert!(config.workspace.include_plan_tool);
                assert!(config.workspace.resume_last);
            }
            other => panic!("expected codex config, got {other:?}"),
        }
    }

    #[test]
    fn profile_defaults_should_override_workspace_and_agent_overrides_profile() {
        let config = validate_config(PartialArkyConfig {
            workspace: PartialWorkspaceConfig::default(),
            providers: BTreeMap::from([(
                "default".to_owned(),
                PartialProviderConfig {
                    driver: Some("codex".to_owned()),
                    model: Some("install-model".to_owned()),
                    ..PartialProviderConfig::default()
                },
            )]),
            profiles: BTreeMap::from([(
                "safe-doc-writer".to_owned(),
                PartialProviderProfileConfig {
                    driver: Some("codex".to_owned()),
                    model: Some("profile-model".to_owned()),
                    defaults: ProviderRequestDefaults {
                        max_tokens: Some(700),
                        reasoning_effort: Some(ReasoningEffort::Low),
                    },
                    ..PartialProviderProfileConfig::default()
                },
            )]),
            agents: BTreeMap::from([
                (
                    "profile_only".to_owned(),
                    PartialAgentConfig {
                        provider: Some("default".to_owned()),
                        profile: Some("safe-doc-writer".to_owned()),
                        ..PartialAgentConfig::default()
                    },
                ),
                (
                    "agent_override".to_owned(),
                    PartialAgentConfig {
                        provider: Some("default".to_owned()),
                        profile: Some("safe-doc-writer".to_owned()),
                        model: Some("agent-model".to_owned()),
                        defaults: ProviderRequestDefaults {
                            max_tokens: Some(900),
                            reasoning_effort: None,
                        },
                        ..PartialAgentConfig::default()
                    },
                ),
            ]),
        })
        .expect("config should validate");

        let profile_only = config
            .resolve_agent_provider("profile_only")
            .expect("profile_only should resolve");
        let agent_override = config
            .resolve_agent_provider("agent_override")
            .expect("agent_override should resolve");

        assert_eq!(profile_only.install.model(), Some("install-model"));
        assert_eq!(profile_only.model.as_deref(), Some("profile-model"));
        assert_eq!(profile_only.defaults.max_tokens, Some(700));
        assert_eq!(
            profile_only.defaults.reasoning_effort,
            Some(ReasoningEffort::Low)
        );
        assert_eq!(agent_override.model.as_deref(), Some("agent-model"));
        assert_eq!(agent_override.defaults.max_tokens, Some(900));
        assert_eq!(
            agent_override.defaults.reasoning_effort,
            Some(ReasoningEffort::Low)
        );
    }

    #[test]
    fn profile_table_should_parse_and_validate_to_provider_profile_config() {
        let directory = tempdir().expect("temp directory should be created");
        let path = directory.path().join("profiles.toml");
        fs::write(
            &path,
            r#"
                [providers.default]
                driver = "codex"

                [profiles.fast-research]
                driver = "codex"
                model = "gpt-4o"

                [profiles.fast-research.config.codex]
                web_search = true
            "#,
        )
        .expect("config file should be written");

        let config = ConfigLoader::from_path(&path)
            .load()
            .expect("config should load");
        let profile = config
            .profile("fast-research")
            .expect("profile should be parsed");

        assert_eq!(profile.driver(), "codex");
        assert_eq!(profile.model(), Some("gpt-4o"));
    }

    #[test]
    fn safe_doc_writer_profile_reference_should_resolve_merged_config() {
        let directory = tempdir().expect("temp directory should be created");
        let path = directory.path().join("safe-doc-writer.toml");
        fs::write(
            &path,
            r#"
                [providers.default]
                driver = "codex"
                binary = "cargo"

                [profiles.safe-doc-writer]
                driver = "codex"
                model = "gpt-4o"

                [profiles.safe-doc-writer.defaults]
                reasoning_effort = "medium"

                [profiles.safe-doc-writer.config.codex]
                include_plan_tool = true

                [agents.writer]
                provider = "default"
                profile = "safe-doc-writer"

                [agents.writer.config.codex]
                resume_last = true
            "#,
        )
        .expect("config file should be written");

        let config = ConfigLoader::from_path(&path)
            .load()
            .expect("config should load");
        let resolved = config
            .resolve_agent_provider("writer")
            .expect("writer should resolve");

        assert_eq!(resolved.model.as_deref(), Some("gpt-4o"));
        assert_eq!(
            resolved.defaults.reasoning_effort,
            Some(ReasoningEffort::Medium)
        );

        match resolved.config.expect("config should resolve") {
            ResolvedProviderBehaviorConfig::Codex(config) => {
                assert!(config.workspace.include_plan_tool);
                assert!(config.workspace.resume_last);
            }
            other => panic!("expected codex config, got {other:?}"),
        }
    }

    #[test]
    fn missing_profile_reference_should_fail_with_profile_name() {
        let directory = tempdir().expect("temp directory should be created");
        let path = directory.path().join("missing-profile.toml");
        fs::write(
            &path,
            r#"
                [providers.default]
                driver = "codex"

                [agents.writer]
                provider = "default"
                profile = "missing-profile"
            "#,
        )
        .expect("config file should be written");

        let error = ConfigLoader::from_path(&path)
            .load()
            .expect_err("missing profile should fail");

        let actual = collect_validation_messages(error);
        let expected = vec![(
            "agents.writer.profile".to_owned(),
            "references unknown profile `missing-profile`".to_owned(),
        )];

        assert_eq!(actual, expected);
    }

    #[test]
    fn request_extra_api_key_should_fail_validation_before_compilation() {
        let directory = tempdir().expect("temp directory should be created");
        let path = directory.path().join("request-extra.toml");
        fs::write(
            &path,
            r#"
                [providers.default]
                driver = "codex"

                [agents.writer]
                provider = "default"

                [agents.writer.request_extra]
                api_key = "secret"
            "#,
        )
        .expect("config file should be written");

        let error = ConfigLoader::from_path(&path)
            .load()
            .expect_err("request_extra should fail validation");

        let actual = collect_validation_messages(error);
        let expected = vec![(
            "agents.writer.request_extra.api_key".to_owned(),
            "is reserved for installation/workspace provider config and is not allowed in request_extra"
                .to_owned(),
        )];

        assert_eq!(actual, expected);
    }

    fn collect_validation_messages(error: ConfigError) -> Vec<(String, String)> {
        match error {
            ConfigError::ValidationFailed { issues, .. } => issues
                .iter()
                .map(|issue| (issue.field().to_owned(), issue.message().to_owned()))
                .collect(),
            other => panic!("expected validation error, got {other:?}"),
        }
    }
}
