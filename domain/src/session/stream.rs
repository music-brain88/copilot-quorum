//! Streaming events for LLM session communication.
//!
//! [`StreamEvent`] represents individual events in a streaming LLM response,
//! enabling real-time display of model output as it's generated.

/// An event in a streaming LLM response.
///
/// Used to bridge infrastructure-level streaming (e.g., SSE chunks from Copilot CLI)
/// to the application layer, enabling real-time progress display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    /// A text chunk from the model (corresponds to assistant.message.delta).
    Delta(String),
    /// The complete response text (signals stream end).
    Completed(String),
    /// An error that occurred during streaming.
    Error(String),
}

impl StreamEvent {
    /// Returns the text content if this is a Delta or Completed event.
    pub fn text(&self) -> Option<&str> {
        match self {
            StreamEvent::Delta(s) | StreamEvent::Completed(s) => Some(s),
            StreamEvent::Error(_) => None,
        }
    }

    /// Returns true if this event signals the end of the stream.
    pub fn is_terminal(&self) -> bool {
        matches!(self, StreamEvent::Completed(_) | StreamEvent::Error(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_text_returns_content() {
        let event = StreamEvent::Delta("hello".to_string());
        assert_eq!(event.text(), Some("hello"));
        assert!(!event.is_terminal());
    }

    #[test]
    fn completed_text_returns_content_and_is_terminal() {
        let event = StreamEvent::Completed("full response".to_string());
        assert_eq!(event.text(), Some("full response"));
        assert!(event.is_terminal());
    }

    #[test]
    fn error_text_returns_none_and_is_terminal() {
        let event = StreamEvent::Error("oops".to_string());
        assert_eq!(event.text(), None);
        assert!(event.is_terminal());
    }

    #[test]
    fn events_are_eq() {
        assert_eq!(
            StreamEvent::Delta("a".to_string()),
            StreamEvent::Delta("a".to_string())
        );
        assert_ne!(
            StreamEvent::Delta("a".to_string()),
            StreamEvent::Completed("a".to_string())
        );
    }
}
