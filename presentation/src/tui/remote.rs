//! Remote Control API — drive the TUI from an external process.
//!
//! When the TUI is started with `--listen <PATH>`, a JSON-RPC server is
//! exposed on a Unix domain socket (LSP-style `Content-Length` framing,
//! the same wire format as `copilot --server`). External agents can
//! inspect state and inject input as a peer of the keyboard:
//!
//! ```text
//! agent ──JSON-RPC──> UnixListener task ──remote_tx──> TuiApp select! loop
//!                                                        │  (&mut TuiState)
//! agent <──response── oneshot reply <─────────────────────┘
//! ```
//!
//! Every request is handled *inside* the main event loop with full access
//! to `TuiState` and the controller `cmd_tx`, so remote operations follow
//! exactly the same code paths as keyboard input (`KeyAction::SubmitInput`
//! → `TuiCommand::ProcessRequest`, etc.).
//!
//! # Methods (Phase 1)
//!
//! | method                 | params                                | effect |
//! |------------------------|---------------------------------------|--------|
//! | `state.get`            | —                                     | mode, models, tabs, pending HiL |
//! | `panes.list`           | —                                     | all tabs/panes with metadata |
//! | `pane.read`            | `{tab?: usize, last?: usize}`         | conversation messages (structured) |
//! | `input.send`           | `{text: string}`                      | submit prompt to active pane |
//! | `command.exec`         | `{command: string}`                   | run `:command` (e.g. "solo", "tabnew ask") |
//! | `interaction.spawn`    | `{form: "agent"\|"ask"\|"discuss", query: string}` | spawn interaction (new tab) |
//! | `interaction.activate` | `{interaction_id: usize}`             | focus an interaction's tab |
//! | `hil.respond`          | `{decision: "approve"\|"reject"}`     | answer a pending HiL modal |
//!
//! Security: the socket is created with `0600` permissions and no TCP
//! listener is offered — same trust model as `nvim --listen`.

use super::event::TuiCommand;
use super::state::{DisplayMessage, MessageRole, TuiState};
use super::tab::PaneKind;
use quorum_domain::HumanDecision;
use quorum_domain::interaction::InteractionForm;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

/// A remote JSON-RPC request forwarded into the TUI main loop.
///
/// `reply` is `None` for JSON-RPC notifications (requests without `id`).
pub struct RemoteRequest {
    pub method: String,
    pub params: Value,
    pub reply: Option<oneshot::Sender<Result<Value, RemoteError>>>,
}

/// JSON-RPC error (code + message) returned to the remote client.
pub struct RemoteError {
    pub code: i64,
    pub message: String,
}

impl RemoteError {
    fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("Method not found: {method}"),
        }
    }

    fn invalid_params(msg: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: msg.into(),
        }
    }

    fn failed(msg: impl Into<String>) -> Self {
        Self {
            code: -32000,
            message: msg.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Socket listener
// ---------------------------------------------------------------------------

/// Bind the Unix socket and spawn the accept loop.
///
/// A stale socket file from a previous run is removed before binding.
/// The socket is chmod'ed to `0600` so only the owning user can connect.
pub fn spawn_listener(
    path: PathBuf,
    tx: mpsc::UnboundedSender<RemoteRequest>,
) -> std::io::Result<()> {
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    debug!("Remote control listening on {}", path.display());

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(stream, tx).await {
                            debug!("Remote connection closed: {}", e);
                        }
                    });
                }
                Err(e) => {
                    warn!("Remote accept failed: {}", e);
                    break;
                }
            }
        }
    });
    Ok(())
}

/// Serve one client connection: read framed requests, forward each into
/// the main loop, and write the framed response back. Requests on a single
/// connection are processed sequentially.
async fn handle_connection(
    stream: UnixStream,
    tx: mpsc::UnboundedSender<RemoteRequest>,
) -> std::io::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    loop {
        let body = match read_frame(&mut reader).await? {
            Some(b) => b,
            None => return Ok(()), // clean EOF
        };

        let msg: Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                write_frame(
                    &mut write_half,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": null,
                        "error": {"code": -32700, "message": format!("Parse error: {e}")},
                    }),
                )
                .await?;
                continue;
            }
        };

        let id = msg.get("id").cloned();
        let method = msg
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or_default()
            .to_string();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        let (reply_tx, reply_rx) = oneshot::channel();
        let is_notification = id.is_none();
        let request = RemoteRequest {
            method,
            params,
            reply: if is_notification {
                None
            } else {
                Some(reply_tx)
            },
        };

        if tx.send(request).is_err() {
            // Main loop is gone — shut the connection down.
            return Ok(());
        }
        if is_notification {
            continue;
        }

        let response = match reply_rx.await {
            Ok(Ok(result)) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Ok(Err(e)) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": e.code, "message": e.message},
            }),
            Err(_) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32000, "message": "TUI dropped the request"},
            }),
        };
        write_frame(&mut write_half, &response).await?;
    }
}

