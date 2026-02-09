//! Message router for demultiplexing Copilot CLI events to sessions.
//!
//! The Copilot CLI communicates over a single TCP connection, but multiple
//! sessions can be active concurrently (e.g. during Ensemble mode).
//! [`MessageRouter`] runs a background reader task that owns the TCP reader
//! exclusively (no `Mutex` contention) and routes incoming messages to the
//! correct [`SessionChannel`] by `session_id`.

use crate::copilot::error::{CopilotError, Result};
use crate::copilot::protocol::{
    CreateSessionParams, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, JsonRpcResponseOut,
    ToolCallParams,
};
use crate::copilot::transport::{MessageKind, StreamingOutcome, classify_message};
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

/// A message routed to a specific session's channel.
#[derive(Debug)]
pub enum RoutedMessage {
    /// A `session.event` notification (delta, message, idle, etc.).
    SessionEvent {
        event_type: String,
        event: serde_json::Value,
    },
    /// An incoming `tool.call` request from the CLI.
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

/// A per-session channel for receiving routed messages.
///
/// Each [`CopilotSession`](super::session::CopilotSession) owns a
/// `SessionChannel` for its lifetime.  When dropped, the session is
/// automatically deregistered from the router.
pub struct SessionChannel {
    rx: mpsc::UnboundedReceiver<RoutedMessage>,
    session_id: String,
    router: Arc<MessageRouter>,
}

impl SessionChannel {
    /// Receive the next routed message (blocks until available).
    pub async fn recv(&mut self) -> Result<RoutedMessage> {
        self.rx.recv().await.ok_or(CopilotError::RouterStopped)
    }

