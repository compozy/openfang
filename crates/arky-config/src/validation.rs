//! Rich schema validation helpers with typo suggestions.

use serde_json::Value;

use crate::ValidationIssue;

/// Minimal field schema used to produce rich validation issues.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RichValidationSchema {
    /// Fully qualified field path.
    pub field: String,
    /// Whether the field must be present.
    pub required: bool,
    /// Human-readable type description.
    pub expected_type: &'static str,
}

impl RichValidationSchema {
    /// Creates a new field schema.
    #[must_use]
    pub fn new(field: impl Into<String>, required: bool, expected_type: &'static str) -> Self {
        Self {
            field: field.into(),
            required,
            expected_type,
        }
    }
}

/// Validates a JSON object against a field schema and returns rich issues.
#[must_use]
pub fn validate_against_schema(
    value: &Value,
    schema: &[RichValidationSchema],
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let Some(object) = value.as_object() else {
        issues.push(ValidationIssue::new(
            "$",
            "expected a JSON object for configuration validation",
        ));
        return issues;
    };

    for field_schema in schema {
        match value_at_path(value, &field_schema.field) {
            None if field_schema.required => issues.push(ValidationIssue::new(
                field_schema.field.clone(),
                format!("is required and must be {}", field_schema.expected_type),
            )),
            Some(field_value)
                if !matches_expected_type(field_value, field_schema.expected_type) =>
            {
                issues.push(ValidationIssue::new(
                    field_schema.field.clone(),
                    format!("must be {}", field_schema.expected_type),
                ));
            }
            _ => {}
        }
    }

    collect_unknown_fields(object, "", schema, &mut issues);

    issues
}

fn matches_expected_type(value: &Value, expected_type: &str) -> bool {
    match expected_type {
        "string" => value.is_string(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        _ => true,
    }
}

fn value_at_path<'a>(value: &'a Value, field: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in field.split('.') {
        current = current.as_object()?.get(segment)?;
    }

    Some(current)
}

fn collect_unknown_fields(
    object: &serde_json::Map<String, Value>,
    prefix: &str,
    schema: &[RichValidationSchema],
    issues: &mut Vec<ValidationIssue>,
) {
    let candidates = schema
        .iter()
        .filter_map(|field| field_segment_for_prefix(field.field.as_str(), prefix))
        .collect::<Vec<_>>();

    for (key, value) in object {
        let field_path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };

        let Some(candidate) = field_segment_for_prefix(field_path.as_str(), prefix) else {
            continue;
        };

        if !candidates.contains(&candidate) {
            let mut issue = ValidationIssue::new(
                field_path.clone(),
                "is not recognized by the configuration schema",
            );
            if let Some(suggestion) = suggest_correction(candidate, candidates.as_slice()) {
                issue = issue.with_suggestion(format!("did you mean `{suggestion}`?"));
            }
            issues.push(issue);
            continue;
        }

        if let Some(child_object) = value.as_object() {
            collect_unknown_fields(child_object, field_path.as_str(), schema, issues);
        }
    }
}

fn field_segment_for_prefix<'a>(field: &'a str, prefix: &str) -> Option<&'a str> {
    if prefix.is_empty() {
        return field.split('.').next();
    }

    let remainder = field.strip_prefix(prefix)?.strip_prefix('.')?;
    remainder.split('.').next()
}

fn suggest_correction<'a>(field: &str, candidates: &'a [&'a str]) -> Option<&'a str> {
    let mut best: Option<(&str, usize)> = None;

    for candidate in candidates {
        let distance = levenshtein(field, candidate);
        if distance > 3 {
            continue;
        }

        if best.is_none_or(|(_, current_distance)| distance < current_distance) {
            best = Some((candidate, distance));
        }
    }

    best.map(|(candidate, _)| candidate)
}

fn levenshtein(left: &str, right: &str) -> usize {
    let left_chars = left.chars().collect::<Vec<_>>();
    let right_chars = right.chars().collect::<Vec<_>>();

    let mut previous = (0..=right_chars.len()).collect::<Vec<_>>();
    let mut current = vec![0usize; right_chars.len() + 1];

    for (left_index, left_char) in left_chars.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_char) in right_chars.iter().enumerate() {
            let substitution_cost = usize::from(left_char != right_char);
            current[right_index + 1] = (current[right_index] + 1)
                .min(previous[right_index + 1] + 1)
                .min(previous[right_index] + substitution_cost);
        }
        previous.clone_from(&current);
    }

    previous[right_chars.len()]
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{validate_against_schema, RichValidationSchema};

    #[test]
    fn validate_against_schema_should_report_field_level_errors() {
        let issues = validate_against_schema(
            &json!({
                "agent": {
                    "provider": 42
                }
            }),
            &[RichValidationSchema::new("agent.provider", true, "string")],
        );

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].field(), "agent.provider");
        assert_eq!(issues[0].message(), "must be string");
    }

    #[test]
    fn validate_against_schema_should_suggest_corrections_for_typos() {
        let issues = validate_against_schema(
            &json!({
                "provider": {
                    "modle": "gpt-4o"
                }
            }),
            &[
                RichValidationSchema::new("provider.model", false, "string"),
                RichValidationSchema::new("provider.kind", true, "string"),
            ],
        );

        let typo_issue = issues
            .iter()
            .find(|issue| issue.field() == "provider.modle")
            .expect("typo issue should be reported");

        assert_eq!(typo_issue.suggestion(), Some("did you mean `model`?"));
    }
}
