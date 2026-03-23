//! Claude-compatible provider profiles and wrapper types.

use std::collections::BTreeMap;

use arky_protocol::ProviderId;
use arky_provider::{
    GenerateResponse, Provider, ProviderCapabilities, ProviderDescriptor, ProviderError,
    ProviderEventStream, ProviderFamily, ProviderRequest,
};

use crate::{ClaudeCodeProvider, ClaudeCodeProviderConfig, config::KNOWN_CLAUDE_MODEL_IDS};

/// Shared Claude-compatible base config reused by all derived providers.
pub type ClaudeCompatibleProviderConfig = ClaudeCodeProviderConfig;

/// Canonical Claude-compatible provider IDs supported by Arky.
pub const CLAUDE_COMPATIBLE_PROVIDER_IDS: [&str; 9] = [
    "claude-code",
    "zai",
    "openrouter",
    "vercel",
    "moonshot",
    "minimax",
    "bedrock",
    "vertex",
    "ollama",
];

/// Supported Claude-compatible provider kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClaudeCompatibleProviderKind {
    /// Direct Claude Code CLI provider.
    ClaudeCode,
    /// Z.ai Anthropic-compatible gateway.
    Zai,
    /// `OpenRouter` Anthropic-compatible gateway.
    OpenRouter,
    /// Vercel AI Gateway Anthropic-compatible endpoint.
    Vercel,
    /// Moonshot Anthropic-compatible gateway.
    Moonshot,
    /// `MiniMax` Anthropic-compatible gateway.
    Minimax,
    /// Amazon Bedrock Claude routing.
    Bedrock,
    /// Vertex Claude routing.
    Vertex,
    /// Ollama Anthropic-compatible gateway.
    Ollama,
}

impl ClaudeCompatibleProviderKind {
    /// Returns the canonical provider identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Zai => "zai",
            Self::OpenRouter => "openrouter",
            Self::Vercel => "vercel",
            Self::Moonshot => "moonshot",
            Self::Minimax => "minimax",
            Self::Bedrock => "bedrock",
            Self::Vertex => "vertex",
            Self::Ollama => "ollama",
        }
    }

    /// Returns a stable human-readable label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Zai => "Z.ai",
            Self::OpenRouter => "OpenRouter",
            Self::Vercel => "Vercel",
            Self::Moonshot => "Moonshot",
            Self::Minimax => "MiniMax",
            Self::Bedrock => "Bedrock",
            Self::Vertex => "Vertex",
            Self::Ollama => "Ollama",
        }
    }

    /// Parses one supported config/provider kind into the canonical enum.
    #[must_use]
    pub fn from_kind(kind: &str) -> Option<Self> {
        match kind {
            "claude" | "claude-code" => Some(Self::ClaudeCode),
            "zai" => Some(Self::Zai),
            "openrouter" => Some(Self::OpenRouter),
            "vercel" => Some(Self::Vercel),
            "moonshot" => Some(Self::Moonshot),
            "minimax" => Some(Self::Minimax),
            "bedrock" => Some(Self::Bedrock),
            "vertex" => Some(Self::Vertex),
            "ollama" => Some(Self::Ollama),
            _ => None,
        }
    }

    /// Returns the provider identifier object.
    #[must_use]
    pub fn provider_id(self) -> ProviderId {
        ProviderId::new(self.as_str())
    }

    pub(crate) fn descriptor(self) -> ProviderDescriptor {
        ProviderDescriptor::new(
            self.provider_id(),
            ProviderFamily::ClaudeCode,
            claude_compatible_capabilities(),
        )
    }
}

/// Typed config for the Bedrock wrapper.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BedrockProviderConfig {
    /// Shared Claude CLI settings.
    pub base: ClaudeCompatibleProviderConfig,
    /// Optional upstream Bedrock model override.
    pub selected_model: Option<String>,
    /// Optional AWS region override.
    pub region: Option<String>,
}

