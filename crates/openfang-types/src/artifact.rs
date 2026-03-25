//! Shared artifact domain types and canonical content hashing.
//!
//! The canonical hash form sorts all JSON object keys recursively and renders
//! compact JSON without insignificant whitespace before hashing the UTF-8 bytes
//! with SHA-256. Arrays preserve element order.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sha2::{Digest as _, Sha256};

use crate::task::TaskId;

macro_rules! string_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// Create a new typed value from any string-like input.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.0.as_str())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.0.as_str()
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }
    };
}

string_newtype!(ArtifactId);
string_newtype!(ArtifactVersionId);
string_newtype!(ArtifactType);
string_newtype!(ContentHash);

/// Stable provenance categories for artifact/doc version writers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceKind {
    /// Workflow run level provenance.
    Run,
    /// Agent dispatch provenance.
    Dispatch,
    /// Looper run provenance.
    LooperRun,
    /// API-triggered provenance.
    Api,
    /// Direct agent provenance.
    Agent,
}

impl ProvenanceKind {
    /// Return the stable SQLite / JSON encoding.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Dispatch => "dispatch",
            Self::LooperRun => "looper_run",
            Self::Api => "api",
            Self::Agent => "agent",
        }
    }

    fn parse_db_text(value: &str) -> Option<Self> {
        match value {
            "run" => Some(Self::Run),
            "dispatch" => Some(Self::Dispatch),
            "looper_run" => Some(Self::LooperRun),
            "api" => Some(Self::Api),
            "agent" => Some(Self::Agent),
            _ => None,
        }
    }
}

impl std::fmt::Display for ProvenanceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ProvenanceKind {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_db_text(value).ok_or("invalid provenance kind")
    }
}

/// Optional runtime provenance attached to immutable content versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvenanceRef {
    /// Runtime object category.
    pub kind: ProvenanceKind,
    /// Stable referenced identifier.
    #[serde(rename = "ref")]
    pub ref_id: String,
}

/// Stable artifact identity row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactRecord {
    /// Stable artifact identifier.
    pub artifact_id: ArtifactId,
    /// Artifact classification.
    #[serde(rename = "type")]
    pub type_name: ArtifactType,
    /// Current immutable head version identifier.
    pub current_version_id: ArtifactVersionId,
    /// Arbitrary artifact metadata.
    pub metadata: JsonValue,
    /// Creation timestamp in RFC 3339 UTC format.
    pub created_at: String,
    /// Last head-update timestamp in RFC 3339 UTC format.
    pub updated_at: String,
}

/// Immutable artifact version row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactVersionRecord {
    /// Stable artifact version identifier.
    pub artifact_version_id: ArtifactVersionId,
    /// Owning artifact identifier.
    pub artifact_id: ArtifactId,
    /// Monotonic version number within one artifact.
    pub version_no: i64,
    /// Immutable content payload.
    pub content: JsonValue,
    /// Canonical SHA-256 hex digest of `content`.
    pub content_hash: ContentHash,
    /// Optional producing runtime provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<ProvenanceRef>,
    /// Version creation timestamp in RFC 3339 UTC format.
    pub created_at: String,
}

/// Input payload for creating a new artifact with its first immutable version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewArtifact {
    /// Stable artifact identifier.
    pub artifact_id: ArtifactId,
    /// First version identifier.
    pub artifact_version_id: ArtifactVersionId,
    /// Artifact classification.
    #[serde(rename = "type")]
    pub type_name: ArtifactType,
    /// Artifact metadata payload.
    pub metadata: JsonValue,
    /// First immutable content payload.
    pub content: JsonValue,
    /// Optional producing runtime provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<ProvenanceRef>,
    /// Creation timestamp in RFC 3339 UTC format.
    pub created_at: String,
}

/// Input payload for appending a new immutable artifact version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewArtifactVersion {
    /// Stable new version identifier.
    pub artifact_version_id: ArtifactVersionId,
    /// Immutable content payload.
    pub content: JsonValue,
    /// Optional producing runtime provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<ProvenanceRef>,
    /// Creation timestamp in RFC 3339 UTC format.
    pub created_at: String,
}

/// Cursor-backed artifact list query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactListQuery {
    /// Maximum number of items to return.
    pub limit: usize,
    /// Opaque pagination cursor returned by the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Optional artifact type filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_type: Option<ArtifactType>,
    /// Optional linked task filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    /// Optional fuzzy search needle.
    #[serde(default, rename = "q", skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
}

impl Default for ArtifactListQuery {
    fn default() -> Self {
        Self {
            limit: 50,
            cursor: None,
            artifact_type: None,
            task_id: None,
            search: None,
        }
    }
}

/// Summary payload returned by standalone artifact list endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArtifactSummary {
    /// Stable artifact identifier.
    pub id: ArtifactId,
    /// Artifact classification.
    pub artifact_type: ArtifactType,
    /// Linked durable task identifier when one is known.
    pub task_id: Option<TaskId>,
    /// Current immutable head version identifier.
    pub current_version_id: ArtifactVersionId,
    /// Creation timestamp in RFC 3339 UTC format.
    pub created_at: String,
    /// Last head-update timestamp in RFC 3339 UTC format.
    pub updated_at: String,
}

