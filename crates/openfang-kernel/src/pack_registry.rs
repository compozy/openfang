//! Boot-time registry of installed pack manifests.

use chrono::{DateTime, Utc};
use openfang_types::pack::{PackManifest, PackObjectRef, PackResourceType};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// One installed pack discovered during boot.
#[derive(Debug, Clone)]
pub struct InstalledPack {
    /// Parsed `pack.toml` manifest.
    pub manifest: PackManifest,
    /// Root directory of this pack under `<home>/packs/<pack_id>`.
    pub root_dir: PathBuf,
    /// Last modified timestamp across the pack directory tree.
    pub updated_at: String,
}

impl InstalledPack {
    /// Source definition file path for one managed object in this pack.
    #[must_use]
    pub fn object_path(&self, object: &PackObjectRef) -> PathBuf {
        self.root_dir
            .join(object.resource_type.directory())
            .join(format!("{}.toml", object.resource_id))
    }
}

/// In-memory registry of installed packs loaded at kernel boot.
#[derive(Debug, Default)]
pub struct PackRegistry {
    packs: RwLock<BTreeMap<String, InstalledPack>>,
}

impl Clone for PackRegistry {
    fn clone(&self) -> Self {
        Self::from_packs(self.snapshot())
    }
}

impl PackRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn from_packs(packs: BTreeMap<String, InstalledPack>) -> Self {
        Self {
            packs: RwLock::new(packs),
        }
    }

    fn snapshot(&self) -> BTreeMap<String, InstalledPack> {
        self.packs
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    /// Scan `<home>/packs` for installed packs.
    #[must_use]
    pub fn scan(home_dir: &Path) -> PackRegistryScanReport {
        let packs_dir = home_dir.join("packs");
        let mut packs = BTreeMap::new();
        let mut warnings = Vec::new();

        if !packs_dir.exists() {
            return PackRegistryScanReport {
                registry: Self::new(),
                warnings,
            };
        }

        let entries = match std::fs::read_dir(&packs_dir) {
            Ok(entries) => entries,
            Err(error) => {
                warnings.push(format!(
                    "Failed to read packs directory '{}': {error}",
                    packs_dir.display()
                ));
                return PackRegistryScanReport {
                    registry: Self::new(),
                    warnings,
                };
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warnings.push(format!("Failed to read one pack directory entry: {error}"));
                    continue;
                }
            };
            let pack_root = entry.path();
            if !pack_root.is_dir() {
                continue;
            }

            let manifest_path = pack_root.join("pack.toml");
            if !manifest_path.is_file() {
                warnings.push(format!(
                    "Skipping pack directory '{}' because 'pack.toml' is missing",
                    pack_root.display()
                ));
                continue;
            }

            let manifest_text = match std::fs::read_to_string(&manifest_path) {
                Ok(text) => text,
                Err(error) => {
                    warnings.push(format!(
                        "Skipping pack directory '{}' because 'pack.toml' could not be read: {error}",
                        pack_root.display()
                    ));
                    continue;
                }
            };

            let manifest = match toml::from_str::<PackManifest>(&manifest_text) {
                Ok(manifest) => manifest,
                Err(error) => {
                    warnings.push(format!(
                        "Skipping pack directory '{}' because 'pack.toml' is invalid: {error}",
                        pack_root.display()
                    ));
                    continue;
                }
            };

            if packs.contains_key(&manifest.id) {
                warnings.push(format!(
                    "Skipping duplicate pack ID '{}' from '{}'",
                    manifest.id,
                    pack_root.display()
                ));
                continue;
            }

            let updated_at = latest_modified_timestamp(&pack_root)
                .or_else(|| std::fs::metadata(&manifest_path).ok()?.modified().ok())
                .map(system_time_to_rfc3339)
                .unwrap_or_else(|| Utc::now().to_rfc3339());

            packs.insert(
                manifest.id.clone(),
                InstalledPack {
                    manifest,
                    root_dir: pack_root,
                    updated_at,
                },
            );
        }

        PackRegistryScanReport {
            registry: Self::from_packs(packs),
            warnings,
        }
    }

    /// Replace the current in-memory snapshot with a fresh filesystem scan.
    pub fn refresh_from_home(&self, home_dir: &Path) -> Vec<String> {
        let report = Self::scan(home_dir);
        self.replace_all(report.registry.snapshot());
        report.warnings
    }

    fn replace_all(&self, packs: BTreeMap<String, InstalledPack>) {
        *self
            .packs
            .write()
            .unwrap_or_else(|error| error.into_inner()) = packs;
    }

    /// List all discovered packs sorted by pack ID.
    #[must_use]
    pub fn list_packs(&self) -> Vec<InstalledPack> {
        self.snapshot().into_values().collect()
    }

    /// Look up one pack by ID.
    #[must_use]
    pub fn get_pack(&self, id: &str) -> Option<InstalledPack> {
        self.snapshot().remove(id)
    }

    /// List all managed objects for one pack.
    #[must_use]
    pub fn list_objects(&self, pack_id: &str) -> Option<Vec<PackObjectRef>> {
        self.snapshot()
            .get(pack_id)
            .map(|pack| pack.manifest.objects.clone())
    }

    /// Find the first pack that manages one resource type/ID pair.
    #[must_use]
    pub fn find_pack_for_object(
        &self,
        resource_type: PackResourceType,
        resource_id: &str,
    ) -> Option<InstalledPack> {
        self.list_packs().into_iter().find(|pack| {
            pack.manifest.objects.iter().any(|object| {
                object.resource_type == resource_type && object.resource_id == resource_id
            })
        })
    }
}