/// Typed config for the Vertex wrapper.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VertexProviderConfig {
    /// Shared Claude CLI settings.
    pub base: ClaudeCompatibleProviderConfig,
    /// Optional upstream Vertex model override.
    pub selected_model: Option<String>,
    /// Optional Vertex project override.
    pub project_id: Option<String>,
}

/// Typed config for the Z.ai wrapper.
#[derive(Debug, Clone, PartialEq)]
pub struct ZaiProviderConfig {
    /// Shared Claude CLI settings.
    pub base: ClaudeCompatibleProviderConfig,
    /// Z.ai API key.
    pub api_key: String,
    /// Selected upstream Z.ai model.
    pub selected_model: String,
}

impl ZaiProviderConfig {
    /// Creates a wrapper config with default base Claude settings.
    #[must_use]
    pub fn new(api_key: impl Into<String>, selected_model: impl Into<String>) -> Self {
        Self {
            base: ClaudeCompatibleProviderConfig::default(),
            api_key: api_key.into(),
            selected_model: selected_model.into(),
        }
    }
}

/// Typed config for the `OpenRouter` wrapper.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenRouterProviderConfig {
    /// Shared Claude CLI settings.
    pub base: ClaudeCompatibleProviderConfig,
    /// `OpenRouter` API key.
    pub api_key: String,
    /// Selected upstream `OpenRouter` model.
    pub selected_model: String,
}

impl OpenRouterProviderConfig {
    /// Creates a wrapper config with default base Claude settings.
    pub fn new(api_key: impl Into<String>, selected_model: impl Into<String>) -> Self {
        Self {
            base: ClaudeCompatibleProviderConfig::default(),
            api_key: api_key.into(),
            selected_model: selected_model.into(),
        }
    }
}

/// Typed config for the Vercel wrapper.
#[derive(Debug, Clone, PartialEq)]
pub struct VercelProviderConfig {
    /// Shared Claude CLI settings.
    pub base: ClaudeCompatibleProviderConfig,
    /// Vercel gateway API key.
    pub api_key: String,
    /// Selected upstream Vercel model.
    pub selected_model: String,
}

impl VercelProviderConfig {
    /// Creates a wrapper config with default base Claude settings.
    #[must_use]
    pub fn new(api_key: impl Into<String>, selected_model: impl Into<String>) -> Self {
        Self {
            base: ClaudeCompatibleProviderConfig::default(),
            api_key: api_key.into(),
            selected_model: selected_model.into(),
        }
    }
}

/// Typed config for the Moonshot wrapper.
#[derive(Debug, Clone, PartialEq)]
pub struct MoonshotProviderConfig {
    /// Shared Claude CLI settings.
    pub base: ClaudeCompatibleProviderConfig,
    /// Moonshot API key.
    pub api_key: String,
    /// Selected upstream Moonshot model.
    pub selected_model: String,
}

impl MoonshotProviderConfig {
    /// Creates a wrapper config with default base Claude settings.
    #[must_use]
    pub fn new(api_key: impl Into<String>, selected_model: impl Into<String>) -> Self {
        Self {
            base: ClaudeCompatibleProviderConfig::default(),
            api_key: api_key.into(),
            selected_model: selected_model.into(),
        }
    }
}

/// Typed config for the `MiniMax` wrapper.
#[derive(Debug, Clone, PartialEq)]
pub struct MinimaxProviderConfig {
    /// Shared Claude CLI settings.
    pub base: ClaudeCompatibleProviderConfig,
    /// `MiniMax` API key.
    pub api_key: String,
    /// Optional upstream `MiniMax` model override.
    pub selected_model: Option<String>,
}

impl MinimaxProviderConfig {
    /// Creates a wrapper config with default base Claude settings.
    #[must_use]
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            base: ClaudeCompatibleProviderConfig::default(),
            api_key: api_key.into(),
            selected_model: None,
        }
    }
}

