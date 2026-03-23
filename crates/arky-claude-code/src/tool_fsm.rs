//! Tool lifecycle finite-state machine validation.

use std::collections::HashMap;

use arky_provider::ProviderError;

/// Concrete lifecycle state for one tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolLifecycleState {
    /// No lifecycle events have been seen for this tool yet.
    Idle,
    /// The tool call has started.
    Started {
        /// Tool name attached to the call.
        tool_name: String,
    },
    /// Streaming input fragments are still arriving.
    InputReceiving {
        /// Tool name attached to the call.
        tool_name: String,
        /// Accumulated input snapshot.
        input_snapshot: String,
    },
    /// The tool input has been finalized and the tool is executing.
    Executing {
        /// Tool name attached to the call.
        tool_name: String,
        /// Final serialized input.
        final_input: String,
    },
    /// The tool finished with either success or failure.
    Completed {
        /// Tool name attached to the call.
        tool_name: String,
        /// Whether the tool completed successfully.
        success: bool,
    },
}

/// Per-tool lifecycle validator.
#[derive(Debug, Default, Clone)]
pub struct ToolLifecycleTracker {
    states: HashMap<String, ToolLifecycleState>,
}

impl ToolLifecycleTracker {
    /// Creates an empty tracker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the current state for a tool call identifier.
    #[must_use]
    pub fn state(&self, tool_call_id: &str) -> ToolLifecycleState {
        self.states
            .get(tool_call_id)
            .cloned()
            .unwrap_or(ToolLifecycleState::Idle)
    }

    /// Processes a tool-start transition.
    pub fn start(&mut self, tool_call_id: &str, tool_name: &str) -> Result<(), ProviderError> {
        match self.state(tool_call_id) {
            ToolLifecycleState::Idle | ToolLifecycleState::Completed { .. } => {
                self.states.insert(
                    tool_call_id.to_owned(),
                    ToolLifecycleState::Started {
                        tool_name: tool_name.to_owned(),
                    },
                );
                Ok(())
            }
            current => Err(invalid_transition(tool_call_id, &current, "ToolUseStart")),
        }
    }

    /// Processes a streaming-input transition.
    pub fn input_delta(&mut self, tool_call_id: &str, delta: &str) -> Result<(), ProviderError> {
        match self.state(tool_call_id) {
            ToolLifecycleState::Started { tool_name } => {
                self.states.insert(
                    tool_call_id.to_owned(),
                    ToolLifecycleState::InputReceiving {
                        tool_name,
                        input_snapshot: delta.to_owned(),
                    },
                );
                Ok(())
            }
            ToolLifecycleState::InputReceiving {
                tool_name,
                mut input_snapshot,
            } => {
                input_snapshot.push_str(delta);
                self.states.insert(
                    tool_call_id.to_owned(),
                    ToolLifecycleState::InputReceiving {
                        tool_name,
                        input_snapshot,
                    },
                );
                Ok(())
            }
            current => Err(invalid_transition(
                tool_call_id,
                &current,
                "ToolUseInputDelta",
            )),
        }
    }

    /// Processes the transition from input collection to executing.
    pub fn complete_input(
        &mut self,
        tool_call_id: &str,
        tool_name: &str,
        final_input: &str,
    ) -> Result<(), ProviderError> {
        match self.state(tool_call_id) {
            ToolLifecycleState::Started { .. } => {
                self.states.insert(
                    tool_call_id.to_owned(),
                    ToolLifecycleState::Executing {
                        tool_name: tool_name.to_owned(),
                        final_input: final_input.to_owned(),
                    },
                );
                Ok(())
            }
            ToolLifecycleState::InputReceiving { input_snapshot, .. } => {
                self.states.insert(
                    tool_call_id.to_owned(),
                    ToolLifecycleState::Executing {
                        tool_name: tool_name.to_owned(),
                        final_input: if final_input.is_empty() {
                            input_snapshot
                        } else {
                            final_input.to_owned()
                        },
                    },
                );
                Ok(())
            }
            current => Err(invalid_transition(
                tool_call_id,
                &current,
                "ToolUseComplete",
            )),
        }
    }

    /// Processes the terminal tool-result transition.
    pub fn result(&mut self, tool_call_id: &str, success: bool) -> Result<(), ProviderError> {
        match self.state(tool_call_id) {
            ToolLifecycleState::Executing { tool_name, .. } => {
                self.states.insert(
                    tool_call_id.to_owned(),
                    ToolLifecycleState::Completed { tool_name, success },
                );
                Ok(())
            }
            current => Err(invalid_transition(tool_call_id, &current, "ToolResult")),
        }
    }
}

fn invalid_transition(
    tool_call_id: &str,
    current: &ToolLifecycleState,
    next: &str,
) -> ProviderError {
    ProviderError::protocol_violation(
        format!(
            "invalid Claude tool lifecycle transition for `{tool_call_id}`: {current:?} -> {next}"
        ),
        None,
    )
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{ToolLifecycleState, ToolLifecycleTracker};
    use arky_provider::ProviderError;

    #[test]
    fn tool_lifecycle_should_track_start_input_complete_and_result() {
        let mut tracker = ToolLifecycleTracker::new();

        tracker
            .start("tool-1", "search")
            .expect("start should work");
        tracker
            .input_delta("tool-1", "{\"q\":\"docs\"}")
            .expect("input should work");
        tracker
            .complete_input("tool-1", "search", "")
            .expect("complete should work");
        tracker.result("tool-1", true).expect("result should work");

        assert_eq!(
            tracker.state("tool-1"),
            ToolLifecycleState::Completed {
                tool_name: "search".to_owned(),
                success: true,
            }
        );
    }

    #[test]
    fn tool_lifecycle_should_reject_invalid_transitions() {
        let mut tracker = ToolLifecycleTracker::new();

        let error = tracker
            .input_delta("tool-1", "{")
            .expect_err("delta without start should fail");

        assert!(matches!(error, ProviderError::ProtocolViolation { .. }));
    }
}
