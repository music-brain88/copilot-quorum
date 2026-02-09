//! Streaming events for LLM session communication.
//!
//! [`StreamEvent`] represents individual events in a streaming LLM response,
//! enabling real-time display of model output as it's generated.
//!
//! # Native Tool Use Streaming
//!
//! When using the Native Tool Use API, additional event variants carry
//! incremental tool call data and the final structured response:
//!
//! - [`ToolCallDelta`](StreamEvent::ToolCallDelta) — incremental tool call fields
//! - [`CompletedResponse`](StreamEvent::CompletedResponse) — full [`LlmResponse`]

use super::response::LlmResponse;

/// An event in a streaming LLM response.
///
/// Used to bridge infrastructure-level streaming (e.g., SSE chunks from Copilot CLI)
/// to the application layer, enabling real-time progress display.
///
/// The `Delta`, `Completed`, and `Error` variants cover the prompt-based path.
/// `ToolCallDelta` and `CompletedResponse` are added for the Native Tool Use path.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A text chunk from the model (corresponds to assistant.message.delta).
    Delta(String),
    /// The complete response text (signals stream end).
    Completed(String),
    /// An error that occurred during streaming.
    Error(String),

    // ==================== Native Tool Use Events ====================
    /// Incremental tool call data from a streaming Native Tool Use response.
    ///
    /// Tool calls may arrive in chunks: first `id` and `name`, then
    /// incremental `arguments_delta` fragments that must be concatenated.
    ///
    /// `index` identifies which tool call this delta belongs to when
    /// the model makes multiple tool calls in a single response.
    ToolCallDelta {
        /// Index of this tool call in the response's tool call list.
        index: usize,
        /// API-assigned tool use ID (sent in the first delta for this index).
        id: Option<String>,
        /// Tool name (sent in the first delta for this index).
        name: Option<String>,
        /// Incremental JSON fragment of tool arguments.
        arguments_delta: Option<String>,
    },

    /// The full structured response (signals stream end for Native mode).
    ///
    /// Contains all content blocks (text + tool use) and the stop reason.
    /// This is the terminal event for Native Tool Use streaming.
    CompletedResponse(LlmResponse),
}

impl StreamEvent {
    /// Returns the text content if this is a Delta or Completed event.
    pub fn text(&self) -> Option<&str> {
        match self {
            StreamEvent::Delta(s) | StreamEvent::Completed(s) => Some(s),
            _ => None,
        }
    }

    /// Returns true if this event signals the end of the stream.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            StreamEvent::Completed(_) | StreamEvent::Error(_) | StreamEvent::CompletedResponse(_)
        )
    }
}

impl PartialEq for StreamEvent {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (StreamEvent::Delta(a), StreamEvent::Delta(b)) => a == b,
            (StreamEvent::Completed(a), StreamEvent::Completed(b)) => a == b,
            (StreamEvent::Error(a), StreamEvent::Error(b)) => a == b,
            (
                StreamEvent::ToolCallDelta {
                    index: ai,
                    id: a_id,
                    name: a_name,
                    arguments_delta: a_args,
                },
                StreamEvent::ToolCallDelta {
                    index: bi,
                    id: b_id,
                    name: b_name,
                    arguments_delta: b_args,
                },
            ) => ai == bi && a_id == b_id && a_name == b_name && a_args == b_args,
            // CompletedResponse doesn't implement PartialEq (LlmResponse doesn't)
            _ => false,
        }
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
    fn events_partial_eq() {
        assert!(StreamEvent::Delta("a".to_string()) == StreamEvent::Delta("a".to_string()));
        assert!(StreamEvent::Delta("a".to_string()) != StreamEvent::Completed("a".to_string()));
    }

    #[test]
    fn tool_call_delta_is_not_terminal() {
        let event = StreamEvent::ToolCallDelta {
            index: 0,
            id: Some("toolu_1".to_string()),
            name: Some("read_file".to_string()),
            arguments_delta: None,
        };
        assert!(!event.is_terminal());
        assert_eq!(event.text(), None);
    }

    #[test]
    fn completed_response_is_terminal() {
        let response = LlmResponse::from_text("done");
        let event = StreamEvent::CompletedResponse(response);
        assert!(event.is_terminal());
        assert_eq!(event.text(), None);
    }

    #[test]
    fn tool_call_delta_equality() {
        let a = StreamEvent::ToolCallDelta {
            index: 0,
            id: Some("id1".to_string()),
            name: Some("read_file".to_string()),
            arguments_delta: Some("{\"path\":".to_string()),
        };
        let b = StreamEvent::ToolCallDelta {
            index: 0,
            id: Some("id1".to_string()),
            name: Some("read_file".to_string()),
            arguments_delta: Some("{\"path\":".to_string()),
        };
        assert!(a == b);
    }
}
