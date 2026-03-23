//! Tool output truncation helpers.

use serde_json::{Map, Value};

use crate::ToolContent;

const DEFAULT_MAX_BYTES: usize = 100_000;
const DEFAULT_WARN_BYTES: usize = 50_000;
const STRING_MARKER: &str = "[truncated]";
const ARRAY_NOTICE_KEY: &str = "_truncated";
const ARRAY_NOTICE_MESSAGE_KEY: &str = "_message";
const OBJECT_REMOVED_KEYS_KEY: &str = "_removed_keys";
const OBJECT_NOTICE_KEY: &str = "_truncated";

/// Configures tool-output truncation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TruncationConfig {
    /// Hard byte limit for one output payload.
    pub max_bytes: usize,
    /// Warning threshold for large-but-untruncated payloads.
    pub warn_bytes: usize,
    /// Whether truncation is enabled.
    pub enabled: bool,
}

impl Default for TruncationConfig {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_BYTES,
            warn_bytes: DEFAULT_WARN_BYTES,
            enabled: false,
        }
    }
}

/// Outcome from truncating one tool-output fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TruncationResult {
    /// Final content after applying truncation rules.
    pub content: ToolContent,
    /// Whether the payload was truncated.
    pub was_truncated: bool,
    /// Original payload size in bytes.
    pub original_size: usize,
    /// Final payload size in bytes.
    pub final_size: usize,
    /// Advisory warning when the payload exceeds the configured warning size.
    pub warning: Option<String>,
}

impl TruncationResult {
    fn passthrough(content: ToolContent, config: TruncationConfig) -> Self {
        let original_size = content_size(&content);
        Self {
            final_size: original_size,
            warning: build_warning(original_size, config.warn_bytes),
            content,
            was_truncated: false,
            original_size,
        }
    }
}

/// Truncates one tool-output fragment when it exceeds the configured budget.
#[must_use]
pub fn truncate_tool_output(content: &ToolContent, config: TruncationConfig) -> TruncationResult {
    if !config.enabled {
        return TruncationResult::passthrough(content.clone(), config);
    }

    let original_size = content_size(content);
    if original_size <= config.max_bytes {
        return TruncationResult {
            content: content.clone(),
            was_truncated: false,
            original_size,
            final_size: original_size,
            warning: build_warning(original_size, config.warn_bytes),
        };
    }

    match content {
        ToolContent::Text { text } => {
            let truncated = truncate_string(text, config.max_bytes);
            TruncationResult {
                final_size: truncated.len(),
                warning: build_warning(original_size, config.warn_bytes),
                content: ToolContent::text(truncated),
                was_truncated: true,
                original_size,
            }
        }
        ToolContent::Image { .. } => TruncationResult::passthrough(content.clone(), config),
        ToolContent::Json { value } => {
            let truncated = truncate_json_value(value, config.max_bytes);
            let final_size = json_size(&truncated);
            TruncationResult {
                content: ToolContent::json(truncated),
                was_truncated: final_size < original_size,
                original_size,
                final_size,
                warning: build_warning(original_size, config.warn_bytes),
            }
        }
    }
}

fn build_warning(size: usize, warn_bytes: usize) -> Option<String> {
    if size > warn_bytes {
        Some(format!(
            "tool output is large ({size} bytes); consider reducing output volume"
        ))
    } else {
        None
    }
}

fn content_size(content: &ToolContent) -> usize {
    match content {
        ToolContent::Text { text } => text.len(),
        ToolContent::Image { data, .. } => data.len(),
        ToolContent::Json { value } => json_size(value),
    }
}

fn json_size(value: &Value) -> usize {
    serde_json::to_vec(value)
        .map(|encoded| encoded.len())
        .unwrap_or_default()
}

fn truncate_json_value(value: &Value, max_bytes: usize) -> Value {
    match value {
        Value::Array(items) => truncate_array(items, max_bytes),
        Value::Object(map) => truncate_object(map, max_bytes),
        Value::String(text) => Value::String(truncate_string(text, max_bytes)),
        other => {
            if json_size(other) <= max_bytes {
                other.clone()
            } else {
                Value::String(truncate_string(&other.to_string(), max_bytes))
            }
        }
    }
}

fn truncate_string(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }

    if max_bytes <= STRING_MARKER.len() {
        return STRING_MARKER.chars().take(max_bytes).collect::<String>();
    }

    let budget = max_bytes - STRING_MARKER.len();
    let mut end = budget.min(value.len());
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }

    let mut truncated = String::with_capacity(end + STRING_MARKER.len());
    truncated.push_str(&value[..end]);
    truncated.push_str(STRING_MARKER);
    truncated
}

fn truncate_array(items: &[Value], max_bytes: usize) -> Value {
    if json_size(&Value::Array(items.to_vec())) <= max_bytes {
        return Value::Array(items.to_vec());
    }

    let mut low = 0usize;
    let mut high = items.len();
    let mut best = 0usize;

    while low <= high {
        let mid = low + (high - low) / 2;
        let candidate = Value::Array(items[..mid].to_vec());
        if json_size(&candidate) <= max_bytes {
            best = mid;
            low = mid.saturating_add(1);
        } else {
            if mid == 0 {
                break;
            }
            high = mid - 1;
        }
    }

    let removed = items.len().saturating_sub(best);
    let mut truncated = items[..best].to_vec();
    if removed > 0 {
        truncated.push(Value::Object(Map::from_iter([
            (ARRAY_NOTICE_KEY.to_owned(), Value::Bool(true)),
            (
                ARRAY_NOTICE_MESSAGE_KEY.to_owned(),
                Value::String(format!(
                    "{removed} trailing array item(s) removed to fit byte limits"
                )),
            ),
        ])));
    }

    while json_size(&Value::Array(truncated.clone())) > max_bytes && !truncated.is_empty() {
        let _ = truncated.pop();
    }

    Value::Array(truncated)
}