/// Typed config for the Ollama wrapper.
#[derive(Debug, Clone, PartialEq)]
pub struct OllamaProviderConfig {
    /// Shared Claude CLI settings.
    pub base: ClaudeCompatibleProviderConfig,
    /// Optional Ollama base URL override.
    pub base_url: Option<String>,
    /// Selected upstream Ollama model.
    pub selected_model: String,
}

impl OllamaProviderConfig {
    /// Creates a wrapper config with default base Claude settings.
    #[must_use]
    pub fn new(selected_model: impl Into<String>) -> Self {
        Self {
            base: ClaudeCompatibleProviderConfig::default(),
            base_url: None,
            selected_model: selected_model.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClaudeProviderProfile {
    ClaudeCode,
    Zai(ZaiProviderConfig),
    OpenRouter(OpenRouterProviderConfig),
    Vercel(VercelProviderConfig),
    Moonshot(MoonshotProviderConfig),
    Minimax(MinimaxProviderConfig),
    Bedrock(BedrockProviderConfig),
    Vertex(VertexProviderConfig),
    Ollama(OllamaProviderConfig),
}

impl ClaudeProviderProfile {
    #[must_use]
    pub const fn kind(&self) -> ClaudeCompatibleProviderKind {
        match self {
            Self::ClaudeCode => ClaudeCompatibleProviderKind::ClaudeCode,
            Self::Zai(_) => ClaudeCompatibleProviderKind::Zai,
            Self::OpenRouter(_) => ClaudeCompatibleProviderKind::OpenRouter,
            Self::Vercel(_) => ClaudeCompatibleProviderKind::Vercel,
            Self::Moonshot(_) => ClaudeCompatibleProviderKind::Moonshot,
            Self::Minimax(_) => ClaudeCompatibleProviderKind::Minimax,
            Self::Bedrock(_) => ClaudeCompatibleProviderKind::Bedrock,
            Self::Vertex(_) => ClaudeCompatibleProviderKind::Vertex,
            Self::Ollama(_) => ClaudeCompatibleProviderKind::Ollama,
        }
    }

    #[must_use]
    pub fn descriptor(&self) -> ProviderDescriptor {
        self.kind().descriptor()
    }

    #[must_use]
    pub fn runtime_model(&self, request: &ProviderRequest) -> String {
        match self {
            Self::ClaudeCode => request
                .model
                .provider_model_id
                .clone()
                .unwrap_or_else(|| request.model.model_id.clone()),
            _ => request.model.model_id.clone(),
        }
    }

    #[must_use]
    pub fn env_overrides(
        &self,
        request: &ProviderRequest,
        request_env: &BTreeMap<String, String>,
    ) -> BTreeMap<String, String> {
        match self {
            Self::ClaudeCode => BTreeMap::new(),
            Self::Zai(config) => zai_env(config, request),
            Self::OpenRouter(config) => openrouter_env(config, request),
            Self::Vercel(config) => vercel_env(config, request),
            Self::Moonshot(config) => moonshot_env(config, request),
            Self::Minimax(config) => minimax_env(config, request),
            Self::Bedrock(config) => bedrock_env(config, request, request_env),
            Self::Vertex(config) => vertex_env(config, request, request_env),
            Self::Ollama(config) => ollama_env(config, request),
        }
    }
}

fn zai_env(config: &ZaiProviderConfig, request: &ProviderRequest) -> BTreeMap<String, String> {
    let selected_model = selected_model(request, Some(config.selected_model.as_str()));
    anthropic_gateway_env(
        "https://api.z.ai/api/anthropic",
        Some(config.api_key.as_str()),
        Some(config.api_key.as_str()),
        selected_model.as_deref(),
    )
}

fn openrouter_env(
    config: &OpenRouterProviderConfig,
    request: &ProviderRequest,
) -> BTreeMap<String, String> {
    let selected_model = selected_model(request, Some(config.selected_model.as_str()));
    anthropic_gateway_env(
        "https://openrouter.ai/api",
        Some(config.api_key.as_str()),
        Some(""),
        selected_model.as_deref(),
    )
}

fn vercel_env(
    config: &VercelProviderConfig,
    request: &ProviderRequest,
) -> BTreeMap<String, String> {
    let selected_model = selected_model(request, Some(config.selected_model.as_str()));
    anthropic_gateway_env(
        "https://ai-gateway.vercel.sh",
        Some(config.api_key.as_str()),
        Some(""),
        selected_model.as_deref(),
    )
}

fn moonshot_env(
    config: &MoonshotProviderConfig,
    request: &ProviderRequest,
) -> BTreeMap<String, String> {
    let selected_model = selected_model(request, Some(config.selected_model.as_str()));
    let mut env = anthropic_gateway_env(
        "https://api.moonshot.ai/anthropic",
        Some(config.api_key.as_str()),
        Some(""),
        selected_model.as_deref(),
    );
    if let Some(selected_model) = selected_model {
        env.insert("ANTHROPIC_MODEL".to_owned(), selected_model);
    }
    env
}

fn minimax_env(
    config: &MinimaxProviderConfig,
    request: &ProviderRequest,
) -> BTreeMap<String, String> {
    anthropic_gateway_env(
        "https://api.minimax.io/anthropic",
        Some(config.api_key.as_str()),
        Some(""),
        selected_model(request, config.selected_model.as_deref()).as_deref(),
    )
}

fn bedrock_env(
    config: &BedrockProviderConfig,
    request: &ProviderRequest,
    request_env: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::from([
        ("CLAUDE_CODE_USE_BEDROCK".to_owned(), "1".to_owned()),
        ("ANTHROPIC_API_KEY".to_owned(), String::new()),
    ]);
    if let Some(region) = config
        .region
        .clone()
        .or_else(|| request_env.get("AWS_REGION").cloned())
        .or_else(|| std::env::var("AWS_REGION").ok())
    {
        env.insert("AWS_REGION".to_owned(), region);
    }
    if let Some(selected_model) = selected_model(request, config.selected_model.as_deref()) {
        insert_default_model_env(&mut env, &selected_model);
        env.insert("ANTHROPIC_MODEL".to_owned(), selected_model);
    }
    env
}

fn vertex_env(
    config: &VertexProviderConfig,
    request: &ProviderRequest,
    request_env: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::from([
        ("CLAUDE_CODE_USE_VERTEX".to_owned(), "1".to_owned()),
        ("ANTHROPIC_API_KEY".to_owned(), String::new()),
    ]);
    if let Some(project_id) = config
        .project_id
        .clone()
        .or_else(|| request_env.get("ANTHROPIC_VERTEX_PROJECT_ID").cloned())
        .or_else(|| std::env::var("ANTHROPIC_VERTEX_PROJECT_ID").ok())
    {
        env.insert("ANTHROPIC_VERTEX_PROJECT_ID".to_owned(), project_id);
    }
    if let Some(selected_model) = selected_model(request, config.selected_model.as_deref()) {
        insert_default_model_env(&mut env, &selected_model);
        env.insert("ANTHROPIC_MODEL".to_owned(), selected_model);
    }
    env
}

fn ollama_env(
    config: &OllamaProviderConfig,
    request: &ProviderRequest,
) -> BTreeMap<String, String> {
    anthropic_gateway_env(
        config
            .base_url
            .as_deref()
            .unwrap_or("http://localhost:11434"),
        Some("ollama"),
        Some(""),
        selected_model(request, Some(config.selected_model.as_str())).as_deref(),
    )
}

/// A first-class Bedrock provider backed by the Claude CLI harness.
#[derive(Clone)]
pub struct BedrockProvider {
    inner: ClaudeCodeProvider,
}

impl BedrockProvider {
    /// Creates a Bedrock provider with default wrapper config.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(BedrockProviderConfig::default())
    }

    /// Creates a Bedrock provider with an explicit wrapper config.
    #[must_use]
    pub fn with_config(config: BedrockProviderConfig) -> Self {
        let base = config.base.clone();
        Self {
            inner: ClaudeCodeProvider::with_profile_config(
                ClaudeProviderProfile::Bedrock(config),
                base,
            ),
        }
    }
}

impl Default for BedrockProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// A first-class Z.ai provider backed by the Claude CLI harness.
#[derive(Clone)]
pub struct ZaiProvider {
    inner: ClaudeCodeProvider,
}

impl ZaiProvider {
    /// Creates a Z.ai provider with default base Claude settings.
    #[must_use]
    pub fn new(api_key: impl Into<String>, selected_model: impl Into<String>) -> Self {
        Self::with_config(ZaiProviderConfig::new(api_key, selected_model))
    }

    /// Creates a Z.ai provider with an explicit wrapper config.
    #[must_use]
    pub fn with_config(config: ZaiProviderConfig) -> Self {
        let base = config.base.clone();
        Self {
            inner: ClaudeCodeProvider::with_profile_config(
                ClaudeProviderProfile::Zai(config),
                base,
            ),
        }
    }
}

/// A first-class `OpenRouter` provider backed by the Claude CLI harness.
#[derive(Clone)]
pub struct OpenRouterProvider {
    inner: ClaudeCodeProvider,
}

impl OpenRouterProvider {
    /// Creates an `OpenRouter` provider with default base Claude settings.
    #[must_use]
    pub fn new(api_key: impl Into<String>, selected_model: impl Into<String>) -> Self {
        Self::with_config(OpenRouterProviderConfig::new(api_key, selected_model))
    }

    /// Creates an `OpenRouter` provider with an explicit wrapper config.
    #[must_use]
    pub fn with_config(config: OpenRouterProviderConfig) -> Self {
        let base = config.base.clone();
        Self {
            inner: ClaudeCodeProvider::with_profile_config(
                ClaudeProviderProfile::OpenRouter(config),
                base,
            ),
        }
    }
}

/// A first-class Vercel provider backed by the Claude CLI harness.
#[derive(Clone)]
pub struct VercelProvider {
    inner: ClaudeCodeProvider,
}

impl VercelProvider {
    /// Creates a Vercel provider with default base Claude settings.
    #[must_use]
    pub fn new(api_key: impl Into<String>, selected_model: impl Into<String>) -> Self {
        Self::with_config(VercelProviderConfig::new(api_key, selected_model))
    }

    /// Creates a Vercel provider with an explicit wrapper config.
    #[must_use]
    pub fn with_config(config: VercelProviderConfig) -> Self {
        let base = config.base.clone();
        Self {
            inner: ClaudeCodeProvider::with_profile_config(
                ClaudeProviderProfile::Vercel(config),
                base,
            ),
        }
    }
}

/// A first-class Moonshot provider backed by the Claude CLI harness.
#[derive(Clone)]
pub struct MoonshotProvider {
    inner: ClaudeCodeProvider,
}

impl MoonshotProvider {
    /// Creates a Moonshot provider with default base Claude settings.
    #[must_use]
    pub fn new(api_key: impl Into<String>, selected_model: impl Into<String>) -> Self {
        Self::with_config(MoonshotProviderConfig::new(api_key, selected_model))
    }

    /// Creates a Moonshot provider with an explicit wrapper config.
    #[must_use]
    pub fn with_config(config: MoonshotProviderConfig) -> Self {
        let base = config.base.clone();
        Self {
            inner: ClaudeCodeProvider::with_profile_config(
                ClaudeProviderProfile::Moonshot(config),
                base,
            ),
        }
    }
}

/// A first-class `MiniMax` provider backed by the Claude CLI harness.
#[derive(Clone)]
pub struct MinimaxProvider {
    inner: ClaudeCodeProvider,
}

impl MinimaxProvider {
    /// Creates a `MiniMax` provider with default base Claude settings.
    #[must_use]
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_config(MinimaxProviderConfig::new(api_key))
    }

    /// Creates a `MiniMax` provider with an explicit wrapper config.
    #[must_use]
    pub fn with_config(config: MinimaxProviderConfig) -> Self {
        let base = config.base.clone();
        Self {
            inner: ClaudeCodeProvider::with_profile_config(
                ClaudeProviderProfile::Minimax(config),
                base,
            ),
        }
    }
}

/// A first-class Vertex provider backed by the Claude CLI harness.
#[derive(Clone)]
pub struct VertexProvider {
    inner: ClaudeCodeProvider,
}

impl VertexProvider {
    /// Creates a Vertex provider with default wrapper config.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(VertexProviderConfig::default())
    }

