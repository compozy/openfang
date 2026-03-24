//! Canonical file-backed storage for public agent definitions.

use crate::types::AgentResponse;
use openfang_agent_definition::stage4_normalize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct AgentDefinitionStore {
    dir: PathBuf,
}

impl AgentDefinitionStore {
    pub(crate) fn new(home_dir: &Path) -> Self {
        Self {
            dir: home_dir.join("agents"),
        }
    }

    fn definition_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.toml"))
    }

    fn temp_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.toml.tmp"))
    }

    fn deserialize_definition(content: &str, path: &Path) -> Result<AgentResponse, String> {
        let mut definition = toml::from_str::<AgentResponse>(content).map_err(|error| {
            format!(
                "Failed to deserialize agent definition '{}': {error}",
                path.display()
            )
        })?;
        definition.definition = stage4_normalize(definition.definition);
        Ok(definition)
    }

    pub(crate) fn load(&self, id: &str) -> Result<Option<AgentResponse>, String> {
        let path = self.definition_path(id);
        match std::fs::read_to_string(&path) {
            Ok(content) => Self::deserialize_definition(&content, &path).map(Some),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!(
                "Failed to read agent definition '{}': {error}",
                path.display()
            )),
        }
    }

    pub(crate) fn list(&self) -> Result<Vec<AgentResponse>, String> {
        if !self.dir.exists() {
            return Ok(Vec::new());
        }

        let mut definitions = Vec::new();
        let entries = std::fs::read_dir(&self.dir)
            .map_err(|error| format!("Failed to read agent definitions directory: {error}"))?;

        for entry in entries {
            let entry = entry.map_err(|error| {
                format!("Failed to read one agent definition directory entry: {error}")
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
                    "Failed to read agent definition '{}': {error}",
                    path.display()
                )
            })?;
            let definition = Self::deserialize_definition(&content, &path)?;
            definitions.push(definition);
        }

        definitions.sort_by(|left, right| left.definition.id.cmp(&right.definition.id));
        Ok(definitions)
    }

    pub(crate) fn persist(&self, definition: &AgentResponse) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|error| {
            format!(
                "Failed to create agent definitions directory '{}': {error}",
                self.dir.display()
            )
        })?;

        let payload = toml::to_string_pretty(definition).map_err(|error| {
            format!(
                "Failed to serialize agent definition '{}': {error}",
                definition.definition.id
            )
        })?;
        let path = self.definition_path(&definition.definition.id);
        let tmp_path = self.temp_path(&definition.definition.id);

        std::fs::write(&tmp_path, payload.as_bytes()).map_err(|error| {
            format!(
                "Failed to write temporary agent definition '{}': {error}",
                tmp_path.display()
            )
        })?;

        if let Err(error) = std::fs::rename(&tmp_path, &path) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(format!(
                "Failed to replace agent definition '{}': {error}",
                path.display()
            ));
        }

        let persisted = std::fs::read_to_string(&path).map_err(|error| {
            format!(
                "Failed to read back agent definition '{}': {error}",
                path.display()
            )
        })?;
        let reloaded = Self::deserialize_definition(&persisted, &path).map_err(|error| {
            format!(
                "Persisted agent definition '{}' failed to round-trip: {error}",
                path.display()
            )
        })?;

        if reloaded != *definition {
            return Err(format!(
                "Persisted agent definition '{}' did not round-trip cleanly",
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
                "Failed to remove agent definition '{}': {error}",
                path.display()
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AgentDefinitionStore;
    use crate::types::{AgentOrigin, AgentResponse};
    use openfang_agent_definition::{
        stage4_normalize, AgentDefinition, CapabilitiesBlock, PromptBlock, ProviderBlock,
        RuntimeBlock,
    };
    use tempfile::tempdir;

    fn normalized_response() -> AgentResponse {
        AgentResponse {
            definition: stage4_normalize(AgentDefinition {
                id: "persisted-prd-writer".to_owned(),
                name: "Persisted PRD Writer".to_owned(),
                version: "1.0.0".to_owned(),
                description: "Round-trip regression fixture".to_owned(),
                enabled: None,
                group: Some("product".to_owned()),
                tags: vec!["writing".to_owned(), "prd".to_owned()],
                provider: ProviderBlock {
                    driver: "codex".to_owned(),
                    model: "gpt-5".to_owned(),
                    ..ProviderBlock::default()
                },
                prompt: PromptBlock::default(),
                capabilities: CapabilitiesBlock::default(),
                runtime: RuntimeBlock::default(),
                input: None,
                output: None,
            }),
            origin: AgentOrigin::user(),
            forked_from: None,
            created_at: "2026-03-24T00:00:00Z".to_owned(),
            updated_at: "2026-03-24T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn persist_should_round_trip_normalized_codex_definitions() {
        let dir = tempdir().expect("temp dir");
        let store = AgentDefinitionStore::new(dir.path());
        let expected = normalized_response();

        store.persist(&expected).expect("persist");
        let loaded = store.load(&expected.definition.id).expect("load");

        assert!(
            loaded == Some(expected.clone()),
            "expected persisted definition to round-trip cleanly\nexpected: {expected:#?}\nactual: {loaded:#?}"
        );
    }
}
