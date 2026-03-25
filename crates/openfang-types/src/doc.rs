//! Shared document domain types.

use crate::artifact::{ContentHash, ProvenanceRef};
use crate::task::TaskId;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

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

string_newtype!(DocId);
string_newtype!(DocVersionId);
string_newtype!(DocType);

/// Stable document identity row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocRecord {
    /// Stable document identifier.
    pub doc_id: DocId,
    /// Document classification.
    #[serde(rename = "type")]
    pub type_name: DocType,
    /// Current immutable head version identifier.
    pub current_version_id: DocVersionId,
    /// Arbitrary document metadata.
    pub metadata: JsonValue,
    /// Creation timestamp in RFC 3339 UTC format.
    pub created_at: String,
    /// Last head-update timestamp in RFC 3339 UTC format.
    pub updated_at: String,
}

/// Immutable document version row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocVersionRecord {
    /// Stable document version identifier.
    pub doc_version_id: DocVersionId,
    /// Owning document identifier.
    pub doc_id: DocId,
    /// Monotonic version number within one document.
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

/// Input payload for creating a new document with its first immutable version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewDoc {
    /// Stable document identifier.
    pub doc_id: DocId,
    /// First version identifier.
    pub doc_version_id: DocVersionId,
    /// Document classification.
    #[serde(rename = "type")]
    pub type_name: DocType,
    /// Document metadata payload.
    pub metadata: JsonValue,
    /// First immutable content payload.
    pub content: JsonValue,
    /// Optional producing runtime provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<ProvenanceRef>,
    /// Creation timestamp in RFC 3339 UTC format.
    pub created_at: String,
}

/// Input payload for appending a new immutable document version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewDocVersion {
    /// Stable new version identifier.
    pub doc_version_id: DocVersionId,
    /// Immutable content payload.
    pub content: JsonValue,
    /// Optional producing runtime provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<ProvenanceRef>,
    /// Creation timestamp in RFC 3339 UTC format.
    pub created_at: String,
}

/// Cursor-backed document list query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocListQuery {
    /// Maximum number of items to return.
    pub limit: usize,
    /// Opaque pagination cursor returned by the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Optional document type filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_type: Option<DocType>,
    /// Optional linked task filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    /// Optional fuzzy search needle.
    #[serde(default, rename = "q", skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
}

impl Default for DocListQuery {
    fn default() -> Self {
        Self {
            limit: 50,
            cursor: None,
            doc_type: None,
            task_id: None,
            search: None,
        }
    }
}

/// Summary payload returned by standalone doc list endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DocSummary {
    /// Stable doc identifier.
    pub id: DocId,
    /// Document classification.
    pub doc_type: DocType,
    /// Linked durable task identifier when one is known.
    pub task_id: Option<TaskId>,
    /// Current immutable head version identifier.
    pub current_version_id: DocVersionId,
    /// Creation timestamp in RFC 3339 UTC format.
    pub created_at: String,
    /// Last head-update timestamp in RFC 3339 UTC format.
    pub updated_at: String,
}

/// Summary payload returned by doc version history endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DocVersionSummary {
    /// Stable doc version identifier.
    pub id: DocVersionId,
    /// Monotonic version number within one doc.
    pub version_number: i64,
    /// Canonical SHA-256 hex digest of the version content.
    pub content_hash: ContentHash,
    /// Optional producing runtime provenance.
    pub created_by: Option<ProvenanceRef>,
    /// Version creation timestamp in RFC 3339 UTC format.
    pub created_at: String,
}

/// Detail payload returned by standalone doc detail endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DocDetail {
    /// Stable doc identifier.
    pub id: DocId,
    /// Document classification.
    pub doc_type: DocType,
    /// Linked durable task identifier when one is known.
    pub task_id: Option<TaskId>,
    /// Current immutable head version identifier.
    pub current_version_id: DocVersionId,
    /// Current version summary projected inline.
    pub current_version: DocVersionSummary,
    /// Creation timestamp in RFC 3339 UTC format.
    pub created_at: String,
    /// Last head-update timestamp in RFC 3339 UTC format.
    pub updated_at: String,
}

/// Cursor-backed document list response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocListPage {
    /// Page of document summaries.
    pub items: Vec<DocSummary>,
    /// Cursor for the next page, or `None` when exhausted.
    pub next_cursor: Option<String>,
}

/// Cursor-backed doc version list response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocVersionListPage {
    /// Page of doc version summaries.
    pub items: Vec<DocVersionSummary>,
    /// Cursor for the next page, or `None` when exhausted.
    pub next_cursor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::ProvenanceKind;
    use pretty_assertions::assert_eq;

    #[test]
    fn doc_summary_should_serialize_expected_shape() {
        let summary = DocSummary {
            id: DocId::new("doc_001"),
            doc_type: DocType::new("brief"),
            task_id: Some(TaskId::new("task_001")),
            current_version_id: DocVersionId::new("doc_v2"),
            created_at: "2026-03-21T14:00:00Z".to_string(),
            updated_at: "2026-03-21T14:30:00Z".to_string(),
        };

        let json = serde_json::to_value(summary).expect("doc summary should serialize");

        assert_eq!(
            json,
            serde_json::json!({
                "id": "doc_001",
                "doc_type": "brief",
                "task_id": "task_001",
                "current_version_id": "doc_v2",
                "created_at": "2026-03-21T14:00:00Z",
                "updated_at": "2026-03-21T14:30:00Z",
            })
        );
    }

    #[test]
    fn doc_detail_should_serialize_current_version_inline() {
        let detail = DocDetail {
            id: DocId::new("doc_001"),
            doc_type: DocType::new("brief"),
            task_id: Some(TaskId::new("task_001")),
            current_version_id: DocVersionId::new("doc_v2"),
            current_version: DocVersionSummary {
                id: DocVersionId::new("doc_v2"),
                version_number: 2,
                content_hash: ContentHash::new("sha256:def456"),
                created_by: Some(ProvenanceRef {
                    kind: ProvenanceKind::Agent,
                    ref_id: "brief-writer".to_string(),
                }),
                created_at: "2026-03-21T14:30:00Z".to_string(),
            },
            created_at: "2026-03-21T14:00:00Z".to_string(),
            updated_at: "2026-03-21T14:30:00Z".to_string(),
        };

        let json = serde_json::to_value(detail).expect("doc detail should serialize");

        assert_eq!(
            json["current_version"]["version_number"],
            serde_json::json!(2)
        );
        assert_eq!(
            json["current_version"]["content_hash"],
            serde_json::json!("sha256:def456")
        );
    }
}
