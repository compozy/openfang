//! Provider descriptor and capability metadata.

use std::borrow::Cow;

use arky_protocol::ProviderId;
use serde::{Deserialize, Serialize};

use crate::ProviderRequest;

/// Stable provider family classification used for routing and UX.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFamily {
    /// Anthropic Claude Code CLI wrapper.
    ClaudeCode,
    /// `OpenAI` Codex App Server wrapper.
    Codex,
    /// User-defined or third-party family label.
    Custom(String),
}

/// Capability flags exposed by a provider implementation.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the techspec requires these discrete capability flags on the public contract"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    /// Whether streaming is supported.
    pub streaming: bool,
    /// Whether direct generation is supported.
    pub generate: bool,
    /// Whether tool calls are supported.
    pub tool_calls: bool,
    /// Whether MCP servers can be passed through directly.
    pub mcp_passthrough: bool,
    /// Whether provider-native session resume is supported.
    pub session_resume: bool,
    /// Whether mid-turn steering is supported.
    pub steering: bool,
    /// Whether follow-up turns can be issued natively.
    pub follow_up: bool,
    /// Whether image inputs are accepted.
    pub image_inputs: bool,
    /// Whether extended thinking/reasoning is supported.
    pub extended_thinking: bool,
    /// Whether code execution is supported.
    pub code_execution: bool,
}

impl ProviderCapabilities {
    /// Creates an empty capability set.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            streaming: false,
            generate: false,
            tool_calls: false,
            mcp_passthrough: false,
            session_resume: false,
            steering: false,
            follow_up: false,
            image_inputs: false,
            extended_thinking: false,
            code_execution: false,
        }
    }

    /// Sets the streaming flag.
    #[must_use]
    pub const fn with_streaming(mut self, streaming: bool) -> Self {
        self.streaming = streaming;
        self
    }

    /// Sets the generate flag.
    #[must_use]
    pub const fn with_generate(mut self, generate: bool) -> Self {
        self.generate = generate;
        self
    }

    /// Sets the tool-calls flag.
    #[must_use]
    pub const fn with_tool_calls(mut self, tool_calls: bool) -> Self {
        self.tool_calls = tool_calls;
        self
    }

    /// Sets the MCP passthrough flag.
    #[must_use]
    pub const fn with_mcp_passthrough(mut self, mcp_passthrough: bool) -> Self {
        self.mcp_passthrough = mcp_passthrough;
        self
    }

    /// Sets the session-resume flag.
    #[must_use]
    pub const fn with_session_resume(mut self, session_resume: bool) -> Self {
        self.session_resume = session_resume;
        self
    }

    /// Sets the steering flag.
    #[must_use]
    pub const fn with_steering(mut self, steering: bool) -> Self {
        self.steering = steering;
        self
    }

    /// Sets the follow-up flag.
    #[must_use]
    pub const fn with_follow_up(mut self, follow_up: bool) -> Self {
        self.follow_up = follow_up;
        self
    }

    /// Sets the image-inputs flag.
    #[must_use]
    pub const fn with_image_inputs(mut self, image_inputs: bool) -> Self {
        self.image_inputs = image_inputs;
        self
    }

    /// Sets the extended-thinking flag.
    #[must_use]
    pub const fn with_extended_thinking(mut self, extended_thinking: bool) -> Self {
        self.extended_thinking = extended_thinking;
        self
    }

    /// Sets the code-execution flag.
    #[must_use]
    pub const fn with_code_execution(mut self, code_execution: bool) -> Self {
        self.code_execution = code_execution;
        self
    }
}

/// Non-fatal capability incompatibility detected before provider execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityWarning {
    /// Stable capability key that triggered the warning.
    pub capability: Cow<'static, str>,
    /// Human-readable explanation.
    pub message: String,
}

impl CapabilityWarning {
    /// Creates a new capability warning.
    #[must_use]
    pub fn new(capability: impl Into<Cow<'static, str>>, message: impl Into<String>) -> Self {
        Self {
            capability: capability.into(),
            message: message.into(),
        }
    }
}

/// Immutable provider metadata exposed through the registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDescriptor {
    /// Stable provider identifier.
    pub id: ProviderId,
    /// Provider family classification.
    pub family: ProviderFamily,
    /// Supported capabilities.
    pub capabilities: ProviderCapabilities,
}

impl ProviderDescriptor {
    /// Creates a provider descriptor.
    #[must_use]
    pub const fn new(
        id: ProviderId,
        family: ProviderFamily,
        capabilities: ProviderCapabilities,
    ) -> Self {
        Self {
            id,
            family,
            capabilities,
        }
    }
}

/// Returns whether any request message includes image content.
#[must_use]
pub fn messages_have_image_inputs(messages: &[arky_protocol::Message]) -> bool {
    messages.iter().any(|message| {
        message
            .content
            .iter()
            .any(|block| matches!(block, arky_protocol::ContentBlock::Image { .. }))
    })
}

