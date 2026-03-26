//! Shared pack installation lifecycle for API mutations and boot-time bundles.

use crate::error::KernelError;
use crate::kernel::OpenFangKernel;
use crate::pack_registry::InstalledPack;
use chrono::Utc;
use openfang_agent_definition::{
    stage4_normalize, AgentDefinition, CapabilitiesBlock, PromptBlock, ProviderBlock, RuntimeBlock,
};
use openfang_memory::PackStoreError;
use openfang_types::agent::AgentId;
use openfang_types::error::OpenFangError;
use openfang_types::pack::{
    PackInstallSource, PackManifest, PackObjectCounts, PackObjectRef, PackRecord, PackResourceType,
    PackSource, PackSourceKind, PackUpgradeDryRunEffects, PackUpgradeDryRunExplanation,
    PackUpgradeDryRunResolved, PackUpgradeDryRunResponse,
};
use openfang_types::scheduler::{
    CronAction, CronDefinitionOrigin, CronDefinitionOriginKind, CronDelivery, CronJob, CronJobId,
    CronSchedule,
};
use openfang_types::trigger::{TriggerMatch, TriggerTarget, TriggerV2Definition};
use openfang_types::workflow::WorkflowV2Definition;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::warn;

const BUNDLED_SDLC_PACK_ID: &str = "sdlc";
pub const BUNDLED_SDLC_PACK_VERSION: &str = "1.2.0";

fn default_schedule_enabled() -> bool {
    true
}

fn default_schedule_delivery() -> CronDelivery {
    CronDelivery::None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct PackScheduleDefinition {
    agent: String,
    name: String,
    #[serde(default = "default_schedule_enabled")]
    enabled: bool,
    schedule: CronSchedule,
    action: CronAction,
    #[serde(default = "default_schedule_delivery")]
    delivery: CronDelivery,
}

#[derive(Debug, Clone)]
enum ResolvedPackObjectContent {
    Agent(Box<AgentDefinition>),
    Workflow(Box<WorkflowV2Definition>),
    Trigger(Box<TriggerV2Definition>),
    Schedule(Box<PackScheduleDefinition>),
    Template(String),
}

impl ResolvedPackObjectContent {
    fn render(&self) -> Result<String, PackInstallerError> {
        match self {
            Self::Agent(definition) => toml::to_string_pretty(definition.as_ref())
                .map_err(|error| PackInstallerError::Serialization(error.to_string())),
            Self::Workflow(definition) => toml::to_string_pretty(definition.as_ref())
                .map_err(|error| PackInstallerError::Serialization(error.to_string())),
            Self::Trigger(definition) => toml::to_string_pretty(definition.as_ref())
                .map_err(|error| PackInstallerError::Serialization(error.to_string())),
            Self::Schedule(definition) => toml::to_string_pretty(definition.as_ref())
                .map_err(|error| PackInstallerError::Serialization(error.to_string())),
            Self::Template(content) => Ok(content.clone()),
        }
    }
}

#[derive(Debug, Clone)]
struct ResolvedPackObject {
    reference: PackObjectRef,
    content: ResolvedPackObjectContent,
}

#[derive(Debug, Clone)]
struct ResolvedPackContent {
    manifest: PackManifest,
    objects: Vec<ResolvedPackObject>,
}

impl ResolvedPackContent {
    fn object_map(&self) -> BTreeMap<(PackResourceType, String), &ResolvedPackObject> {
        self.objects
            .iter()
            .map(|object| {
                (
                    (
                        object.reference.resource_type,
                        object.reference.resource_id.clone(),
                    ),
                    object,
                )
            })
            .collect()
    }
}

#[derive(Debug, Error)]
pub enum PackInstallerError {
    #[error("pack '{pack_id}' was not found")]
    PackNotFound { pack_id: String },
    #[error(
        "pack '{pack_id}' is already installed at version '{current_version}'; use upgrade to move to '{requested_version}'"
    )]
    AlreadyInstalledDifferentVersion {
        pack_id: String,
        current_version: String,
        requested_version: String,
    },
    #[error("bundled pack '{pack_id}' version '{version}' is not available")]
    BundledPackVersionNotFound { pack_id: String, version: String },
    #[error("external pack '{pack_id}' version '{version}' is not staged under '{stage_path}'")]
    ExternalPackNotStaged {
        pack_id: String,
        version: String,
        stage_path: String,
    },
    #[error("pack '{pack_id}' has user forks and cannot be uninstalled without force")]
    UserForksPresent {
        pack_id: String,
        forked_ids: Vec<String>,
    },
    #[error("pack '{pack_id}' manifest is invalid: {message}")]
    InvalidManifest { pack_id: String, message: String },
    #[error("pack '{pack_id}' {resource_type:?} '{resource_id}' is invalid: {message}")]
    InvalidObject {
        pack_id: String,
        resource_type: PackResourceType,
        resource_id: String,
        message: String,
    },
    #[error("pack filesystem error: {0}")]
    Filesystem(String),
    #[error("pack serialization failed: {0}")]
    Serialization(String),
    #[error(transparent)]
    PackStore(#[from] PackStoreError),
    #[error("failed to persist cron scheduler state: {0}")]
    CronPersist(String),
    #[error("failed to update cron scheduler metadata: {0}")]
    CronSchedule(String),
}

impl From<PackInstallerError> for KernelError {
    fn from(error: PackInstallerError) -> Self {
        Self::OpenFang(OpenFangError::Internal(error.to_string()))
    }
}

pub struct PackInstaller<'a> {
    kernel: &'a OpenFangKernel,
}

impl<'a> PackInstaller<'a> {
    #[must_use]
    pub fn new(kernel: &'a OpenFangKernel) -> Self {
        Self { kernel }
    }

