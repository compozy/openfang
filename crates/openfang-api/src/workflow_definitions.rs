//! Canonical file-backed storage for public workflow definitions.

use crate::types::WorkflowResponse;
use openfang_types::contract::normalize as normalize_contract;
use openfang_types::workflow::WorkflowV2Definition;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct WorkflowDefinitionStore {
    dir: PathBuf,
}

impl WorkflowDefinitionStore {
    pub(crate) fn new(home_dir: &Path) -> Self {
        Self {
            dir: home_dir.join("workflows"),
        }
    }

    fn definition_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.toml"))
    }

    fn deserialize_definition(content: &str, path: &Path) -> Result<WorkflowResponse, String> {
        let mut definition = toml::from_str::<WorkflowResponse>(content).map_err(|error| {
            format!(
                "Failed to deserialize workflow definition '{}': {error}",
                path.display()
            )
        })?;
        definition.definition = canonicalize_workflow_definition(definition.definition);
        Ok(definition)
    }

    pub(crate) fn load(&self, id: &str) -> Result<Option<WorkflowResponse>, String> {
        let path = self.definition_path(id);
        match std::fs::read_to_string(&path) {
            Ok(content) => Self::deserialize_definition(&content, &path).map(Some),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!(
                "Failed to read workflow definition '{}': {error}",
                path.display()
            )),
        }
    }

    pub(crate) fn list(&self) -> Result<Vec<WorkflowResponse>, String> {
        if !self.dir.exists() {
            return Ok(Vec::new());
        }

        let mut definitions = Vec::new();
        let entries = std::fs::read_dir(&self.dir)
            .map_err(|error| format!("Failed to read workflow definitions directory: {error}"))?;

        for entry in entries {
            let entry = entry.map_err(|error| {
                format!("Failed to read one workflow definition directory entry: {error}")
            })?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
                continue;
            };
            if !extension.eq_ignore_ascii_case("toml") {
                continue;
            }

            let content = std::fs::read_to_string(&path).map_err(|error| {
                format!(
                    "Failed to read workflow definition '{}': {error}",
                    path.display()
                )
            })?;
            let definition = Self::deserialize_definition(&content, &path)?;
            definitions.push(definition);
        }

        definitions.sort_by(|left, right| left.definition.id.cmp(&right.definition.id));
        Ok(definitions)
    }

    pub(crate) fn persist(&self, definition: &WorkflowResponse) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|error| {
            format!(
                "Failed to create workflow definitions directory '{}': {error}",
                self.dir.display()
            )
        })?;

        let payload = toml::to_string_pretty(definition).map_err(|error| {
            format!(
                "Failed to serialize workflow definition '{}': {error}",
                definition.definition.id
            )
        })?;
        let path = self.definition_path(&definition.definition.id);
        let mut temp_file = tempfile::Builder::new()
            .prefix(&definition.definition.id)
            .suffix(".toml.tmp")
            .tempfile_in(&self.dir)
            .map_err(|error| {
                format!(
                    "Failed to create temporary workflow definition near '{}': {error}",
                    path.display()
                )
            })?;

        temp_file.write_all(payload.as_bytes()).map_err(|error| {
            format!(
                "Failed to write temporary workflow definition '{}': {error}",
                path.display()
            )
        })?;
        temp_file.flush().map_err(|error| {
            format!(
                "Failed to flush temporary workflow definition '{}': {error}",
                path.display()
            )
        })?;

        temp_file.persist(&path).map_err(|error| {
            format!(
                "Failed to replace workflow definition '{}': {error}",
                path.display()
            )
        })?;

        let persisted = std::fs::read_to_string(&path).map_err(|error| {
            format!(
                "Failed to read back workflow definition '{}': {error}",
                path.display()
            )
        })?;
        let reloaded = Self::deserialize_definition(&persisted, &path).map_err(|error| {
            format!(
                "Persisted workflow definition '{}' failed to round-trip: {error}",
                path.display()
            )
        })?;

        if reloaded != *definition {
            return Err(format!(
                "Persisted workflow definition '{}' did not round-trip cleanly",
                path.display()
            ));
        }

        Ok(())
    }

    pub(crate) fn delete(&self, id: &str) -> Result<bool, String> {
        let path = self.definition_path(id);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(format!(
                "Failed to remove workflow definition '{}': {error}",
                path.display()
            )),
        }
    }
}

pub(crate) fn canonicalize_workflow_definition(
    mut definition: WorkflowV2Definition,
) -> WorkflowV2Definition {
    definition.input = normalize_contract(definition.input);
    definition.output = normalize_contract(definition.output);
    definition
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_workflow_definition, WorkflowDefinitionStore};
    use crate::types::{WorkflowOrigin, WorkflowResponse};
    use openfang_types::workflow::WorkflowV2Definition;
    use serde_json::json;
    use tempfile::tempdir;

    fn workflow_resource() -> WorkflowResponse {
        let definition: WorkflowV2Definition = serde_json::from_value(json!({
            "id": "persisted-sdlc",
            "name": "Persisted SDLC",
            "version": "1.0.0",
            "description": "Round-trip regression fixture",
            "enabled": true,
            "tags": ["sdlc", "planning"],
            "input": {
                "kind": "text"
            },
            "output": {
                "kind": "object",
                "required": ["prd"],
                "open": false,
                "fields": {
                    "prd": {
                        "kind": "artifact_ref",
                        "artifact_type": "prd"
                    }
                }
            },
            "defaults": {
                "timeout_secs": 120,
                "error_mode": "fail"
            },
            "steps": [{
                "id": "noop-step",
                "name": "Noop Step",
                "kind": "noop",
                "save_as": "prd",
                "flow": {
                    "mode": "sequential"
                }
            }],
            "outputs": {
                "prd": "{{ vars.prd }}"
            }
        }))
        .expect("workflow fixture should deserialize");

        WorkflowResponse {
            definition: canonicalize_workflow_definition(definition),
            origin: WorkflowOrigin::user(),
            forked_from: None,
            created_at: "2026-03-24T00:00:00Z".to_owned(),
            updated_at: "2026-03-24T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn persist_should_round_trip_full_workflow_resource_shape() {
        let dir = tempdir().expect("temp dir");
        let store = WorkflowDefinitionStore::new(dir.path());
        let expected = workflow_resource();

        store.persist(&expected).expect("persist");
        let loaded = store.load(&expected.definition.id).expect("load");

        assert_eq!(loaded, Some(expected));
    }

    #[cfg(unix)]
    #[test]
    fn persist_should_preserve_existing_definition_when_replace_fails() {
        let dir = tempdir().expect("temp dir");
        let store = WorkflowDefinitionStore::new(dir.path());
        let original = workflow_resource();
        store
            .persist(&original)
            .expect("original persist should succeed");

        let mut updated = original.clone();
        updated.definition.description = "Updated description".to_string();

        use std::os::unix::fs::PermissionsExt;

        let permissions = std::fs::Permissions::from_mode(0o555);
        std::fs::set_permissions(dir.path().join("workflows"), permissions)
            .expect("workflow directory should become read-only");

        let error = store
            .persist(&updated)
            .expect_err("persist should fail when the directory is not writable");
        assert!(
            error.contains("temporary workflow definition")
                || error.contains("Failed to create temporary workflow definition")
        );

        let loaded = store
            .load(&original.definition.id)
            .expect("original definition should still load")
            .expect("original definition should remain on disk");
        assert_eq!(loaded, original);
    }

    #[test]
    fn canonicalize_should_normalize_contract_aliases() {
        let resource = workflow_resource();
        assert_eq!(resource.definition.input.kind.as_str(), "string");
    }
}
