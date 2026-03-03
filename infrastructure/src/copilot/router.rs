//! Transport demultiplexer — message routing for concurrent Copilot CLI sessions.
//!
//! The Copilot CLI communicates over a **single TCP connection** using JSON-RPC 2.0,
//! but several features require **multiple sessions running concurrently**:
//!
//! | Feature | Concurrent sessions |
//! |---------|-------------------|
//! | **Solo mode** | 1 (+ 1 tool session if using Native Tool Use) |
//! | **Solo + `/discuss`** | Up to 7 (initial × 3 + review × 3 + synthesis) |
//! | **Ensemble Planning** | N² peak (N plan-generation + N×(N−1) voting) |
//! | **Agent + Tool Use** | 2 per model (main + tool-enabled session) |
//!
//! [`MessageRouter`] solves this by running a single background reader task that
//! owns the TCP read-half exclusively — no `Mutex` contention — and routes
//! incoming messages to the correct [`SessionChannel`] by `session_id`.
//!
//! See [docs/systems/transport.md](../../../../docs/systems/transport.md) for
//! the full design rationale and concurrency patterns.

use crate::copilot::error::{CopilotError, Result};
use crate::copilot::protocol::{
    CreateSessionParams, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, JsonRpcResponseOut,
    ToolCallParams, ToolCallResult,
};
use crate::copilot::transport::{MessageKind, StreamingOutcome, classify_message};
use quorum_application::ports::conversation_logger::{
    ConversationEvent, ConversationLogger, NoConversationLogger,
};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, trace, warn};

/// Timeout for session creation (waiting for `session.start` event).
const SESSION_CREATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// A message routed to a specific session's channel.
///
/// The background reader task classifies every incoming JSON-RPC message
/// (via [`classify_message`]) and wraps session-relevant ones in this enum
/// before sending them through the per-session `mpsc` channel.
#[derive(Debug)]
pub enum RoutedMessage {
    /// A `session.event` notification (delta, message, idle, etc.).
    ///
    /// Used by **all features** — every LLM response is delivered as a stream
    /// of `assistant.message.delta` events followed by `session.idle`.
    SessionEvent {
        event_type: String,
        event: serde_json::Value,
    },
    /// An incoming `tool.call` request from the CLI.
    ///
    /// Used by **Native Tool Use** and the **Agent System** — the LLM decides
    /// to invoke a tool, and the CLI forwards the request to us for execution.
    ToolCall {
        request_id: u64,
        params: ToolCallParams,
    },
}

/// Information extracted from a `session.start` event.
#[derive(Debug)]
struct SessionStartEvent {
    session_id: String,
}

/// Try to extract text content from a session event's data payload.
///
/// Handles multiple possible JSON structures that the Copilot CLI
/// may use for events like `assistant.turn_end`:
///
/// - `{ "data": { "content": "text" } }` — string content
/// - `{ "data": { "content": [{ "type": "text", "text": "..." }] } }` — content blocks array
/// - `{ "data": { "message": { "content": "text" } } }` — nested message
/// - `{ "data": { "text": "..." } }` — direct text field
fn extract_event_text(event: &serde_json::Value) -> Option<String> {
    let data = event.get("data")?;

    // Path 1: data.content as string
    if let Some(s) = data.get("content").and_then(|c| c.as_str())
        && !s.is_empty()
    {
        return Some(s.to_string());
    }

    // Path 2: data.content as array of content blocks
    if let Some(arr) = data.get("content").and_then(|c| c.as_array()) {
        let mut text = String::new();
        for block in arr {
            if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(t);
            }
        }
        if !text.is_empty() {
            return Some(text);
        }
    }

    // Path 3: data.message.content as string
    if let Some(s) = data
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        && !s.is_empty()
    {
        return Some(s.to_string());
    }

    // Path 4: data.text as string
    if let Some(s) = data.get("text").and_then(|t| t.as_str())
        && !s.is_empty()
    {
        return Some(s.to_string());
    }

    None
}

/// A per-session channel for receiving routed messages.
///
/// Each [`CopilotSession`](super::session::CopilotSession) owns a
/// `SessionChannel` for its lifetime. When dropped, the session is
/// automatically deregistered from the router via [`Drop`].
///
/// # Feature usage
///
/// - **Solo / Quorum Discussion**: one channel per session, reading text via
///   [`read_streaming`](Self::read_streaming).
/// - **Native Tool Use**: the tool-enabled session uses
///   [`read_streaming_for_tools`](Self::read_streaming_for_tools) to detect
///   `tool.call` requests interleaved with text deltas.
/// - **Quorum Discussion with cancellation**: long-running parallel sessions
///   use [`read_streaming_with_cancellation`](Self::read_streaming_with_cancellation).
pub struct SessionChannel {
    rx: mpsc::UnboundedReceiver<RoutedMessage>,
    session_id: String,
    router: Arc<MessageRouter>,
    conversation_logger: Arc<dyn ConversationLogger>,
    /// Maps `toolCallId` → `toolName` from `tool.execution_start` events,
    /// used to resolve the tool name when `tool.execution_complete` arrives.
    tool_names: HashMap<String, String>,
}

/// Extract tool name directly from event fields (without toolCallId correlation).
///
/// Tries multiple field paths for robustness:
/// 1. Top-level `toolName` (Copilot CLI standard)
/// 2. Top-level `name`
/// 3. `data.toolName`
/// 4. `data.name`
/// 5. `data.tool.name` (nested structure)
fn extract_tool_name_from_event(event: &serde_json::Value) -> Option<&str> {
    let data = event.get("data");
    event
        .get("toolName")
        .and_then(|v| v.as_str())
        .or_else(|| event.get("name").and_then(|v| v.as_str()))
        .or_else(|| {
            data.and_then(|d| {
                d.get("toolName")
                    .and_then(|v| v.as_str())
                    .or_else(|| d.get("name").and_then(|v| v.as_str()))
                    .or_else(|| {
                        d.get("tool")
                            .and_then(|t| t.get("name"))
                            .and_then(|v| v.as_str())
                    })
            })
        })
}

/// Extract `data.toolCallId` from a tool event.
fn extract_tool_call_id(event: &serde_json::Value) -> Option<&str> {
    event
        .get("data")
        .and_then(|d| d.get("toolCallId"))
        .and_then(|v| v.as_str())
}

