//! Port for structured conversation logging.
//!
//! Defines the [`ConversationLogger`] trait for recording conversation events
//! (LLM prompts, responses, tool calls, plan voting, etc.) to a structured log.
//!
//! This is separate from `tracing`-based operation logs: tracing handles
//! human-readable diagnostic messages, while this port captures the full
//! conversation transcript in a machine-readable format (JSONL).

use serde_json::Value;

/// A structured conversation event for logging.
///
/// Each event has a type string, a UTC timestamp, and a JSON payload
/// containing event-specific fields.
pub struct ConversationEvent {
    /// Event type identifier (e.g., "llm_response", "tool_call", "plan_generated").
    pub event_type: &'static str,
    /// JSON payload with event-specific data.
    pub payload: Value,
}

impl ConversationEvent {
    /// Create a new conversation event with the current UTC timestamp.
    pub fn new(event_type: &'static str, payload: Value) -> Self {
        Self {
            event_type,
            payload,
        }
    }
}

/// Port for logging conversation events to a structured log.
///
/// Implementations write each event as a single record (e.g., one JSONL line).
/// The `log` method is intentionally synchronous and non-fallible to avoid
/// disrupting the main execution flow — logging failures are silently ignored.
pub trait ConversationLogger: Send + Sync {
    /// Record a conversation event.
    fn log(&self, event: ConversationEvent);
}

/// No-op implementation for tests and when logging is disabled.
pub struct NoConversationLogger;

impl ConversationLogger for NoConversationLogger {
    fn log(&self, _event: ConversationEvent) {}
}
