//! Text and tool-state assembly for normalized provider events.

use std::collections::HashMap;

use arky_protocol::{
    ContentBlock, Message, MessageMetadata, Role, ToolCall, ToolContent, ToolResult, TurnId,
};
use serde_json::{Map, Value};

/// Tracks incremental assistant text assembly.
#[derive(Debug, Clone)]
pub struct TextAccumulator {
    message: Message,
    part_id: String,
}

impl TextAccumulator {
    /// Creates an empty assistant message accumulator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            message: Message::new(Role::Assistant, Vec::new()),
            part_id: TurnId::new().to_string(),
        }
    }

    /// Appends one text delta to the current assistant message.
    pub fn push_delta(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }

        if let Some(ContentBlock::Text { text }) = self.message.content.last_mut() {
            text.push_str(delta);
            return;
        }

        self.message
            .content
            .push(ContentBlock::text(delta.to_owned()));
    }

    /// Reconciles the assistant message to an authoritative snapshot.
    pub fn apply_snapshot(&mut self, snapshot: &str) {
        self.message.content.clear();
        if !snapshot.is_empty() {
            self.message
                .content
                .push(ContentBlock::text(snapshot.to_owned()));
        }
    }

    /// Returns the currently assembled assistant message.
    #[must_use]
    pub fn message(&self) -> Message {
        self.message.clone()
    }

    /// Returns the assembled assistant message tagged with the current part id.
    #[must_use]
    pub fn message_with_part_id(&self) -> Message {
        self.message()
            .with_metadata(MessageMetadata::new().with_id(self.part_id.clone()))
    }

    /// Returns the stable identifier associated with the current text part.
    #[must_use]
    pub const fn part_id(&self) -> &str {
        self.part_id.as_str()
    }
}

impl Default for TextAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

/// Runtime state for one in-flight tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRuntimeState {
    /// The normalized tool call metadata.
    pub call: ToolCall,
    /// Aggregated stdout/stderr or delta output.
    pub output: String,
}

/// Tracks tool lifecycle state across start, updates, and completion.
#[derive(Debug, Clone, Default)]
pub struct ToolTracker {
    active: HashMap<String, ToolRuntimeState>,
}

impl ToolTracker {
    /// Creates an empty tool tracker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts tracking one tool call.
    pub fn start(
        &mut self,
        id: impl Into<String>,
        name: impl Into<String>,
        input: Value,
        parent_id: Option<String>,
    ) -> ToolCall {
        let id = id.into();
        let mut call = ToolCall::new(id.clone(), name, input);
        if let Some(parent_id) = parent_id {
            call = call.with_parent_id(parent_id);
        }

        self.active.insert(
            id,
            ToolRuntimeState {
                call: call.clone(),
                output: String::new(),
            },
        );

        call
    }

    /// Appends one output fragment to a running tool.
    pub fn push_output(
        &mut self,
        id: &str,
        name: impl Into<String>,
        delta: &str,
    ) -> ToolRuntimeState {
        let name = name.into();
        let entry = self
            .active
            .entry(id.to_owned())
            .or_insert_with(|| ToolRuntimeState {
                call: ToolCall::new(id.to_owned(), name.clone(), Value::Null),
                output: String::new(),
            });

        if delta.is_empty() {
            return entry.clone();
        }

        entry.output.push_str(delta);
        entry.clone()
    }

    /// Completes a tool call and returns a normalized tool result.
    pub fn complete(
        &mut self,
        id: &str,
        name: impl Into<String>,
        result: Option<Value>,
        is_error: bool,
    ) -> ToolResult {
        let name = name.into();
        let Some(state) = self.active.remove(id) else {
            return fallback_tool_result(id, name, result, is_error, None);
        };

        let mut output_content = Vec::new();
        if !state.output.is_empty() {
            output_content.push(ToolContent::text(state.output));
        }
        if let Some(result) = result {
            match result {
                Value::String(text) => output_content.push(ToolContent::text(text)),
                value => output_content.push(ToolContent::json(value)),
            }
        }
        if output_content.is_empty() {
            output_content.push(ToolContent::json(Value::Object(Map::default())));
        }

        let mut tool_result =
            ToolResult::new(state.call.id, state.call.name, output_content, is_error);
        if let Some(parent_id) = state.call.parent_id {
            tool_result = tool_result.with_parent_id(parent_id);
        }

        tool_result
    }