impl SessionChannel {
    /// Record tool name from a `tool.execution_start` event for later correlation.
    ///
    /// The Copilot CLI sends `toolName` in the start event but not in the
    /// complete event. We store `toolCallId → toolName` so that
    /// `log_internal_tool_execution()` can resolve the name on completion.
    fn record_tool_start(&mut self, event: &serde_json::Value) {
        if let (Some(call_id), Some(name)) = (
            extract_tool_call_id(event),
            extract_tool_name_from_event(event),
        ) {
            self.tool_names.insert(call_id.to_string(), name.to_string());
        }
    }

    /// Resolve tool name for a `tool.execution_complete` event.
    ///
    /// Priority:
    /// 1. `data.toolCallId` → look up in `tool_names` map (from start event)
    /// 2. Direct field extraction from the event itself (fallback)
    /// 3. `"unknown"` if nothing matches
    fn resolve_tool_name(&self, event: &serde_json::Value) -> String {
        // First: try toolCallId correlation from start events
        if let Some(call_id) = extract_tool_call_id(event) {
            if let Some(name) = self.tool_names.get(call_id) {
                return name.clone();
            }
        }
        // Fallback: direct field extraction
        extract_tool_name_from_event(event)
            .unwrap_or("unknown")
            .to_string()
    }

    /// Log a Copilot CLI internal tool execution event to the conversation logger.
    ///
    /// Extracts tool name, output size, and status from the `tool.execution_complete`
    /// event and records it as an `internal_tool_complete` conversation event.
    fn log_internal_tool_execution(&mut self, event: &serde_json::Value) {
        let data = event.get("data");
        let tool_name = self.resolve_tool_name(event);
        // Clean up the toolCallId mapping after resolution
        if let Some(call_id) = extract_tool_call_id(event) {
            self.tool_names.remove(call_id);
        }
        let mut payload = serde_json::json!({
            "session_id": self.session_id,
            "tool": tool_name,
            "source": "copilot_cli",
        });
        // Try top-level result/output first, then data.result/output
        let result = event
            .get("result")
            .or_else(|| event.get("output"))
            .or_else(|| data.and_then(|d| d.get("result").or_else(|| d.get("output"))));
        if let Some(r) = result {
            let size = serde_json::to_string(r).map(|s| s.len()).unwrap_or(0);
            payload["output_bytes"] = serde_json::json!(size);
        }
        // Try top-level status first, then data.status
        let status = event
            .get("status")
            .and_then(|v| v.as_str())
            .or_else(|| data.and_then(|d| d.get("status").and_then(|v| v.as_str())));
        if let Some(s) = status {
            payload["status"] = serde_json::json!(s);
        }
        self.conversation_logger
            .log(ConversationEvent::new("internal_tool_complete", payload));
    }

    /// Receive the next routed message, blocking until one arrives.
    ///
    /// Returns [`CopilotError::RouterStopped`] if the background reader task
    /// has ended (e.g. TCP disconnection or Copilot CLI crash).
    pub async fn recv(&mut self) -> Result<RoutedMessage> {
        self.rx.recv().await.ok_or(CopilotError::RouterStopped)
    }

    /// Read streaming session events until `session.idle`, calling `on_chunk`
    /// for each text delta.
    ///
    /// Used by **Solo mode**, **Quorum Discussion**, and **Ensemble Planning**
    /// for regular text-only LLM responses. Tool calls arriving during this
    /// read are logged as warnings and ignored.
    pub async fn read_streaming(&mut self, mut on_chunk: impl FnMut(&str)) -> Result<String> {
        let mut full_content = String::new();
        let mut turn_delta_bytes: usize = 0;

        loop {
            let msg = self.recv().await?;
            match msg {
                RoutedMessage::SessionEvent { event_type, event } => match event_type.as_str() {
                    "assistant.message.delta" => {
                        if let Some(data) = event.get("data")
                            && let Some(content) = data.get("content").and_then(|c| c.as_str())
                            && !content.is_empty()
                        {
                            on_chunk(content);
                            full_content.push_str(content);
                            turn_delta_bytes += content.len();
                        }
                    }
                    "assistant.message" | "assistant.message.completed" => {
                        // Use completed content when no deltas were received for this turn.
                        // Unlike checking full_content.is_empty(), using turn_delta_bytes
                        // allows content accumulation across multiple turns.
                        if turn_delta_bytes == 0
                            && let Some(data) = event.get("data")
                            && let Some(content) = data.get("content").and_then(|c| c.as_str())
                            && !content.is_empty()
                        {
                            on_chunk(content);
                            full_content.push_str(content);
                        }
                    }
                    "assistant.turn_start" => {
                        turn_delta_bytes = 0;
                    }
                    "assistant.turn_end" => {
                        // Fallback: extract content from turn_end when no deltas were received
                        if turn_delta_bytes == 0
                            && let Some(text) = extract_event_text(&event)
                        {
                            debug!("Stream: turn_end fallback content ({} bytes)", text.len());
                            on_chunk(&text);
                            full_content.push_str(&text);
                        }
                        debug!(
                            "Stream: assistant.turn_end (turn_deltas: {}, total: {} bytes)",
                            turn_delta_bytes,
                            full_content.len()
                        );
                    }
                    "session.idle" => {
                        debug!(
                            "Session idle, streaming complete ({} bytes)",
                            full_content.len()
                        );
                        return Ok(full_content);
                    }
                    "session.error" => {
                        let error_msg = event
                            .get("data")
                            .and_then(|d| d.get("message"))
                            .and_then(|m| m.as_str())
                            .unwrap_or("Unknown session error");
                        warn!("Session error: {}", error_msg);
                        return Err(CopilotError::RpcError {
                            code: -1,
                            message: error_msg.to_string(),
                        });
                    }
                    // Known informational events
                    "pending_messages.modified"
                    | "user.message"
                    | "session.usage_info"
                    | "assistant.usage"
                    | "assistant.reasoning"
                    | "tool.execution_partial_result" => {
                        trace!("Stream: {}", event_type);
                    }
                    "tool.execution_start" => {
                        self.record_tool_start(&event);
                        trace!("Stream: tool.execution_start");
                    }
                    "tool.execution_complete" => {
                        let size = serde_json::to_string(&event).map(|s| s.len()).unwrap_or(0);
                        debug!("Stream: tool.execution_complete ({} bytes)", size);
                        trace!(
                            "Stream: tool.execution_complete: {}",
                            serde_json::to_string(&event).unwrap_or_default()
                        );
                        self.log_internal_tool_execution(&event);
                    }
                    other => {
                        let size = serde_json::to_string(&event).map(|s| s.len()).unwrap_or(0);
                        debug!("Stream: unhandled event '{}' ({} bytes)", other, size);
                        trace!(
                            "Stream: unhandled event '{}': {}",
                            other,
                            serde_json::to_string(&event).unwrap_or_default()
                        );
                    }
                },
                RoutedMessage::ToolCall { request_id, params } => {
                    warn!(
                        "Unexpected tool.call in read_streaming: {}, rejecting",
                        params.tool_name
                    );
                    let result =
                        ToolCallResult::error("Tool not available in this session context");
                    let response = JsonRpcResponseOut::new(request_id, result.into_rpc_value());
                    let _ = self.router.send_response(&response).await;
                }
            }
        }
    }

