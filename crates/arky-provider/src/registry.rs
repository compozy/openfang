//! Thread-safe provider registry.

use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

use arky_protocol::ProviderId;
use tracing::warn;

use crate::{ModelRef, Provider, ProviderDescriptor, ProviderError};

type ProviderMap = BTreeMap<ProviderId, Arc<dyn Provider>>;

/// Thread-safe registry for provider implementations.
#[derive(Clone, Default)]
pub struct ProviderRegistry {
    providers: Arc<RwLock<ProviderMap>>,
}

impl ProviderRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a provider instance keyed by its descriptor identifier.
    pub fn register<P>(&self, provider: P) -> Result<(), ProviderError>
    where
        P: Provider + 'static,
    {
        self.register_arc(Arc::new(provider))
    }

    /// Registers a provider instance from an existing shared pointer.
    pub fn register_arc(&self, provider: Arc<dyn Provider>) -> Result<(), ProviderError> {
        let descriptor = provider.descriptor().clone();
        let mut providers = write_providers(&self.providers);

        if providers.contains_key(&descriptor.id) {
            return Err(ProviderError::protocol_violation(
                format!("provider `{}` is already registered", descriptor.id),
                None,
            ));
        }

        providers.insert(descriptor.id, provider);
        drop(providers);
        Ok(())
    }

    /// Returns a provider by identifier.
    pub fn get(&self, id: &ProviderId) -> Result<Arc<dyn Provider>, ProviderError> {
        read_providers(&self.providers)
            .get(id)
            .cloned()
            .ok_or_else(|| ProviderError::not_found(id.clone()))
    }

    /// Returns a provider by identifier when present.
    #[must_use]
    pub fn maybe_get(&self, id: &ProviderId) -> Option<Arc<dyn Provider>> {
        read_providers(&self.providers).get(id).cloned()
    }

    /// Infers a provider identifier from a model prefix.
    #[must_use]
    pub fn infer_provider_id(&self, model: &str) -> Option<ProviderId> {
        infer_provider_id(model)
    }

    /// Resolves a provider using an explicit identifier or model prefix fallback.
    pub fn resolve(
        &self,
        provider_id: Option<&ProviderId>,
        model: Option<&str>,
    ) -> Result<Arc<dyn Provider>, ProviderError> {
        if let Some(provider_id) = provider_id {
            return self.get(provider_id);
        }

        if let Some(model) = model
            && let Some(provider_id) = infer_provider_id(model)
            && let Some(provider) = self.maybe_get(&provider_id)
        {
            return Ok(provider);
        }

        let provider_ids = {
            let providers = read_providers(&self.providers);
            if providers.is_empty() {
                return Err(ProviderError::protocol_violation(
                    "cannot resolve a provider because the registry is empty",
                    None,
                ));
            }

            if providers.len() == 1 {
                return Ok(providers
                    .values()
                    .next()
                    .cloned()
                    .expect("checked non-empty"));
            }

            providers
                .keys()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        };
        Err(ProviderError::protocol_violation(
            format!(
                "cannot resolve a provider{} across registered providers: {provider_ids}",
                model
                    .map(|model| format!(" for model `{model}`"))
                    .unwrap_or_default()
            ),
            None,
        ))
    }

    /// Resolves a provider using a model reference.
    pub fn resolve_model(&self, model: &ModelRef) -> Result<Arc<dyn Provider>, ProviderError> {
        self.resolve(model.provider_id.as_ref(), Some(model.model_id.as_str()))
    }

    /// Lists descriptors for all registered providers in identifier order.
    #[must_use]
    pub fn list(&self) -> Vec<ProviderDescriptor> {
        read_providers(&self.providers)
            .values()
            .map(|provider| provider.descriptor().clone())
            .collect()
    }

    /// Removes a registered provider.
    pub fn remove(&self, id: &ProviderId) -> Option<Arc<dyn Provider>> {
        write_providers(&self.providers).remove(id)
    }

    /// Clears the registry.
    pub fn clear(&self) {
        write_providers(&self.providers).clear();
    }
}

const DEFAULT_MODEL_PREFIX_MAP: [(&str, &str); 5] = [
    ("claude-", "claude-code"),
    ("gpt-", "codex"),
    ("o1-", "codex"),
    ("o3-", "codex"),
    ("codex-", "codex"),
];

