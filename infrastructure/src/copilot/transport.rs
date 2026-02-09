//! Transport layer types for Copilot CLI communication.
//!
//! This module provides the low-level message classification and streaming
//! outcome types used by the [`MessageRouter`](super::router::MessageRouter)'s
//! background reader task.
//!
//! # Types
//!
//! - [`MessageKind`] — Classifies each incoming JSON-RPC frame so the router
//!   can dispatch it correctly (response correlation, session routing, or
//!   tool-call forwarding).
//! - [`classify_message`] — Pure function that inspects `id` / `method` fields.
//! - [`StreamingOutcome`] — Returned by
//!   [`SessionChannel::read_streaming_for_tools`](super::router::SessionChannel::read_streaming_for_tools)
//!   to signal whether the LLM finished speaking (`Idle`) or wants a tool
//!   invoked (`ToolCall`). This is the pivot point in the **Native Tool Use**
//!   multi-turn loop.

use crate::copilot::protocol::ToolCallParams;

/// Classification of an incoming JSON-RPC message.
///
/// Used by the [`MessageRouter`](super::router::MessageRouter) background
/// reader task to determine how to dispatch each frame:
///
/// - `Response` → oneshot correlation for pending requests
/// - `IncomingRequest` → forwarded to session channel (e.g. `tool.call`)
/// - `Notification` → session event routing or `session.start` handling
#[derive(Debug, PartialEq, Eq)]
pub enum MessageKind {
    /// A response to a request we sent (has `id`, no `method`).
    Response,
    /// An incoming request from the CLI (has `id` + `method`), e.g. `tool.call`
    /// for **Native Tool Use**.
    IncomingRequest { id: u64 },
    /// A notification (has `method`, no `id`), e.g. `session.event` carrying
    /// streaming deltas, `session.idle`, or `session.start`.
    Notification,
}

/// Classify a JSON-RPC message by inspecting `id` and `method` fields.
///
/// This is a pure function with no side effects, called once per frame in
/// the router's background reader loop.
pub fn classify_message(json: &serde_json::Value) -> MessageKind {
    let has_id = json.get("id").and_then(|v| v.as_u64());
    let has_method = json.get("method").and_then(|v| v.as_str());

    match (has_id, has_method) {
        (Some(id), Some(_)) => MessageKind::IncomingRequest { id },
        (Some(_), None) => MessageKind::Response,
        _ => MessageKind::Notification,
    }
}

/// Outcome of streaming reads that support tool calls.
///
/// Returned by [`SessionChannel::read_streaming_for_tools`](super::router::SessionChannel::read_streaming_for_tools).
/// This is the decision point in the **Native Tool Use** / **Agent System**
/// multi-turn loop — the caller checks the variant to decide whether to
/// return the text to the user or execute a tool and continue.
#[derive(Debug)]
pub enum StreamingOutcome {
    /// `session.idle` reached — the LLM has finished responding with text.
    Idle(String),
    /// A `tool.call` request was received — the LLM wants to invoke a tool.
    ///
    /// `text_so_far` contains any text the LLM emitted before the tool call.
    /// The caller should execute the tool and send results back via
    /// [`MessageRouter::send_response`](super::router::MessageRouter::send_response).
    ToolCall {
        text_so_far: String,
        request_id: u64,
        params: ToolCallParams,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_response() {
        let json = serde_json::json!({"id": 1, "result": {}});
        assert_eq!(classify_message(&json), MessageKind::Response);
    }

    #[test]
    fn classify_incoming_request() {
        let json = serde_json::json!({"id": 1, "method": "tool.call", "params": {}});
        assert_eq!(
            classify_message(&json),
            MessageKind::IncomingRequest { id: 1 }
        );
    }

    #[test]
    fn classify_notification() {
        let json = serde_json::json!({"method": "session.event", "params": {}});
        assert_eq!(classify_message(&json), MessageKind::Notification);
    }

    #[test]
    fn classify_no_id_no_method() {
        // Edge case: neither id nor method → treated as Notification
        let json = serde_json::json!({"data": "something"});
        assert_eq!(classify_message(&json), MessageKind::Notification);
    }
}