/// Result of scanning the packs directory.
#[derive(Debug, Clone)]
pub struct PackRegistryScanReport {
    pub registry: PackRegistry,
    pub warnings: Vec<String>,
}

fn latest_modified_timestamp(root: &Path) -> Option<std::time::SystemTime> {
    let mut latest = None;
    let mut pending = vec![root.to_path_buf()];

    while let Some(path) = pending.pop() {
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            let Ok(entries) = std::fs::read_dir(&path) else {
                continue;
            };
            for entry in entries.flatten() {
                pending.push(entry.path());
            }
        }
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        latest = Some(match latest {
            Some(current) if current >= modified => current,
            _ => modified,
        });
    }

    latest
}

fn system_time_to_rfc3339(timestamp: std::time::SystemTime) -> String {
    DateTime::<Utc>::from(timestamp).to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::PackRegistry;
    use openfang_types::pack::PackResourceType;
    use pretty_assertions::assert_eq;
    use std::fs;
    use tempfile::tempdir;

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent directory should exist");
        }
        fs::write(path, content).expect("fixture file should be written");
    }

    use std::path::Path;

    #[test]
    fn scan_should_skip_invalid_pack_manifests_and_keep_valid_packs() {
        let root = tempdir().expect("temp dir");
        let packs_dir = root.path().join("packs");

        write_file(
            &packs_dir.join("valid/pack.toml"),
            r#"
id = "sdlc"
name = "SDLC"
version = "1.2.0"
description = "Valid pack"

[source]
kind = "bundled"

[[objects]]
resource_type = "workflow"
resource_id = "sdlc-main"
"#,
        );
        fs::create_dir_all(packs_dir.join("missing-manifest"))
            .expect("missing manifest directory should exist");
        write_file(
            &packs_dir.join("invalid/pack.toml"),
            r#"
id = "broken"
name = "Broken Pack"
[source]
kind = "bundled"
"#,
        );

        let report = PackRegistry::scan(root.path());

        assert_eq!(report.registry.list_packs().len(), 1);
        assert_eq!(report.registry.list_packs()[0].manifest.id, "sdlc");
        assert_eq!(report.warnings.len(), 2);
    }

    #[test]
    fn find_pack_for_object_should_resolve_pack_by_resource_type_and_id() {
        let root = tempdir().expect("temp dir");
        let packs_dir = root.path().join("packs");

        write_file(
            &packs_dir.join("sdlc/pack.toml"),
            r#"
id = "sdlc"
name = "SDLC"
version = "1.2.0"
description = "Workflow pack"

[source]
kind = "bundled"

[[objects]]
resource_type = "workflow"
resource_id = "sdlc-main"
"#,
        );

        let report = PackRegistry::scan(root.path());
        let pack = report
            .registry
            .find_pack_for_object(PackResourceType::Workflow, "sdlc-main")
            .expect("pack should be found");

        assert_eq!(pack.manifest.id, "sdlc");
    }

    #[cfg(unix)]
    #[test]
    fn latest_modified_timestamp_should_skip_broken_symlinks() {
        use std::os::unix::fs::symlink;

        let root = tempdir().expect("temp dir");
        let pack_root = root.path().join("packs/demo");

        write_file(
            &pack_root.join("pack.toml"),
            r#"
id = "demo"
name = "Demo"
version = "1.0.0"
description = "Demo pack"

[source]
kind = "bundled"
"#,
        );
        write_file(
            &pack_root.join("templates/demo-template.toml"),
            "title = \"demo\"\nbody = \"demo\"\n",
        );
        symlink(
            Path::new("missing-target"),
            pack_root.join("templates/broken-link"),
        )
        .expect("broken symlink should be created");

        let timestamp = super::latest_modified_timestamp(&pack_root);

        assert!(timestamp.is_some());
    }
}