/// Validates a request against provider capability flags.
#[must_use]
pub fn validate_capabilities(
    request: &ProviderRequest,
    capabilities: &ProviderCapabilities,
) -> Vec<CapabilityWarning> {
    let mut warnings = Vec::new();

    if messages_have_image_inputs(&request.messages) && !capabilities.image_inputs {
        warnings.push(CapabilityWarning::new(
            "image_inputs",
            "image inputs are not supported by this provider",
        ));
    }

    if request.settings.reasoning_effort.is_some() && !capabilities.extended_thinking {
        warnings.push(CapabilityWarning::new(
            "extended_thinking",
            "reasoning effort is not supported by this provider",
        ));
    }

    if !request.tools.definitions.is_empty() && !capabilities.tool_calls {
        warnings.push(CapabilityWarning::new(
            "tool_calls",
            "tool registrations are not supported by this provider",
        ));
    }

    if request.session.provider_session_id.is_some() && !capabilities.session_resume {
        warnings.push(CapabilityWarning::new(
            "session_resume",
            "session resume is not supported by this provider",
        ));
    }

    warnings
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{
        messages_have_image_inputs, validate_capabilities, CapabilityWarning, ProviderCapabilities,
        ProviderDescriptor, ProviderFamily,
    };
    use crate::ProviderRequest;
    use arky_protocol::{
        ContentBlock, Message, ModelRef, ProviderId, ProviderSettings, SessionRef, ToolContext,
        ToolDefinition, TurnContext, TurnId,
    };

    #[test]
    fn provider_descriptor_should_preserve_construction_inputs() {
        let capabilities = ProviderCapabilities::new()
            .with_streaming(true)
            .with_generate(true)
            .with_tool_calls(true)
            .with_session_resume(true);
        let descriptor = ProviderDescriptor::new(
            ProviderId::new("claude-code"),
            ProviderFamily::ClaudeCode,
            capabilities,
        );

        assert_eq!(descriptor.id.as_str(), "claude-code");
        assert_eq!(descriptor.family, ProviderFamily::ClaudeCode);
        assert_eq!(descriptor.capabilities, capabilities);
    }

    #[test]
    fn provider_capabilities_should_toggle_individual_flags() {
        let capabilities = ProviderCapabilities::new()
            .with_streaming(true)
            .with_generate(true)
            .with_tool_calls(true)
            .with_mcp_passthrough(true)
            .with_session_resume(true)
            .with_steering(true)
            .with_follow_up(true)
            .with_image_inputs(true)
            .with_extended_thinking(true)
            .with_code_execution(true);

        assert_eq!(
            capabilities,
            ProviderCapabilities {
                streaming: true,
                generate: true,
                tool_calls: true,
                mcp_passthrough: true,
                session_resume: true,
                steering: true,
                follow_up: true,
                image_inputs: true,
                extended_thinking: true,
                code_execution: true,
            }
        );
    }

    #[test]
    fn messages_have_image_inputs_should_detect_image_blocks() {
        let messages = vec![Message::new(
            arky_protocol::Role::User,
            vec![
                ContentBlock::text("hello"),
                ContentBlock::image([1_u8, 2, 3], "image/png"),
            ],
        )];

        assert_eq!(messages_have_image_inputs(&messages), true);
    }

    #[test]
    fn validate_capabilities_should_warn_on_image_and_reasoning_mismatch() {
        let mut settings = ProviderSettings::new();
        settings.reasoning_effort = Some(arky_protocol::ReasoningEffort::Low);
        let request = ProviderRequest::new(
            SessionRef::new(None).with_provider_session_id("provider-session"),
            TurnContext::new(TurnId::new(), 1),
            ModelRef::new("claude-3.5-sonnet"),
            vec![Message::new(
                arky_protocol::Role::User,
                vec![ContentBlock::image([7_u8, 8, 9], "image/png")],
            )],
        )
        .with_tools(
            ToolContext::new().with_definitions(vec![ToolDefinition::new(
                "mcp/files/read_file",
                "Read a file",
                json!({"type": "object"}),
            )]),
        )
        .with_settings(settings);

        let warnings = validate_capabilities(&request, &ProviderCapabilities::new());

        assert_eq!(
            warnings,
            vec![
                CapabilityWarning::new(
                    "image_inputs",
                    "image inputs are not supported by this provider",
                ),
                CapabilityWarning::new(
                    "extended_thinking",
                    "reasoning effort is not supported by this provider",
                ),
                CapabilityWarning::new(
                    "tool_calls",
                    "tool registrations are not supported by this provider",
                ),
                CapabilityWarning::new(
                    "session_resume",
                    "session resume is not supported by this provider",
                ),
            ],
        );
    }
}
