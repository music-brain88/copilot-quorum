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
//! | `state.get`            | —                                     | mode, models, tabs, pending HiL, flash, focus, input drafts |
//! | `panes.list`           | —                                     | all tabs/panes with metadata (incl. scroll state) |
//! | `pane.read`            | `{tab?: usize, last?: usize}`         | conversation messages (structured) |
//! | `input.send`           | `{text: string}`                      | submit prompt to active pane |
//! | `command.exec`         | `{command: string}`                   | run `:command` (e.g. "solo", "tabnew ask") |
//! | `interaction.spawn`    | `{form: "agent"\|"ask"\|"discuss", query: string}` | spawn interaction (new tab) |
//! | `interaction.activate` | `{interaction_id: usize}`             | focus an interaction's tab |
//! | `hil.respond`          | `{decision: "approve"\|"reject"}`     | answer a pending HiL modal |
//!
//! # Methods (Phase 2 — screen visibility & layout)
//!
//! | method           | params                          | effect |
//! |------------------|---------------------------------|--------|
//! | `screen.capture` | `{width?, height?, styles?}`    | off-screen render → text lines (+style runs) |
//! | `layout.get`     | `{width?, height?}`             | surface rects, preset, splits, routes, overlays |
//! | `layout.set`     | `{preset: string}`              | switch layout preset live |
//! | `route.set`      | `{content, surface}`            | re-route a content slot live |
//! | `keys.feed`      | `{keys: ["i", "Esc", "Ctrl+w"]}`| inject synthetic key events |
//!
//! `screen.capture` / `layout.get` default to the live terminal size.
//! Captured lines are `trim_end()`ed; style runs are column-indexed
//! (end-exclusive) against the untrimmed grid. `layout.set` rejects
//! unknown preset names (unlike the Lua path, which silently creates a
//! custom preset). `keys.feed` cannot launch `$EDITOR` — the `I` binding
//! is swallowed and reported as `editor_suppressed`.
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
#[derive(Debug)]
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

/// Main-loop context handed to each remote request.
///
/// `deps` carries the same borrows used for keyboard dispatch;
/// `terminal_size` is the real terminal size at dispatch time (used as the
/// default for `screen.capture` / `layout.get`).
pub(super) struct RemoteContext<'a> {
    pub deps: super::app::InputDeps<'a>,
    pub terminal_size: ratatui::layout::Size,
}

/// Dispatch a remote request with full access to the TUI state.
///
/// Called from the main event loop, so mutations here are exactly as safe
/// (and as visible) as those triggered by keyboard input.
pub(super) fn handle_request(
    state: &mut TuiState,
    ctx: &RemoteContext<'_>,
    request: RemoteRequest,
) {
    let result = dispatch(state, ctx, &request.method, &request.params);
    if let Some(reply) = request.reply {
        let _ = reply.send(result);
    }
}

fn dispatch(
    state: &mut TuiState,
    ctx: &RemoteContext<'_>,
    method: &str,
    params: &Value,
) -> Result<Value, RemoteError> {
    match method {
        "state.get" => Ok(state_snapshot(state)),
        "panes.list" => Ok(panes_list(state)),
        "pane.read" => pane_read(state, params),
        "input.send" => input_send(state, ctx.deps.cmd_tx, params),
        "command.exec" => command_exec(state, ctx.deps.cmd_tx, params),
        "interaction.spawn" => interaction_spawn(ctx.deps.cmd_tx, params),
        "interaction.activate" => interaction_activate(ctx.deps.cmd_tx, params),
        "hil.respond" => hil_respond(state, ctx.deps.pending_hil_tx, params),
        "screen.capture" => screen_capture(state, ctx, params),
        "layout.get" => layout_get(state, ctx, params),
        "layout.set" => layout_set(state, params),
        "route.set" => route_set(state, params),
        "keys.feed" => keys_feed(state, ctx, params),
        other => Err(RemoteError::method_not_found(other)),
    }
}

