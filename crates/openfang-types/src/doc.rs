//! Shared document domain types.

use crate::artifact::{ContentHash, ProvenanceRef};
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
}

impl Default for DocListQuery {
    fn default() -> Self {
        Self {
            limit: 50,
            cursor: None,
            doc_type: None,
        }
    }
}

/// Cursor-backed document list response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocListPage {
    /// Page of document records.
    pub items: Vec<DocRecord>,
    /// Cursor for the next page, or `None` when exhausted.
    pub next_cursor: Option<String>,
}
