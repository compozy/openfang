//! Pack metadata and control-plane payloads.

use serde::{Deserialize, Serialize};

/// One supported managed resource type inside a pack.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PackResourceType {
    /// Agent definition.
    Agent,
    /// Workflow definition.
    Workflow,
    /// Trigger definition.
    Trigger,
    /// Schedule definition.
    Schedule,
    /// Template definition.
    Template,
}

impl PackResourceType {
    /// Stable directory name for this resource type inside pack and top-level homes.
    #[must_use]
    pub fn directory(self) -> &'static str {
        match self {
            Self::Agent => "agents",
            Self::Workflow => "workflows",
            Self::Trigger => "triggers",
            Self::Schedule => "schedules",
            Self::Template => "templates",
        }
    }

    /// Stable singular label used in provenance blocks.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Workflow => "workflow",
            Self::Trigger => "trigger",
            Self::Schedule => "schedule",
            Self::Template => "template",
        }
    }
}

/// Source classification for an installed pack.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PackSourceKind {
    /// Bundled first-party pack content.
    Bundled,
    /// External installed pack content.
    External,
}

impl PackSourceKind {
    /// Stable string encoding used in API payloads and durable storage.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bundled => "bundled",
            Self::External => "external",
        }
    }
}

/// Source metadata for one installed pack.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackSource {
    /// Source kind.
    pub kind: PackSourceKind,
}

/// Source descriptor accepted by install and upgrade flows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackInstallSource {
    /// Source kind.
    pub kind: PackSourceKind,
    /// Stable pack identifier.
    pub pack_id: String,
    /// Exact requested version.
    pub version: String,
}

/// One managed object reference listed in `pack.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub struct PackObjectRef {
    /// Resource family.
    pub resource_type: PackResourceType,
    /// Stable resource identifier.
    pub resource_id: String,
}

/// Parsed `pack.toml` manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackManifest {
    /// Stable pack identifier.
    pub id: String,
    /// Human-readable pack name.
    pub name: String,
    /// Exact installed version.
    pub version: String,
    /// Human-readable description.
    pub description: String,
    /// Source descriptor.
    pub source: PackSource,
    /// Managed object references owned by this pack.
    pub objects: Vec<PackObjectRef>,
}

/// Summary counts of managed objects per resource family.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackObjectCounts {
    pub agents: u32,
    pub workflows: u32,
    pub triggers: u32,
    pub schedules: u32,
    pub templates: u32,
}

impl PackObjectCounts {
    /// Build counts from one pack manifest object list.
    #[must_use]
    pub fn from_objects(objects: &[PackObjectRef]) -> Self {
        let mut counts = Self::default();
        for object in objects {
            counts.increment(object.resource_type);
        }
        counts
    }

    /// Total number of managed objects across all resource families.
    #[must_use]
    pub const fn total(&self) -> u32 {
        self.agents + self.workflows + self.triggers + self.schedules + self.templates
    }

    fn increment(&mut self, resource_type: PackResourceType) {
        match resource_type {
            PackResourceType::Agent => self.agents += 1,
            PackResourceType::Workflow => self.workflows += 1,
            PackResourceType::Trigger => self.triggers += 1,
            PackResourceType::Schedule => self.schedules += 1,
            PackResourceType::Template => self.templates += 1,
        }
    }
}

/// One item returned by `GET /api/v1/packs`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub source: PackSource,
    pub installed: bool,
    pub managed: bool,
    pub objects: PackObjectCounts,
    pub updated_at: String,
}

/// Full detail returned by `GET /api/v1/packs/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackDetail {
    #[serde(flatten)]
    pub manifest: PackManifest,
    pub installed: bool,
    pub managed: bool,
    pub updated_at: String,
}

/// One managed object list item returned by `GET /api/v1/packs/{id}/objects`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackObjectSummary {
    #[serde(flatten)]
    pub object: PackObjectRef,
    pub forked: bool,
}

/// Paginated pack list response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackListResponse {
    pub items: Vec<PackSummary>,
    pub next_cursor: Option<String>,
}

/// Paginated object list response for one pack.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackObjectListResponse {
    pub items: Vec<PackObjectSummary>,
    pub next_cursor: Option<String>,
}

/// Durable installed-pack row stored in `compozy.db`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackRecord {
    pub pack_id: String,
    pub name: String,
    pub version: String,
    pub source_kind: PackSourceKind,
    pub installed: u32,
    pub managed: u32,
    pub installed_at: String,
    pub updated_at: String,
    pub objects: PackObjectCounts,
}

fn default_pack_fork_mode() -> String {
    "shadow".to_string()
}

/// Request payload for pack installation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackInstallRequest {
    pub source: PackInstallSource,
}

/// Request payload for explicit pack upgrades.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackUpgradeRequest {
    pub target_version: String,
}

/// Request payload for pack uninstall operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackUninstallRequest {
    #[serde(default)]
    pub force: bool,
}

/// Standard operational action response for pack lifecycle endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackActionResponse {
    pub accepted: bool,
    pub resource_id: String,
    pub status: String,
}

impl PackActionResponse {
    /// Build a standard accepted response for one pack resource.
    #[must_use]
    pub fn accepted(resource_id: impl Into<String>) -> Self {
        Self {
            accepted: true,
            resource_id: resource_id.into(),
            status: "accepted".to_string(),
        }
    }
}

/// Resolved version transition returned by upgrade dry-run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackUpgradeDryRunResolved {
    pub pack_id: String,
    pub from_version: String,
    pub to_version: String,
}