/// Inject synthetic key events — exactly the keyboard dispatch path
/// (HiL modal keys, help close, Lua keymaps, built-in bindings).
///
/// Descriptors use the Lua keymap syntax: `"j"`, `"Esc"`, `"Ctrl+w"`,
/// `"Shift+Enter"`, `"F5"`. All descriptors are parsed before any is fed,
/// so one bad descriptor rejects the whole batch without side effects.
fn keys_feed(
    state: &mut TuiState,
    ctx: &RemoteContext<'_>,
    params: &Value,
) -> Result<Value, RemoteError> {
    let keys = params
        .get("keys")
        .and_then(|v| v.as_array())
        .ok_or_else(|| RemoteError::invalid_params("missing 'keys' (array of key descriptors)"))?;

    let mut events = Vec::with_capacity(keys.len());
    for (i, k) in keys.iter().enumerate() {
        let desc = k
            .as_str()
            .ok_or_else(|| RemoteError::invalid_params(format!("keys[{i}] must be a string")))?;
        let (code, mods) = super::mode::parse_key_descriptor(desc).ok_or_else(|| {
            RemoteError::invalid_params(format!("keys[{i}]: unrecognized descriptor '{desc}'"))
        })?;
        events.push(crossterm::event::KeyEvent::new(code, mods));
    }

    let mut editor_suppressed = false;
    let mut fed = 0usize;
    for key in events {
        if state.should_quit {
            break;
        }
        if let Some(super::app::SideEffect::LaunchEditor) =
            super::app::dispatch_terminal_event(state, crossterm::event::Event::Key(key), &ctx.deps)
        {
            // The terminal cannot be suspended from a remote request — swallow.
            editor_suppressed = true;
        }
        fed += 1;
    }

    Ok(json!({
        "ok": true,
        "fed": fed,
        "mode": format!("{:?}", state.mode).to_lowercase(),
        "editor_suppressed": editor_suppressed,
        "quit": state.should_quit,
    }))
}

/// Rebuild the route table after a preset/override change — the live
/// mutation path shared with the Lua accessor (see `app_tui_changes`).
fn rebuild_route(state: &mut TuiState) {
    state.route = super::route::RouteTable::from_preset_and_overrides(
        state.layout_config.preset.clone(),
        &state.layout_config.route_overrides,
    );
}

/// Current routes with visibility at the live layout — shared by the
/// mutation methods' responses so agents see the effect immediately.
fn routes_snapshot(state: &TuiState) -> Value {
    let area = ratatui::layout::Rect::new(0, 0, 500, 300); // visibility only depends on preset
    let (layout, pane_surfaces) = super::app_render::compute_layout(state, area);
    let surfaces = super::surface::SurfaceLayout::from_main_layout(&layout, &pane_surfaces);
    super::remote_view::routes_json(state, &surfaces)
}

/// Switch the layout preset live — mirrors the Lua `quorum.tui` path.
///
/// Unlike the Lua path (which silently treats any unknown name as a custom
/// preset), unknown names are rejected unless a custom preset with that
/// name was previously registered.
fn layout_set(state: &mut TuiState, params: &Value) -> Result<Value, RemoteError> {
    let name = params
        .get("preset")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RemoteError::invalid_params("missing 'preset' (string)"))?;

    let preset = match name.parse::<super::layout::LayoutPreset>() {
        Ok(p) => p,
        Err(_) if state.layout_config.custom_presets.contains_key(name) => {
            super::layout::LayoutPreset::Custom(name.to_string())
        }
        Err(e) => return Err(RemoteError::invalid_params(e)),
    };

    state.layout_config.preset = preset;
    rebuild_route(state);
    state.set_flash(format!("Layout: {} (remote)", state.layout_config.preset));
    Ok(json!({
        "ok": true,
        "preset": state.layout_config.preset.to_string(),
        "routes": routes_snapshot(state),
    }))
}