    /// Returns a snapshot of one active tool, when present.
    #[must_use]
    pub fn state(&self, id: &str) -> Option<ToolRuntimeState> {
        self.active.get(id).cloned()
    }

    /// Fails and clears every tool still open when the stream terminates.
    pub fn fail_open_tools(&mut self) -> Vec<ToolResult> {
        let mut open = Vec::with_capacity(self.active.len());
        for (_, state) in self.active.drain() {
            let mut content = Vec::new();
            if !state.output.is_empty() {
                content.push(ToolContent::text(state.output));
            }
            content.push(ToolContent::text(
                "Tool call finished without a completion event".to_owned(),
            ));
            let mut result = ToolResult::failure(state.call.id, state.call.name, content);
            if let Some(parent_id) = state.call.parent_id {
                result = result.with_parent_id(parent_id);
            }
            open.push(result);
        }
        open
    }
}

fn fallback_tool_result(
    id: &str,
    name: String,
    result: Option<Value>,
    is_error: bool,
    parent_id: Option<String>,
) -> ToolResult {
    let mut content = Vec::new();
    if let Some(result) = result {
        match result {
            Value::String(text) => content.push(ToolContent::text(text)),
            value => content.push(ToolContent::json(value)),
        }
    }
    if content.is_empty() {
        content.push(ToolContent::json(Value::Object(Map::default())));
    }

    let mut tool_result = ToolResult::new(id.to_owned(), name, content, is_error);
    if let Some(parent_id) = parent_id {
        tool_result = tool_result.with_parent_id(parent_id);
    }
    tool_result
}

#[cfg(test)]
mod tests {
    use arky_protocol::{ContentBlock, TurnId};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{TextAccumulator, ToolTracker};

    #[test]
    fn text_accumulator_should_assemble_incremental_text() {
        let mut accumulator = TextAccumulator::new();
        accumulator.push_delta("Hello");
        accumulator.push_delta(" world");

        let message = accumulator.message_with_part_id();
        assert_eq!(message.content.len(), 1);
        match &message.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Hello world"),
            other => panic!("expected text block, got {other:?}"),
        }
        let metadata = message.metadata.expect("message metadata should exist");
        assert_eq!(
            TurnId::parse_str(metadata.id.as_deref().expect("id")).is_ok(),
            true
        );
    }

    #[test]
    fn text_accumulator_should_replace_with_authoritative_snapshot() {
        let mut accumulator = TextAccumulator::new();
        accumulator.push_delta("Draft");
        accumulator.apply_snapshot("Final");

        let message = accumulator.message_with_part_id();
        match &message.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Final"),
            other => panic!("expected text block, got {other:?}"),
        }
    }

    #[test]
    fn tool_tracker_should_track_lifecycle_and_completion() {
        let mut tracker = ToolTracker::new();
        tracker.start("tool-1", "shell", json!({"command": "pwd"}), None);
        tracker.push_output("tool-1", "shell", "/workspace");

        let result = tracker.complete("tool-1", "shell", Some(json!({"exitCode": 0})), false);

        assert_eq!(result.id, "tool-1");
        assert_eq!(result.name, "shell");
        assert_eq!(tracker.state("tool-1"), None);
        assert_eq!(result.is_error, false);
        assert_eq!(result.content.len(), 2);
    }

    #[test]
    fn tool_tracker_should_fail_open_tools() {
        let mut tracker = ToolTracker::new();
        tracker.start("tool-1", "apply_patch", json!({"path": "lib.rs"}), None);

        let results = tracker.fail_open_tools();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].is_error, true);
    }
}
