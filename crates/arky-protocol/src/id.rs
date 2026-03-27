//! Strongly typed identifiers shared across the SDK.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A stable SDK session identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(Uuid);

impl SessionId {
    /// Generates a fresh session identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Wraps an existing UUID.
    #[must_use]
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Parses a session identifier from its canonical string form.
    pub fn parse_str(input: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(input).map(Self)
    }

    /// Returns the wrapped UUID.
    #[must_use]
    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::str::FromStr for SessionId {
    type Err = uuid::Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse_str(input)
    }
}

impl From<Uuid> for SessionId {
    fn from(value: Uuid) -> Self {
        Self::from_uuid(value)
    }
}

impl From<SessionId> for Uuid {
    fn from(value: SessionId) -> Self {
        value.0
    }
}

/// A provider identifier used for routing and observability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderId(String);

impl ProviderId {
    /// Creates a provider identifier.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the provider identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the identifier and returns the owned string.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl From<&str> for ProviderId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ProviderId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for ProviderId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// A stable identifier for a single turn within a session.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TurnId(Uuid);

impl TurnId {
    /// Generates a fresh turn identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Wraps an existing UUID.
    #[must_use]
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Parses a turn identifier from its canonical string form.
    pub fn parse_str(input: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(input).map(Self)
    }

    /// Returns the wrapped UUID.
    #[must_use]
    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for TurnId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TurnId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl From<Uuid> for TurnId {
    fn from(value: Uuid) -> Self {
        Self::from_uuid(value)
    }
}

impl From<TurnId> for Uuid {
    fn from(value: TurnId) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{ProviderId, SessionId, TurnId};

    #[test]
    fn session_id_should_support_display_and_serde_round_trip() {
        let session_id = SessionId::new();
        let encoded = serde_json::to_string(&session_id).expect("session id should serialize");
        let decoded: SessionId =
            serde_json::from_str(&encoded).expect("session id should deserialize");

        assert_eq!(
            (session_id.to_string(), decoded),
            (session_id.to_string(), session_id)
        );
    }

    #[test]
    fn session_id_should_round_trip_uuid_and_parse_from_display() {
        let uuid = uuid::Uuid::new_v4();
        let session_id = SessionId::from_uuid(uuid);
        let parsed: SessionId = session_id
            .to_string()
            .parse()
            .expect("session id display should parse");

        assert_eq!(session_id.as_uuid(), &uuid);
        assert_eq!(parsed, session_id);
    }

    #[test]
    fn provider_id_should_support_display_and_serde_round_trip() {
        let provider_id = ProviderId::new("codex");
        let encoded = serde_json::to_string(&provider_id).expect("provider id should serialize");
        let decoded: ProviderId =
            serde_json::from_str(&encoded).expect("provider id should deserialize");

        assert_eq!(
            (provider_id.to_string(), decoded),
            ("codex".to_owned(), provider_id)
        );
    }

    #[test]
    fn turn_id_should_support_display_and_serde_round_trip() {
        let turn_id = TurnId::new();
        let encoded = serde_json::to_string(&turn_id).expect("turn id should serialize");
        let decoded: TurnId = serde_json::from_str(&encoded).expect("turn id should deserialize");

        assert_eq!(
            (turn_id.to_string(), decoded),
            (turn_id.to_string(), turn_id)
        );
    }
}