/// Re-route a content slot to a surface live — mirrors the Lua path, but
/// replaces (rather than appends) any existing override for the same slot.
fn route_set(state: &mut TuiState, params: &Value) -> Result<Value, RemoteError> {
    let content_name = params
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RemoteError::invalid_params("missing 'content' (string)"))?;
    let surface_name = params
        .get("surface")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RemoteError::invalid_params("missing 'surface' (string)"))?;

    let content = super::layout::parse_content_slot(content_name).ok_or_else(|| {
        RemoteError::invalid_params(format!(
            "unknown content slot '{content_name}' (expected conversation|progress|notification|hil_prompt|help|tool_log|model_stream:<name>|lua:<name>)"
        ))
    })?;
    let surface = super::layout::parse_surface_id(surface_name).ok_or_else(|| {
        RemoteError::invalid_params(format!(
            "unknown surface '{surface_name}' (expected main_pane|sidebar|overlay|header|input|status_bar|tab_bar|tool_pane|tool_float|dynamic_pane:<name>)"
        ))
    })?;

    state
        .layout_config
        .route_overrides
        .retain(|ov| ov.content != content);
    state
        .layout_config
        .route_overrides
        .push(super::layout::RouteOverride { content, surface });
    rebuild_route(state);
    Ok(json!({"ok": true, "routes": routes_snapshot(state)}))
}

/// Resolve the `{width?, height?}` params against the live terminal size,
/// with sanity bounds (guards widget panics at degenerate sizes and
/// multi-MB responses).
fn capture_size(ctx: &RemoteContext<'_>, params: &Value) -> Result<(u16, u16), RemoteError> {
    let width = match params.get("width").and_then(|v| v.as_u64()) {
        Some(w @ 10..=500) => w as u16,
        Some(w) => {
            return Err(RemoteError::invalid_params(format!(
                "'width' must be in 10..=500, got {w}"
            )));
        }
        None => ctx.terminal_size.width,
    };
    let height = match params.get("height").and_then(|v| v.as_u64()) {
        Some(h @ 5..=300) => h as u16,
        Some(h) => {
            return Err(RemoteError::invalid_params(format!(
                "'height' must be in 5..=300, got {h}"
            )));
        }
        None => ctx.terminal_size.height,
    };
    Ok((width, height))
}

/// Off-screen render of the current screen — what the user actually sees.
fn screen_capture(
    state: &TuiState,
    ctx: &RemoteContext<'_>,
    params: &Value,
) -> Result<Value, RemoteError> {
    let (width, height) = capture_size(ctx, params)?;
    let with_styles = params
        .get("styles")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let (lines, styles) = super::remote_view::capture_screen(
        state,
        ctx.deps.content_registry,
        width,
        height,
        with_styles,
    )
    .map_err(|e| RemoteError::failed(format!("render failed: {e}")))?;

    let mut result = json!({"width": width, "height": height, "lines": lines});
    if let Some(styles) = styles {
        result["styles"] = Value::Array(
            styles
                .iter()
                .map(|runs| Value::Array(runs.iter().map(|r| r.to_json()).collect()))
                .collect(),
        );
    }
    Ok(result)
}

