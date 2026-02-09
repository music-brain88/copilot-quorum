//! Transport layer types for Copilot CLI communication.
//!
//! Contains message classification and streaming outcome types used by
//! the [`MessageRouter`](super::router::MessageRouter).

use crate::copilot::protocol::ToolCallParams;

/// Classification of an incoming JSON-RPC message.
#[derive(Debug, PartialEq, Eq)]
pub enum MessageKind {
    /// A response to a request we sent (has `id`, no `method`).
    Response,
    /// An incoming request from the CLI (has `id` + `method`).
    IncomingRequest { id: u64 },
    /// A notification (has `method`, no `id`).
    Notification,
}

/// Classify a JSON-RPC message by its structure.
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
#[derive(Debug)]
pub enum StreamingOutcome {
    /// session.idle reached — text streaming is complete.
    Idle(String),
    /// A `tool.call` request was received from the CLI.
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