/// Read one `Content-Length`-framed JSON body. Returns `None` on clean EOF.
async fn read_frame<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
) -> std::io::Result<Option<Vec<u8>>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if content_length.is_some() {
                break;
            }
            continue; // stray blank line before headers
        }
        if let Some(rest) = trimmed
            .strip_prefix("Content-Length:")
            .or_else(|| trimmed.strip_prefix("content-length:"))
        {
            content_length = rest.trim().parse::<usize>().ok();
        }
        // other headers (Content-Type etc.) are ignored
    }
    let len = content_length.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing Content-Length")
    })?;
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;
    Ok(Some(body))
}

/// Write one `Content-Length`-framed JSON message.
async fn write_frame<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    msg: &Value,
) -> std::io::Result<()> {
    let body = serde_json::to_vec(msg)?;
    writer
        .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .await?;
    writer.write_all(&body).await?;
    writer.flush().await
}

// ---------------------------------------------------------------------------
// Request handling (runs inside the TuiApp select! loop)
// ---------------------------------------------------------------------------

/// Dispatch a remote request with full access to the TUI state.
///
/// Called from the main event loop, so mutations here are exactly as safe
/// (and as visible) as those triggered by keyboard input.
pub(super) fn handle_request(
    state: &mut TuiState,
    cmd_tx: &mpsc::UnboundedSender<TuiCommand>,
    pending_hil_tx: &Arc<Mutex<Option<oneshot::Sender<HumanDecision>>>>,
    request: RemoteRequest,
) {
    let result = dispatch(
        state,
        cmd_tx,
        pending_hil_tx,
        &request.method,
        &request.params,
    );
    if let Some(reply) = request.reply {
        let _ = reply.send(result);
    }
}

fn dispatch(
    state: &mut TuiState,
    cmd_tx: &mpsc::UnboundedSender<TuiCommand>,
    pending_hil_tx: &Arc<Mutex<Option<oneshot::Sender<HumanDecision>>>>,
    method: &str,
    params: &Value,
) -> Result<Value, RemoteError> {
    match method {
        "state.get" => Ok(state_snapshot(state)),
        "panes.list" => Ok(panes_list(state)),
        "pane.read" => pane_read(state, params),
        "input.send" => input_send(state, cmd_tx, params),
        "command.exec" => command_exec(state, cmd_tx, params),
        "interaction.spawn" => interaction_spawn(cmd_tx, params),
        "interaction.activate" => interaction_activate(cmd_tx, params),
        "hil.respond" => hil_respond(state, pending_hil_tx, params),
        other => Err(RemoteError::method_not_found(other)),
    }
}

fn role_label(role: MessageRole) -> &'static str {
    match role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => "system",
    }
}

fn pane_kind_json(kind: &PaneKind) -> Value {
    match kind {
        PaneKind::Interaction(form, id) => json!({
            "form": format!("{form:?}").to_lowercase(),
            "interaction_id": id.map(|i| i.0),
        }),
    }
}

fn state_snapshot(state: &TuiState) -> Value {
    json!({
        "mode": format!("{:?}", state.mode).to_lowercase(),
        "consensus_level": state.consensus_level.to_string(),
        "phase_scope": state.phase_scope.to_string(),
        "model": state.model_name,
        "tab_count": state.tabs.len(),
        "active_tab": state.tabs.active_index(),
        "hil_pending": state.hil_prompt.is_some(),
        "hil": state.hil_prompt.as_ref().map(|h| json!({
            "title": h.title,
            "objective": h.objective,
            "tasks": h.tasks,
            "message": h.message,
        })),
    })
}

fn panes_list(state: &TuiState) -> Value {
    let tabs: Vec<Value> = state
        .tabs
        .tabs()
        .iter()
        .enumerate()
        .map(|(idx, tab)| {
            let pane = &tab.pane;
            let mut entry = json!({
                "tab": idx,
                "tab_id": tab.id.0,
                "pane_id": pane.id.0,
                "title": pane.display_title(),
                "active": idx == state.tabs.active_index(),
                "message_count": pane.conversation.messages.len(),
                "is_streaming": !pane.conversation.streaming_text.is_empty(),
                "input_draft_len": pane.input.len(),
            });
            if let (Value::Object(map), Value::Object(kind)) =
                (&mut entry, pane_kind_json(&pane.kind))
            {
                map.extend(kind);
            }
            entry
        })
        .collect();
    json!({ "tabs": tabs })
}

fn pane_read(state: &TuiState, params: &Value) -> Result<Value, RemoteError> {
    let tabs = state.tabs.tabs();
    let index = params
        .get("tab")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or_else(|| state.tabs.active_index());
    let tab = tabs
        .get(index)
        .ok_or_else(|| RemoteError::invalid_params(format!("no such tab: {index}")))?;
    let pane = &tab.pane;

    let messages = &pane.conversation.messages;
    let skip = params
        .get("last")
        .and_then(|v| v.as_u64())
        .map(|last| messages.len().saturating_sub(last as usize))
        .unwrap_or(0);

    let rendered: Vec<Value> = messages[skip..]
        .iter()
        .map(|m| json!({"role": role_label(m.role), "content": m.content}))
        .collect();

    Ok(json!({
        "tab": index,
        "title": pane.display_title(),
        "kind": pane_kind_json(&pane.kind),
        "total_messages": messages.len(),
        "messages": rendered,
        "streaming_text": pane.conversation.streaming_text,
    }))
}

