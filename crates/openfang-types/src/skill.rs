//! Shared skill resource types.

use serde::{Deserialize, Serialize};

/// Summary payload returned by skill list endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct SkillSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: String,
}

/// Full skill payload returned by skill detail endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct SkillDetail {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn skill_summary_serialization_should_match_expected_shape() {
        let summary = SkillSummary {
            id: "writing".to_string(),
            name: "Writing".to_string(),
            description: "Skill for structured document authoring".to_string(),
            source: "/tmp/skills/writing/skill.toml".to_string(),
        };

        let serialized = serde_json::to_value(summary).expect("skill summary should serialize");

        assert_eq!(
            serialized,
            json!({
                "id": "writing",
                "name": "Writing",
                "description": "Skill for structured document authoring",
                "source": "/tmp/skills/writing/skill.toml",
            })
        );
    }

    #[test]
    fn skill_detail_serialization_should_include_timestamps() {
        let detail = SkillDetail {
            id: "writing".to_string(),
            name: "Writing".to_string(),
            description: "Skill for structured document authoring".to_string(),
            source: "/tmp/skills/writing/skill.toml".to_string(),
            created_at: "2026-03-21T12:00:00Z".to_string(),
            updated_at: "2026-03-21T14:00:00Z".to_string(),
        };

        let serialized = serde_json::to_value(detail).expect("skill detail should serialize");

        assert_eq!(
            serialized,
            json!({
                "id": "writing",
                "name": "Writing",
                "description": "Skill for structured document authoring",
                "source": "/tmp/skills/writing/skill.toml",
                "created_at": "2026-03-21T12:00:00Z",
                "updated_at": "2026-03-21T14:00:00Z",
            })
        );
    }
}
