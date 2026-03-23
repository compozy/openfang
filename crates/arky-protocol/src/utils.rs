//! Event extraction utilities for native Arky protocol streams.

use crate::{AgentEvent, ContentBlock, ToolCall, ToolContent, ToolResult, Usage};

/// Concatenates assistant text emitted across a stream of protocol events.
#[must_use]
pub fn extract_text_from_events(events: &[AgentEvent]) -> String {
    let mut text = String::new();

    for event in events {
        match event {
            AgentEvent::MessageUpdate {
                delta: crate::StreamDelta::Text { text: delta },
                ..
            } => {
                text.push_str(delta);
            }
            AgentEvent::MessageEnd { message, .. } if text.is_empty() => {
                text.push_str(message_text(message.content.as_slice()).as_str());
            }
            AgentEvent::TurnEnd { message, .. } if text.is_empty() => {
                text.push_str(message_text(message.content.as_slice()).as_str());
            }
            _ => {}
        }
    }

    text
}

/// Extracts normalized tool-call snapshots from protocol events.
#[must_use]
pub fn extract_tool_uses(events: &[AgentEvent]) -> Vec<ToolCall> {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::ToolExecutionStart {
                tool_call_id,
                tool_name,
                args,
                ..
            } => Some(ToolCall::new(
                tool_call_id.clone(),
                tool_name.clone(),
                args.clone(),
            )),
            _ => None,
        })
        .collect()
}

/// Extracts normalized tool-result snapshots from protocol events.
#[must_use]
pub fn extract_tool_results(events: &[AgentEvent]) -> Vec<ToolResult> {
    let mut results = Vec::new();

    for event in events {
        match event {
            AgentEvent::ToolExecutionEnd {
                tool_call_id,
                tool_name,
                result,
                is_error,
                ..
            } => results.push(ToolResult::new(
                tool_call_id.clone(),
                tool_name.clone(),
                vec![ToolContent::json(result.clone())],
                *is_error,
            )),
            AgentEvent::TurnEnd { tool_results, .. } if results.is_empty() => {
                results.extend(tool_results.clone());
            }
            _ => {}
        }
    }

    results
}

/// Extracts the latest usage snapshot from protocol events, summing repeated turn-end usage.
#[must_use]
pub fn extract_usage(events: &[AgentEvent]) -> Option<Usage> {
    let mut aggregated = Usage::default();
    let mut saw_usage = false;

    for event in events {
        if let AgentEvent::TurnEnd {
            usage: Some(usage), ..
        } = event
        {
            saw_usage = true;
            merge_usage(&mut aggregated, usage);
        }
    }

    saw_usage.then_some(aggregated)
}

fn message_text(blocks: &[ContentBlock]) -> String {
    let mut text = String::new();
    for block in blocks {
        if let ContentBlock::Text { text: block_text } = block {
            text.push_str(block_text);
        }
    }
    text
}

fn merge_usage(target: &mut Usage, usage: &Usage) {
    target.input_tokens = sum_option_u64(target.input_tokens, usage.input_tokens);
    target.output_tokens = sum_option_u64(target.output_tokens, usage.output_tokens);
    target.total_tokens = sum_option_u64(target.total_tokens, usage.total_tokens);
    target.cost_usd = sum_option_f64(target.cost_usd, usage.cost_usd);
    target.duration_ms = sum_option_f64(target.duration_ms, usage.duration_ms);
    target.input_details = target
        .input_details
        .clone()
        .or_else(|| usage.input_details.clone());
    target.output_details = target
        .output_details
        .clone()
        .or_else(|| usage.output_details.clone());
}

const fn sum_option_u64(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.saturating_add(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn sum_option_f64(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left + right),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{extract_text_from_events, extract_tool_results, extract_tool_uses, extract_usage};
    use crate::{AgentEvent, EventMetadata, Message, Usage};

    #[test]
    fn extract_text_from_events_should_concatenate_text_deltas() {
        let events = vec![
            AgentEvent::MessageUpdate {
                meta: EventMetadata::new(1, 1),
                message: Message::assistant("hel"),
                delta: crate::StreamDelta::text("hel"),
            },
            AgentEvent::MessageUpdate {
                meta: EventMetadata::new(1, 2),
                message: Message::assistant("hello"),
                delta: crate::StreamDelta::text("lo"),
            },
        ];

        assert_eq!(extract_text_from_events(&events), "hello");
    }

    #[test]
    fn extract_tool_uses_should_collect_tool_execution_starts() {
        let events = vec![AgentEvent::ToolExecutionStart {
            meta: EventMetadata::new(1, 1),
            tool_call_id: "tool-1".to_owned(),
            tool_name: "shell".to_owned(),
            args: json!({ "command": "pwd" }),
        }];

        let tool_uses = extract_tool_uses(&events);

        assert_eq!(tool_uses.len(), 1);
        assert_eq!(tool_uses[0].name, "shell");
    }

    #[test]
    fn extract_tool_results_should_collect_tool_execution_ends() {
        let events = vec![AgentEvent::ToolExecutionEnd {
            meta: EventMetadata::new(1, 1),
            tool_call_id: "tool-1".to_owned(),
            tool_name: "shell".to_owned(),
            result: json!({ "exitCode": 0 }),
            is_error: false,
        }];

        let tool_results = extract_tool_results(&events);

        assert_eq!(tool_results.len(), 1);
        assert_eq!(tool_results[0].name, "shell");
        assert_eq!(tool_results[0].is_error, false);
    }

    #[test]
    fn extract_usage_should_sum_turn_end_usage() {
        let events = vec![
            AgentEvent::TurnEnd {
                meta: EventMetadata::new(1, 1),
                message: Message::assistant("done"),
                tool_results: Vec::new(),
                usage: Some(Usage {
                    input_tokens: Some(10),
                    output_tokens: Some(5),
                    total_tokens: Some(15),
                    ..Usage::default()
                }),
            },
            AgentEvent::TurnEnd {
                meta: EventMetadata::new(2, 2),
                message: Message::assistant("done again"),
                tool_results: Vec::new(),
                usage: Some(Usage {
                    input_tokens: Some(3),
                    output_tokens: Some(2),
                    total_tokens: Some(5),
                    ..Usage::default()
                }),
            },
        ];

        let usage = extract_usage(&events).expect("usage should exist");

        assert_eq!(usage.input_tokens, Some(13));
        assert_eq!(usage.output_tokens, Some(7));
        assert_eq!(usage.total_tokens, Some(20));
    }
}