/// Submit text to the active pane — mirrors `KeyAction::SubmitInput`.
fn input_send(
    state: &mut TuiState,
    cmd_tx: &mpsc::UnboundedSender<TuiCommand>,
    params: &Value,
) -> Result<Value, RemoteError> {
    let text = params
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RemoteError::invalid_params("missing 'text' (string)"))?
        .trim()
        .to_string();
    if text.is_empty() {
        return Err(RemoteError::invalid_params("'text' must not be empty"));
    }

    state.tabs.active_pane_mut().set_title_if_empty(&text);
    state.push_message(DisplayMessage::user(&text));
    let interaction_id = state.active_interaction_id();
    cmd_tx
        .send(TuiCommand::ProcessRequest {
            interaction_id,
            request: text,
        })
        .map_err(|_| RemoteError::failed("controller unavailable"))?;

    Ok(json!({
        "ok": true,
        "interaction_id": interaction_id.map(|i| i.0),
    }))
}

/// Execute a `:command` — mirrors `KeyAction::SubmitCommand`.
fn command_exec(
    state: &mut TuiState,
    cmd_tx: &mpsc::UnboundedSender<TuiCommand>,
    params: &Value,
) -> Result<Value, RemoteError> {
    let cmd = params
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RemoteError::invalid_params("missing 'command' (string)"))?
        .trim()
        .to_string();
    if cmd.is_empty() {
        return Err(RemoteError::invalid_params("'command' must not be empty"));
    }

    if cmd == "q" || cmd == "quit" || cmd == "exit" {
        state.should_quit = true;
        return Ok(json!({"ok": true, "quit": true}));
    }
    if let Some(flash) = super::app_tab_command::handle_tab_command(state, &cmd, cmd_tx) {
        state.set_flash(flash.clone());
        return Ok(json!({"ok": true, "flash": flash}));
    }
    let interaction_id = state.active_interaction_id();
    cmd_tx
        .send(TuiCommand::HandleCommand {
            interaction_id,
            command: cmd,
        })
        .map_err(|_| RemoteError::failed("controller unavailable"))?;
    Ok(json!({"ok": true}))
}

fn parse_form(s: &str) -> Result<InteractionForm, RemoteError> {
    match s.to_ascii_lowercase().as_str() {
        "agent" => Ok(InteractionForm::Agent),
        "ask" => Ok(InteractionForm::Ask),
        "discuss" => Ok(InteractionForm::Discuss),
        other => Err(RemoteError::invalid_params(format!(
            "unknown form '{other}' (expected agent|ask|discuss)"
        ))),
    }
}

fn interaction_spawn(
    cmd_tx: &mpsc::UnboundedSender<TuiCommand>,
    params: &Value,
) -> Result<Value, RemoteError> {
    let form = parse_form(
        params
            .get("form")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RemoteError::invalid_params("missing 'form' (string)"))?,
    )?;
    let query = params
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RemoteError::invalid_params("missing 'query' (string)"))?
        .to_string();

    cmd_tx
        .send(TuiCommand::SpawnInteraction {
            form,
            query,
            context_mode_override: None,
        })
        .map_err(|_| RemoteError::failed("controller unavailable"))?;
    Ok(json!({"ok": true}))
}

fn interaction_activate(
    cmd_tx: &mpsc::UnboundedSender<TuiCommand>,
    params: &Value,
) -> Result<Value, RemoteError> {
    let id = params
        .get("interaction_id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RemoteError::invalid_params("missing 'interaction_id' (number)"))?;
    cmd_tx
        .send(TuiCommand::ActivateInteraction(
            quorum_domain::interaction::InteractionId(id as usize),
        ))
        .map_err(|_| RemoteError::failed("controller unavailable"))?;
    Ok(json!({"ok": true}))
}

/// Answer a pending HiL modal — mirrors the `y`/`n` keys in `app_hil`.
fn hil_respond(
    state: &mut TuiState,
    pending_hil_tx: &Arc<Mutex<Option<oneshot::Sender<HumanDecision>>>>,
    params: &Value,
) -> Result<Value, RemoteError> {
    let decision = match params.get("decision").and_then(|v| v.as_str()) {
        Some("approve") => HumanDecision::Approve,
        Some("reject") => HumanDecision::Reject,
        _ => {
            return Err(RemoteError::invalid_params(
                "missing 'decision' (approve|reject)",
            ));
        }
    };

    if state.hil_prompt.is_none() {
        return Err(RemoteError::failed("no pending HiL prompt"));
    }
    let tx = pending_hil_tx
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| RemoteError::failed("no pending HiL response channel"))?;

    state.hil_prompt = None;
    let approved = matches!(decision, HumanDecision::Approve);
    state.set_flash(if approved {
        "Plan approved (remote)"
    } else {
        "Plan rejected (remote)"
    });
    tx.send(decision)
        .map_err(|_| RemoteError::failed("HiL requester is gone"))?;
    Ok(json!({"ok": true, "approved": approved}))
}