    pub fn install(&self, source: &PackInstallSource) -> Result<PackRecord, PackInstallerError> {
        let existing = self
            .kernel
            .workflow_stores
            .pack
            .find_by_id(&source.pack_id)?;
        if let Some(existing) = existing.as_ref() {
            if existing.version != source.version {
                return Err(PackInstallerError::AlreadyInstalledDifferentVersion {
                    pack_id: source.pack_id.clone(),
                    current_version: existing.version.clone(),
                    requested_version: source.version.clone(),
                });
            }
        }

        let resolved = self.resolve_pack_content(source)?;
        let current_pack = self.kernel.pack_registry.get_pack(&source.pack_id);
        self.apply_pack_content(current_pack.as_ref(), &resolved)?;
        self.sync_pack_record(&resolved.manifest.id)
            .and_then(|record| {
                record.ok_or_else(|| PackInstallerError::PackNotFound {
                    pack_id: resolved.manifest.id.clone(),
                })
            })
    }

    pub fn upgrade_dry_run(
        &self,
        pack_id: &str,
        target_version: &str,
    ) -> Result<PackUpgradeDryRunResponse, PackInstallerError> {
        let current_pack = self.current_pack(pack_id)?;
        let source_kind = self
            .kernel
            .workflow_stores
            .pack
            .find_by_id(pack_id)?
            .map(|record| record.source_kind)
            .unwrap_or(current_pack.manifest.source.kind);
        let resolved = self.resolve_pack_content(&PackInstallSource {
            kind: source_kind,
            pack_id: pack_id.to_string(),
            version: target_version.to_string(),
        })?;

        let effects = self.diff_pack_effects(&current_pack, &resolved)?;
        Ok(PackUpgradeDryRunResponse {
            would_execute: current_pack.manifest.version != target_version
                || effects.managed_objects_added > 0
                || effects.managed_objects_updated > 0
                || effects.managed_objects_removed > 0,
            resolved: PackUpgradeDryRunResolved {
                pack_id: pack_id.to_string(),
                from_version: current_pack.manifest.version.clone(),
                to_version: target_version.to_string(),
            },
            effects,
            explanation: PackUpgradeDryRunExplanation {
                managed_objects_only: true,
                forks_remain_detached: true,
            },
        })
    }

    pub fn upgrade(
        &self,
        pack_id: &str,
        target_version: &str,
    ) -> Result<PackRecord, PackInstallerError> {
        let current_pack = self.current_pack(pack_id)?;
        let existing_record = self.kernel.workflow_stores.pack.find_by_id(pack_id)?;
        let source_kind = existing_record
            .as_ref()
            .map(|record| record.source_kind)
            .unwrap_or(current_pack.manifest.source.kind);
        let resolved = self.resolve_pack_content(&PackInstallSource {
            kind: source_kind,
            pack_id: pack_id.to_string(),
            version: target_version.to_string(),
        })?;
        self.apply_pack_content(Some(&current_pack), &resolved)?;
        self.sync_pack_record(pack_id)?
            .ok_or_else(|| PackInstallerError::PackNotFound {
                pack_id: pack_id.to_string(),
            })
    }

    pub fn uninstall(&self, pack_id: &str, force: bool) -> Result<(), PackInstallerError> {
        let current_pack = self.current_pack(pack_id)?;
        let forked_ids = self.forked_object_ids(&current_pack.manifest.objects);
        if !force && !forked_ids.is_empty() {
            return Err(PackInstallerError::UserForksPresent {
                pack_id: pack_id.to_string(),
                forked_ids,
            });
        }

        let mut schedules_changed = false;
        for object in &current_pack.manifest.objects {
            if object.resource_type == PackResourceType::Schedule {
                if self.pack_schedule_is_shadowed(&object.resource_id) {
                    continue;
                }
                match self
                    .kernel
                    .cron_scheduler
                    .remove_job_by_definition_id(&object.resource_id)
                {
                    Ok(_) => schedules_changed = true,
                    Err(error) => {
                        return Err(PackInstallerError::CronSchedule(error.to_string()));
                    }
                }
            }
        }
        if schedules_changed {
            self.kernel
                .cron_scheduler
                .persist()
                .map_err(|error| PackInstallerError::CronPersist(error.to_string()))?;
        }

        match std::fs::remove_dir_all(&current_pack.root_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(PackInstallerError::Filesystem(format!(
                    "failed to remove pack directory '{}': {error}",
                    current_pack.root_dir.display()
                )));
            }
        }