    /// Read streaming events until `session.idle` **or** `tool.call`.
    ///
    /// Returns [`StreamingOutcome::Idle`] when the LLM finishes responding, or
    /// [`StreamingOutcome::ToolCall`] when the LLM requests tool execution.
    ///
    /// Used by **Native Tool Use** and the **Agent System** — the multi-turn
    /// loop in [`CopilotSession::send_with_tools`](super::session::CopilotSession)
    /// and [`send_tool_results`](super::session::CopilotSession) relies on
    /// this to detect when the LLM wants a tool invoked.
    pub async fn read_streaming_for_tools(
        &mut self,
        mut on_chunk: impl FnMut(&str),
    ) -> Result<StreamingOutcome> {
        let mut full_content = String::new();
        let mut turn_delta_bytes: usize = 0;

        loop {
            let msg = self.recv().await?;
            match msg {
                RoutedMessage::SessionEvent { event_type, event } => match event_type.as_str() {
                    "assistant.message.delta" => {
                        if let Some(data) = event.get("data")
                            && let Some(content) = data.get("content").and_then(|c| c.as_str())
                            && !content.is_empty()
                        {
                            on_chunk(content);
                            full_content.push_str(content);
                            turn_delta_bytes += content.len();
                        } else {
                            let size = serde_json::to_string(&event).map(|s| s.len()).unwrap_or(0);
                            debug!("Tool stream: delta with unexpected format ({} bytes)", size);
                            trace!(
                                "Tool stream: delta with unexpected format: {}",
                                serde_json::to_string(&event).unwrap_or_default()
                            );
                        }
                    }
                    "assistant.message" | "assistant.message.completed" => {
                        // Use completed content when no deltas were received for this turn.
                        // Unlike checking full_content.is_empty(), using turn_delta_bytes
                        // allows content accumulation across multiple turns.
                        if turn_delta_bytes == 0
                            && let Some(data) = event.get("data")
                            && let Some(content) = data.get("content").and_then(|c| c.as_str())
                            && !content.is_empty()
                        {
                            on_chunk(content);
                            full_content.push_str(content);
                        }
                    }
                    "assistant.turn_start" => {
                        turn_delta_bytes = 0;
                    }
                    "assistant.turn_end" => {
                        // Fallback: extract content from turn_end when no deltas were received
                        if turn_delta_bytes == 0
                            && let Some(text) = extract_event_text(&event)
                        {
                            debug!(
                                "Tool stream: turn_end fallback content ({} bytes)",
                                text.len()
                            );
                            on_chunk(&text);
                            full_content.push_str(&text);
                        }
                        debug!(
                            "Tool stream: assistant.turn_end (turn_deltas: {}, total: {} bytes)",
                            turn_delta_bytes,
                            full_content.len()
                        );
                    }
                    "tool.execution_complete" => {
                        let size = serde_json::to_string(&event).map(|s| s.len()).unwrap_or(0);
                        debug!("Tool stream: tool.execution_complete ({} bytes)", size);
                        trace!(
                            "Tool stream: tool.execution_complete: {}",
                            serde_json::to_string(&event).unwrap_or_default()
                        );
                        self.log_internal_tool_execution(&event);
                    }
                    "session.error" => {
                        let error_msg = event
                            .get("data")
                            .and_then(|d| d.get("message"))
                            .and_then(|m| m.as_str())
                            .unwrap_or("Unknown session error");
                        warn!("Tool stream: session error: {}", error_msg);
                        return Err(CopilotError::RpcError {
                            code: -1,
                            message: error_msg.to_string(),
                        });
                    }
                    "session.idle" => {
                        debug!(
                            "Tool stream: session idle ({} bytes collected)",
                            full_content.len()
                        );
                        return Ok(StreamingOutcome::Idle(full_content));
                    }
                    // Known informational events
                    "pending_messages.modified"
                    | "user.message"
                    | "session.usage_info"
                    | "assistant.usage"
                    | "assistant.reasoning"
                    | "tool.execution_partial_result" => {
                        trace!("Tool stream: {}", event_type);
                    }
                    "tool.execution_start" => {
                        self.record_tool_start(&event);
                        trace!("Tool stream: tool.execution_start");
                    }
                    other => {
                        let size = serde_json::to_string(&event).map(|s| s.len()).unwrap_or(0);
                        debug!("Tool stream: unhandled event '{}' ({} bytes)", other, size);
                        trace!(
                            "Tool stream: unhandled event '{}': {}",
                            other,
                            serde_json::to_string(&event).unwrap_or_default()
                        );
                    }
                },
                RoutedMessage::ToolCall { request_id, params } => {
                    debug!(
                        "Tool call received: {} (request_id={})",
                        params.tool_name, request_id
                    );
                    return Ok(StreamingOutcome::ToolCall {
                        text_so_far: full_content,
                        request_id,
                        params,
                    });
                }
            }
        }
    }

