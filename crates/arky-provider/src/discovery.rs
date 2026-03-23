//! Provider-backed model discovery contracts.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use arky_protocol::ProviderId;

use crate::ProviderError;

/// Normalized pricing metadata for one model.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ModelCost {
    /// Prompt-side price per million tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_per_million: Option<f64>,
    /// Completion-side price per million tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_per_million: Option<f64>,
}

/// User-facing model metadata exposed by discovery services.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Stable model identifier.
    pub id: String,
    /// Provider that owns the model.
    pub provider_id: ProviderId,
    /// Optional display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Optional context window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    /// Optional max output budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    /// Whether tool use is supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_tools: Option<bool>,
    /// Whether reasoning is supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_reasoning: Option<bool>,
    /// Optional description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional pricing metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<ModelCost>,
}

impl ModelInfo {
    /// Creates a minimal model descriptor.
    #[must_use]
    pub fn new(id: impl Into<String>, provider_id: ProviderId) -> Self {
        Self {
            id: id.into(),
            provider_id,
            display_name: None,
            context_window: None,
            max_output_tokens: None,
            supports_tools: None,
            supports_reasoning: None,
            description: None,
            cost: None,
        }
    }
}

/// Shared discovery surface implemented by providers or adapters.
#[async_trait]
pub trait ModelDiscoveryService: Send + Sync {
    /// Lists models currently available from the discovery source.
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use pretty_assertions::assert_eq;

    use super::{ModelDiscoveryService, ModelInfo};
    use crate::ProviderError;
    use arky_protocol::ProviderId;

    struct StaticDiscoveryService;

    #[async_trait]
    impl ModelDiscoveryService for StaticDiscoveryService {
        async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![ModelInfo::new(
                "claude-sonnet-4",
                ProviderId::new("claude-code"),
            )])
        }
    }

    #[tokio::test]
    async fn model_discovery_service_should_return_models() {
        let service = StaticDiscoveryService;
        let models = service
            .list_models()
            .await
            .expect("model discovery should succeed");

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "claude-sonnet-4");
        assert_eq!(models[0].provider_id, ProviderId::new("claude-code"));
    }
}
