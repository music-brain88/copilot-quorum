//! Transport layer for Copilot CLI communication.
//!
//! Handles the low-level JSON-RPC communication with the Copilot CLI process,
//! including process spawning, TCP connection, and message serialization.

use crate::copilot::error::{CopilotError, Result};
use crate::copilot::protocol::{
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, JsonRpcResponseOut, SessionEventParams,
    ToolCallParams,
};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, trace, warn};

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

/// Outcome of `read_streaming_for_tools()`.
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

/// Transport for communicating with Copilot CLI via TCP.
///
/// Manages the Copilot CLI child process and TCP socket connection,
/// providing methods for sending JSON-RPC requests and receiving responses.
pub struct StdioTransport {
    #[allow(dead_code)]
    child: Child,
    reader: Mutex<BufReader<tokio::net::tcp::OwnedReadHalf>>,
    writer: Mutex<BufWriter<tokio::net::tcp::OwnedWriteHalf>>,
}

impl StdioTransport {
    /// Spawn a new Copilot CLI process and create a transport
    pub async fn spawn() -> Result<Self> {
        Self::spawn_with_command("copilot").await
    }

    /// Spawn with a custom command (useful for testing)
    pub async fn spawn_with_command(cmd: &str) -> Result<Self> {
        debug!("Spawning Copilot CLI: {} --server", cmd);

        let mut child = Command::new(cmd)
            .arg("--server")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        // Read stdout to get the port number
        let stdout = child.stdout.take().ok_or_else(|| {
            CopilotError::SpawnError(std::io::Error::other("Failed to capture stdout"))
        })?;

        let mut stdout_reader = BufReader::new(stdout);
        let mut line = String::new();

        // Read lines until we find the port announcement
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

            // Parse "CLI server listening on port XXXXX"
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

        // Connect to the TCP port
        let stream = TcpStream::connect(format!("127.0.0.1:{}", port)).await?;
        let (read_half, write_half) = stream.into_split();

        Ok(Self {
            child,
            reader: Mutex::new(BufReader::new(read_half)),
            writer: Mutex::new(BufWriter::new(write_half)),
        })
    }

    /// Send a JSON-RPC request without waiting for response
    pub async fn send_request(&self, request: &JsonRpcRequest) -> Result<()> {
        let request_json = serde_json::to_string(request)?;
        trace!("Sending request: {}", request_json);

        // Send request with Content-Length header (LSP-style framing)
        {
            let mut writer = self.writer.lock().await;
            let header = format!("Content-Length: {}\r\n\r\n", request_json.len());
            writer.write_all(header.as_bytes()).await?;
            writer.write_all(request_json.as_bytes()).await?;
            writer.flush().await?;
        }
        Ok(())
    }

    /// Send a JSON-RPC request and wait for a response
    pub async fn request(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        self.send_request(request).await?;
        // Read response
        let response = self.read_response().await?;
        Ok(response)
    }

    /// Read a single JSON-RPC response (with Content-Length header)
    async fn read_response(&self) -> Result<JsonRpcResponse> {
        let mut reader = self.reader.lock().await;
        let mut line = String::new();

        // Read Content-Length header
        let content_length: usize = loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line).await?;

            if bytes_read == 0 {
                return Err(CopilotError::TransportClosed);
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Some(len_str) = trimmed.strip_prefix("Content-Length:") {
                match len_str.trim().parse::<usize>() {
                    Ok(len) => break len,
                    Err(_) => {
                        debug!("Invalid Content-Length: {}", trimmed);
                        continue;
                    }
                }
            } else {
                debug!("Skipping non-header line: {}", trimmed);
            }
        };

        // Skip empty line after headers
        loop {
            line.clear();
            reader.read_line(&mut line).await?;
            if line.trim().is_empty() {
                break;
            }
        }

        // Read exact content length
        let mut body = vec![0u8; content_length];
        use tokio::io::AsyncReadExt;
        reader.read_exact(&mut body).await?;

        let body_str = String::from_utf8_lossy(&body);
        trace!("Received response: {}", body_str);