    /// Read streaming events with cancellation support.
    ///
    /// Behaves like [`read_streaming`](Self::read_streaming) but can be
    /// aborted via a [`CancellationToken`]. Used by **Quorum Discussion**
    /// where a user may cancel a long-running parallel discussion.
    pub async fn read_streaming_with_cancellation(
        &mut self,
        mut on_chunk: impl FnMut(&str),
        cancellation: CancellationToken,
    ) -> Result<String> {
        let mut full_content = String::new();
        let mut turn_delta_bytes: usize = 0;

        loop {
            if cancellation.is_cancelled() {
                return Err(CopilotError::Cancelled);
            }

            let msg = tokio::select! {
                biased;
                _ = cancellation.cancelled() => {
                    return Err(CopilotError::Cancelled);
                }
                msg = self.rx.recv() => {
                    msg.ok_or(CopilotError::RouterStopped)?
                }
            };

            match msg {
                RoutedMessage::SessionEvent { event_type, event } => match event_type.as_str() {
                    "assistant.message.delta" => {
                        if let Some(data) = event.get("data")
                            && let Some(content) = data.get("content").and_then(|c| c.as_str())
                            && !content.is_empty()
                        {
                            on_chunk(content);
                            full_content.push_str(content);
                            turn_delta_bytes += content.len();
                        }
                    }
                    "assistant.message" | "assistant.message.completed" => {
                        // Use completed content when no deltas were received for this turn.
                        // Unlike checking full_content.is_empty(), using turn_delta_bytes
                        // allows content accumulation across multiple turns.
                        if turn_delta_bytes == 0
                            && let Some(data) = event.get("data")
                            && let Some(content) = data.get("content").and_then(|c| c.as_str())
                            && !content.is_empty()
                        {
                            on_chunk(content);
                            full_content.push_str(content);
                        }
                    }
                    "assistant.turn_start" => {
                        turn_delta_bytes = 0;
                    }
                    "assistant.turn_end" => {
                        if turn_delta_bytes == 0
                            && let Some(text) = extract_event_text(&event)
                        {
                            debug!("Stream: turn_end fallback content ({} bytes)", text.len());
                            on_chunk(&text);
                            full_content.push_str(&text);
                        }
                        debug!(
                            "Stream: assistant.turn_end (turn_deltas: {}, total: {} bytes)",
                            turn_delta_bytes,
                            full_content.len()
                        );
                    }
                    "session.idle" => {
                        debug!(
                            "Session idle, streaming complete ({} bytes)",
                            full_content.len()
                        );
                        return Ok(full_content);
                    }
                    "session.error" => {
                        let error_msg = event
                            .get("data")
                            .and_then(|d| d.get("message"))
                            .and_then(|m| m.as_str())
                            .unwrap_or("Unknown session error");
                        warn!("Session error: {}", error_msg);
                        return Err(CopilotError::RpcError {
                            code: -1,
                            message: error_msg.to_string(),
                        });
                    }
                    "pending_messages.modified"
                    | "user.message"
                    | "session.usage_info"
                    | "assistant.usage"
                    | "assistant.reasoning"
                    | "tool.execution_partial_result" => {
                        trace!("Stream: {}", event_type);
                    }
                    "tool.execution_start" => {
                        self.record_tool_start(&event);
                        trace!("Stream: tool.execution_start");
                    }
                    "tool.execution_complete" => {
                        let size = serde_json::to_string(&event).map(|s| s.len()).unwrap_or(0);
                        debug!("Stream: tool.execution_complete ({} bytes)", size);
                        trace!(
                            "Stream: tool.execution_complete: {}",
                            serde_json::to_string(&event).unwrap_or_default()
                        );
                        self.log_internal_tool_execution(&event);
                    }
                    other => {
                        let size = serde_json::to_string(&event).map(|s| s.len()).unwrap_or(0);
                        debug!("Stream: unhandled event '{}' ({} bytes)", other, size);
                        trace!(
                            "Stream: unhandled event '{}': {}",
                            other,
                            serde_json::to_string(&event).unwrap_or_default()
                        );
                    }
                },
                RoutedMessage::ToolCall { request_id, params } => {
                    warn!(
                        "Unexpected tool.call in read_streaming_with_cancellation: {}, rejecting",
                        params.tool_name
                    );
                    let result =
                        ToolCallResult::error("Tool not available in this session context");
                    let response = JsonRpcResponseOut::new(request_id, result.into_rpc_value());
                    let _ = self.router.send_response(&response).await;
                }
            }
        }
    }

    /// Returns the session ID associated with this channel.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

impl Drop for SessionChannel {
    fn drop(&mut self) {
        self.router.deregister_session(&self.session_id);
    }
}

/// Central message router that demultiplexes a single TCP connection
/// across multiple concurrent Copilot sessions.
///
/// # Responsibilities
///
/// 1. **Spawn** the Copilot CLI process and establish a TCP connection.
/// 2. **Own** the TCP read-half in a background [`tokio::spawn`] task — no
///    `Mutex` on the reader, eliminating cross-session contention.
/// 3. **Route** incoming JSON-RPC messages by `session_id` to per-session
///    [`SessionChannel`]s via `mpsc::UnboundedSender`.
/// 4. **Correlate** request–response pairs via `oneshot` channels (used by
///    [`request`](Self::request)).
/// 5. **Serialize** session creation through [`create_lock`](Self) to prevent
///    `session.start` event mix-ups.
///
/// # Feature usage
///
/// Every feature that talks to an LLM goes through this router:
/// - **Solo mode**: 1 session
/// - **Quorum Discussion**: 3–7 parallel sessions (initial + review + synthesis)
/// - **Ensemble Planning**: N² sessions at peak (plan generation + voting)
/// - **Native Tool Use**: 2 sessions per model (main + tool-enabled)
pub struct MessageRouter {
    /// Background reader task handle.
    _reader_handle: JoinHandle<()>,

    /// Session-specific event channels (session_id -> sender).
    ///
    /// Uses `std::sync::RwLock` (not `tokio::sync::RwLock`) so that
    /// [`deregister_session`](Self::deregister_session) can be called
    /// synchronously from [`SessionChannel::drop`]. The lock is only held
    /// briefly for HashMap insert/remove, so blocking is negligible.
    routes: Arc<std::sync::RwLock<HashMap<String, mpsc::UnboundedSender<RoutedMessage>>>>,

    /// Request-response correlation (request_id -> oneshot sender).
    pending_responses: Arc<RwLock<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>,

    /// Channel for session.start events (consumed during session creation).
    _session_start_tx: mpsc::UnboundedSender<SessionStartEvent>,
    session_start_rx: Mutex<mpsc::UnboundedReceiver<SessionStartEvent>>,

    /// Serializes session creation (prevent concurrent session.start confusion).
    create_lock: Mutex<()>,

    /// Writer (serialized writes, independent of reader).
    ///
    /// Wrapped in `Arc` so the background reader loop can also send error
    /// responses for orphaned `tool.call` requests (sessions already dropped).
    writer: Arc<Mutex<BufWriter<OwnedWriteHalf>>>,

    /// Copilot CLI child process (killed on Drop to prevent orphans).
    child: Child,

    /// Conversation logger for recording internal tool executions.
    conversation_logger: Arc<dyn ConversationLogger>,
}

impl MessageRouter {
    /// Spawn the Copilot CLI (`copilot --server`) and build the router.
    ///
    /// Called by [`CopilotLlmGateway::new`](super::gateway::CopilotLlmGateway::new)
    /// during application startup. The returned `Arc<Self>` is shared by all
    /// [`CopilotSession`](super::session::CopilotSession)s.
    pub async fn spawn() -> Result<Arc<Self>> {
        Self::spawn_internal("copilot", Arc::new(NoConversationLogger)).await
    }

