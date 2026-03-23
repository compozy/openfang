//! Shared contract types for agent and workflow definition I/O.
//!
//! The Compozy definition pipeline uses this module as the single source of
//! truth for lightweight input/output contracts. The authoring surface remains
//! intentionally smaller than full JSON Schema so the compile pipeline can
//! validate and normalize definitions without provider boot, network calls, or
//! template execution.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Top-level schema type for lightweight definition contracts.
pub type ContractSchema = ContractNode;

/// Canonical kind for a contract node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractKind {
    /// UTF-8 string value.
    String,
    /// Signed integer number.
    Integer,
    /// Arbitrary JSON number.
    Number,
    /// Boolean value.
    Boolean,
    /// Object with named fields.
    Object,
    /// Array of items.
    Array,
    /// Arbitrary JSON value.
    Any,
    /// Reference to an artifact resource.
    ArtifactRef,
    /// Reference to a document resource.
    DocRef,
    /// Reference to an issue resource.
    IssueRef,
    /// Reference to a task resource.
    TaskRef,
    /// Collection of task references.
    TaskList,
    /// Reference to a run resource.
    RunRef,
}

impl ContractKind {
    /// Returns the canonical serialized name for this kind.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Object => "object",
            Self::Array => "array",
            Self::Any => "any",
            Self::ArtifactRef => "artifact_ref",
            Self::DocRef => "doc_ref",
            Self::IssueRef => "issue_ref",
            Self::TaskRef => "task_ref",
            Self::TaskList => "task_list",
            Self::RunRef => "run_ref",
        }
    }
}

impl fmt::Display for ContractKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ContractKind {
    type Err = ContractValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "string" => Ok(Self::String),
            "integer" => Ok(Self::Integer),
            "number" => Ok(Self::Number),
            "boolean" => Ok(Self::Boolean),
            "object" => Ok(Self::Object),
            "array" => Ok(Self::Array),
            "any" => Ok(Self::Any),
            "artifact_ref" => Ok(Self::ArtifactRef),
            "doc_ref" => Ok(Self::DocRef),
            "issue_ref" => Ok(Self::IssueRef),
            "task_ref" => Ok(Self::TaskRef),
            "task_list" => Ok(Self::TaskList),
            "run_ref" => Ok(Self::RunRef),
            _ => Err(ContractValidationError::UnknownKind {
                kind: value.to_owned(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContractKindAlias {
    Text,
    Json,
}

impl ContractKindAlias {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "text" => Some(Self::Text),
            "json" => Some(Self::Json),
            _ => None,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
        }
    }

    const fn canonical_kind(self) -> ContractKind {
        match self {
            Self::Text => ContractKind::String,
            Self::Json => ContractKind::Any,
        }
    }
}

/// Machine-readable validation issue for a contract node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractValidationIssue {
    path: String,
    message: String,
}

impl ContractValidationIssue {
    /// Creates a validation issue.
    #[must_use]
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }

    /// Returns the path that failed validation.
    #[must_use]
    pub const fn path(&self) -> &str {
        self.path.as_str()
    }

    /// Returns the human-readable validation message.
    #[must_use]
    pub const fn message(&self) -> &str {
        self.message.as_str()
    }

    fn from_error(path: impl Into<String>, error: ContractValidationError) -> Self {
        Self::new(path, error.to_string())
    }
}

/// Structured validation error variants for contract normalization and checks.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContractValidationError {
    /// The supplied kind is neither canonical nor a supported alias.
    #[error("unknown contract kind `{kind}`")]
    UnknownKind { kind: String },

    /// A field was used on an incompatible kind.
    #[error("`{field}` is only allowed on {allowed_on} contracts, not `{kind}`")]
    MisplacedField {
        /// Field name that was used incorrectly.
        field: &'static str,
        /// Actual kind found on the node.
        kind: ContractKind,
        /// Kind that is allowed to carry the field.
        allowed_on: &'static str,
    },

    /// A `required` entry referenced a key missing from `fields`.
    #[error("`required` references missing field `{field}`")]
    MissingRequiredFieldReference {
        /// Missing field name.
        field: String,
    },
}

