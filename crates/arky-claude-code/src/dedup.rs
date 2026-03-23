//! Text deduplication between streamed deltas and final assistant snapshots.

/// Origin of a text fragment emitted by the Claude CLI protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextSource {
    /// A streaming delta emitted incrementally.
    StreamEvent,
    /// A final assistant snapshot emitted after stream events.
    Assistant,
}

/// Stateful deduplicator used to suppress duplicate assistant snapshots.
#[derive(Debug, Default, Clone)]
pub struct TextDeduplicator {
    streamed_text_length: usize,
}

impl TextDeduplicator {
    /// Creates an empty deduplicator.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            streamed_text_length: 0,
        }
    }

    /// Applies deduplication and returns the text that should be emitted.
    pub fn process(&mut self, source: TextSource, text: &str) -> String {
        match source {
            TextSource::StreamEvent => {
                self.streamed_text_length = self.streamed_text_length.saturating_add(text.len());
                text.to_owned()
            }
            TextSource::Assistant => {
                let deduplicated = if text.len() > self.streamed_text_length {
                    text[self.streamed_text_length..].to_owned()
                } else {
                    String::new()
                };
                self.streamed_text_length = self.streamed_text_length.max(text.len());
                deduplicated
            }
        }
    }

    /// Clears all tracked text state.
    pub const fn reset(&mut self) {
        self.streamed_text_length = 0;
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{TextDeduplicator, TextSource};

    #[test]
    fn deduplicator_should_strip_assistant_prefix_that_was_already_streamed() {
        let mut deduplicator = TextDeduplicator::new();

        assert_eq!(
            deduplicator.process(TextSource::StreamEvent, "hello "),
            "hello "
        );
        assert_eq!(
            deduplicator.process(TextSource::Assistant, "hello world"),
            "world"
        );
    }

    #[test]
    fn deduplicator_should_reset_after_tool_boundary() {
        let mut deduplicator = TextDeduplicator::new();

        let _ = deduplicator.process(TextSource::StreamEvent, "hello");
        deduplicator.reset();

        assert_eq!(
            deduplicator.process(TextSource::Assistant, "hello"),
            "hello"
        );
    }
}