    /// Spawn with a custom command (useful for testing).
    pub async fn spawn_with_command(cmd: &str) -> Result<Arc<Self>> {
        Self::spawn_internal(cmd, Arc::new(NoConversationLogger)).await
    }

    /// Spawn with a conversation logger for recording internal tool executions.
    pub async fn spawn_with_logger(logger: Arc<dyn ConversationLogger>) -> Result<Arc<Self>> {
        Self::spawn_internal("copilot", logger).await
    }

    /// Internal spawn implementation shared by all public constructors.
    async fn spawn_internal(
        cmd: &str,
        conversation_logger: Arc<dyn ConversationLogger>,
    ) -> Result<Arc<Self>> {
        debug!("Spawning Copilot CLI: {} --server", cmd);

        let mut cmd = Command::new(cmd);
        cmd.arg("--server")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        // Linux: request kernel to send SIGTERM to child when parent dies.
        // This catches cases where Drop doesn't run (SIGKILL, OOM kill).
        #[cfg(target_os = "linux")]
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }

        let mut child = cmd.spawn()?;

        // Read stdout to get the port number
        let stdout = child.stdout.take().ok_or_else(|| {
            CopilotError::SpawnError(std::io::Error::other("Failed to capture stdout"))
        })?;

        let mut stdout_reader = BufReader::new(stdout);
        let mut line = String::new();

        let port: u16 = loop {
            line.clear();
            let bytes_read = stdout_reader.read_line(&mut line).await?;
            if bytes_read == 0 {
                return Err(CopilotError::UnexpectedResponse(
                    "Copilot CLI exited without announcing port".into(),
                ));
            }

            let trimmed = line.trim();
            debug!("Copilot CLI output: {}", trimmed);

            if let Some(port_str) = trimmed.strip_prefix("CLI server listening on port ") {
                match port_str.trim().parse::<u16>() {
                    Ok(p) => break p,
                    Err(_) => {
                        return Err(CopilotError::UnexpectedResponse(format!(
                            "Failed to parse port number: {}",
                            port_str
                        )));
                    }
                }
            }
        };

        info!("Copilot CLI listening on port {}, connecting...", port);

        let stream = TcpStream::connect(format!("127.0.0.1:{}", port)).await?;
        let (read_half, write_half) = stream.into_split();

        let routes: Arc<std::sync::RwLock<HashMap<String, mpsc::UnboundedSender<RoutedMessage>>>> =
            Arc::new(std::sync::RwLock::new(HashMap::new()));
        let pending_responses: Arc<RwLock<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let (session_start_tx, session_start_rx) = mpsc::unbounded_channel();
        let writer = Arc::new(Mutex::new(BufWriter::new(write_half)));

        // Clone refs for the background reader task
        let routes_bg = Arc::clone(&routes);
        let pending_bg = Arc::clone(&pending_responses);
        let start_tx_bg = session_start_tx.clone();
        let writer_bg = Arc::clone(&writer);

        let reader_handle = tokio::spawn(async move {
            Self::reader_loop(read_half, routes_bg, pending_bg, start_tx_bg, writer_bg).await;
        });

        let router = Arc::new(Self {
            _reader_handle: reader_handle,
            routes,
            pending_responses,
            _session_start_tx: session_start_tx,
            session_start_rx: Mutex::new(session_start_rx),
            create_lock: Mutex::new(()),
            writer,
            child,
            conversation_logger,
        });

        Ok(router)
    }

    /// Background reader loop — single owner of the TCP read half.
    ///
    /// Runs indefinitely until the TCP connection closes or an I/O error
    /// occurs. Each incoming JSON-RPC message is classified by
    /// [`classify_message`] and dispatched:
    ///
    /// - **Response** → `pending_responses` oneshot (request correlation)
    /// - **Notification `session.start`** → `session_start_tx` (session creation)
    /// - **Notification (other)** → `routes[session_id]` → [`SessionChannel`]
    /// - **IncomingRequest `tool.call`** → `routes[session_id]` → [`SessionChannel`]
    ///
    /// When the loop exits, all senders are dropped so that receivers observe
    /// `None` / `RecvError`, which propagates as [`CopilotError::RouterStopped`].
    async fn reader_loop(
        read_half: OwnedReadHalf,
        routes: Arc<std::sync::RwLock<HashMap<String, mpsc::UnboundedSender<RoutedMessage>>>>,
        pending_responses: Arc<RwLock<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>,
        session_start_tx: mpsc::UnboundedSender<SessionStartEvent>,
        writer: Arc<Mutex<BufWriter<OwnedWriteHalf>>>,
    ) {
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        loop {
            // Read Content-Length header
            let content_length: usize =
                match Self::read_content_length(&mut reader, &mut line).await {
                    Ok(len) => len,
                    Err(e) => {
                        warn!("Reader loop: failed to read content length: {}", e);
                        break;
                    }
                };

            // Skip empty line after headers
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        warn!("Reader loop: connection closed during header skip");
                        return;
                    }
                    Ok(_) => {
                        if line.trim().is_empty() {
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("Reader loop: read error during header skip: {}", e);
                        return;
                    }
                }
            }

            // Read exact content length
            let mut body = vec![0u8; content_length];
            if let Err(e) = reader.read_exact(&mut body).await {
                warn!("Reader loop: failed to read body: {}", e);
                break;
            }

            let body_str = String::from_utf8_lossy(&body);
            trace!("Router received: {}", body_str);