        self.refresh_registry();
        let _ = self.kernel.workflow_stores.pack.delete(pack_id)?;
        Ok(())
    }

    pub fn sync_pack_record(
        &self,
        pack_id: &str,
    ) -> Result<Option<PackRecord>, PackInstallerError> {
        let Some(pack) = self.kernel.pack_registry.get_pack(pack_id) else {
            let _ = self.kernel.workflow_stores.pack.delete(pack_id)?;
            return Ok(None);
        };

        let existing = self.kernel.workflow_stores.pack.find_by_id(pack_id)?;
        let installed_at = existing
            .as_ref()
            .map(|record| record.installed_at.clone())
            .unwrap_or_else(now_timestamp);
        let objects = PackObjectCounts::from_objects(&pack.manifest.objects);
        let managed = pack
            .manifest
            .objects
            .iter()
            .filter(|object| !self.object_has_user_shadow(object))
            .count() as u32;
        let record = PackRecord {
            pack_id: pack.manifest.id.clone(),
            name: pack.manifest.name.clone(),
            version: pack.manifest.version.clone(),
            source_kind: pack.manifest.source.kind,
            installed: objects.total(),
            managed,
            installed_at,
            updated_at: now_timestamp(),
            objects,
        };

        self.kernel
            .workflow_stores
            .pack
            .upsert(&record)
            .map(Some)
            .map_err(Into::into)
    }

    pub fn ensure_bundled_sdlc_installed(&self) -> Result<(), PackInstallerError> {
        if self
            .kernel
            .workflow_stores
            .pack
            .find_by_id(BUNDLED_SDLC_PACK_ID)?
            .is_some()
        {
            return Ok(());
        }

        let source = PackInstallSource {
            kind: PackSourceKind::Bundled,
            pack_id: BUNDLED_SDLC_PACK_ID.to_string(),
            version: BUNDLED_SDLC_PACK_VERSION.to_string(),
        };
        self.install(&source).map(|_| ())
    }

    fn current_pack(&self, pack_id: &str) -> Result<InstalledPack, PackInstallerError> {
        self.kernel.pack_registry.get_pack(pack_id).ok_or_else(|| {
            PackInstallerError::PackNotFound {
                pack_id: pack_id.to_string(),
            }
        })
    }

    fn resolve_pack_content(
        &self,
        source: &PackInstallSource,
    ) -> Result<ResolvedPackContent, PackInstallerError> {
        match source.kind {
            PackSourceKind::Bundled => self.resolve_bundled_pack(source),
            PackSourceKind::External => self.resolve_external_pack(source),
        }
    }

    fn resolve_bundled_pack(
        &self,
        source: &PackInstallSource,
    ) -> Result<ResolvedPackContent, PackInstallerError> {
        match (source.pack_id.as_str(), source.version.as_str()) {
            (BUNDLED_SDLC_PACK_ID, "1.2.0") => Ok(bundled_sdlc_v1_2_0()),
            (BUNDLED_SDLC_PACK_ID, "1.3.0") => Ok(bundled_sdlc_v1_3_0()),
            _ => Err(PackInstallerError::BundledPackVersionNotFound {
                pack_id: source.pack_id.clone(),
                version: source.version.clone(),
            }),
        }
    }

    fn resolve_external_pack(
        &self,
        source: &PackInstallSource,
    ) -> Result<ResolvedPackContent, PackInstallerError> {
        let stage_root = self.external_stage_root(&source.pack_id, &source.version);
        if !stage_root.exists() {
            return Err(PackInstallerError::ExternalPackNotStaged {
                pack_id: source.pack_id.clone(),
                version: source.version.clone(),
                stage_path: stage_root.display().to_string(),
            });
        }

        let manifest_path = stage_root.join("pack.toml");
        let manifest_text = std::fs::read_to_string(&manifest_path).map_err(|error| {
            PackInstallerError::InvalidManifest {
                pack_id: source.pack_id.clone(),
                message: format!("failed to read '{}': {error}", manifest_path.display()),
            }
        })?;
        let mut manifest = toml::from_str::<PackManifest>(&manifest_text).map_err(|error| {
            PackInstallerError::InvalidManifest {
                pack_id: source.pack_id.clone(),
                message: error.to_string(),
            }
        })?;
        if manifest.id != source.pack_id {
            return Err(PackInstallerError::InvalidManifest {
                pack_id: source.pack_id.clone(),
                message: format!(
                    "expected manifest id '{}', found '{}'",
                    source.pack_id, manifest.id
                ),
            });
        }
        if manifest.version != source.version {
            return Err(PackInstallerError::InvalidManifest {
                pack_id: source.pack_id.clone(),
                message: format!(
                    "expected manifest version '{}', found '{}'",
                    source.version, manifest.version
                ),
            });
        }
        manifest.source = PackSource {
            kind: PackSourceKind::External,
        };

        let mut objects = Vec::with_capacity(manifest.objects.len());
        for reference in &manifest.objects {
            let content = self.load_staged_object(&manifest.id, &stage_root, reference)?;
            objects.push(ResolvedPackObject {
                reference: reference.clone(),
                content,
            });
        }

        Ok(ResolvedPackContent { manifest, objects })
    }

    fn load_staged_object(
        &self,
        pack_id: &str,
        stage_root: &Path,
        reference: &PackObjectRef,
    ) -> Result<ResolvedPackObjectContent, PackInstallerError> {
        let path = stage_root
            .join(reference.resource_type.directory())
            .join(format!("{}.toml", reference.resource_id));
        let text =
            std::fs::read_to_string(&path).map_err(|error| PackInstallerError::InvalidObject {
                pack_id: pack_id.to_string(),
                resource_type: reference.resource_type,
                resource_id: reference.resource_id.clone(),
                message: format!("failed to read '{}': {error}", path.display()),
            })?;

        match reference.resource_type {
            PackResourceType::Agent => toml::from_str::<AgentDefinition>(&text)
                .map(stage4_normalize)
                .map(Box::new)
                .map(ResolvedPackObjectContent::Agent)
                .map_err(|error| PackInstallerError::InvalidObject {
                    pack_id: pack_id.to_string(),
                    resource_type: reference.resource_type,
                    resource_id: reference.resource_id.clone(),
                    message: error.to_string(),
                }),
            PackResourceType::Workflow => toml::from_str::<WorkflowV2Definition>(&text)
                .map(Box::new)
                .map(ResolvedPackObjectContent::Workflow)
                .map_err(|error| PackInstallerError::InvalidObject {
                    pack_id: pack_id.to_string(),
                    resource_type: reference.resource_type,
                    resource_id: reference.resource_id.clone(),
                    message: error.to_string(),
                }),
            PackResourceType::Trigger => toml::from_str::<TriggerV2Definition>(&text)
                .map(Box::new)
                .map(ResolvedPackObjectContent::Trigger)
                .map_err(|error| PackInstallerError::InvalidObject {
                    pack_id: pack_id.to_string(),
                    resource_type: reference.resource_type,
                    resource_id: reference.resource_id.clone(),
                    message: error.to_string(),
                }),
            PackResourceType::Schedule => toml::from_str::<PackScheduleDefinition>(&text)
                .map(Box::new)
                .map(ResolvedPackObjectContent::Schedule)
                .map_err(|error| PackInstallerError::InvalidObject {
                    pack_id: pack_id.to_string(),
                    resource_type: reference.resource_type,
                    resource_id: reference.resource_id.clone(),
                    message: error.to_string(),
                }),
            PackResourceType::Template => toml::from_str::<toml::Value>(&text)
                .map(|_| ResolvedPackObjectContent::Template(text))
                .map_err(|error| PackInstallerError::InvalidObject {
                    pack_id: pack_id.to_string(),
                    resource_type: reference.resource_type,
                    resource_id: reference.resource_id.clone(),
                    message: error.to_string(),
                }),
        }
    }

    fn apply_pack_content(
        &self,
        current_pack: Option<&InstalledPack>,
        resolved: &ResolvedPackContent,
    ) -> Result<(), PackInstallerError> {
        let pack_root = self.pack_root(&resolved.manifest.id);
        std::fs::create_dir_all(&pack_root).map_err(|error| {
            PackInstallerError::Filesystem(format!(
                "failed to create pack directory '{}': {error}",
                pack_root.display()
            ))
        })?;

        for object in &resolved.objects {
            let path = pack_root
                .join(object.reference.resource_type.directory())
                .join(format!("{}.toml", object.reference.resource_id));
            let rendered = object.content.render()?;
            write_text_file_atomically(&path, &object.reference.resource_id, &rendered)?;
        }

        self.sync_schedule_objects(current_pack, resolved)?;
        self.remove_deleted_pack_objects(current_pack, resolved)?;

        let manifest_payload = toml::to_string_pretty(&resolved.manifest)
            .map_err(|error| PackInstallerError::Serialization(error.to_string()))?;
        write_text_file_atomically(
            &pack_root.join("pack.toml"),
            &resolved.manifest.id,
            &manifest_payload,
        )?;

        self.refresh_registry();
        let _ = self.sync_pack_record(&resolved.manifest.id)?;
        Ok(())
    }

    fn sync_schedule_objects(
        &self,
        current_pack: Option<&InstalledPack>,
        resolved: &ResolvedPackContent,
    ) -> Result<(), PackInstallerError> {
        let current_schedule_ids = current_pack
            .map(|pack| {
                pack.manifest
                    .objects
                    .iter()
                    .filter(|object| object.resource_type == PackResourceType::Schedule)
                    .map(|object| object.resource_id.clone())
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        let next_schedules = resolved
            .objects
            .iter()
            .filter_map(|object| match &object.content {
                ResolvedPackObjectContent::Schedule(definition) => {
                    Some((object.reference.resource_id.clone(), definition))
                }
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();

        let mut changed = false;
        for schedule_id in current_schedule_ids
            .difference(&next_schedules.keys().cloned().collect::<BTreeSet<_>>())
        {
            if self.pack_schedule_is_shadowed(schedule_id) {
                continue;
            }
            match self
                .kernel
                .cron_scheduler
                .remove_job_by_definition_id(schedule_id)
            {
                Ok(_) => changed = true,
                Err(error) => return Err(PackInstallerError::CronSchedule(error.to_string())),
            }
        }

        for (schedule_id, definition) in next_schedules {
            if self.pack_schedule_is_shadowed(&schedule_id) {
                continue;
            }
            let replacement = self.pack_schedule_meta(
                &resolved.manifest.id,
                &resolved.manifest.version,
                resolved,
                &schedule_id,
                definition,
            )?;
            if self
                .kernel
                .cron_scheduler
                .get_meta_by_definition_id(&schedule_id)
                .is_some()
            {
                self.kernel
                    .cron_scheduler
                    .replace_job_meta_by_definition_id(&schedule_id, replacement)
                    .map_err(|error| PackInstallerError::CronSchedule(error.to_string()))?;
            } else {
                self.kernel
                    .cron_scheduler
                    .add_job_meta(replacement)
                    .map_err(|error| PackInstallerError::CronSchedule(error.to_string()))?;
            }
            changed = true;
        }

        if changed {
            self.kernel
                .cron_scheduler
                .persist()
                .map_err(|error| PackInstallerError::CronPersist(error.to_string()))?;
        }
        Ok(())
    }

    fn remove_deleted_pack_objects(
        &self,
        current_pack: Option<&InstalledPack>,
        resolved: &ResolvedPackContent,
    ) -> Result<(), PackInstallerError> {
        let Some(current_pack) = current_pack else {
            return Ok(());
        };
        let next_keys = resolved.object_map().into_keys().collect::<BTreeSet<_>>();

        for object in &current_pack.manifest.objects {
            let key = (object.resource_type, object.resource_id.clone());
            if next_keys.contains(&key) {
                continue;
            }
            let path = current_pack.object_path(object);
            match std::fs::remove_file(&path) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(PackInstallerError::Filesystem(format!(
                        "failed to remove stale pack object '{}': {error}",
                        path.display()
                    )));
                }
            }
        }

        Ok(())
    }

    fn diff_pack_effects(
        &self,
        current_pack: &InstalledPack,
        resolved: &ResolvedPackContent,
    ) -> Result<PackUpgradeDryRunEffects, PackInstallerError> {
        let current_objects = current_pack
            .manifest
            .objects
            .iter()
            .map(|object| ((object.resource_type, object.resource_id.clone()), object))
            .collect::<BTreeMap<_, _>>();
        let next_objects = resolved.object_map();

        let mut added = 0u32;
        let mut updated = 0u32;
        let mut removed = 0u32;

        for (key, next) in &next_objects {
            match current_objects.get(key) {
                None => added += 1,
                Some(current) => {
                    let current_text =
                        std::fs::read_to_string(current_pack.object_path(current))
                            .map_err(|error| PackInstallerError::Filesystem(error.to_string()))?;
                    if current_text != next.content.render()? {
                        updated += 1;
                    }
                }
            }
        }

        for key in current_objects.keys() {
            if !next_objects.contains_key(key) {
                removed += 1;
            }
        }

        let fork_union = current_pack
            .manifest
            .objects
            .iter()
            .cloned()
            .chain(resolved.manifest.objects.iter().cloned())
            .collect::<BTreeSet<_>>();
        let forks_untouched = fork_union
            .iter()
            .filter(|object| self.object_has_user_shadow(object))
            .count() as u32;

        Ok(PackUpgradeDryRunEffects {
            managed_objects_added: added,
            managed_objects_updated: updated,
            managed_objects_removed: removed,
            forks_untouched,
        })
    }

    fn object_has_user_shadow(&self, object: &PackObjectRef) -> bool {
        match object.resource_type {
            PackResourceType::Schedule => self.pack_schedule_is_shadowed(&object.resource_id),
            _ => self
                .kernel
                .config
                .home_dir
                .join(object.resource_type.directory())
                .join(format!("{}.toml", object.resource_id))
                .exists(),
        }
    }

    fn pack_schedule_is_shadowed(&self, schedule_id: &str) -> bool {
        self.kernel
            .cron_scheduler
            .get_meta_by_definition_id(schedule_id)
            .is_some_and(|meta| meta.origin.kind == CronDefinitionOriginKind::User)
    }

    fn forked_object_ids(&self, objects: &[PackObjectRef]) -> Vec<String> {
        let mut ids = objects
            .iter()
            .filter(|object| self.object_has_user_shadow(object))
            .map(|object| object.resource_id.clone())
            .collect::<Vec<_>>();
        ids.sort();
        ids.dedup();
        ids
    }

    fn pack_schedule_meta(
        &self,
        pack_id: &str,
        pack_version: &str,
        resolved: &ResolvedPackContent,
        definition_id: &str,
        definition: &PackScheduleDefinition,
    ) -> Result<crate::cron::JobMeta, PackInstallerError> {
        let runtime_agent_id =
            self.resolve_schedule_agent_runtime_id(resolved, &definition.agent)?;
        let created_at = Utc::now();
        let job = CronJob {
            id: CronJobId::new(),
            agent_id: runtime_agent_id,
            name: definition.name.clone(),
            enabled: definition.enabled,
            schedule: definition.schedule.clone(),
            action: definition.action.clone(),
            delivery: definition.delivery.clone(),
            created_at,
            last_run: None,
            next_run: None,
        };

        Ok(crate::cron::JobMeta {
            job,
            definition_id: definition_id.to_string(),
            agent_ref: definition.agent.clone(),
            origin: CronDefinitionOrigin {
                kind: CronDefinitionOriginKind::Pack,
                pack_id: Some(pack_id.to_string()),
                pack_version: Some(pack_version.to_string()),
                source: Some("pack".to_string()),
            },
            forked_from: None,
            updated_at: created_at.to_rfc3339(),
            one_shot: matches!(definition.schedule, CronSchedule::At { .. }),
            last_status: None,
            consecutive_errors: 0,
        })
    }

    fn resolve_schedule_agent_runtime_id(
        &self,
        resolved: &ResolvedPackContent,
        agent_ref: &str,
    ) -> Result<AgentId, PackInstallerError> {
        if let Ok(agent_id) = agent_ref.parse::<AgentId>() {
            return Ok(agent_id);
        }
        if let Some(definition_id) = resolved_agent_definition_id(resolved, agent_ref) {
            return Ok(OpenFangKernel::stable_runtime_agent_id(&definition_id));
        }
        if let Some(entry) = self.kernel.registry.find_by_name(agent_ref) {
            return Ok(entry.id);
        }
        if self.definition_exists(agent_ref) {
            return Ok(OpenFangKernel::stable_runtime_agent_id(agent_ref));
        }

        let matching_name = self.find_definition_id_by_name(agent_ref)?;
        matching_name
            .map(|definition_id| OpenFangKernel::stable_runtime_agent_id(&definition_id))
            .ok_or_else(|| PackInstallerError::InvalidObject {
                pack_id: "pack".to_string(),
                resource_type: PackResourceType::Schedule,
                resource_id: agent_ref.to_string(),
                message: format!("unknown schedule agent reference '{agent_ref}'"),
            })
    }

    fn definition_exists(&self, definition_id: &str) -> bool {
        self.kernel
            .config
            .home_dir
            .join("agents")
            .join(format!("{definition_id}.toml"))
            .is_file()
            || self
                .kernel
                .pack_registry
                .find_pack_for_object(PackResourceType::Agent, definition_id)
                .is_some()
    }

    fn find_definition_id_by_name(
        &self,
        agent_name: &str,
    ) -> Result<Option<String>, PackInstallerError> {
        let agents_dir = self.kernel.config.home_dir.join("agents");
        if agents_dir.exists() {
            for entry in std::fs::read_dir(&agents_dir).map_err(|error| {
                PackInstallerError::Filesystem(format!(
                    "failed to read agent definitions directory '{}': {error}",
                    agents_dir.display()
                ))
            })? {
                let path = entry
                    .map_err(|error| PackInstallerError::Filesystem(error.to_string()))?
                    .path();
                if path.extension().and_then(|value| value.to_str()) != Some("toml") {
                    continue;
                }
                let text = std::fs::read_to_string(&path)
                    .map_err(|error| PackInstallerError::Filesystem(error.to_string()))?;
                let definition = toml::from_str::<AgentDefinition>(&text)
                    .map(stage4_normalize)
                    .map_err(|error| PackInstallerError::Serialization(error.to_string()))?;
                if definition.name == agent_name {
                    return Ok(Some(definition.id));
                }
            }
        }

        for pack in self.kernel.pack_registry.list_packs() {
            for object in &pack.manifest.objects {
                if object.resource_type != PackResourceType::Agent {
                    continue;
                }
                let text = std::fs::read_to_string(pack.object_path(object))
                    .map_err(|error| PackInstallerError::Filesystem(error.to_string()))?;
                let definition = toml::from_str::<AgentDefinition>(&text)
                    .map(stage4_normalize)
                    .map_err(|error| PackInstallerError::Serialization(error.to_string()))?;
                if definition.name == agent_name {
                    return Ok(Some(definition.id));
                }
            }
        }

        Ok(None)
    }

    fn refresh_registry(&self) {
        for warning in self
            .kernel
            .pack_registry
            .refresh_from_home(&self.kernel.config.home_dir)
        {
            warn!("{warning}");
        }
    }

    fn pack_root(&self, pack_id: &str) -> PathBuf {
        self.kernel.config.home_dir.join("packs").join(pack_id)
    }

    fn external_stage_root(&self, pack_id: &str, version: &str) -> PathBuf {
        self.kernel
            .config
            .home_dir
            .join(".pack-staging")
            .join(pack_id)
            .join(version)
    }
}

fn write_text_file_atomically(
    path: &Path,
    id: &str,
    payload: &str,
) -> Result<(), PackInstallerError> {
    let Some(parent) = path.parent() else {
        return Err(PackInstallerError::Filesystem(format!(
            "failed to determine parent directory for '{}'",
            path.display()
        )));
    };
    std::fs::create_dir_all(parent).map_err(|error| {
        PackInstallerError::Filesystem(format!(
            "failed to create directory '{}': {error}",
            parent.display()
        ))
    })?;

    let mut temp_file = tempfile::Builder::new()
        .prefix(id)
        .suffix(".toml.tmp")
        .tempfile_in(parent)
        .map_err(|error| {
            PackInstallerError::Filesystem(format!(
                "failed to create temporary file near '{}': {error}",
                path.display()
            ))
        })?;
    temp_file.write_all(payload.as_bytes()).map_err(|error| {
        PackInstallerError::Filesystem(format!(
            "failed to write temporary file '{}': {error}",
            path.display()
        ))
    })?;
    temp_file.flush().map_err(|error| {
        PackInstallerError::Filesystem(format!(
            "failed to flush temporary file '{}': {error}",
            path.display()
        ))
    })?;
    temp_file.persist(path).map_err(|error| {
        PackInstallerError::Filesystem(format!(
            "failed to replace file '{}': {error}",
            path.display()
        ))
    })?;
    Ok(())
}

fn resolved_agent_definition_id(resolved: &ResolvedPackContent, agent_ref: &str) -> Option<String> {
    resolved
        .objects
        .iter()
        .find_map(|object| match &object.content {
            ResolvedPackObjectContent::Agent(definition)
                if definition.id == agent_ref || definition.name == agent_ref =>
            {
                Some(definition.id.clone())
            }
            _ => None,
        })
}

fn now_timestamp() -> String {
    Utc::now().to_rfc3339()
}

fn bundled_sdlc_v1_2_0() -> ResolvedPackContent {
    let manifest = PackManifest {
        id: BUNDLED_SDLC_PACK_ID.to_string(),
        name: "SDLC".to_string(),
        version: "1.2.0".to_string(),
        description: "Bundled software delivery lifecycle automation".to_string(),
        source: PackSource {
            kind: PackSourceKind::Bundled,
        },
        objects: vec![
            PackObjectRef {
                resource_type: PackResourceType::Agent,
                resource_id: "sdlc-planner".to_string(),
            },
            PackObjectRef {
                resource_type: PackResourceType::Agent,
                resource_id: "sdlc-implementer".to_string(),
            },
            PackObjectRef {
                resource_type: PackResourceType::Workflow,
                resource_id: "sdlc-main".to_string(),
            },
            PackObjectRef {
                resource_type: PackResourceType::Trigger,
                resource_id: "sdlc-issue-created".to_string(),
            },
        ],
    };

    ResolvedPackContent {
        manifest,
        objects: vec![
            ResolvedPackObject {
                reference: PackObjectRef {
                    resource_type: PackResourceType::Agent,
                    resource_id: "sdlc-planner".to_string(),
                },
                content: ResolvedPackObjectContent::Agent(Box::new(sdlc_agent_definition(
                    "sdlc-planner",
                    "SDLC Planner",
                    "Plans the work and writes an execution-ready PRD.",
                    &["sdlc", "planning"],
                ))),
            },
            ResolvedPackObject {
                reference: PackObjectRef {
                    resource_type: PackResourceType::Agent,
                    resource_id: "sdlc-implementer".to_string(),
                },
                content: ResolvedPackObjectContent::Agent(Box::new(sdlc_agent_definition(
                    "sdlc-implementer",
                    "SDLC Implementer",
                    "Implements the approved plan and produces code changes.",
                    &["sdlc", "implementation"],
                ))),
            },
            ResolvedPackObject {
                reference: PackObjectRef {
                    resource_type: PackResourceType::Workflow,
                    resource_id: "sdlc-main".to_string(),
                },
                content: ResolvedPackObjectContent::Workflow(Box::new(sdlc_workflow_v1_2_0())),
            },
            ResolvedPackObject {
                reference: PackObjectRef {
                    resource_type: PackResourceType::Trigger,
                    resource_id: "sdlc-issue-created".to_string(),
                },
                content: ResolvedPackObjectContent::Trigger(Box::new(sdlc_trigger_v1_2_0())),
            },
        ],
    }
}

fn bundled_sdlc_v1_3_0() -> ResolvedPackContent {
    let manifest = PackManifest {
        id: BUNDLED_SDLC_PACK_ID.to_string(),
        name: "SDLC".to_string(),
        version: "1.3.0".to_string(),
        description: "Bundled software delivery lifecycle automation".to_string(),
        source: PackSource {
            kind: PackSourceKind::Bundled,
        },
        objects: vec![
            PackObjectRef {
                resource_type: PackResourceType::Agent,
                resource_id: "sdlc-planner".to_string(),
            },
            PackObjectRef {
                resource_type: PackResourceType::Agent,
                resource_id: "sdlc-implementer".to_string(),
            },
            PackObjectRef {
                resource_type: PackResourceType::Agent,
                resource_id: "sdlc-reviewer".to_string(),
            },
            PackObjectRef {
                resource_type: PackResourceType::Workflow,
                resource_id: "sdlc-main".to_string(),
            },
            PackObjectRef {
                resource_type: PackResourceType::Trigger,
                resource_id: "sdlc-issue-created".to_string(),
            },
        ],
    };

    ResolvedPackContent {
        manifest,
        objects: vec![
            ResolvedPackObject {
                reference: PackObjectRef {
                    resource_type: PackResourceType::Agent,
                    resource_id: "sdlc-planner".to_string(),
                },
                content: ResolvedPackObjectContent::Agent(Box::new(sdlc_agent_definition(
                    "sdlc-planner",
                    "SDLC Planner",
                    "Plans the work and writes an execution-ready PRD.",
                    &["sdlc", "planning"],
                ))),
            },
            ResolvedPackObject {
                reference: PackObjectRef {
                    resource_type: PackResourceType::Agent,
                    resource_id: "sdlc-implementer".to_string(),
                },
                content: ResolvedPackObjectContent::Agent(Box::new(sdlc_agent_definition(
                    "sdlc-implementer",
                    "SDLC Implementer",
                    "Implements the approved plan and prepares the delivery patch.",
                    &["sdlc", "implementation"],
                ))),
            },
            ResolvedPackObject {
                reference: PackObjectRef {
                    resource_type: PackResourceType::Agent,
                    resource_id: "sdlc-reviewer".to_string(),
                },
                content: ResolvedPackObjectContent::Agent(Box::new(sdlc_agent_definition(
                    "sdlc-reviewer",
                    "SDLC Reviewer",
                    "Reviews the implementation and produces release notes.",
                    &["sdlc", "review"],
                ))),
            },
            ResolvedPackObject {
                reference: PackObjectRef {
                    resource_type: PackResourceType::Workflow,
                    resource_id: "sdlc-main".to_string(),
                },
                content: ResolvedPackObjectContent::Workflow(Box::new(sdlc_workflow_v1_3_0())),
            },
            ResolvedPackObject {
                reference: PackObjectRef {
                    resource_type: PackResourceType::Trigger,
                    resource_id: "sdlc-issue-created".to_string(),
                },
                content: ResolvedPackObjectContent::Trigger(Box::new(sdlc_trigger_v1_3_0())),
            },
        ],
    }
}

fn sdlc_agent_definition(
    id: &str,
    name: &str,
    description: &str,
    tags: &[&str],
) -> AgentDefinition {
    stage4_normalize(AgentDefinition {
        id: id.to_string(),
        name: name.to_string(),
        version: "1.0.0".to_string(),
        description: description.to_string(),
        enabled: Some(true),
        group: Some("sdlc".to_string()),
        tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
        provider: ProviderBlock {
            driver: "codex".to_string(),
            model: "gpt-5".to_string(),
            ..ProviderBlock::default()
        },
        prompt: PromptBlock::default(),
        capabilities: CapabilitiesBlock::default(),
        runtime: RuntimeBlock::default(),
        input: None,
        output: None,
    })
}

fn sdlc_workflow_v1_2_0() -> WorkflowV2Definition {
    serde_json::from_value(json!({
        "id": "sdlc-main",
        "name": "SDLC Main",
        "version": "1.2.0",
        "description": "Plans and implements a single delivery task.",
        "enabled": true,
        "tags": ["sdlc", "delivery"],
        "input": {
            "kind": "object",
            "required": ["issue_id"],
            "open": false,
            "fields": {
                "issue_id": { "kind": "string" }
            }
        },
        "output": {
            "kind": "object",
            "required": ["implementation"],
            "open": false,
            "fields": {
                "implementation": { "kind": "string" }
            }
        },
        "steps": [
            {
                "id": "plan",
                "name": "Plan Delivery",
                "kind": "agent",
                "uses": { "agent": "sdlc-planner" },
                "with": {
                    "issue_id": "{{ input.issue_id }}"
                },
                "save_as": "plan",
                "flow": { "mode": "sequential" },
                "runtime": { "timeout_secs": 300, "error_mode": "fail" }
            },
            {
                "id": "implement",
                "name": "Implement Delivery",
                "kind": "agent",
                "uses": { "agent": "sdlc-implementer" },
                "with": {
                    "plan": "{{ vars.plan }}"
                },
                "save_as": "implementation",
                "flow": { "mode": "sequential" },
                "runtime": { "timeout_secs": 600, "error_mode": "fail" }
            }
        ],
        "outputs": {
            "implementation": "{{ vars.implementation }}"
        }
    }))
    .expect("bundled SDLC workflow v1.2.0 should deserialize")
}

fn sdlc_workflow_v1_3_0() -> WorkflowV2Definition {
    serde_json::from_value(json!({
        "id": "sdlc-main",
        "name": "SDLC Main",
        "version": "1.3.0",
        "description": "Plans, implements, and reviews a single delivery task.",
        "enabled": true,
        "tags": ["sdlc", "delivery"],
        "input": {
            "kind": "object",
            "required": ["issue_id"],
            "open": false,
            "fields": {
                "issue_id": { "kind": "string" }
            }
        },
        "output": {
            "kind": "object",
            "required": ["review"],
            "open": false,
            "fields": {
                "review": { "kind": "string" }
            }
        },
        "steps": [
            {
                "id": "plan",
                "name": "Plan Delivery",
                "kind": "agent",
                "uses": { "agent": "sdlc-planner" },
                "with": {
                    "issue_id": "{{ input.issue_id }}"
                },
                "save_as": "plan",
                "flow": { "mode": "sequential" },
                "runtime": { "timeout_secs": 300, "error_mode": "fail" }
            },
            {
                "id": "implement",
                "name": "Implement Delivery",
                "kind": "agent",
                "uses": { "agent": "sdlc-implementer" },
                "with": {
                    "plan": "{{ vars.plan }}"
                },
                "save_as": "implementation",
                "flow": { "mode": "sequential" },
                "runtime": { "timeout_secs": 600, "error_mode": "fail" }
            },
            {
                "id": "review",
                "name": "Review Delivery",
                "kind": "agent",
                "uses": { "agent": "sdlc-reviewer" },
                "with": {
                    "implementation": "{{ vars.implementation }}"
                },
                "save_as": "review",
                "flow": { "mode": "sequential" },
                "runtime": { "timeout_secs": 300, "error_mode": "fail" }
            }
        ],
        "outputs": {
            "review": "{{ vars.review }}"
        }
    }))
    .expect("bundled SDLC workflow v1.3.0 should deserialize")
}

fn sdlc_trigger_v1_2_0() -> TriggerV2Definition {
    TriggerV2Definition {
        id: "sdlc-issue-created".to_string(),
        name: "Start SDLC On Issue Created".to_string(),
        description: "Starts the SDLC workflow when a new issue arrives.".to_string(),
        enabled: true,
        max_fires: 0,
        cooldown_secs: 0,
        trigger_match: TriggerMatch {
            event: Some("issue.created".to_string()),
            source: Some("compozy".to_string()),
            contains: None,
            filters: BTreeMap::new(),
        },
        target: TriggerTarget::WorkflowStart {
            workflow: "sdlc-main".to_string(),
            input: json!({
                "issue_id": "{{ event.issue_id }}"
            }),
        },
    }
}

fn sdlc_trigger_v1_3_0() -> TriggerV2Definition {
    TriggerV2Definition {
        id: "sdlc-issue-created".to_string(),
        name: "Start SDLC On Issue Created".to_string(),
        description: "Starts the reviewed SDLC workflow when a new issue arrives.".to_string(),
        enabled: true,
        max_fires: 0,
        cooldown_secs: 30,
        trigger_match: TriggerMatch {
            event: Some("issue.created".to_string()),
            source: Some("compozy".to_string()),
            contains: None,
            filters: BTreeMap::new(),
        },
        target: TriggerTarget::WorkflowStart {
            workflow: "sdlc-main".to_string(),
            input: json!({
                "issue_id": "{{ event.issue_id }}"
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{PackInstaller, PackInstallerError, BUNDLED_SDLC_PACK_VERSION};
    use crate::kernel::OpenFangKernel;
    use openfang_types::config::{DefaultModelConfig, KernelConfig};
    use openfang_types::pack::{PackInstallSource, PackSourceKind};
    use pretty_assertions::assert_eq;
    fn test_kernel(home_dir: &std::path::Path) -> OpenFangKernel {
        OpenFangKernel::boot_with_config(KernelConfig {
            home_dir: home_dir.to_path_buf(),
            data_dir: home_dir.join("data"),
            default_model: DefaultModelConfig {
                provider: "ollama".to_string(),
                model: "test-model".to_string(),
                api_key_env: "OLLAMA_API_KEY".to_string(),
                base_url: None,
            },
            ..KernelConfig::default()
        })
        .expect("kernel should boot")
    }

    #[test]
    fn install_should_bootstrap_bundled_sdlc_pack() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let kernel = test_kernel(tmp.path());
        let installer = PackInstaller::new(&kernel);

        let record = installer
            .install(&PackInstallSource {
                kind: PackSourceKind::Bundled,
                pack_id: "sdlc".to_string(),
                version: BUNDLED_SDLC_PACK_VERSION.to_string(),
            })
            .expect("bundled install should succeed");

        assert_eq!(record.pack_id, "sdlc");
        assert_eq!(record.version, BUNDLED_SDLC_PACK_VERSION);
        assert!(tmp.path().join("packs/sdlc/pack.toml").exists());
    }

    #[test]
    fn upgrade_dry_run_should_report_expected_effects_for_bundled_sdlc() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let kernel = test_kernel(tmp.path());
        let installer = PackInstaller::new(&kernel);
        installer
            .install(&PackInstallSource {
                kind: PackSourceKind::Bundled,
                pack_id: "sdlc".to_string(),
                version: "1.2.0".to_string(),
            })
            .expect("bundled install should succeed");

        let dry_run = installer
            .upgrade_dry_run("sdlc", "1.3.0")
            .expect("dry run should succeed");

        assert!(dry_run.would_execute);
        assert_eq!(dry_run.effects.managed_objects_added, 1);
        assert_eq!(dry_run.effects.managed_objects_updated, 3);
        assert_eq!(dry_run.effects.managed_objects_removed, 0);
    }

    #[test]
    fn uninstall_should_error_when_user_forks_exist_without_force() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let kernel = test_kernel(tmp.path());
        let installer = PackInstaller::new(&kernel);
        installer
            .install(&PackInstallSource {
                kind: PackSourceKind::Bundled,
                pack_id: "sdlc".to_string(),
                version: "1.2.0".to_string(),
            })
            .expect("bundled install should succeed");
        std::fs::create_dir_all(tmp.path().join("workflows"))
            .expect("workflow directory should be created");
        std::fs::write(
            tmp.path().join("workflows/sdlc-main.toml"),
            "id = 'sdlc-main'\nname = 'Fork'\nversion = '1.0.0'\ndescription = 'fork'\nenabled = true\ninput = { kind = 'text' }\noutput = { kind = 'text' }\nsteps = []\n",
        )
        .expect("user shadow should be written");

        let error = installer
            .uninstall("sdlc", false)
            .expect_err("uninstall should reject user forks");

        match error {
            PackInstallerError::UserForksPresent { forked_ids, .. } => {
                assert_eq!(forked_ids, vec!["sdlc-main".to_string()]);
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn upgrade_should_preserve_user_forks_and_update_pack_record() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let kernel = test_kernel(tmp.path());
        let installer = PackInstaller::new(&kernel);
        installer
            .install(&PackInstallSource {
                kind: PackSourceKind::Bundled,
                pack_id: "sdlc".to_string(),
                version: "1.2.0".to_string(),
            })
            .expect("bundled install should succeed");

        let shadow_dir = tmp.path().join("workflows");
        std::fs::create_dir_all(&shadow_dir).expect("workflow directory should be created");
        let shadow_path = shadow_dir.join("sdlc-main.toml");
        let shadow_payload = r#"
id = "sdlc-main"
name = "Forked SDLC Main"
version = "1.0.0"
description = "fork"
enabled = true
tags = []
steps = []

[input]
kind = "object"
required = []
open = true
fields = {}

[output]
kind = "object"
required = []
open = true
fields = {}

[origin]
kind = "user"

[forked_from]
kind = "pack"
pack_id = "sdlc"
pack_version = "1.2.0"
resource_type = "workflow"
resource_id = "sdlc-main"
"#;
        std::fs::write(&shadow_path, shadow_payload).expect("shadow should be written");

        let record = installer
            .upgrade("sdlc", "1.3.0")
            .expect("upgrade should succeed");

        assert_eq!(record.version, "1.3.0");
        assert_eq!(record.installed, 5);
        assert_eq!(record.managed, 4);
        assert_eq!(
            std::fs::read_to_string(&shadow_path).expect("shadow should remain"),
            shadow_payload
        );
        assert!(tmp
            .path()
            .join("packs/sdlc/agents/sdlc-reviewer.toml")
            .exists());
    }

    #[test]
    fn uninstall_should_remove_pack_record_and_managed_files() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let kernel = test_kernel(tmp.path());
        let installer = PackInstaller::new(&kernel);
        installer
            .install(&PackInstallSource {
                kind: PackSourceKind::Bundled,
                pack_id: "sdlc".to_string(),
                version: "1.2.0".to_string(),
            })
            .expect("bundled install should succeed");

        installer
            .uninstall("sdlc", false)
            .expect("uninstall should succeed");

        assert!(!tmp.path().join("packs/sdlc").is_dir());
        assert!(kernel
            .workflow_stores
            .pack
            .find_by_id("sdlc")
            .expect("pack lookup should succeed")
            .is_none());
    }
}