/// Estimated lifecycle effects returned by upgrade dry-run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackUpgradeDryRunEffects {
    pub managed_objects_added: u32,
    pub managed_objects_updated: u32,
    pub managed_objects_removed: u32,
    pub forks_untouched: u32,
}

/// Explanation payload returned by upgrade dry-run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackUpgradeDryRunExplanation {
    pub managed_objects_only: bool,
    pub forks_remain_detached: bool,
}

/// Response payload for pack upgrade dry-run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackUpgradeDryRunResponse {
    pub would_execute: bool,
    pub resolved: PackUpgradeDryRunResolved,
    pub effects: PackUpgradeDryRunEffects,
    pub explanation: PackUpgradeDryRunExplanation,
}

/// Request payload for explicit pack object forks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackForkRequest {
    pub resource_type: PackResourceType,
    pub resource_id: String,
    #[serde(default = "default_pack_fork_mode")]
    pub mode: String,
}

/// Generic origin metadata returned by pack forks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackForkOrigin {
    pub kind: PackForkOriginKind,
}

impl PackForkOrigin {
    /// User-owned origin metadata.
    #[must_use]
    pub fn user() -> Self {
        Self {
            kind: PackForkOriginKind::User,
        }
    }
}

/// Generic origin kinds returned by pack forks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PackForkOriginKind {
    /// User-owned resource.
    User,
    /// Pack-managed upstream resource.
    Pack,
}

/// Upstream provenance block returned by pack forks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackForkedFrom {
    pub kind: PackForkOriginKind,
    pub pack_id: String,
    pub pack_version: String,
    pub resource_type: PackResourceType,
    pub resource_id: String,
}

/// Generic pack fork response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PackForkResponse {
    pub id: String,
    pub origin: PackForkOrigin,
    pub forked_from: PackForkedFrom,
}

#[cfg(test)]
mod tests {
    use super::{
        PackManifest, PackObjectCounts, PackObjectCounts as Counts, PackObjectRef,
        PackResourceType, PackSourceKind, PackSummary,
    };
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn pack_manifest_should_deserialize_valid_pack_toml() {
        let manifest: PackManifest = toml::from_str(
            r#"
id = "sdlc"
name = "SDLC"
version = "1.2.0"
description = "First-party SDLC workflow package"

[source]
kind = "bundled"

[[objects]]
resource_type = "agent"
resource_id = "prd-writer"

[[objects]]
resource_type = "workflow"
resource_id = "sdlc"
"#,
        )
        .expect("manifest should deserialize");

        assert_eq!(manifest.id, "sdlc");
        assert_eq!(manifest.version, "1.2.0");
        assert_eq!(manifest.source.kind, PackSourceKind::Bundled);
        assert_eq!(
            manifest.objects,
            vec![
                PackObjectRef {
                    resource_type: PackResourceType::Agent,
                    resource_id: "prd-writer".to_string(),
                },
                PackObjectRef {
                    resource_type: PackResourceType::Workflow,
                    resource_id: "sdlc".to_string(),
                },
            ]
        );
    }

    #[test]
    fn pack_manifest_should_report_missing_required_fields() {
        let error = toml::from_str::<PackManifest>(
            r#"
name = "Missing Id"
version = "1.0.0"

[source]
kind = "external"
"#,
        )
        .expect_err("manifest should fail to deserialize");

        let message = error.to_string();
        assert!(
            message.contains("missing field"),
            "expected a missing field error, got: {message}"
        );
    }

    #[test]
    fn pack_summary_should_serialize_expected_object_count_shape() {
        let summary = PackSummary {
            id: "sdlc".to_string(),
            name: "SDLC".to_string(),
            version: "1.2.0".to_string(),
            source: super::PackSource {
                kind: PackSourceKind::Bundled,
            },
            installed: true,
            managed: true,
            objects: Counts {
                agents: 5,
                workflows: 2,
                triggers: 3,
                schedules: 1,
                templates: 4,
            },
            updated_at: "2026-03-21T14:00:00Z".to_string(),
        };

        let value = serde_json::to_value(&summary).expect("summary should serialize");
        assert_eq!(
            value,
            json!({
                "id": "sdlc",
                "name": "SDLC",
                "version": "1.2.0",
                "source": {
                    "kind": "bundled"
                },
                "installed": true,
                "managed": true,
                "objects": {
                    "agents": 5,
                    "workflows": 2,
                    "triggers": 3,
                    "schedules": 1,
                    "templates": 4
                },
                "updated_at": "2026-03-21T14:00:00Z"
            })
        );
    }

    #[test]
    fn pack_object_counts_should_accumulate_by_resource_type() {
        let counts = PackObjectCounts::from_objects(&[
            PackObjectRef {
                resource_type: PackResourceType::Agent,
                resource_id: "agent-a".to_string(),
            },
            PackObjectRef {
                resource_type: PackResourceType::Agent,
                resource_id: "agent-b".to_string(),
            },
            PackObjectRef {
                resource_type: PackResourceType::Agent,
                resource_id: "agent-c".to_string(),
            },
            PackObjectRef {
                resource_type: PackResourceType::Workflow,
                resource_id: "workflow-a".to_string(),
            },
            PackObjectRef {
                resource_type: PackResourceType::Workflow,
                resource_id: "workflow-b".to_string(),
            },
            PackObjectRef {
                resource_type: PackResourceType::Trigger,
                resource_id: "trigger-a".to_string(),
            },
        ]);

        assert_eq!(
            counts,
            PackObjectCounts {
                agents: 3,
                workflows: 2,
                triggers: 1,
                schedules: 0,
                templates: 0,
            }
        );
    }
}