            let json_value: serde_json::Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(e) => {
                    warn!("Router: failed to parse JSON: {} — {}", e, body_str);
                    continue;
                }
            };

            match classify_message(&json_value) {
                // Response to a request we sent
                MessageKind::Response => {
                    if let Some(id) = json_value.get("id").and_then(|v| v.as_u64()) {
                        let response: JsonRpcResponse = match serde_json::from_value(json_value) {
                            Ok(r) => r,
                            Err(e) => {
                                warn!("Router: failed to parse response: {}", e);
                                continue;
                            }
                        };
                        let sender = {
                            let mut pending = pending_responses.write().await;
                            pending.remove(&id)
                        };
                        if let Some(tx) = sender {
                            let _ = tx.send(response);
                        } else {
                            debug!("Router: no pending receiver for response id={}", id);
                        }
                    }
                }

                // Incoming request (e.g. tool.call)
                MessageKind::IncomingRequest { id } => {
                    if let Some(method) = json_value.get("method").and_then(|v| v.as_str()) {
                        if method == "tool.call" {
                            let params: ToolCallParams = match json_value
                                .get("params")
                                .and_then(|p| serde_json::from_value(p.clone()).ok())
                            {
                                Some(p) => p,
                                None => {
                                    warn!("Router: failed to parse tool.call params (id={})", id);
                                    continue;
                                }
                            };

                            let session_id = params.session_id.clone();
                            let routed = {
                                let routes_read = routes.read().unwrap_or_else(|e| e.into_inner());
                                if let Some(tx) = routes_read.get(&session_id) {
                                    let _ = tx.send(RoutedMessage::ToolCall {
                                        request_id: id,
                                        params,
                                    });
                                    true
                                } else {
                                    false
                                }
                            };
                            if !routed {
                                // Session already deregistered (e.g. timeout) — reject
                                // the tool.call so the CLI doesn't hang waiting.
                                warn!(
                                    "Router: no route for tool.call session_id={}, rejecting",
                                    session_id
                                );
                                let result = ToolCallResult::error(
                                    "Session no longer active (timed out or completed)",
                                );
                                let response = JsonRpcResponseOut::new(id, result.into_rpc_value());
                                if let Ok(json) = serde_json::to_string(&response) {
                                    let header = format!("Content-Length: {}\r\n\r\n", json.len());
                                    let mut w = writer.lock().await;
                                    let _ = w.write_all(header.as_bytes()).await;
                                    let _ = w.write_all(json.as_bytes()).await;
                                    let _ = w.flush().await;
                                }
                            }
                        } else {
                            debug!("Router: ignoring incoming request method={}", method);
                        }
                    }
                }

                // Notification (session.event, etc.)
                MessageKind::Notification => {
                    let notification: JsonRpcNotification = match serde_json::from_value(json_value)
                    {
                        Ok(n) => n,
                        Err(e) => {
                            warn!("Router: failed to parse notification: {}", e);
                            continue;
                        }
                    };

                    if notification.method == "session.event" {
                        if let Some(params) = notification.params {
                            // Try to extract session_id and event
                            let session_id = params
                                .get("sessionId")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());

                            let event = params.get("event").cloned();

                            if let (Some(sid), Some(ev)) = (session_id, event) {
                                let event_type = ev
                                    .get("type")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or("")
                                    .to_string();

                                // Check for session.start
                                if event_type == "session.start" {
                                    debug!("Router: session.start for {}", sid);
                                    let _ = session_start_tx
                                        .send(SessionStartEvent { session_id: sid });
                                    continue;
                                }

                                // Route to session channel
                                let routes_read = routes.read().unwrap_or_else(|e| e.into_inner());
                                if let Some(tx) = routes_read.get(&sid) {
                                    let _ = tx.send(RoutedMessage::SessionEvent {
                                        event_type,
                                        event: ev,
                                    });
                                } else {
                                    debug!(
                                        "Router: no route for session_id={}, dropping event type={}",
                                        sid, event_type
                                    );
                                }
                            } else {
                                debug!("Router: session.event without sessionId/event");
                            }
                        }
                    } else {
                        trace!(
                            "Router: ignoring notification method={}",
                            notification.method
                        );
                    }
                }
            }
        }

        // Reader ended — drop all senders so receivers get None
        info!("Router: reader loop ended, closing all session channels");
        {
            let mut routes_w = routes.write().unwrap_or_else(|e| e.into_inner());
            routes_w.clear();
        }
        {
            let mut pending_w = pending_responses.write().await;
            pending_w.clear();
        }
    }

    /// Helper: read the Content-Length header value.
    async fn read_content_length(
        reader: &mut BufReader<OwnedReadHalf>,
        line: &mut String,
    ) -> Result<usize> {
        loop {
            line.clear();
            let bytes_read = reader.read_line(line).await?;
            if bytes_read == 0 {
                return Err(CopilotError::TransportClosed);
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Some(len_str) = trimmed.strip_prefix("Content-Length:")
                && let Ok(len) = len_str.trim().parse::<usize>()
            {
                return Ok(len);
            }
        }
    }

    /// Create a new Copilot session and return its ID + channel.
    ///
    /// Session creation is serialized via `create_lock` to prevent
    /// concurrent `session.create` requests from confusing which
    /// `session.start` event belongs to which caller.
    ///
    /// Called by [`CopilotSession::new`](super::session::CopilotSession::new)
    /// for the main session, and again by
    /// [`CopilotSession::create_tool_session_and_send`](super::session::CopilotSession)
    /// when Native Tool Use needs a separate tool-enabled session.
    pub async fn create_session(
        self: &Arc<Self>,
        params: CreateSessionParams,
    ) -> Result<(String, SessionChannel)> {
        let _guard = self.create_lock.lock().await;

        let params_value = serde_json::to_value(&params)?;
        let params_size = serde_json::to_string(&params_value)
            .map(|s| s.len())
            .unwrap_or(0);
        debug!("session.create params ({} bytes)", params_size);
        trace!(
            "session.create params: {}",
            serde_json::to_string(&params_value).unwrap_or_default()
        );
        let request = JsonRpcRequest::new("session.create", Some(params_value));

        self.send_request(&request).await?;

        // Wait for session.start event with timeout
        let start_event = {
            let mut rx = self.session_start_rx.lock().await;
            match tokio::time::timeout(SESSION_CREATE_TIMEOUT, rx.recv()).await {
                Ok(Some(event)) => event,
                Ok(None) => return Err(CopilotError::RouterStopped),
                Err(_) => {
                    return Err(CopilotError::Timeout(
                        "session.create timed out waiting for session.start".into(),
                    ));
                }
            }
        };

        let session_id = start_event.session_id;
        debug!("Router: session created: {}", session_id);

        // Create the channel pair and register
        let (tx, rx) = mpsc::unbounded_channel();
        {
            let mut routes = self.routes.write().unwrap_or_else(|e| e.into_inner());
            routes.insert(session_id.clone(), tx);
        }

        let channel = SessionChannel {
            rx,
            session_id: session_id.clone(),
            router: Arc::clone(self),
            conversation_logger: Arc::clone(&self.conversation_logger),
            tool_names: HashMap::new(),
        };

        Ok((session_id, channel))
    }

    /// Send a JSON-RPC request and wait for the correlated response.
    ///
    /// Uses a `oneshot` channel: the request ID is registered in
    /// `pending_responses`, and the background reader task fulfils it
    /// when the matching response arrives.
    ///
    /// Used by [`CopilotSession::ask_streaming`](super::session::CopilotSession)
    /// for `session.send` and similar request–response pairs.
    pub async fn request(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        let (tx, rx) = oneshot::channel();
        let request_id = request.id;

        {
            let mut pending = self.pending_responses.write().await;
            pending.insert(request_id, tx);
        }

        if let Err(e) = self.send_request(request).await {
            // Clean up the pending entry to prevent leaks
            let mut pending = self.pending_responses.write().await;
            pending.remove(&request_id);
            return Err(e);
        }

        rx.await.map_err(|_| CopilotError::RouterStopped)
    }

    /// Send a JSON-RPC request without waiting for a response (fire-and-forget).
    ///
    /// Used for `session.create` where the response is an asynchronous
    /// `session.start` event rather than a direct JSON-RPC response.
    pub async fn send_request(&self, request: &JsonRpcRequest) -> Result<()> {
        let request_json = serde_json::to_string(request)?;
        trace!("Router sending: {}", request_json);

        let mut writer = self.writer.lock().await;
        let header = format!("Content-Length: {}\r\n\r\n", request_json.len());
        writer.write_all(header.as_bytes()).await?;
        writer.write_all(request_json.as_bytes()).await?;
        writer.flush().await?;
        Ok(())
    }

    /// Send a JSON-RPC response (SDK → CLI), returning tool execution results.
    ///
    /// Used by **Native Tool Use** — after the agent executes a tool,
    /// [`CopilotSession::send_tool_results`](super::session::CopilotSession)
    /// calls this to deliver the result back to the CLI-side LLM.
    pub async fn send_response(&self, response: &JsonRpcResponseOut) -> Result<()> {
        let response_json = serde_json::to_string(response)?;
        trace!("Router sending response: {}", response_json);

        let mut writer = self.writer.lock().await;
        let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
        writer.write_all(header.as_bytes()).await?;
        writer.write_all(response_json.as_bytes()).await?;
        writer.flush().await?;
        Ok(())
    }

    /// Deregister a session from the routing table.
    ///
    /// Automatically called by [`SessionChannel::drop`] — callers do not
    /// normally need to invoke this directly.
    pub fn deregister_session(&self, session_id: &str) {
        let mut routes = self.routes.write().unwrap_or_else(|e| e.into_inner());
        if routes.remove(session_id).is_some() {
            debug!("Router: deregistered session {}", session_id);
        }
    }
}