    /// Creates a Vertex provider with an explicit wrapper config.
    #[must_use]
    pub fn with_config(config: VertexProviderConfig) -> Self {
        let base = config.base.clone();
        Self {
            inner: ClaudeCodeProvider::with_profile_config(
                ClaudeProviderProfile::Vertex(config),
                base,
            ),
        }
    }
}

impl Default for VertexProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// A first-class Ollama provider backed by the Claude CLI harness.
#[derive(Clone)]
pub struct OllamaProvider {
    inner: ClaudeCodeProvider,
}

impl OllamaProvider {
    /// Creates an Ollama provider with default base Claude settings.
    #[must_use]
    pub fn new(selected_model: impl Into<String>) -> Self {
        Self::with_config(OllamaProviderConfig::new(selected_model))
    }

    /// Creates an Ollama provider with an explicit wrapper config.
    #[must_use]
    pub fn with_config(config: OllamaProviderConfig) -> Self {
        let base = config.base.clone();
        Self {
            inner: ClaudeCodeProvider::with_profile_config(
                ClaudeProviderProfile::Ollama(config),
                base,
            ),
        }
    }
}

macro_rules! impl_provider_wrapper {
    ($wrapper:ident) => {
        #[async_trait::async_trait]
        impl Provider for $wrapper {
            fn descriptor(&self) -> &ProviderDescriptor {
                self.inner.descriptor()
            }

            async fn stream(
                &self,
                request: ProviderRequest,
            ) -> Result<ProviderEventStream, ProviderError> {
                self.inner.stream(request).await
            }

            async fn generate(
                &self,
                request: ProviderRequest,
            ) -> Result<GenerateResponse, ProviderError> {
                self.inner.generate(request).await
            }
        }
    };
}

