//! Nested tool-call tracking and parent result merging.

use std::collections::HashMap;

use serde_json::{Value, json};

use crate::parser::ClaudeToolResultEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum NestedToolStatus {
    Running,
    Completed,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
struct NestedToolInfo {
    id: String,
    tool_name: String,
    status: NestedToolStatus,
    input: Option<Value>,
    output: Option<Value>,
    error: Option<Value>,
}

#[derive(Debug, Default)]
pub struct NestedToolTracker {
    stores: HashMap<String, Vec<NestedToolInfo>>,
}

impl NestedToolTracker {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_start(
        &mut self,
        parent_tool_call_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        input: Value,
    ) {
        self.upsert(
            parent_tool_call_id,
            NestedToolInfo {
                id: tool_call_id.to_owned(),
                tool_name: tool_name.to_owned(),
                status: NestedToolStatus::Running,
                input: Some(input),
                output: None,
                error: None,
            },
        );
    }

    pub fn register_result(&mut self, parent_tool_call_id: &str, event: &ClaudeToolResultEvent) {
        let status = if event.is_error {
            NestedToolStatus::Error
        } else {
            NestedToolStatus::Completed
        };
        self.upsert(
            parent_tool_call_id,
            NestedToolInfo {
                id: event.tool_call_id.clone(),
                tool_name: event.tool_name.clone(),
                status,
                input: None,
                output: (!event.is_error).then(|| event.result_json.clone()),
                error: event.is_error.then(|| event.result_json.clone()),
            },
        );
    }

    pub fn merge_into_parent_result(&mut self, parent_tool_call_id: &str, result: Value) -> Value {
        let nested = self.stores.remove(parent_tool_call_id).unwrap_or_default();
        if nested.is_empty() {
            return result;
        }

        let nested_records = nested
            .into_iter()
            .map(|call| {
                json!({
                    "id": call.id,
                    "tool": call.tool_name,
                    "state": match call.status {
                        NestedToolStatus::Running => "running",
                        NestedToolStatus::Completed => "completed",
                        NestedToolStatus::Error => "error",
                    },
                    "input": call.input,
                    "output": call.output,
                    "error": call.error,
                })
            })
            .collect::<Vec<_>>();

        match result {
            Value::Object(mut object) => {
                object.insert("toolCalls".to_owned(), Value::Array(nested_records));
                Value::Object(object)
            }
            other => json!({
                "result": other,
                "toolCalls": nested_records,
            }),
        }
    }

    fn upsert(&mut self, parent_tool_call_id: &str, next: NestedToolInfo) {
        let store = self
            .stores
            .entry(parent_tool_call_id.to_owned())
            .or_default();

        if let Some(existing) = store.iter_mut().find(|entry| entry.id == next.id) {
            if next.status > existing.status {
                existing.status = next.status;
            }
            if next.input.is_some() {
                existing.input = next.input;
            }
            if next.output.is_some() {
                existing.output = next.output;
            }
            if next.error.is_some() {
                existing.error = next.error;
            }
            if existing.tool_name == "unknown" {
                existing.tool_name = next.tool_name;
            }
            return;
        }

        store.push(next);
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::NestedToolTracker;
    use crate::parser::ClaudeToolResultEvent;
    use arky_protocol::ToolContent;

    #[test]
    fn nested_tool_tracker_should_merge_child_results_into_parent_payload() {
        let mut tracker = NestedToolTracker::new();
        tracker.register_start("parent-1", "child-1", "search", json!({ "q": "docs" }));
        tracker.register_result(
            "parent-1",
            &ClaudeToolResultEvent {
                tool_call_id: "child-1".to_owned(),
                tool_name: "search".to_owned(),
                content: vec![ToolContent::text("done")],
                result_json: json!("done"),
                is_error: false,
                parent_tool_call_id: Some("parent-1".to_owned()),
            },
        );

        let merged = tracker.merge_into_parent_result("parent-1", json!({ "ok": true }));

        assert_eq!(
            merged["toolCalls"][0]["id"],
            Value::String("child-1".to_owned())
        );
        assert_eq!(
            merged["toolCalls"][0]["state"],
            Value::String("completed".to_owned())
        );
    }
}