/// Lightweight definition contract node used by agents and workflows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawContractNode", into = "RawContractNode")]
pub struct ContractNode {
    /// Canonical kind for the node.
    pub kind: ContractKind,
    /// Optional human-readable description.
    pub description: Option<String>,
    /// Whether the node accepts `null`.
    pub nullable: bool,
    /// Named child fields for object contracts.
    pub fields: Option<BTreeMap<String, ContractNode>>,
    /// Required field names for object contracts.
    pub required: Vec<String>,
    /// Whether an object contract allows keys outside `fields`.
    pub open: bool,
    /// Item schema for array contracts.
    pub items: Option<Box<ContractNode>>,
    /// Optional artifact subtype for `artifact_ref`.
    pub artifact_type: Option<String>,
    /// Optional document subtype for `doc_ref`.
    pub doc_type: Option<String>,
    kind_alias: Option<ContractKindAlias>,
    nullable_explicit: bool,
    required_explicit: bool,
    open_explicit: bool,
}

impl ContractNode {
    /// Returns the declared kind string, preserving supported aliases until the
    /// node is normalized.
    #[must_use]
    pub fn declared_kind(&self) -> &str {
        match self.kind_alias {
            Some(alias) => alias.as_str(),
            None => self.kind.as_str(),
        }
    }

    /// Returns a normalized copy of the node with canonical kinds and explicit
    /// structural defaults.
    #[must_use]
    pub fn normalize(mut self) -> Self {
        self.kind_alias = None;
        self.nullable_explicit = true;

        if self.kind == ContractKind::Object {
            self.open_explicit = true;
        }

        if let Some(fields) = self.fields.take() {
            self.fields = Some(
                fields
                    .into_iter()
                    .map(|(field, node)| (field, node.normalize()))
                    .collect(),
            );
        }

        if let Some(items) = self.items.take() {
            self.items = Some(Box::new(items.normalize()));
        }

        self
    }

    /// Validates the node and all descendants, collecting every issue in a
    /// single pass.
    #[must_use]
    pub fn validate(&self) -> Vec<ContractValidationIssue> {
        let mut issues = Vec::new();
        self.collect_validation_issues("", &mut issues);
        issues
    }

    fn collect_validation_issues(&self, path: &str, issues: &mut Vec<ContractValidationIssue>) {
        match self.kind {
            ContractKind::Object => {
                if self.items.is_some() {
                    issues.push(ContractValidationIssue::from_error(
                        child_path(path, "items"),
                        ContractValidationError::MisplacedField {
                            field: "items",
                            kind: self.kind,
                            allowed_on: "`array`",
                        },
                    ));
                }

                if self.artifact_type.is_some() {
                    issues.push(ContractValidationIssue::from_error(
                        child_path(path, "artifact_type"),
                        ContractValidationError::MisplacedField {
                            field: "artifact_type",
                            kind: self.kind,
                            allowed_on: "`artifact_ref`",
                        },
                    ));
                }

                if self.doc_type.is_some() {
                    issues.push(ContractValidationIssue::from_error(
                        child_path(path, "doc_type"),
                        ContractValidationError::MisplacedField {
                            field: "doc_type",
                            kind: self.kind,
                            allowed_on: "`doc_ref`",
                        },
                    ));
                }

                let fields = self.fields.as_ref();
                for (index, required_field) in self.required.iter().enumerate() {
                    let field_exists =
                        fields.is_some_and(|entries| entries.contains_key(required_field));
                    if !field_exists {
                        issues.push(ContractValidationIssue::from_error(
                            indexed_child_path(path, "required", index),
                            ContractValidationError::MissingRequiredFieldReference {
                                field: required_field.clone(),
                            },
                        ));
                    }
                }
            }
            ContractKind::Array => {
                self.push_object_only_field_issue(path, "fields", self.fields.is_some(), issues);
                self.push_object_only_field_issue(
                    path,
                    "required",
                    self.required_explicit || !self.required.is_empty(),
                    issues,
                );
                self.push_object_only_field_issue(path, "open", self.open_explicit, issues);
                self.push_semantic_metadata_issues(path, issues);
            }
            _ => {
                self.push_object_only_field_issue(path, "fields", self.fields.is_some(), issues);
                self.push_object_only_field_issue(
                    path,
                    "required",
                    self.required_explicit || !self.required.is_empty(),
                    issues,
                );
                self.push_object_only_field_issue(path, "open", self.open_explicit, issues);
                self.push_array_only_field_issue(path, issues);
                self.push_semantic_metadata_issues(path, issues);
            }
        }

        if let Some(fields) = &self.fields {
            for (field_name, field_node) in fields {
                field_node.collect_validation_issues(&field_path(path, field_name), issues);
            }
        }

        if let Some(items) = &self.items {
            items.collect_validation_issues(&child_path(path, "items"), issues);
        }
    }