impl Drop for MessageRouter {
    fn drop(&mut self) {
        debug!("MessageRouter dropping, killing copilot-cli child process");
        let _ = self.child.start_kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_text_from_string_content() {
        let event = serde_json::json!({
            "type": "assistant.turn_end",
            "data": { "content": "Hello world" }
        });
        assert_eq!(extract_event_text(&event).as_deref(), Some("Hello world"));
    }

    #[test]
    fn extract_text_from_content_blocks_array() {
        let event = serde_json::json!({
            "type": "assistant.turn_end",
            "data": {
                "content": [
                    { "type": "text", "text": "First block" },
                    { "type": "text", "text": "Second block" }
                ]
            }
        });
        assert_eq!(
            extract_event_text(&event).as_deref(),
            Some("First block\nSecond block")
        );
    }

    #[test]
    fn extract_text_from_message_content() {
        let event = serde_json::json!({
            "type": "assistant.turn_end",
            "data": {
                "message": { "role": "assistant", "content": "Nested content" }
            }
        });
        assert_eq!(
            extract_event_text(&event).as_deref(),
            Some("Nested content")
        );
    }

    #[test]
    fn extract_text_from_data_text() {
        let event = serde_json::json!({
            "type": "assistant.turn_end",
            "data": { "text": "Direct text" }
        });
        assert_eq!(extract_event_text(&event).as_deref(), Some("Direct text"));
    }

    #[test]
    fn extract_text_returns_none_for_empty() {
        let event = serde_json::json!({
            "type": "assistant.turn_end",
            "data": { "content": "" }
        });
        assert!(extract_event_text(&event).is_none());
    }

    #[test]
    fn extract_text_returns_none_for_no_data() {
        let event = serde_json::json!({
            "type": "assistant.turn_end"
        });
        assert!(extract_event_text(&event).is_none());
    }

    #[test]
    fn extract_text_skips_non_text_blocks() {
        let event = serde_json::json!({
            "type": "assistant.turn_end",
            "data": {
                "content": [
                    { "type": "tool_use", "name": "create_plan", "input": {} },
                    { "type": "text", "text": "Here is the plan." }
                ]
            }
        });
        assert_eq!(
            extract_event_text(&event).as_deref(),
            Some("Here is the plan.")
        );
    }

    // Tests for extract_tool_name_from_event (Issue #181)

    #[test]
    fn extract_tool_name_from_top_level_tool_name() {
        let event = serde_json::json!({
            "type": "tool.execution_complete",
            "toolName": "apply_patch",
            "result": {}
        });
        assert_eq!(extract_tool_name_from_event(&event), Some("apply_patch"));
    }

    #[test]
    fn extract_tool_name_from_top_level_name() {
        let event = serde_json::json!({
            "type": "tool.execution_complete",
            "name": "read_file",
            "result": {}
        });
        assert_eq!(extract_tool_name_from_event(&event), Some("read_file"));
    }

    #[test]
    fn extract_tool_name_from_data_tool_name() {
        let event = serde_json::json!({
            "type": "tool.execution_complete",
            "data": {
                "toolName": "shell",
                "result": "output"
            }
        });
        assert_eq!(extract_tool_name_from_event(&event), Some("shell"));
    }

    #[test]
    fn extract_tool_name_from_data_name() {
        let event = serde_json::json!({
            "type": "tool.execution_complete",
            "data": {
                "name": "grep_search",
                "output": "found"
            }
        });
        assert_eq!(extract_tool_name_from_event(&event), Some("grep_search"));
    }

    #[test]
    fn extract_tool_name_from_nested_tool_name() {
        let event = serde_json::json!({
            "type": "tool.execution_complete",
            "data": {
                "tool": { "name": "web_fetch" },
                "result": {}
            }
        });
        assert_eq!(extract_tool_name_from_event(&event), Some("web_fetch"));
    }

    #[test]
    fn extract_tool_name_returns_none_for_missing() {
        let event = serde_json::json!({
            "type": "tool.execution_complete",
            "data": { "result": "output" }
        });
        assert_eq!(extract_tool_name_from_event(&event), None);
    }

    #[test]
    fn extract_tool_name_prefers_top_level_over_data() {
        let event = serde_json::json!({
            "type": "tool.execution_complete",
            "toolName": "top_level_tool",
            "data": { "toolName": "data_tool" }
        });
        assert_eq!(extract_tool_name_from_event(&event), Some("top_level_tool"));
    }

    // Tests for extract_tool_call_id

    #[test]
    fn extract_tool_call_id_from_data() {
        let event = serde_json::json!({
            "type": "tool.execution_complete",
            "data": { "toolCallId": "call_abc123" }
        });
        assert_eq!(extract_tool_call_id(&event), Some("call_abc123"));
    }

    #[test]
    fn extract_tool_call_id_returns_none_when_missing() {
        let event = serde_json::json!({
            "type": "tool.execution_complete",
            "data": { "result": "output" }
        });
        assert_eq!(extract_tool_call_id(&event), None);
    }

    // Tests for toolCallId correlation (real Copilot CLI event structure)
    //
    // These tests verify the start→complete correlation logic using
    // the HashMap directly, since SessionChannel requires a live
    // MessageRouter with TCP connections.

    #[test]
    fn tool_call_id_correlation_start_then_complete() {
        // Real Copilot CLI flow: start has toolName, complete does not
        let mut tool_names: HashMap<String, String> = HashMap::new();

        let start_event = serde_json::json!({
            "type": "tool.execution_start",
            "data": {
                "toolCallId": "call_FRQIX6oZmZJYkbqJqZBuZREq",
                "toolName": "read_file",
                "arguments": { "path": "/home/user/Cargo.toml" }
            }
        });

        // Simulate record_tool_start
        if let (Some(call_id), Some(name)) = (
            extract_tool_call_id(&start_event),
            extract_tool_name_from_event(&start_event),
        ) {
            tool_names.insert(call_id.to_string(), name.to_string());
        }

        // Simulate resolve_tool_name for complete event
        let complete_event = serde_json::json!({
            "type": "tool.execution_complete",
            "data": {
                "toolCallId": "call_FRQIX6oZmZJYkbqJqZBuZREq",
                "result": { "content": "file contents..." },
                "success": true
            }
        });
        let call_id = extract_tool_call_id(&complete_event).unwrap();
        let resolved = tool_names
            .get(call_id)
            .map(|s| s.as_str())
            .or_else(|| extract_tool_name_from_event(&complete_event))
            .unwrap_or("unknown");
        assert_eq!(resolved, "read_file");
    }

    #[test]
    fn tool_call_id_correlation_unknown_without_start() {
        let tool_names: HashMap<String, String> = HashMap::new();

        let complete_event = serde_json::json!({
            "type": "tool.execution_complete",
            "data": {
                "toolCallId": "call_unknown",
                "result": { "content": "output" },
                "success": true
            }
        });
        let call_id = extract_tool_call_id(&complete_event).unwrap();
        let resolved = tool_names
            .get(call_id)
            .map(|s| s.as_str())
            .or_else(|| extract_tool_name_from_event(&complete_event))
            .unwrap_or("unknown");
        assert_eq!(resolved, "unknown");
    }

    #[test]
    fn tool_call_id_correlation_multiple_tools() {
        let mut tool_names: HashMap<String, String> = HashMap::new();

        // Two different tool starts
        let start_a = serde_json::json!({
            "data": { "toolCallId": "call_aaa", "toolName": "read_file" }
        });
        let start_b = serde_json::json!({
            "data": { "toolCallId": "call_bbb", "toolName": "glob_search" }
        });
        for ev in [&start_a, &start_b] {
            if let (Some(id), Some(name)) =
                (extract_tool_call_id(ev), extract_tool_name_from_event(ev))
            {
                tool_names.insert(id.to_string(), name.to_string());
            }
        }

        // Complete events arrive in different order
        let complete_b = serde_json::json!({
            "data": { "toolCallId": "call_bbb", "success": true }
        });
        let complete_a = serde_json::json!({
            "data": { "toolCallId": "call_aaa", "success": true }
        });

        let resolve = |ev: &serde_json::Value| -> String {
            extract_tool_call_id(ev)
                .and_then(|id| tool_names.get(id))
                .map(|s| s.to_string())
                .or_else(|| extract_tool_name_from_event(ev).map(|s| s.to_string()))
                .unwrap_or_else(|| "unknown".to_string())
        };

        assert_eq!(resolve(&complete_b), "glob_search");
        assert_eq!(resolve(&complete_a), "read_file");
    }

    #[test]
    fn tool_call_id_real_copilot_cli_events() {
        // Test with actual Copilot CLI event shapes captured from logs
        let mut tool_names: HashMap<String, String> = HashMap::new();

        let start = serde_json::json!({
            "type": "tool.execution_start",
            "id": "f9ac3120-246a-4851-b8fe-15b2eec6a565",
            "parentId": "66cb6507-efae-418f-a699-54723cec5b6e",
            "timestamp": "2026-03-03T15:46:05.901Z",
            "data": {
                "toolCallId": "call_FRQIX6oZmZJYkbqJqZBuZREq",
                "toolName": "read_file",
                "arguments": { "path": "/home/archie/workspace/copilot-quorum/Cargo.toml" }
            }
        });
        if let (Some(id), Some(name)) = (
            extract_tool_call_id(&start),
            extract_tool_name_from_event(&start),
        ) {
            tool_names.insert(id.to_string(), name.to_string());
        }

        let complete = serde_json::json!({
            "type": "tool.execution_complete",
            "id": "27e1f6a6-6ba4-4dfb-bbcc-d7b83a22e251",
            "parentId": "2439aafc-200e-440c-8f8a-5596f4279521",
            "timestamp": "2026-03-03T15:38:43.412Z",
            "data": {
                "interactionId": "ce472ca8-5fb6-45c0-ad9d-b20d3e1628f7",
                "model": "gpt-5.3-codex",
                "result": { "content": "[workspace]...", "detailedContent": "[workspace]..." },
                "success": true,
                "toolCallId": "call_FRQIX6oZmZJYkbqJqZBuZREq"
            }
        });
        let call_id = extract_tool_call_id(&complete).unwrap();
        let resolved = tool_names
            .get(call_id)
            .map(|s| s.as_str())
            .or_else(|| extract_tool_name_from_event(&complete))
            .unwrap_or("unknown");
        assert_eq!(resolved, "read_file");

        // Also verify that extract_tool_name_from_event finds nothing in the complete event
        assert_eq!(extract_tool_name_from_event(&complete), None);
    }
}
