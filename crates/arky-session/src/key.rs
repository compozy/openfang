//! Compound session-key helpers for provider/model-scoped lookups.

use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

/// Stable compound key for provider/model/session scoped lookup.
#[derive(Debug, Clone, Eq, Serialize, Deserialize)]
pub struct SessionKey {
    /// Provider identifier owning the session.
    pub provider_id: String,
    /// Model identifier associated with the session.
    pub model_id: String,
    /// Provider-native or caller-defined session identifier.
    pub session_id: String,
}

impl SessionKey {
    /// Creates a new compound session key.
    #[must_use]
    pub fn new(
        provider_id: impl Into<String>,
        model_id: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            session_id: session_id.into(),
        }
    }
}

impl PartialEq for SessionKey {
    fn eq(&self, other: &Self) -> bool {
        self.provider_id == other.provider_id
            && self.model_id == other.model_id
            && self.session_id == other.session_id
    }
}

impl Hash for SessionKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.provider_id.hash(state);
        self.model_id.hash(state);
        self.session_id.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use pretty_assertions::assert_eq;

    use super::SessionKey;

    #[test]
    fn session_key_should_support_equality_and_hashing() {
        let left = SessionKey::new("codex", "gpt-4o", "thread-1");
        let same = SessionKey::new("codex", "gpt-4o", "thread-1");
        let other = SessionKey::new("codex", "gpt-4o", "thread-2");

        let mut keys = HashSet::new();
        keys.insert(left.clone());
        keys.insert(same.clone());
        keys.insert(other.clone());

        assert_eq!(left, same);
        assert_eq!(keys.len(), 2);
        assert_eq!(keys.contains(&other), true);
    }
}