    fn push_object_only_field_issue(
        &self,
        path: &str,
        field: &'static str,
        present: bool,
        issues: &mut Vec<ContractValidationIssue>,
    ) {
        if present {
            issues.push(ContractValidationIssue::from_error(
                child_path(path, field),
                ContractValidationError::MisplacedField {
                    field,
                    kind: self.kind,
                    allowed_on: "`object`",
                },
            ));
        }
    }

    fn push_array_only_field_issue(&self, path: &str, issues: &mut Vec<ContractValidationIssue>) {
        if self.items.is_some() {
            issues.push(ContractValidationIssue::from_error(
                child_path(path, "items"),
                ContractValidationError::MisplacedField {
                    field: "items",
                    kind: self.kind,
                    allowed_on: "`array`",
                },
            ));
        }
    }

    fn push_semantic_metadata_issues(&self, path: &str, issues: &mut Vec<ContractValidationIssue>) {
        if self.kind != ContractKind::ArtifactRef && self.artifact_type.is_some() {
            issues.push(ContractValidationIssue::from_error(
                child_path(path, "artifact_type"),
                ContractValidationError::MisplacedField {
                    field: "artifact_type",
                    kind: self.kind,
                    allowed_on: "`artifact_ref`",
                },
            ));
        }

        if self.kind != ContractKind::DocRef && self.doc_type.is_some() {
            issues.push(ContractValidationIssue::from_error(
                child_path(path, "doc_type"),
                ContractValidationError::MisplacedField {
                    field: "doc_type",
                    kind: self.kind,
                    allowed_on: "`doc_ref`",
                },
            ));
        }
    }
}

/// Normalizes a contract node into its canonical form.
#[must_use]
pub fn normalize(node: ContractNode) -> ContractNode {
    node.normalize()
}