        let response: JsonRpcResponse = serde_json::from_slice(&body).map_err(|e| {
            warn!("Failed to parse JSON response: {}", body_str);
            CopilotError::ParseError {
                error: e.to_string(),
                raw: body_str.to_string(),
            }
        })?;
        Ok(response)
    }

    /// Wait for session.start event and return the session ID
    pub async fn wait_for_session_start(&self) -> Result<String> {
        let mut reader = self.reader.lock().await;
        let mut line = String::new();

        loop {
            // Read Content-Length header
            let content_length: usize = loop {
                line.clear();
                let bytes_read = reader.read_line(&mut line).await?;

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
                    break len;
                }
            };

            // Skip empty line after headers
            loop {
                line.clear();
                reader.read_line(&mut line).await?;
                if line.trim().is_empty() {
                    break;
                }
            }

            // Read exact content length
            let mut body = vec![0u8; content_length];
            use tokio::io::AsyncReadExt;
            reader.read_exact(&mut body).await?;

            let body_str = String::from_utf8_lossy(&body);
            trace!("Received message: {}", body_str);

            // Try to parse and look for session.event with session.start
            if let Ok(notification) = serde_json::from_slice::<JsonRpcNotification>(&body)
                && notification.method == "session.event"
                && let Some(params) = notification.params
                && let Ok(event_params) = serde_json::from_value::<SessionEventParams>(params)
                && event_params.event.event_type == "session.start"
            {
                debug!("Got session.start with id: {}", event_params.session_id);
                return Ok(event_params.session_id);
            }
            // If not session.start, continue waiting
        }
    }

    /// Read streaming notifications until session.idle
    pub async fn read_streaming<F>(&self, mut on_chunk: F) -> Result<String>
    where
        F: FnMut(&str),
    {
        let mut reader = self.reader.lock().await;
        let mut line = String::new();
        let mut full_content = String::new();

        loop {
            // Read Content-Length header
            let content_length: usize = loop {
                line.clear();
                let bytes_read = reader.read_line(&mut line).await?;

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
                    break len;
                }
            };

            // Skip empty line after headers
            loop {
                line.clear();
                reader.read_line(&mut line).await?;
                if line.trim().is_empty() {
                    break;
                }
            }

            // Read exact content length
            let mut body = vec![0u8; content_length];
            use tokio::io::AsyncReadExt;
            reader.read_exact(&mut body).await?;

            let body_str = String::from_utf8_lossy(&body);
            trace!("Received message: {}", body_str);

            // Check if it's a response (has "id" and "result") or notification (has "method")
            let json_value: serde_json::Value = serde_json::from_slice(&body).map_err(|e| {
                warn!("Failed to parse JSON: {}", body_str);
                CopilotError::ParseError {
                    error: e.to_string(),
                    raw: body_str.to_string(),
                }
            })?;

            // Skip responses (they have "id" and "result" but no "method")
            if json_value.get("id").is_some() && json_value.get("method").is_none() {
                debug!("Skipping response in streaming: {}", body_str);
                continue;
            }

            // Parse as notification
            let notification: JsonRpcNotification =
                serde_json::from_value(json_value).map_err(|e| {
                    warn!("Failed to parse notification: {}", body_str);
                    CopilotError::ParseError {
                        error: e.to_string(),
                        raw: body_str.to_string(),
                    }
                })?;

            // Handle session.event notifications
            if notification.method == "session.event" {
                if let Some(params) = notification.params {
                    // Extract event type from params.event.type
                    if let Some(event) = params.get("event")
                        && let Some(event_type) = event.get("type").and_then(|t| t.as_str())
                    {
                        match event_type {
                            "assistant.message.delta" => {
                                // Extract content from event.data.content
                                if let Some(data) = event.get("data")
                                    && let Some(content) =
                                        data.get("content").and_then(|c| c.as_str())
                                    && !content.is_empty()
                                {
                                    on_chunk(content);
                                    full_content.push_str(content);
                                }
                            }
                            "assistant.message" => {
                                // Final message, extract content
                                if let Some(data) = event.get("data")
                                    && let Some(content) =
                                        data.get("content").and_then(|c| c.as_str())
                                    && !content.is_empty()
                                    && full_content.is_empty()
                                {
                                    // Only use if we haven't gotten deltas
                                    on_chunk(content);
                                    full_content.push_str(content);
                                }
                            }
                            "session.idle" => {
                                debug!("Session idle, streaming complete");
                                return Ok(full_content);
                            }
                            other => {
                                trace!("Ignoring event type: {}", other);
                            }
                        }
                    }
                }
            } else {
                debug!("Ignoring notification method: {}", notification.method);
            }
        }
    }

    /// Send a JSON-RPC response (SDK → CLI), e.g. for tool.call results.
    pub async fn send_response(&self, response: &JsonRpcResponseOut) -> Result<()> {
        let response_json = serde_json::to_string(response)?;
        trace!("Sending response: {}", response_json);

        let mut writer = self.writer.lock().await;
        let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
        writer.write_all(header.as_bytes()).await?;
        writer.write_all(response_json.as_bytes()).await?;
        writer.flush().await?;
        Ok(())
    }

    /// Read streaming notifications until session.idle or a tool.call request.
    ///
    /// Unlike [`read_streaming()`](Self::read_streaming), this method also
    /// detects incoming `tool.call` JSON-RPC requests and returns early with
    /// [`StreamingOutcome::ToolCall`] so the caller can execute the tool and
    /// send back a response.
    pub async fn read_streaming_for_tools<F>(&self, mut on_chunk: F) -> Result<StreamingOutcome>
    where
        F: FnMut(&str),
    {
        let mut reader = self.reader.lock().await;
        let mut line = String::new();
        let mut full_content = String::new();

        loop {
            // Read Content-Length header
            let content_length: usize = loop {
                line.clear();
                let bytes_read = reader.read_line(&mut line).await?;

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
                    break len;
                }
            };

            // Skip empty line after headers
            loop {
                line.clear();
                reader.read_line(&mut line).await?;
                if line.trim().is_empty() {
                    break;
                }
            }

            // Read exact content length
            let mut body = vec![0u8; content_length];
            use tokio::io::AsyncReadExt;
            reader.read_exact(&mut body).await?;

            let body_str = String::from_utf8_lossy(&body);
            trace!("Received message: {}", body_str);

            let json_value: serde_json::Value = serde_json::from_slice(&body).map_err(|e| {
                warn!("Failed to parse JSON: {}", body_str);
                CopilotError::ParseError {
                    error: e.to_string(),
                    raw: body_str.to_string(),
                }
            })?;

            match classify_message(&json_value) {
                MessageKind::IncomingRequest { id } => {
                    // Check if it's a tool.call
                    if let Some(method) = json_value.get("method").and_then(|v| v.as_str())
                        && method == "tool.call"
                    {
                        let params: ToolCallParams = json_value
                            .get("params")
                            .and_then(|p| serde_json::from_value(p.clone()).ok())
                            .ok_or_else(|| {
                                CopilotError::ToolCallError(
                                    "Failed to parse tool.call params".to_string(),
                                )
                            })?;

                        debug!("Received tool.call: {} (id={})", params.tool_name, id);

                        return Ok(StreamingOutcome::ToolCall {
                            text_so_far: full_content,
                            request_id: id,
                            params,
                        });
                    }
                    debug!("Skipping incoming request: {}", body_str);
                }
                MessageKind::Response => {
                    debug!("Skipping response in streaming: {}", body_str);
                }
                MessageKind::Notification => {
                    let notification: JsonRpcNotification = serde_json::from_value(json_value)
                        .map_err(|e| {
                            warn!("Failed to parse notification: {}", body_str);
                            CopilotError::ParseError {
                                error: e.to_string(),
                                raw: body_str.to_string(),
                            }
                        })?;

                    if notification.method == "session.event"
                        && let Some(params) = notification.params
                        && let Some(event) = params.get("event")
                        && let Some(event_type) = event.get("type").and_then(|t| t.as_str())
                    {
                        match event_type {
                            "assistant.message.delta" => {
                                if let Some(data) = event.get("data")
                                    && let Some(content) =
                                        data.get("content").and_then(|c| c.as_str())
                                    && !content.is_empty()
                                {
                                    on_chunk(content);
                                    full_content.push_str(content);
                                }
                            }
                            "assistant.message" => {
                                if let Some(data) = event.get("data")
                                    && let Some(content) =
                                        data.get("content").and_then(|c| c.as_str())
                                    && !content.is_empty()
                                    && full_content.is_empty()
                                {
                                    on_chunk(content);
                                    full_content.push_str(content);
                                }
                            }
                            "session.idle" => {
                                debug!("Session idle, streaming complete");
                                return Ok(StreamingOutcome::Idle(full_content));
                            }
                            other => {
                                trace!("Ignoring event type: {}", other);
                            }
                        }
                    } else if notification.method != "session.event" {
                        debug!("Ignoring notification method: {}", notification.method);
                    }
                }
            }
        }
    }

    /// Read streaming notifications until session.idle with cancellation support
    pub async fn read_streaming_with_cancellation<F>(
        &self,
        mut on_chunk: F,
        cancellation: CancellationToken,
    ) -> Result<String>
    where
        F: FnMut(&str),
    {
        let mut reader = self.reader.lock().await;
        let mut line = String::new();
        let mut full_content = String::new();

        loop {
            // Check for cancellation
            if cancellation.is_cancelled() {
                return Err(CopilotError::Cancelled);
            }

            // Read Content-Length header with cancellation support
            let content_length: usize = loop {
                line.clear();

                // Use select! to allow cancellation during read
                let bytes_read = tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => {
                        return Err(CopilotError::Cancelled);
                    }
                    result = reader.read_line(&mut line) => result?,
                };

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
                    break len;
                }
            };

            // Skip empty line after headers
            loop {
                line.clear();
                reader.read_line(&mut line).await?;
                if line.trim().is_empty() {
                    break;
                }
            }

            // Read exact content length with cancellation support
            let mut body = vec![0u8; content_length];
            use tokio::io::AsyncReadExt;

            tokio::select! {
                biased;
                _ = cancellation.cancelled() => {
                    return Err(CopilotError::Cancelled);
                }
                result = reader.read_exact(&mut body) => { result?; }
            }

            let body_str = String::from_utf8_lossy(&body);
            trace!("Received message: {}", body_str);

            // Check if it's a response (has "id" and "result") or notification (has "method")
            let json_value: serde_json::Value = serde_json::from_slice(&body).map_err(|e| {
                warn!("Failed to parse JSON: {}", body_str);
                CopilotError::ParseError {
                    error: e.to_string(),
                    raw: body_str.to_string(),
                }
            })?;

            // Skip responses (they have "id" and "result" but no "method")
            if json_value.get("id").is_some() && json_value.get("method").is_none() {
                debug!("Skipping response in streaming: {}", body_str);
                continue;
            }

            // Parse as notification
            let notification: JsonRpcNotification =
                serde_json::from_value(json_value).map_err(|e| {
                    warn!("Failed to parse notification: {}", body_str);
                    CopilotError::ParseError {
                        error: e.to_string(),
                        raw: body_str.to_string(),
                    }
                })?;

            // Handle session.event notifications
            if notification.method == "session.event" {
                if let Some(params) = notification.params {
                    // Extract event type from params.event.type
                    if let Some(event) = params.get("event")
                        && let Some(event_type) = event.get("type").and_then(|t| t.as_str())
                    {
                        match event_type {
                            "assistant.message.delta" => {
                                // Extract content from event.data.content
                                if let Some(data) = event.get("data")
                                    && let Some(content) =
                                        data.get("content").and_then(|c| c.as_str())
                                    && !content.is_empty()
                                {
                                    on_chunk(content);
                                    full_content.push_str(content);
                                }
                            }
                            "assistant.message" => {
                                // Final message, extract content
                                if let Some(data) = event.get("data")
                                    && let Some(content) =
                                        data.get("content").and_then(|c| c.as_str())
                                    && !content.is_empty()
                                    && full_content.is_empty()
                                {
                                    // Only use if we haven't gotten deltas
                                    on_chunk(content);
                                    full_content.push_str(content);
                                }
                            }
                            "session.idle" => {
                                debug!("Session idle, streaming complete");
                                return Ok(full_content);
                            }
                            other => {
                                trace!("Ignoring event type: {}", other);
                            }
                        }
                    }
                }
            } else {
                debug!("Ignoring notification method: {}", notification.method);
            }
        }
    }
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