impl_provider_wrapper!(BedrockProvider);
impl_provider_wrapper!(ZaiProvider);
impl_provider_wrapper!(OpenRouterProvider);
impl_provider_wrapper!(VercelProvider);
impl_provider_wrapper!(MoonshotProvider);
impl_provider_wrapper!(MinimaxProvider);
impl_provider_wrapper!(VertexProvider);
impl_provider_wrapper!(OllamaProvider);

fn anthropic_gateway_env(
    base_url: &str,
    auth_token: Option<&str>,
    api_key: Option<&str>,
    selected_model: Option<&str>,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::from([("ANTHROPIC_BASE_URL".to_owned(), base_url.to_owned())]);
    if let Some(auth_token) = auth_token {
        env.insert("ANTHROPIC_AUTH_TOKEN".to_owned(), auth_token.to_owned());
    }
    if let Some(api_key) = api_key {
        env.insert("ANTHROPIC_API_KEY".to_owned(), api_key.to_owned());
    }
    if let Some(selected_model) = selected_model {
        insert_default_model_env(&mut env, selected_model);
    }
    env
}

fn insert_default_model_env(env: &mut BTreeMap<String, String>, selected_model: &str) {
    env.insert(
        "ANTHROPIC_DEFAULT_OPUS_MODEL".to_owned(),
        selected_model.to_owned(),
    );
    env.insert(
        "ANTHROPIC_DEFAULT_SONNET_MODEL".to_owned(),
        selected_model.to_owned(),
    );
    env.insert(
        "ANTHROPIC_DEFAULT_HAIKU_MODEL".to_owned(),
        selected_model.to_owned(),
    );
}