/// Validates a contract node and returns every structural issue found.
#[must_use]
pub fn validate(node: &ContractNode) -> Vec<ContractValidationIssue> {
    node.validate()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContractNode {
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nullable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fields: Option<BTreeMap<String, ContractNode>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    required: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    open: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    items: Option<Box<ContractNode>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    artifact_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    doc_type: Option<String>,
}

impl TryFrom<RawContractNode> for ContractNode {
    type Error = ContractValidationError;

    fn try_from(raw: RawContractNode) -> Result<Self, Self::Error> {
        let RawContractNode {
            kind,
            description,
            nullable,
            fields,
            required,
            open,
            items,
            artifact_type,
            doc_type,
        } = raw;
        let kind_alias = ContractKindAlias::from_str(kind.as_str());
        let kind = match kind_alias {
            Some(alias) => alias.canonical_kind(),
            None => ContractKind::from_str(kind.as_str())?,
        };

        Ok(Self {
            kind,
            description,
            nullable: nullable.unwrap_or(false),
            fields,
            required: required.clone().unwrap_or_default(),
            open: open.unwrap_or(false),
            items,
            artifact_type,
            doc_type,
            kind_alias,
            nullable_explicit: nullable.is_some(),
            required_explicit: required.is_some(),
            open_explicit: open.is_some(),
        })
    }
}

impl From<ContractNode> for RawContractNode {
    fn from(node: ContractNode) -> Self {
        let kind = match node.kind_alias {
            Some(alias) => alias.as_str().to_owned(),
            None => node.kind.as_str().to_owned(),
        };

        Self {
            kind,
            description: node.description,
            nullable: node.nullable_explicit.then_some(node.nullable),
            fields: node.fields,
            required: (node.required_explicit || !node.required.is_empty())
                .then_some(node.required),
            open: node.open_explicit.then_some(node.open),
            items: node.items,
            artifact_type: node.artifact_type,
            doc_type: node.doc_type,
        }
    }
}

fn child_path(path: &str, child: &str) -> String {
    if path.is_empty() {
        child.to_owned()
    } else {
        format!("{path}.{child}")
    }
}

fn indexed_child_path(path: &str, child: &str, index: usize) -> String {
    if path.is_empty() {
        format!("{child}[{index}]")
    } else {
        format!("{path}.{child}[{index}]")
    }
}

fn field_path(path: &str, field_name: &str) -> String {
    if field_name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        child_path(path, &format!("fields.{field_name}"))
    } else {
        child_path(path, &format!("fields[{field_name:?}]"))
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize, validate, ContractKind, ContractNode, ContractValidationIssue};
    use pretty_assertions::assert_eq;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;

    fn parse_json_contract(value: Value) -> ContractNode {
        serde_json::from_value(value).expect("contract JSON should deserialize")
    }

    fn serialize_json_contract(node: &ContractNode) -> Value {
        serde_json::to_value(node).expect("contract node should serialize to JSON")
    }

    fn parse_toml_contract(value: &str) -> ContractNode {
        toml::from_str(value).expect("contract TOML should deserialize")
    }

    fn nested_object_contract() -> ContractNode {
        parse_json_contract(json!({
            "kind": "object",
            "fields": {
                "summary": {
                    "kind": "string"
                },
                "metadata": {
                    "kind": "object",
                    "open": true,
                    "fields": {
                        "approved": {
                            "kind": "boolean"
                        }
                    }
                }
            }
        }))
    }

    #[test]
    fn structural_kinds_should_deserialize_from_json() {
        let cases = [
            ("string", ContractKind::String),
            ("integer", ContractKind::Integer),
            ("number", ContractKind::Number),
            ("boolean", ContractKind::Boolean),
            ("object", ContractKind::Object),
            ("array", ContractKind::Array),
            ("any", ContractKind::Any),
        ];

        for (kind_name, expected_kind) in cases {
            let node = parse_json_contract(json!({ "kind": kind_name }));
            assert_eq!(node.kind, expected_kind);
            assert_eq!(node.declared_kind(), kind_name);
        }
    }

    #[test]
    fn semantic_kinds_should_deserialize_from_json() {
        let cases = [
            ("artifact_ref", ContractKind::ArtifactRef),
            ("doc_ref", ContractKind::DocRef),
            ("issue_ref", ContractKind::IssueRef),
            ("task_ref", ContractKind::TaskRef),
            ("task_list", ContractKind::TaskList),
            ("run_ref", ContractKind::RunRef),
        ];

        for (kind_name, expected_kind) in cases {
            let node = parse_json_contract(json!({ "kind": kind_name }));
            assert_eq!(node.kind, expected_kind);
            assert_eq!(node.declared_kind(), kind_name);
        }
    }

    #[test]
    fn text_alias_should_normalize_to_string() {
        let raw = parse_json_contract(json!({ "kind": "text" }));
        let normalized = normalize(raw.clone());

        assert_eq!(raw.kind, ContractKind::String);
        assert_eq!(raw.declared_kind(), "text");
        assert_eq!(normalized.kind, ContractKind::String);
        assert_eq!(normalized.declared_kind(), "string");
        assert_eq!(
            serialize_json_contract(&normalized),
            json!({
                "kind": "string",
                "nullable": false
            })
        );
    }

    #[test]
    fn json_alias_should_normalize_to_any() {
        let raw = parse_json_contract(json!({ "kind": "json" }));
        let normalized = normalize(raw.clone());

        assert_eq!(raw.kind, ContractKind::Any);
        assert_eq!(raw.declared_kind(), "json");
        assert_eq!(normalized.kind, ContractKind::Any);
        assert_eq!(normalized.declared_kind(), "any");
        assert_eq!(
            serialize_json_contract(&normalized),
            json!({
                "kind": "any",
                "nullable": false
            })
        );
    }

    #[test]
    fn object_without_explicit_open_should_default_to_false_after_normalization() {
        let raw = parse_json_contract(json!({ "kind": "object" }));
        let normalized = normalize(raw);

        assert_eq!(normalized.open, false);
        assert_eq!(
            serialize_json_contract(&normalized),
            json!({
                "kind": "object",
                "nullable": false,
                "open": false
            })
        );
    }

    #[test]
    fn string_node_with_fields_should_fail_validation() {
        let node = parse_json_contract(json!({
            "kind": "string",
            "fields": {
                "title": {
                    "kind": "string"
                }
            }
        }));

        let issues = validate(&node);

        assert_eq!(
            issues,
            vec![ContractValidationIssue::new(
                "fields",
                "`fields` is only allowed on `object` contracts, not `string`",
            )]
        );
    }

    #[test]
    fn array_without_items_should_pass_validation() {
        let node = parse_json_contract(json!({ "kind": "array" }));

        assert_eq!(validate(&node), Vec::<ContractValidationIssue>::new());
    }

    #[test]
    fn object_required_entry_should_reference_existing_field() {
        let node = parse_json_contract(json!({
            "kind": "object",
            "required": ["missing_key"],
            "fields": {
                "present_key": {
                    "kind": "string"
                }
            }
        }));

        let issues = validate(&node);

        assert_eq!(
            issues,
            vec![ContractValidationIssue::new(
                "required[0]",
                "`required` references missing field `missing_key`",
            )]
        );
    }

    #[test]
    fn artifact_ref_should_round_trip_with_artifact_type() {
        let node = parse_json_contract(json!({
            "kind": "artifact_ref",
            "artifact_type": "prd"
        }));

        assert_eq!(
            serialize_json_contract(&node),
            json!({
                "kind": "artifact_ref",
                "artifact_type": "prd"
            })
        );

        let round_trip: ContractNode =
            serde_json::from_value(serialize_json_contract(&node)).expect("round-trip should work");
        assert_eq!(round_trip, node);
    }

    #[test]
    fn nested_object_should_round_trip_without_data_loss() {
        let node = nested_object_contract();
        let round_trip: ContractNode =
            serde_json::from_value(serialize_json_contract(&node)).expect("round-trip should work");

        assert_eq!(round_trip, node);
    }

    #[test]
    fn sdlc_workflow_input_contract_should_round_trip_from_api_spec() {
        let payload = json!({
            "kind": "object",
            "required": ["issue_id"],
            "open": false,
            "fields": {
                "issue_id": {
                    "kind": "string"
                }
            }
        });
        let node = parse_json_contract(payload.clone());

        assert_eq!(validate(&node), Vec::<ContractValidationIssue>::new());
        assert_eq!(serialize_json_contract(&node), payload);
    }

    #[test]
    fn prd_writer_output_contract_should_round_trip_from_api_spec() {
        let payload = json!({
            "kind": "artifact_ref",
            "artifact_type": "prd"
        });
        let node = parse_json_contract(payload.clone());

        assert_eq!(validate(&node), Vec::<ContractValidationIssue>::new());
        assert_eq!(serialize_json_contract(&node), payload);
    }

    #[test]
    fn deeply_nested_contract_should_validate_and_normalize_defaults() {
        let node = parse_json_contract(json!({
            "kind": "object",
            "fields": {
                "items": {
                    "kind": "array",
                    "items": {
                        "kind": "object",
                        "fields": {
                            "id": {
                                "kind": "string"
                            }
                        }
                    }
                }
            }
        }));
        let normalized = normalize(node.clone());

        assert_eq!(validate(&node), Vec::<ContractValidationIssue>::new());
        assert_eq!(
            serialize_json_contract(&normalized),
            json!({
                "kind": "object",
                "nullable": false,
                "open": false,
                "fields": {
                    "items": {
                        "kind": "array",
                        "nullable": false,
                        "items": {
                            "kind": "object",
                            "nullable": false,
                            "open": false,
                            "fields": {
                                "id": {
                                    "kind": "string",
                                    "nullable": false
                                }
                            }
                        }
                    }
                }
            })
        );
    }

    #[test]
    fn validate_should_collect_multiple_issues_in_one_pass() {
        let node = parse_json_contract(json!({
            "kind": "array",
            "fields": {
                "title": {
                    "kind": "string"
                }
            },
            "artifact_type": "prd"
        }));

        let issues = validate(&node);

        assert_eq!(
            issues,
            vec![
                ContractValidationIssue::new(
                    "fields",
                    "`fields` is only allowed on `object` contracts, not `array`",
                ),
                ContractValidationIssue::new(
                    "artifact_type",
                    "`artifact_type` is only allowed on `artifact_ref` contracts, not `array`",
                ),
            ]
        );
    }

    #[test]
    fn toml_round_trip_should_match_json_round_trip() {
        let normalized = normalize(parse_json_contract(json!({
            "kind": "object",
            "fields": {
                "context": {
                    "kind": "object",
                    "open": true
                },
                "issue_id": {
                    "kind": "string"
                }
            },
            "required": ["issue_id"]
        })));

        let toml_document =
            toml::to_string(&normalized).expect("normalized contract should serialize to TOML");
        let from_toml = parse_toml_contract(&toml_document);
        let from_json: ContractNode = serde_json::from_value(serialize_json_contract(&normalized))
            .expect("normalized contract should round-trip through JSON");

        assert_eq!(from_toml, from_json);
    }

    #[test]
    fn contract_kind_should_not_accept_alias_variants_directly() {
        let text_error = serde_json::from_str::<ContractKind>("\"text\"");
        let json_error = serde_json::from_str::<ContractKind>("\"json\"");

        assert_eq!(text_error.is_err(), true);
        assert_eq!(json_error.is_err(), true);
    }

    #[test]
    fn contract_node_should_deserialize_from_toml() {
        let node = parse_toml_contract(
            r#"
kind = "object"
required = ["issue_id"]
open = false

[fields.issue_id]
kind = "string"
"#,
        );

        assert_eq!(
            node,
            parse_json_contract(json!({
                "kind": "object",
                "required": ["issue_id"],
                "open": false,
                "fields": {
                    "issue_id": {
                        "kind": "string"
                    }
                }
            }))
        );
    }

    #[test]
    fn validation_should_recurse_into_nested_fields() {
        let node = ContractNode {
            kind: ContractKind::Object,
            description: None,
            nullable: false,
            fields: Some(BTreeMap::from([(
                "payload".to_owned(),
                parse_json_contract(json!({
                    "kind": "string",
                    "fields": {
                        "title": {
                            "kind": "string"
                        }
                    }
                })),
            )])),
            required: Vec::new(),
            open: false,
            items: None,
            artifact_type: None,
            doc_type: None,
            kind_alias: None,
            nullable_explicit: false,
            required_explicit: false,
            open_explicit: true,
        };

        assert_eq!(
            validate(&node),
            vec![ContractValidationIssue::new(
                "fields.payload.fields",
                "`fields` is only allowed on `object` contracts, not `string`",
            )]
        );
    }
}
