//! Canonical file-backed storage for public trigger definitions.

use crate::types::TriggerResponse;
use openfang_types::trigger::{TriggerMatch, TriggerTarget, TriggerV2Definition};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct TriggerDefinitionStore {
    dir: PathBuf,
}

impl TriggerDefinitionStore {
    pub(crate) fn new(home_dir: &Path) -> Self {
        Self {
            dir: home_dir.join("triggers"),
        }
    }

    fn definition_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.toml"))
    }

    fn deserialize_definition(content: &str, path: &Path) -> Result<TriggerResponse, String> {
        let mut definition = toml::from_str::<TriggerResponse>(content).map_err(|error| {
            format!(
                "Failed to deserialize trigger definition '{}': {error}",
                path.display()
            )
        })?;
        definition.definition = canonicalize_trigger_definition(definition.definition);
        Ok(definition)
    }

    pub(crate) fn load(&self, id: &str) -> Result<Option<TriggerResponse>, String> {
        let path = self.definition_path(id);
        match std::fs::read_to_string(&path) {
            Ok(content) => Self::deserialize_definition(&content, &path).map(Some),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!(
                "Failed to read trigger definition '{}': {error}",
                path.display()
            )),
        }
    }

    pub(crate) fn list(&self) -> Result<Vec<TriggerResponse>, String> {
        if !self.dir.exists() {
            return Ok(Vec::new());
        }

        let mut definitions = Vec::new();
        let entries = std::fs::read_dir(&self.dir)
            .map_err(|error| format!("Failed to read trigger definitions directory: {error}"))?;

        for entry in entries {
            let entry = entry.map_err(|error| {
                format!("Failed to read one trigger definition directory entry: {error}")
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
                    "Failed to read trigger definition '{}': {error}",
                    path.display()
                )
            })?;
            let definition = Self::deserialize_definition(&content, &path)?;
            definitions.push(definition);
        }

        definitions.sort_by(|left, right| left.definition.id.cmp(&right.definition.id));
        Ok(definitions)
    }

    pub(crate) fn persist(&self, definition: &TriggerResponse) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|error| {
            format!(
                "Failed to create trigger definitions directory '{}': {error}",
                self.dir.display()
            )
        })?;

        let payload = toml::to_string_pretty(definition).map_err(|error| {
            format!(
                "Failed to serialize trigger definition '{}': {error}",
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
                    "Failed to create temporary trigger definition near '{}': {error}",
                    path.display()
                )
            })?;

        temp_file.write_all(payload.as_bytes()).map_err(|error| {
            format!(
                "Failed to write temporary trigger definition '{}': {error}",
                path.display()
            )
        })?;
        temp_file.flush().map_err(|error| {
            format!(
                "Failed to flush temporary trigger definition '{}': {error}",
                path.display()
            )
        })?;

        temp_file.persist(&path).map_err(|error| {
            format!(
                "Failed to replace trigger definition '{}': {error}",
                path.display()
            )
        })?;

        let persisted = std::fs::read_to_string(&path).map_err(|error| {
            format!(
                "Failed to read back trigger definition '{}': {error}",
                path.display()
            )
        })?;
        let reloaded = Self::deserialize_definition(&persisted, &path).map_err(|error| {
            format!(
                "Persisted trigger definition '{}' failed to round-trip: {error}",
                path.display()
            )
        })?;

        if reloaded != *definition {
            return Err(format!(
                "Persisted trigger definition '{}' did not round-trip cleanly",
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
                "Failed to remove trigger definition '{}': {error}",
                path.display()
            )),
        }
    }
}

pub(crate) fn canonicalize_trigger_definition(
    mut definition: TriggerV2Definition,
) -> TriggerV2Definition {
    definition.id = definition.id.trim().to_string();
    definition.name = definition.name.trim().to_string();
    definition.description = definition.description.trim().to_string();
    definition.trigger_match = TriggerMatch {
        event: definition
            .trigger_match
            .event
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        source: definition
            .trigger_match
            .source
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        contains: definition
            .trigger_match
            .contains
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        filters: definition.trigger_match.filters,
    };
    definition.target = match definition.target {
        TriggerTarget::AgentMessage {
            agent,
            input,
            metadata,
        } => TriggerTarget::AgentMessage {
            agent: agent.trim().to_string(),
            input,
            metadata,
        },
        TriggerTarget::WorkflowStart { workflow, input } => TriggerTarget::WorkflowStart {
            workflow: workflow.trim().to_string(),
            input,
        },
        TriggerTarget::WorkflowSignal {
            signal,
            selector,
            payload,
        } => TriggerTarget::WorkflowSignal {
            signal: signal.trim().to_string(),
            selector: openfang_types::trigger::TriggerWorkflowSignalSelector {
                workflow_id: selector.workflow_id.trim().to_string(),
            },
            payload,
        },
    };
    definition
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_trigger_definition, TriggerDefinitionStore};
    use crate::types::{TriggerOrigin, TriggerResponse};
    use openfang_types::trigger::{
        TriggerInputItem, TriggerInputPayload, TriggerMatch, TriggerTarget, TriggerV2Definition,
    };
    use tempfile::tempdir;

    fn trigger_resource() -> TriggerResponse {
        TriggerResponse {
            definition: canonicalize_trigger_definition(TriggerV2Definition {
                id: " issue-created ".to_string(),
                name: " Issue Created ".to_string(),
                description: " Starts the workflow ".to_string(),
                enabled: true,
                max_fires: 0,
                cooldown_secs: 0,
                trigger_match: TriggerMatch {
                    event: Some(" issue.created ".to_string()),
                    source: Some(" api ".to_string()),
                    contains: None,
                    filters: std::collections::BTreeMap::new(),
                },
                target: TriggerTarget::AgentMessage {
                    agent: " reviewer ".to_string(),
                    input: TriggerInputPayload {
                        items: vec![TriggerInputItem {
                            item_type: "text".to_string(),
                            text: Some("hello".to_string()),
                        }],
                    },
                    metadata: None,
                },
            }),
            origin: TriggerOrigin::user(),
            forked_from: None,
            created_at: "2026-03-25T00:00:00Z".to_string(),
            updated_at: "2026-03-25T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn persist_should_round_trip_full_trigger_resource_shape() {
        let dir = tempdir().expect("temp dir");
        let store = TriggerDefinitionStore::new(dir.path());
        let expected = trigger_resource();

        store.persist(&expected).expect("persist");
        let loaded = store.load(&expected.definition.id).expect("load");

        assert_eq!(loaded, Some(expected));
    }

    #[cfg(unix)]
    #[test]
    fn persist_should_preserve_existing_definition_when_replace_fails() {
        let dir = tempdir().expect("temp dir");
        let store = TriggerDefinitionStore::new(dir.path());
        let original = trigger_resource();
        store.persist(&original).expect("persist original trigger");

        let mut updated = original.clone();
        updated.definition.description = "updated".to_string();

        use std::os::unix::fs::PermissionsExt;

        let permissions = std::fs::Permissions::from_mode(0o555);
        std::fs::set_permissions(dir.path().join("triggers"), permissions)
            .expect("trigger directory should become read-only");

        let error = store
            .persist(&updated)
            .expect_err("persist should fail when the directory is not writable");
        assert!(
            error.contains("temporary trigger definition")
                || error.contains("Failed to create temporary trigger definition")
        );

        let loaded = store
            .load(&original.definition.id)
            .expect("original trigger definition should still load")
            .expect("original trigger definition should remain on disk");
        assert_eq!(loaded, original);
    }
}