fn selected_model(request: &ProviderRequest, fallback: Option<&str>) -> Option<String> {
    if let Some(provider_model_id) = request
        .model
        .provider_model_id
        .as_deref()
        .map(str::trim)
        .filter(|model_id| !model_id.is_empty())
    {
        return Some(provider_model_id.to_owned());
    }

    let request_model = request.model.model_id.trim();
    let fallback_model = fallback
        .map(str::trim)
        .filter(|model_id| !model_id.is_empty());

    if fallback_model.is_some() && KNOWN_CLAUDE_MODEL_IDS.contains(&request_model) {
        return fallback_model.map(ToOwned::to_owned);
    }

    if request_model.is_empty() {
        return fallback_model.map(ToOwned::to_owned);
    }

    Some(request_model.to_owned())
}

const fn claude_compatible_capabilities() -> ProviderCapabilities {
    ProviderCapabilities::new()
        .with_streaming(true)
        .with_generate(true)
        .with_tool_calls(true)
        .with_mcp_passthrough(true)
        .with_session_resume(true)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{CLAUDE_COMPATIBLE_PROVIDER_IDS, ClaudeCompatibleProviderKind, selected_model};
    use arky_protocol::{Message, ModelRef, ProviderRequest, SessionRef, TurnContext, TurnId};

    fn request_with_model(model: ModelRef) -> ProviderRequest {
        ProviderRequest::new(
            SessionRef::new(None),
            TurnContext::new(TurnId::new(), 1),
            model,
            vec![Message::user("hello")],
        )
    }

    #[test]
    fn supported_provider_kinds_should_round_trip() {
        let actual = CLAUDE_COMPATIBLE_PROVIDER_IDS
            .iter()
            .map(|kind| {
                ClaudeCompatibleProviderKind::from_kind(kind)
                    .expect("kind should parse")
                    .as_str()
            })
            .collect::<Vec<_>>();

        assert_eq!(actual, CLAUDE_COMPATIBLE_PROVIDER_IDS);
    }

    #[test]
    fn claude_alias_should_map_to_canonical_claude_code_kind() {
        let kind =
            ClaudeCompatibleProviderKind::from_kind("claude").expect("claude alias should parse");

        assert_eq!(kind, ClaudeCompatibleProviderKind::ClaudeCode);
        assert_eq!(kind.as_str(), "claude-code");
    }

    #[test]
    fn selected_model_should_prefer_provider_model_id() {
        let request =
            request_with_model(ModelRef::new("sonnet").with_provider_model_id("bedrock/haiku"));

        assert_eq!(
            selected_model(&request, Some("bedrock/sonnet")).as_deref(),
            Some("bedrock/haiku")
        );
    }

    #[test]
    fn selected_model_should_use_request_model_when_it_is_provider_specific() {
        let request = request_with_model(ModelRef::new("qwen2.5"));

        assert_eq!(
            selected_model(&request, Some("llama3")).as_deref(),
            Some("qwen2.5")
        );
    }

    #[test]
    fn selected_model_should_keep_wrapper_fallback_for_generic_claude_aliases() {
        let request = request_with_model(ModelRef::new("sonnet"));

        assert_eq!(
            selected_model(&request, Some("moonshot-v1")).as_deref(),
            Some("moonshot-v1")
        );
    }

    #[test]
    fn selected_model_should_use_request_model_when_no_wrapper_fallback_exists() {
        let request = request_with_model(ModelRef::new("sonnet"));

        assert_eq!(selected_model(&request, None).as_deref(), Some("sonnet"));
    }
}