/// Summary payload returned by artifact version history endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArtifactVersionSummary {
    /// Stable artifact version identifier.
    pub id: ArtifactVersionId,
    /// Monotonic version number within one artifact.
    pub version_number: i64,
    /// Canonical SHA-256 hex digest of the version content.
    pub content_hash: ContentHash,
    /// Optional producing runtime provenance.
    pub created_by: Option<ProvenanceRef>,
    /// Version creation timestamp in RFC 3339 UTC format.
    pub created_at: String,
}

/// Detail payload returned by standalone artifact detail endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArtifactDetail {
    /// Stable artifact identifier.
    pub id: ArtifactId,
    /// Artifact classification.
    pub artifact_type: ArtifactType,
    /// Linked durable task identifier when one is known.
    pub task_id: Option<TaskId>,
    /// Current immutable head version identifier.
    pub current_version_id: ArtifactVersionId,
    /// Current version summary projected inline.
    pub current_version: ArtifactVersionSummary,
    /// Creation timestamp in RFC 3339 UTC format.
    pub created_at: String,
    /// Last head-update timestamp in RFC 3339 UTC format.
    pub updated_at: String,
}

/// Cursor-backed artifact list response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactListPage {
    /// Page of artifact summaries.
    pub items: Vec<ArtifactSummary>,
    /// Cursor for the next page, or `None` when exhausted.
    pub next_cursor: Option<String>,
}

/// Cursor-backed artifact version list response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactVersionListPage {
    /// Page of artifact version summaries.
    pub items: Vec<ArtifactVersionSummary>,
    /// Cursor for the next page, or `None` when exhausted.
    pub next_cursor: Option<String>,
}

/// Canonicalize JSON content for hashing by sorting all nested object keys and
/// rendering compact JSON without insignificant whitespace.
pub fn canonical_content_json(content: &JsonValue) -> String {
    let mut canonical = content.clone();
    canonical.sort_all_objects();
    canonical.to_string()
}

/// Compute the lowercase SHA-256 hex digest of canonical JSON content.
pub fn content_hash(content: &JsonValue) -> ContentHash {
    let canonical = canonical_content_json(content);
    ContentHash::new(hex::encode(Sha256::digest(canonical.as_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn content_hash_should_ignore_object_field_ordering() {
        let first = serde_json::json!({
            "title": "PRD",
            "metadata": {
                "owner": "planner",
                "labels": ["spec", "draft"],
            },
        });
        let second = serde_json::json!({
            "metadata": {
                "labels": ["spec", "draft"],
                "owner": "planner",
            },
            "title": "PRD",
        });

        assert_eq!(content_hash(&first), content_hash(&second));
        assert_eq!(
            canonical_content_json(&first),
            canonical_content_json(&second)
        );
    }

    #[test]
    fn artifact_summary_should_serialize_expected_shape() {
        let summary = ArtifactSummary {
            id: ArtifactId::new("artifact_001"),
            artifact_type: ArtifactType::new("prd"),
            task_id: Some(TaskId::new("task_001")),
            current_version_id: ArtifactVersionId::new("artifact_v3"),
            created_at: "2026-03-21T14:00:00Z".to_string(),
            updated_at: "2026-03-21T14:30:00Z".to_string(),
        };

        let json = serde_json::to_value(summary).expect("artifact summary should serialize");

        assert_eq!(
            json,
            serde_json::json!({
                "id": "artifact_001",
                "artifact_type": "prd",
                "task_id": "task_001",
                "current_version_id": "artifact_v3",
                "created_at": "2026-03-21T14:00:00Z",
                "updated_at": "2026-03-21T14:30:00Z",
            })
        );
    }

    #[test]
    fn artifact_detail_should_serialize_current_version_inline() {
        let detail = ArtifactDetail {
            id: ArtifactId::new("artifact_001"),
            artifact_type: ArtifactType::new("prd"),
            task_id: Some(TaskId::new("task_001")),
            current_version_id: ArtifactVersionId::new("artifact_v3"),
            current_version: ArtifactVersionSummary {
                id: ArtifactVersionId::new("artifact_v3"),
                version_number: 3,
                content_hash: ContentHash::new("sha256:abc123"),
                created_by: Some(ProvenanceRef {
                    kind: ProvenanceKind::Agent,
                    ref_id: "prd-writer".to_string(),
                }),
                created_at: "2026-03-21T14:30:00Z".to_string(),
            },
            created_at: "2026-03-21T14:00:00Z".to_string(),
            updated_at: "2026-03-21T14:30:00Z".to_string(),
        };

        let json = serde_json::to_value(detail).expect("artifact detail should serialize");

        assert_eq!(
            json["current_version"]["version_number"],
            serde_json::json!(3)
        );
        assert_eq!(
            json["current_version"]["content_hash"],
            serde_json::json!("sha256:abc123")
        );
    }
}