    /// Read streaming session events until `session.idle`, calling `on_chunk`
    /// for each text delta.
    pub async fn read_streaming(&mut self, mut on_chunk: impl FnMut(&str)) -> Result<String> {
        let mut full_content = String::new();

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
                        }
                    }
                    "assistant.message" => {
                        if let Some(data) = event.get("data")
                            && let Some(content) = data.get("content").and_then(|c| c.as_str())
                            && !content.is_empty()
                            && full_content.is_empty()
                        {
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
                },
                RoutedMessage::ToolCall { .. } => {
                    warn!("Unexpected tool.call in read_streaming, ignoring");
                }
            }
        }
    }

    /// Read streaming events until `session.idle` **or** `tool.call`.
    pub async fn read_streaming_for_tools(
        &mut self,
        mut on_chunk: impl FnMut(&str),
    ) -> Result<StreamingOutcome> {
        let mut full_content = String::new();

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
                        }
                    }
                    "assistant.message" => {
                        if let Some(data) = event.get("data")
                            && let Some(content) = data.get("content").and_then(|c| c.as_str())
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
    pub async fn read_streaming_with_cancellation(
        &mut self,
        mut on_chunk: impl FnMut(&str),
        cancellation: CancellationToken,
    ) -> Result<String> {
        let mut full_content = String::new();

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
                        }
                    }
                    "assistant.message" => {
                        if let Some(data) = event.get("data")
                            && let Some(content) = data.get("content").and_then(|c| c.as_str())
                            && !content.is_empty()
                            && full_content.is_empty()
                        {
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
                },
                RoutedMessage::ToolCall { .. } => {
                    warn!("Unexpected tool.call in read_streaming_with_cancellation, ignoring");
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
pub struct MessageRouter {
    /// Background reader task handle.
    _reader_handle: JoinHandle<()>,

    /// Session-specific event channels (session_id -> sender).
    routes: Arc<RwLock<HashMap<String, mpsc::UnboundedSender<RoutedMessage>>>>,

    /// Request-response correlation (request_id -> oneshot sender).
    pending_responses: Arc<RwLock<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>,

    /// Channel for session.start events (consumed during session creation).
    _session_start_tx: mpsc::UnboundedSender<SessionStartEvent>,
    session_start_rx: Mutex<mpsc::UnboundedReceiver<SessionStartEvent>>,

    /// Serializes session creation (prevent concurrent session.start confusion).
    create_lock: Mutex<()>,

    /// Writer (serialized writes, independent of reader).
    writer: Mutex<BufWriter<OwnedWriteHalf>>,

    /// Copilot CLI child process.
    _child: Child,
}

impl MessageRouter {
    /// Spawn the Copilot CLI and build the router.
    pub async fn spawn() -> Result<Arc<Self>> {
        Self::spawn_with_command("copilot").await
    }

    /// Spawn with a custom command (useful for testing).
    pub async fn spawn_with_command(cmd: &str) -> Result<Arc<Self>> {
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

        let routes: Arc<RwLock<HashMap<String, mpsc::UnboundedSender<RoutedMessage>>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let pending_responses: Arc<RwLock<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let (session_start_tx, session_start_rx) = mpsc::unbounded_channel();

        // Clone refs for the background reader task
        let routes_bg = Arc::clone(&routes);
        let pending_bg = Arc::clone(&pending_responses);
        let start_tx_bg = session_start_tx.clone();

        let reader_handle = tokio::spawn(async move {
            Self::reader_loop(read_half, routes_bg, pending_bg, start_tx_bg).await;
        });

        let router = Arc::new(Self {
            _reader_handle: reader_handle,
            routes,
            pending_responses,
            _session_start_tx: session_start_tx,
            session_start_rx: Mutex::new(session_start_rx),
            create_lock: Mutex::new(()),
            writer: Mutex::new(BufWriter::new(write_half)),
            _child: child,
        });

        Ok(router)
    }

    /// Background reader loop — single owner of the TCP read half.
    async fn reader_loop(
        read_half: OwnedReadHalf,
        routes: Arc<RwLock<HashMap<String, mpsc::UnboundedSender<RoutedMessage>>>>,
        pending_responses: Arc<RwLock<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>,
        session_start_tx: mpsc::UnboundedSender<SessionStartEvent>,
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
                            let routes_read = routes.read().await;
                            if let Some(tx) = routes_read.get(&session_id) {
                                let _ = tx.send(RoutedMessage::ToolCall {
                                    request_id: id,
                                    params,
                                });
                            } else {
                                warn!("Router: no route for tool.call session_id={}", session_id);
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
                                let routes_read = routes.read().await;
                                if let Some(tx) = routes_read.get(&sid) {
                                    let _ = tx.send(RoutedMessage::SessionEvent {
                                        event_type,
                                        event: ev,
                                    });
                                } else {
                                    trace!(
                                        "Router: no route for session_id={}, dropping event",
                                        sid
                                    );
                                }
                            } else {
                                trace!("Router: session.event without sessionId/event");
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
            let mut routes_w = routes.write().await;
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
    /// Session creation is serialized via `create_lock` to avoid
    /// mixing up `session.start` events from concurrent creates.
    pub async fn create_session(
        self: &Arc<Self>,
        params: CreateSessionParams,
    ) -> Result<(String, SessionChannel)> {
        let _guard = self.create_lock.lock().await;

        let request = JsonRpcRequest::new("session.create", Some(serde_json::to_value(&params)?));

        self.send_request(&request).await?;

        // Wait for session.start event
        let start_event = {
            let mut rx = self.session_start_rx.lock().await;
            rx.recv().await.ok_or(CopilotError::RouterStopped)?
        };

        let session_id = start_event.session_id;
        debug!("Router: session created: {}", session_id);

        // Create the channel pair and register
        let (tx, rx) = mpsc::unbounded_channel();
        {
            let mut routes = self.routes.write().await;
            routes.insert(session_id.clone(), tx);
        }

        let channel = SessionChannel {
            rx,
            session_id: session_id.clone(),
            router: Arc::clone(self),
        };

        Ok((session_id, channel))
    }

    /// Send a JSON-RPC request and wait for the correlated response.
    pub async fn request(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        let (tx, rx) = oneshot::channel();

        {
            let mut pending = self.pending_responses.write().await;
            pending.insert(request.id, tx);
        }

        self.send_request(request).await?;

        rx.await.map_err(|_| CopilotError::RouterStopped)
    }

    /// Send a JSON-RPC request (fire-and-forget).
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

    /// Send a JSON-RPC response (SDK -> CLI), e.g. for tool.call results.
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
    pub fn deregister_session(&self, session_id: &str) {
        let routes = Arc::clone(&self.routes);
        let session_id = session_id.to_string();
        tokio::spawn(async move {
            let mut routes = routes.write().await;
            if routes.remove(&session_id).is_some() {
                debug!("Router: deregistered session {}", session_id);
            }
        });
    }
}