/// Computed layout geometry (surface rects, routes, overlays) at a size.
fn layout_get(
    state: &TuiState,
    ctx: &RemoteContext<'_>,
    params: &Value,
) -> Result<Value, RemoteError> {
    let (width, height) = capture_size(ctx, params)?;
    Ok(super::remote_view::layout_snapshot(
        state,
        ratatui::layout::Rect::new(0, 0, width, height),
    ))
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
    let pane = state.tabs.active_pane();
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
        "pending_key": state.pending_key.map(String::from),
        "show_help": state.show_help,
        "help_scroll": state.help_scroll,
        "flash": state.flash_message.as_ref().map(|(text, at)| json!({
            "text": text,
            "age_ms": at.elapsed().as_millis() as u64,
        })),
        "focused_slot": super::layout::content_slot_to_string(&state.focused_slot),
        "command_input": state.command_input,
        "command_cursor": state.command_cursor,
        "active_pane": {
            "input": pane.input,
            "cursor_pos": pane.cursor_pos,
            "scroll_offset": pane.conversation.scroll_offset,
            "auto_scroll": pane.conversation.auto_scroll,
        },
        "layout": {
            "preset": state.layout_config.preset.to_string(),
            "flex_threshold": state.layout_config.flex_threshold,
        },
        "visual_selection": state.visual_selection.as_ref().map(|v| json!({
            "anchor_line": v.anchor_line,
            "cursor_line": v.cursor_line,
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
                "cursor_pos": pane.cursor_pos,
                "scroll_offset": pane.conversation.scroll_offset,
                "auto_scroll": pane.conversation.auto_scroll,
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

#[cfg(test)]
mod tests {
    use super::super::app::InputDeps;
    use super::super::app_render::build_default_registry;
    use super::super::content::{ContentRegistry, ContentSlot};
    use super::super::layout::LayoutPreset;
    use super::super::mode::{CustomKeymap, InputMode};
    use super::super::surface::SurfaceId;
    use super::*;
    use quorum_application::{ClipboardPort, NoClipboard, NoScriptingEngine, ScriptingEnginePort};
    use std::cell::RefCell;

    /// Everything `dispatch()` needs, without constructing a full `TuiApp`
    /// (which spawns a controller task and needs an LLM gateway).
    struct TestHarness {
        cmd_tx: mpsc::UnboundedSender<TuiCommand>,
        _cmd_rx: mpsc::UnboundedReceiver<TuiCommand>,
        pending_hil_tx: Arc<Mutex<Option<oneshot::Sender<HumanDecision>>>>,
        keymap: CustomKeymap,
        engine: Arc<dyn ScriptingEnginePort>,
        clipboard: Arc<dyn ClipboardPort>,
        registry: RefCell<ContentRegistry>,
    }

    impl TestHarness {
        fn new() -> Self {
            let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
            Self {
                cmd_tx,
                _cmd_rx: cmd_rx,
                pending_hil_tx: Arc::new(Mutex::new(None)),
                keymap: CustomKeymap::new(),
                engine: Arc::new(NoScriptingEngine),
                clipboard: Arc::new(NoClipboard),
                registry: RefCell::new(build_default_registry()),
            }
        }

        fn ctx(&self) -> RemoteContext<'_> {
            RemoteContext {
                deps: InputDeps {
                    cmd_tx: &self.cmd_tx,
                    pending_hil_tx: &self.pending_hil_tx,
                    custom_keymap: &self.keymap,
                    scripting_engine: &self.engine,
                    clipboard: &self.clipboard,
                    content_registry: &self.registry,
                },
                terminal_size: ratatui::layout::Size::new(80, 24),
            }
        }
    }

    #[test]
    fn screen_capture_via_dispatch() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();

        // Default size comes from ctx.terminal_size (80x24)
        let result = dispatch(&mut state, &harness.ctx(), "screen.capture", &json!({})).unwrap();
        assert_eq!(result["height"], 24);
        assert_eq!(result["lines"].as_array().unwrap().len(), 24);
        assert!(result.get("styles").is_none());

        // Explicit size respected, styles included on demand
        let result = dispatch(
            &mut state,
            &harness.ctx(),
            "screen.capture",
            &json!({"width": 120, "height": 40, "styles": true}),
        )
        .unwrap();
        assert_eq!(result["width"], 120);
        assert_eq!(result["lines"].as_array().unwrap().len(), 40);
        assert_eq!(result["styles"].as_array().unwrap().len(), 40);

        // Degenerate size rejected
        let err = dispatch(
            &mut state,
            &harness.ctx(),
            "screen.capture",
            &json!({"width": 2}),
        )
        .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn layout_get_via_dispatch() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();
        let result = dispatch(
            &mut state,
            &harness.ctx(),
            "layout.get",
            &json!({"width": 190, "height": 45}),
        )
        .unwrap();
        assert_eq!(result["preset"], "default");
        assert!(result["surfaces"]["main_pane"].is_object());
        assert!(result["routes"].is_array());
    }

    #[test]
    fn layout_set_stacked() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();

        let result = dispatch(
            &mut state,
            &harness.ctx(),
            "layout.set",
            &json!({"preset": "stacked"}),
        )
        .unwrap();
        assert_eq!(result["preset"], "stacked");
        assert_eq!(state.layout_config.preset, LayoutPreset::Stacked);
        assert_eq!(
            state.route.surface_for(&ContentSlot::Progress),
            Some(SurfaceId::Sidebar)
        );

        // Unknown preset rejected, state unchanged
        let err = dispatch(
            &mut state,
            &harness.ctx(),
            "layout.set",
            &json!({"preset": "bogus"}),
        )
        .unwrap_err();
        assert_eq!(err.code, -32602);
        assert_eq!(state.layout_config.preset, LayoutPreset::Stacked);
    }

    #[test]
    fn route_set_and_dedupe() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();

        for _ in 0..2 {
            dispatch(
                &mut state,
                &harness.ctx(),
                "route.set",
                &json!({"content": "tool_log", "surface": "main_pane"}),
            )
            .unwrap();
        }
        assert_eq!(state.layout_config.route_overrides.len(), 1);
        assert_eq!(
            state.route.surface_for(&ContentSlot::ToolLog),
            Some(SurfaceId::MainPane)
        );

        let err = dispatch(
            &mut state,
            &harness.ctx(),
            "route.set",
            &json!({"content": "nope", "surface": "main_pane"}),
        )
        .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn keys_feed_insert_roundtrip() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();
        assert_eq!(state.mode, InputMode::Insert);

        let result = dispatch(
            &mut state,
            &harness.ctx(),
            "keys.feed",
            &json!({"keys": ["h", "i", "Esc"]}),
        )
        .unwrap();
        assert_eq!(result["fed"], 3);
        assert_eq!(result["mode"], "normal");
        assert_eq!(state.tabs.active_pane().input, "hi");

        // '?' in Normal mode toggles help
        dispatch(
            &mut state,
            &harness.ctx(),
            "keys.feed",
            &json!({"keys": ["?"]}),
        )
        .unwrap();
        assert!(state.show_help);
    }

    #[test]
    fn keys_feed_help_overlay_scrolls() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();

        let feed = |state: &mut TuiState, keys: serde_json::Value| {
            dispatch(state, &harness.ctx(), "keys.feed", &json!({ "keys": keys })).unwrap();
        };

        // Open help from Normal mode
        feed(&mut state, json!(["Esc", "?"]));
        assert!(state.show_help);
        assert_eq!(state.help_scroll, 0);

        // j scrolls down, k scrolls up (consumed by the overlay, not the pane)
        feed(&mut state, json!(["j", "j", "j"]));
        assert_eq!(state.help_scroll, 3);
        assert_eq!(state.tabs.active_pane().conversation.scroll_offset, 0);

        feed(&mut state, json!(["k"]));
        assert_eq!(state.help_scroll, 2);

        // G jumps to bottom (clamped to content), then g back to top
        feed(&mut state, json!(["G"]));
        let max_scroll = state.help_scroll;
        assert!(max_scroll > 0);
        feed(&mut state, json!(["j"]));
        assert_eq!(state.help_scroll, max_scroll, "j at bottom must not overshoot");
        feed(&mut state, json!(["k"]));
        assert_eq!(state.help_scroll, max_scroll - 1, "k after bottom must move immediately");

        feed(&mut state, json!(["g"]));
        assert_eq!(state.help_scroll, 0);

        // Esc closes and resets scroll
        feed(&mut state, json!(["j", "Esc"]));
        assert!(!state.show_help);
        assert_eq!(state.help_scroll, 0);
    }

    #[test]
    fn keys_feed_invalid_descriptor_is_atomic() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();

        let err = dispatch(
            &mut state,
            &harness.ctx(),
            "keys.feed",
            &json!({"keys": ["h", "NotAKey+"]}),
        )
        .unwrap_err();
        assert_eq!(err.code, -32602);
        // Nothing was fed: input buffer untouched
        assert_eq!(state.tabs.active_pane().input, "");
    }

    #[test]
    fn state_get_expanded_fields() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();
        state.set_flash("hello");
        state.tabs.active_pane_mut().input = "draft".into();

        let result = dispatch(&mut state, &harness.ctx(), "state.get", &json!({})).unwrap();
        assert_eq!(result["show_help"], false);
        assert_eq!(result["flash"]["text"], "hello");
        assert_eq!(result["focused_slot"], "conversation");
        assert_eq!(result["active_pane"]["input"], "draft");
        assert_eq!(result["active_pane"]["auto_scroll"], true);
        assert_eq!(result["layout"]["preset"], "default");
        assert_eq!(result["layout"]["flex_threshold"], 120);
        assert!(result["visual_selection"].is_null());
    }
}