fn truncate_object(source: &Map<String, Value>, max_bytes: usize) -> Value {
    let mut object = source.clone();
    if json_size(&Value::Object(object.clone())) <= max_bytes {
        return Value::Object(object);
    }

    let mut string_keys = object
        .iter()
        .filter_map(|(key, value)| value.as_str().map(|text| (key.clone(), text.len())))
        .collect::<Vec<_>>();
    string_keys.sort_by(|left, right| right.1.cmp(&left.1));

    for (key, _) in &string_keys {
        if json_size(&Value::Object(object.clone())) <= max_bytes {
            break;
        }
        let Some(Value::String(text)) = object.get(key).cloned() else {
            continue;
        };

        let current_size = json_size(&Value::Object(object.clone()));
        let overflow = current_size.saturating_sub(max_bytes);
        let target_len = text
            .len()
            .saturating_sub(overflow.saturating_add(STRING_MARKER.len()))
            .max(16);
        object.insert(
            key.clone(),
            Value::String(truncate_string(&text, target_len)),
        );
    }

    if json_size(&Value::Object(object.clone())) <= max_bytes {
        return Value::Object(object);
    }

    let mut removable_keys = object.keys().cloned().collect::<Vec<_>>();
    removable_keys.sort();
    let mut removed_keys = Vec::new();
    while json_size(&Value::Object(object.clone())) > max_bytes && !removable_keys.is_empty() {
        let key = removable_keys.pop().expect("checked non-empty");
        if object.remove(&key).is_some() {
            removed_keys.push(key);
        }
    }

    if !removed_keys.is_empty() {
        object.insert(
            OBJECT_REMOVED_KEYS_KEY.to_owned(),
            Value::Array(removed_keys.into_iter().map(Value::String).collect()),
        );
        object.insert(OBJECT_NOTICE_KEY.to_owned(), Value::Bool(true));
    }

    while json_size(&Value::Object(object.clone())) > max_bytes && !object.is_empty() {
        let Some(key) = object.keys().next().cloned() else {
            break;
        };
        object.remove(&key);
    }

    Value::Object(object)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{truncate_tool_output, TruncationConfig};
    use crate::ToolContent;

    #[test]
    fn string_truncation_should_respect_utf8_boundaries() {
        let text = "áéíóú".repeat(32);
        let result = truncate_tool_output(
            &ToolContent::text(text),
            TruncationConfig {
                max_bytes: 31,
                warn_bytes: 10,
                enabled: true,
            },
        );

        let ToolContent::Text { text: truncated } = result.content else {
            panic!("expected text content");
        };
        assert_eq!(truncated.is_char_boundary(truncated.len()), true);
        assert_eq!(truncated.ends_with("[truncated]"), true);
        assert_eq!(result.was_truncated, true);
    }

    #[test]
    fn array_truncation_should_remove_items_from_the_end() {
        let result = truncate_tool_output(
            &ToolContent::json(json!(["one", "two", "three", "four", "five"])),
            TruncationConfig {
                max_bytes: 24,
                warn_bytes: 10,
                enabled: true,
            },
        );

        let ToolContent::Json { value } = result.content else {
            panic!("expected json content");
        };
        let array = value.as_array().expect("json output should stay an array");
        assert_eq!(array[0], json!("one"));
        assert_eq!(array.len() < 5, true);
        assert_eq!(
            array.last().is_some_and(|value| {
                value == "three"
                    || value
                        == &json!({
                            "_truncated": true,
                            "_message": "3 trailing array item(s) removed to fit byte limits"
                        })
            }),
            true,
        );
    }

    #[test]
    fn object_truncation_should_shrink_strings_before_dropping_keys() {
        let result = truncate_tool_output(
            &ToolContent::json(json!({
                "summary": "a".repeat(200),
                "details": "b".repeat(200),
                "tail": "c".repeat(200),
            })),
            TruncationConfig {
                max_bytes: 180,
                warn_bytes: 10,
                enabled: true,
            },
        );

        let ToolContent::Json { value } = result.content else {
            panic!("expected json content");
        };
        let object = value
            .as_object()
            .expect("json output should stay an object");
        assert_eq!(object.contains_key("summary"), true);
        assert_eq!(
            object.values().any(|value| value
                .as_str()
                .is_some_and(|text| text.contains("[truncated]"))),
            true,
        );
    }

    #[test]
    fn disabled_config_should_passthrough_unchanged_content() {
        let content = ToolContent::json(json!({
            "message": "unchanged",
        }));
        let result = truncate_tool_output(&content, TruncationConfig::default());

        assert_eq!(result.content, content);
        assert_eq!(result.was_truncated, false);
    }
}