/// Infers a provider identifier from a model prefix using the default map.
#[must_use]
pub fn infer_provider_id(model: &str) -> Option<ProviderId> {
    let normalized = model.trim().to_lowercase();
    if normalized.is_empty() {
        return None;
    }

    let mut selected: Option<(&str, &str)> = None;
    for (prefix, provider_id) in DEFAULT_MODEL_PREFIX_MAP {
        if normalized.starts_with(prefix)
            && selected.is_none_or(|(selected_prefix, _)| prefix.len() > selected_prefix.len())
        {
            selected = Some((prefix, provider_id));
        }
    }

    selected.map(|(_, provider_id)| ProviderId::new(provider_id))
}

fn read_providers(lock: &RwLock<ProviderMap>) -> std::sync::RwLockReadGuard<'_, ProviderMap> {
    match lock.read() {
        Ok(guard) => guard,
        Err(poisoned) => {
            warn!("provider registry read lock was poisoned; recovering inner state");
            poisoned.into_inner()
        }
    }
}

fn write_providers(lock: &RwLock<ProviderMap>) -> std::sync::RwLockWriteGuard<'_, ProviderMap> {
    match lock.write() {
        Ok(guard) => guard,
        Err(poisoned) => {
            warn!("provider registry write lock was poisoned; recovering inner state");
            poisoned.into_inner()
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::stream;
    use pretty_assertions::assert_eq;

    use super::ProviderRegistry;
    use crate::{
        Provider, ProviderCapabilities, ProviderDescriptor, ProviderError, ProviderEventStream,
        ProviderFamily, ProviderRequest, infer_provider_id,
    };
    use arky_protocol::ProviderId;

    struct StaticProvider {
        descriptor: ProviderDescriptor,
    }

    #[async_trait::async_trait]
    impl Provider for StaticProvider {
        fn descriptor(&self) -> &ProviderDescriptor {
            &self.descriptor
        }

        async fn stream(
            &self,
            _request: ProviderRequest,
        ) -> Result<ProviderEventStream, ProviderError> {
            Ok(Box::pin(stream::empty()))
        }
    }

    fn provider(id: &str) -> StaticProvider {
        StaticProvider {
            descriptor: ProviderDescriptor::new(
                ProviderId::new(id),
                ProviderFamily::Custom(id.to_owned()),
                ProviderCapabilities::new().with_streaming(true),
            ),
        }
    }

    #[test]
    fn provider_registry_should_register_lookup_and_list_providers() {
        let registry = ProviderRegistry::new();
        registry
            .register(provider("codex"))
            .expect("provider should register");
        registry
            .register(provider("claude-code"))
            .expect("provider should register");

        let listed = registry.list();

        assert_eq!(listed.len(), 2);
        assert_eq!(
            registry
                .get(&ProviderId::new("codex"))
                .expect("provider should resolve")
                .descriptor()
                .id
                .as_str(),
            "codex"
        );
    }

    #[test]
    fn provider_registry_should_return_not_found_for_missing_provider() {
        let registry = ProviderRegistry::new();
        let Err(error) = registry.get(&ProviderId::new("missing")) else {
            panic!("missing provider should fail");
        };

        assert!(matches!(error, ProviderError::NotFound { .. }));
    }

    #[test]
    fn provider_registry_should_reject_duplicate_registration() {
        let registry = ProviderRegistry::new();
        registry
            .register(provider("codex"))
            .expect("provider should register");
        let error = registry
            .register(provider("codex"))
            .expect_err("duplicate should fail");

        assert!(matches!(error, ProviderError::ProtocolViolation { .. }));
    }

    #[test]
    fn infer_provider_id_should_map_known_model_prefixes() {
        let inferred = infer_provider_id("claude-3.5-sonnet");

        assert_eq!(inferred, Some(ProviderId::new("claude-code")));
    }

    #[test]
    fn provider_registry_should_fallback_to_single_provider_resolution() {
        let registry = ProviderRegistry::new();
        registry
            .register(provider("codex"))
            .expect("provider should register");

        let resolved = registry
            .resolve(None, Some("unknown-model"))
            .expect("single provider should resolve");

        assert_eq!(resolved.descriptor().id, ProviderId::new("codex"));
    }
}
