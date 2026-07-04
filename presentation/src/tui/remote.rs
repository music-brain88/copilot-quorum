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
//! | `interaction.spawn`    | `{form: "agent"\|"ask"\|"discuss"\|"review", query: string}` | spawn interaction (new tab) |
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
//! # Methods (Phase 3 — introspection & config, #302)
//!
//! | method            | params                    | effect |
//! |--------------------|---------------------------|--------|
//! | `rpc.discover`     | —                         | every method name + params schema + summary + `api_version` |
//! | `commands.list`    | —                         | `:` commands (builtin + Lua `quorum.command.register`) |
//! | `config.keys`      | —                         | all known config keys (description, mutability, valid values) |
//! | `config.get`       | `{key}`                   | current value of a config key |
//! | `config.set`       | `{key, value}`            | set a config key (same `ConfigAccessorPort` as Lua/`:config`) |
//! | `keymaps.list`     | —                         | keybindings (builtin + Lua `quorum.keymap.set`) |
//!
//! These reuse the same registries the TUI itself reads from — a Lua
//! command/keymap or a config change is reflected here without any extra
//! wiring, and `rpc.discover` / `commands.list` / `config.*` share a single
//! source of truth with `dispatch()` so the two cannot drift apart (#302).
//!
//! Security: the socket is created with `0600` permissions and no TCP
//! listener is offered — same trust model as `nvim --listen`.

use super::event::TuiCommand;
use super::state::{DisplayMessage, MessageRole, TuiState};
use super::tab::PaneKind;
use quorum_application::{ConfigAccessorPort, ConfigValue, QuorumConfig};
use quorum_domain::HumanDecision;
use quorum_domain::interaction::InteractionForm;
use serde_json::{Value, json};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

/// Remote Control API version, returned by `rpc.discover`. Bump when a
/// breaking change is made to an existing method's params/result shape
/// (adding a new method is not breaking).
pub(super) const RPC_API_VERSION: u32 = 1;

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
            message: format!("Method not found: {method} (see rpc.discover for the full list)"),
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
// Socket path validation
// ---------------------------------------------------------------------------

/// Maximum byte length of a Unix domain socket path.
///
/// `sockaddr_un.sun_path` holds 108 bytes on Linux/Android (104 on
/// macOS/BSD), including the trailing NUL — so the usable path is one byte
/// shorter. Binding a longer path fails deep inside libc with the opaque
/// "path must be shorter than SUN_LEN"; we check up front instead. (#272)
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) const MAX_SOCKET_PATH_LEN: usize = 107;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub(super) const MAX_SOCKET_PATH_LEN: usize = 103;

/// Validate that `path` fits within the Unix domain socket length limit.
///
/// Returns a human-readable `InvalidInput` error (byte count + limit +
/// example) rather than letting `UnixListener::bind` fail later with the
/// opaque libc `SUN_LEN` message. Called from `TuiApp::run` before the
/// terminal is put into raw mode, so the message reaches the user on a
/// clean screen. (#272)
pub(super) fn validate_socket_path(path: &Path) -> std::io::Result<()> {
    let len = path.as_os_str().as_bytes().len();
    if len > MAX_SOCKET_PATH_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "ソケットパスが長すぎます ({len} バイト、上限は {MAX_SOCKET_PATH_LEN} バイト)。\
                 短いパスを指定してください: --listen /tmp/quorum.sock"
            ),
        ));
    }
    Ok(())
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
    validate_socket_path(&path)?;
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
    /// Shared with `AgentController` — the same handle Lua's `quorum.config`
    /// API and the TUI's `:config` command read/write, so `config.get` /
    /// `config.set` behave identically across all three surfaces (#302).
    pub shared_config: &'a Arc<Mutex<QuorumConfig>>,
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

/// A remote method's handler — normalized to one signature so `dispatch()`
/// and `rpc.discover` can share a single table (#302). Methods that don't
/// need `state` mutation or `ctx` simply ignore the unused parameter.
type MethodHandler = fn(&mut TuiState, &RemoteContext<'_>, &Value) -> Result<Value, RemoteError>;

/// Metadata for one remote method: name, one-line summary, a JSON Schema
/// fragment for `params` (hand-written, not derived — good enough for
/// discovery, not meant for strict validation), and its handler.
pub(super) struct MethodSpec {
    pub name: &'static str,
    pub summary: &'static str,
    pub params_schema: &'static str,
    handler: MethodHandler,
}

const EMPTY_PARAMS_SCHEMA: &str = r#"{"type":"object","properties":{}}"#;

/// Single source of truth for the RPC surface — `dispatch()` and
/// `rpc.discover` both read this table, so a method can't be dispatchable
/// without being discoverable (or vice versa).
static METHODS: &[MethodSpec] = &[
    MethodSpec {
        name: "state.get",
        summary: "Mode, models, tabs, pending HiL, flash, focus, input drafts, layout",
        params_schema: EMPTY_PARAMS_SCHEMA,
        handler: |state, _ctx, _params| Ok(state_snapshot(state)),
    },
    MethodSpec {
        name: "panes.list",
        summary: "All tabs/panes with metadata (incl. scroll state)",
        params_schema: EMPTY_PARAMS_SCHEMA,
        handler: |state, _ctx, _params| Ok(panes_list(state)),
    },
    MethodSpec {
        name: "pane.read",
        summary: "Conversation messages for a pane, structured (no screen scraping)",
        params_schema: r#"{"type":"object","properties":{"tab":{"type":"integer"},"last":{"type":"integer"}}}"#,
        handler: |state, _ctx, params| pane_read(state, params),
    },
    MethodSpec {
        name: "input.send",
        summary: "Submit a prompt to the active pane (same path as SubmitInput)",
        params_schema: r#"{"type":"object","required":["text"],"properties":{"text":{"type":"string"}}}"#,
        handler: input_send,
    },
    MethodSpec {
        name: "command.exec",
        summary: "Run a `:command` (e.g. \"solo\", \"tabnew ask\", \"q\")",
        params_schema: r#"{"type":"object","required":["command"],"properties":{"command":{"type":"string"}}}"#,
        handler: command_exec,
    },
    MethodSpec {
        name: "interaction.spawn",
        summary: "Spawn an Agent/Ask/Discuss/Review interaction in a new tab",
        params_schema: r#"{"type":"object","required":["form","query"],"properties":{"form":{"enum":["agent","ask","discuss","review"]},"query":{"type":"string"}}}"#,
        handler: interaction_spawn,
    },
    MethodSpec {
        name: "interaction.activate",
        summary: "Focus the tab bound to an interaction id",
        params_schema: r#"{"type":"object","required":["interaction_id"],"properties":{"interaction_id":{"type":"integer"}}}"#,
        handler: interaction_activate,
    },
    MethodSpec {
        name: "hil.respond",
        summary: "Answer a pending Human-in-the-Loop modal",
        params_schema: r#"{"type":"object","required":["decision"],"properties":{"decision":{"enum":["approve","reject"]}}}"#,
        handler: hil_respond,
    },
    MethodSpec {
        name: "screen.capture",
        summary: "Off-screen render of the current screen → text lines (+style runs)",
        params_schema: r#"{"type":"object","properties":{"width":{"type":"integer","minimum":10,"maximum":500},"height":{"type":"integer","minimum":5,"maximum":300},"styles":{"type":"boolean"}}}"#,
        handler: screen_capture,
    },
    MethodSpec {
        name: "layout.get",
        summary: "Surface rects, preset, splits, routes, overlays at a size",
        params_schema: r#"{"type":"object","properties":{"width":{"type":"integer"},"height":{"type":"integer"}}}"#,
        handler: layout_get,
    },
    MethodSpec {
        name: "layout.set",
        summary: "Switch the layout preset live",
        params_schema: r#"{"type":"object","required":["preset"],"properties":{"preset":{"type":"string"}}}"#,
        handler: layout_set,
    },
    MethodSpec {
        name: "route.set",
        summary: "Re-route a content slot to a surface live",
        params_schema: r#"{"type":"object","required":["content","surface"],"properties":{"content":{"type":"string"},"surface":{"type":"string"}}}"#,
        handler: route_set,
    },
    MethodSpec {
        name: "keys.feed",
        summary: "Inject synthetic key events (same dispatch path as the keyboard)",
        params_schema: r#"{"type":"object","required":["keys"],"properties":{"keys":{"type":"array","items":{"type":"string"}}}}"#,
        handler: keys_feed,
    },
    MethodSpec {
        name: "rpc.discover",
        summary: "List every RPC method with its params schema, summary, and api_version",
        params_schema: EMPTY_PARAMS_SCHEMA,
        handler: |_state, _ctx, _params| Ok(rpc_discover()),
    },
    MethodSpec {
        name: "commands.list",
        summary: "List `:` commands (builtin + Lua quorum.command.register)",
        params_schema: EMPTY_PARAMS_SCHEMA,
        handler: |_state, ctx, _params| Ok(commands_list(ctx)),
    },
    MethodSpec {
        name: "config.keys",
        summary: "List all known config keys (description, mutability, valid values)",
        params_schema: EMPTY_PARAMS_SCHEMA,
        handler: |_state, _ctx, _params| Ok(config_keys()),
    },
    MethodSpec {
        name: "config.get",
        summary: "Get the current value of a config key",
        params_schema: r#"{"type":"object","required":["key"],"properties":{"key":{"type":"string"}}}"#,
        handler: config_get,
    },
    MethodSpec {
        name: "config.set",
        summary: "Set a config key (same ConfigAccessorPort as Lua and :config)",
        params_schema: r#"{"type":"object","required":["key","value"],"properties":{"key":{"type":"string"},"value":{}}}"#,
        handler: config_set,
    },
    MethodSpec {
        name: "keymaps.list",
        summary: "List keybindings (builtin + Lua quorum.keymap.set)",
        params_schema: EMPTY_PARAMS_SCHEMA,
        handler: |_state, ctx, _params| Ok(keymaps_list(ctx)),
    },
];

fn dispatch(
    state: &mut TuiState,
    ctx: &RemoteContext<'_>,
    method: &str,
    params: &Value,
) -> Result<Value, RemoteError> {
    match METHODS.iter().find(|m| m.name == method) {
        Some(spec) => (spec.handler)(state, ctx, params),
        None => Err(RemoteError::method_not_found(method)),
    }
}

/// `rpc.discover` — capability discovery (LSP `capabilities` / MCP
/// `tools/list` / `nvim_get_api_info` precedent). Parses each method's
/// hand-written schema string back into JSON so the response is real JSON,
/// not an escaped string.
fn rpc_discover() -> Value {
    let methods: Vec<Value> = METHODS
        .iter()
        .map(|m| {
            let schema: Value =
                serde_json::from_str(m.params_schema).unwrap_or(Value::Object(Default::default()));
            json!({
                "method": m.name,
                "summary": m.summary,
                "params_schema": schema,
            })
        })
        .collect();
    json!({
        "api_version": RPC_API_VERSION,
        "methods": methods,
    })
}

/// `commands.list` — builtin `:` commands (`command_registry`) plus
/// Lua-registered ones (`quorum.command.register`, via
/// `ScriptingEnginePort::registered_commands()`). Lua commands are
/// discoverable here even though they can never appear in a static doc.
fn commands_list(ctx: &RemoteContext<'_>) -> Value {
    let builtin: Vec<Value> = super::command_registry::builtin_commands()
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "aliases": c.aliases,
                "usage": c.usage,
                "description": c.description,
                "source": "builtin",
            })
        })
        .collect();

    let lua: Vec<Value> = ctx
        .deps
        .scripting_engine
        .registered_commands()
        .into_iter()
        .map(|(name, description, usage, _callback_id)| {
            json!({
                "name": name,
                "aliases": Vec::<String>::new(),
                "usage": usage,
                "description": description,
                "source": "lua",
            })
        })
        .collect();

    json!({ "commands": builtin.into_iter().chain(lua).collect::<Vec<_>>() })
}

/// `keymaps.list` — builtin keybindings (`keymap_registry`) plus
/// Lua-registered ones (`quorum.keymap.set`).
fn keymaps_list(ctx: &RemoteContext<'_>) -> Value {
    let builtin: Vec<Value> = super::keymap_registry::builtin_keymaps()
        .iter()
        .map(|k| {
            json!({
                "mode": k.mode,
                "key": k.key,
                "action": k.action,
                "description": k.description,
                "source": "builtin",
            })
        })
        .collect();

    let lua: Vec<Value> = ctx
        .deps
        .scripting_engine
        .registered_keymaps()
        .into_iter()
        .map(|(mode, key, action)| {
            let action_name = match action {
                quorum_application::KeymapAction::Builtin(name) => name,
                quorum_application::KeymapAction::LuaCallback(id) => format!("lua_callback:{id}"),
            };
            json!({
                "mode": mode,
                "key": key,
                "action": action_name,
                "description": Value::Null,
                "source": "lua",
            })
        })
        .collect();

    json!({ "keymaps": builtin.into_iter().chain(lua).collect::<Vec<_>>() })
}

/// `config.keys` — the full `ConfigAccessorPort` key registry (same table
/// Lua's `quorum.config.keys()` and `:config` read), enriched with the
/// description/mutability/valid_values metadata `ConfigAccessorPort` itself
/// doesn't carry.
fn config_keys() -> Value {
    let keys: Vec<Value> = quorum_domain::known_keys()
        .iter()
        .map(|k| {
            json!({
                "key": k.key,
                "description": k.description,
                "mutable": k.mutability == quorum_domain::Mutability::Mutable,
                "valid_values": k.valid_values,
            })
        })
        .collect();
    json!({ "keys": keys })
}

/// `config.get` — reads through the same `Arc<Mutex<QuorumConfig>>` as
/// Lua's `quorum.config.get()` and the TUI's `:config`.
fn config_get(
    _state: &mut TuiState,
    ctx: &RemoteContext<'_>,
    params: &Value,
) -> Result<Value, RemoteError> {
    let key = params
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RemoteError::invalid_params("missing 'key' (string)"))?;

    let config = ctx
        .shared_config
        .lock()
        .map_err(|_| RemoteError::failed("config lock poisoned"))?;
    match config.config_get(key) {
        Ok(value) => Ok(json!({"key": key, "value": config_value_to_json(&value)})),
        Err(e) => Err(RemoteError::invalid_params(format!(
            "{e} (see config.keys for the full list)"
        ))),
    }
}

/// `config.set` — writes through the same `Arc<Mutex<QuorumConfig>>` as
/// Lua's `quorum.config.set()` and the TUI's `:config`, so validation and
/// side effects (e.g. mode-combination warnings) are identical across all
/// three surfaces (#302).
fn config_set(
    _state: &mut TuiState,
    ctx: &RemoteContext<'_>,
    params: &Value,
) -> Result<Value, RemoteError> {
    let key = params
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RemoteError::invalid_params("missing 'key' (string)"))?;
    let raw_value = params
        .get("value")
        .ok_or_else(|| RemoteError::invalid_params("missing 'value'"))?;
    let value = json_to_config_value(raw_value)?;

    let mut config = ctx
        .shared_config
        .lock()
        .map_err(|_| RemoteError::failed("config lock poisoned"))?;
    match config.config_set(key, value) {
        Ok(issues) => {
            let warnings: Vec<Value> = issues
                .iter()
                .map(|issue| json!({"severity": format!("{:?}", issue.severity), "message": issue.message}))
                .collect();
            let new_value = config.config_get(key).ok();
            Ok(json!({
                "ok": true,
                "key": key,
                "value": new_value.as_ref().map(config_value_to_json),
                "warnings": warnings,
            }))
        }
        Err(e) => Err(RemoteError::invalid_params(format!(
            "{e} (see config.keys for the full list)"
        ))),
    }
}

/// `ConfigValue` → JSON, mirroring `config_api::push_config_value` (Lua
/// path) so the same value round-trips identically over RPC.
fn config_value_to_json(value: &ConfigValue) -> Value {
    match value {
        ConfigValue::String(s) => Value::String(s.clone()),
        ConfigValue::Integer(n) => Value::Number((*n).into()),
        ConfigValue::Boolean(b) => Value::Bool(*b),
        ConfigValue::StringList(list) => {
            Value::Array(list.iter().map(|s| Value::String(s.clone())).collect())
        }
    }
}

/// JSON → `ConfigValue`, mirroring `config_api::lua_to_config_value` (Lua
/// path): string/bool/integer map directly, an array of strings becomes a
/// `StringList`, anything else is rejected up front instead of surfacing a
/// confusing type error from deep inside `config_set`.
fn json_to_config_value(value: &Value) -> Result<ConfigValue, RemoteError> {
    match value {
        Value::String(s) => Ok(ConfigValue::String(s.clone())),
        Value::Bool(b) => Ok(ConfigValue::Boolean(*b)),
        Value::Number(n) => n
            .as_i64()
            .map(ConfigValue::Integer)
            .ok_or_else(|| RemoteError::invalid_params("'value' must be an integer")),
        Value::Array(items) => {
            if items.is_empty() {
                return Err(RemoteError::invalid_params(
                    "'value' must not be an empty list",
                ));
            }
            let strings: Option<Vec<String>> = items
                .iter()
                .map(|v| v.as_str().map(str::to_string))
                .collect();
            strings
                .map(ConfigValue::StringList)
                .ok_or_else(|| RemoteError::invalid_params("'value' list elements must be strings"))
        }
        other => Err(RemoteError::invalid_params(format!(
            "unsupported 'value' type: {other}"
        ))),
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
fn layout_set(
    state: &mut TuiState,
    _ctx: &RemoteContext<'_>,
    params: &Value,
) -> Result<Value, RemoteError> {
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
fn route_set(
    state: &mut TuiState,
    _ctx: &RemoteContext<'_>,
    params: &Value,
) -> Result<Value, RemoteError> {
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
    state: &mut TuiState,
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
    state: &mut TuiState,
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
    ctx: &RemoteContext<'_>,
    params: &Value,
) -> Result<Value, RemoteError> {
    let cmd_tx = ctx.deps.cmd_tx;
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
    // Mirror KeyAction::SubmitInput (app_action_handler): a bound tab processes
    // the request; a placeholder tab spawns+binds a fresh interaction so the
    // response renders in it, rather than falling back to a possibly-stale
    // controller active_interaction_id (#283).
    match state.tabs.active_pane().kind {
        super::tab::PaneKind::Interaction(_, Some(id)) => {
            state.push_message(DisplayMessage::user(&text));
            cmd_tx
                .send(TuiCommand::ProcessRequest {
                    interaction_id: Some(id),
                    request: text,
                })
                .map_err(|_| RemoteError::failed("controller unavailable"))?;
            Ok(json!({ "ok": true, "interaction_id": id.0 }))
        }
        super::tab::PaneKind::Interaction(form, None) => {
            // Spawn path echoes the user message via InteractionSpawned.
            cmd_tx
                .send(TuiCommand::SpawnInteraction {
                    form,
                    query: text,
                    context_mode_override: None,
                })
                .map_err(|_| RemoteError::failed("controller unavailable"))?;
            Ok(json!({ "ok": true, "interaction_id": Value::Null }))
        }
    }
}

/// Execute a `:command` — mirrors `KeyAction::SubmitCommand`.
fn command_exec(
    state: &mut TuiState,
    ctx: &RemoteContext<'_>,
    params: &Value,
) -> Result<Value, RemoteError> {
    let cmd_tx = ctx.deps.cmd_tx;
    let cmd = params
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RemoteError::invalid_params("missing 'command' (string)"))?
        .trim()
        .to_string();
    if cmd.is_empty() {
        return Err(RemoteError::invalid_params("'command' must not be empty"));
    }

    match super::app_tab_command::handle_quit_command(state, &cmd, cmd_tx) {
        super::app_tab_command::QuitOutcome::QuitApp => {
            state.should_quit = true;
            return Ok(json!({"ok": true, "quit": true}));
        }
        super::app_tab_command::QuitOutcome::TabClosed(flash) => {
            state.set_flash(flash.clone());
            return Ok(json!({"ok": true, "quit": false, "flash": flash}));
        }
        super::app_tab_command::QuitOutcome::NotQuit => {}
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
        // `query` is the raw diff text for a Review spawn — no PR metadata
        // or focus (that's only available via the `review` CLI subcommand,
        // #300). Spawned as a child of the active interaction, same as the
        // other forms.
        "review" => Ok(InteractionForm::Review),
        other => Err(RemoteError::invalid_params(format!(
            "unknown form '{other}' (expected agent|ask|discuss|review)"
        ))),
    }
}

fn interaction_spawn(
    _state: &mut TuiState,
    ctx: &RemoteContext<'_>,
    params: &Value,
) -> Result<Value, RemoteError> {
    let cmd_tx = ctx.deps.cmd_tx;
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
    _state: &mut TuiState,
    ctx: &RemoteContext<'_>,
    params: &Value,
) -> Result<Value, RemoteError> {
    let cmd_tx = ctx.deps.cmd_tx;
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
    ctx: &RemoteContext<'_>,
    params: &Value,
) -> Result<Value, RemoteError> {
    let pending_hil_tx = ctx.deps.pending_hil_tx;
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
        shared_config: Arc<Mutex<QuorumConfig>>,
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
                shared_config: Arc::new(Mutex::new(QuorumConfig::default())),
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
                shared_config: &self.shared_config,
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

    /// Regression test for #269 — mirrors the issue's repro steps:
    /// HiL modal shown → `keys.feed` j/k → `state.get` scroll_offset must change.
    #[test]
    fn keys_feed_scrolls_conversation_while_hil_modal_shown() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();
        state.mode = InputMode::Normal;
        state.hil_prompt = Some(super::super::state::HilPrompt {
            title: "Plan Requires Human Intervention".into(),
            objective: "obj".into(),
            tasks: vec!["task".into()],
            message: "Approve or reject?".into(),
        });
        let (tx, _rx) = oneshot::channel();
        *harness.pending_hil_tx.lock().unwrap() = Some(tx);

        dispatch(
            &mut state,
            &harness.ctx(),
            "keys.feed",
            &json!({"keys": ["k", "k", "j"]}),
        )
        .unwrap();
        assert_eq!(state.tabs.active_pane().conversation.scroll_offset, 1);
        // Modal still pending — scrolling must not answer the prompt
        assert!(state.hil_prompt.is_some());
        assert!(harness.pending_hil_tx.lock().unwrap().is_some());

        // Decision keys still work after scrolling
        dispatch(
            &mut state,
            &harness.ctx(),
            "keys.feed",
            &json!({"keys": ["y"]}),
        )
        .unwrap();
        assert!(state.hil_prompt.is_none());
        assert!(harness.pending_hil_tx.lock().unwrap().is_none());
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
        assert_eq!(
            state.help_scroll, max_scroll,
            "j at bottom must not overshoot"
        );
        feed(&mut state, json!(["k"]));
        assert_eq!(
            state.help_scroll,
            max_scroll - 1,
            "k after bottom must move immediately"
        );

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
    fn validate_socket_path_accepts_short_path() {
        assert!(validate_socket_path(Path::new("/tmp/quorum.sock")).is_ok());
        // Exactly at the limit is allowed.
        let at_limit = "a".repeat(MAX_SOCKET_PATH_LEN);
        assert!(validate_socket_path(Path::new(&at_limit)).is_ok());
    }

    #[test]
    fn validate_socket_path_rejects_long_path_with_friendly_message() {
        let too_long = "a".repeat(MAX_SOCKET_PATH_LEN + 1);
        let err = validate_socket_path(Path::new(&too_long)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        let msg = err.to_string();
        // Human-readable: no libc jargon, includes both byte counts.
        assert!(
            !msg.contains("SUN_LEN"),
            "message leaked libc jargon: {msg}"
        );
        assert!(msg.contains(&(MAX_SOCKET_PATH_LEN + 1).to_string()));
        assert!(msg.contains(&MAX_SOCKET_PATH_LEN.to_string()));
        assert!(msg.contains("--listen"));
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

    #[test]
    fn command_exec_q_on_last_tab_quits() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();
        assert_eq!(state.tabs.len(), 1);

        let result = dispatch(
            &mut state,
            &harness.ctx(),
            "command.exec",
            &json!({"command": "q"}),
        )
        .unwrap();
        assert_eq!(result["quit"], true);
        assert!(state.should_quit);
    }

    #[test]
    fn command_exec_q_with_multiple_tabs_closes_tab() {
        use super::super::tab::PaneKind;
        let harness = TestHarness::new();
        let mut state = TuiState::new();
        state
            .tabs
            .create_tab(PaneKind::Interaction(InteractionForm::Agent, None));
        assert_eq!(state.tabs.len(), 2);

        let result = dispatch(
            &mut state,
            &harness.ctx(),
            "command.exec",
            &json!({"command": "q"}),
        )
        .unwrap();
        // Tab closed, app stays alive.
        assert_eq!(result["quit"], false);
        assert!(result.get("flash").is_some());
        assert!(!state.should_quit);
        assert_eq!(state.tabs.len(), 1);
    }

    #[test]
    fn command_exec_q_bang_is_tab_aware() {
        use super::super::tab::PaneKind;
        let harness = TestHarness::new();
        let mut state = TuiState::new();
        state
            .tabs
            .create_tab(PaneKind::Interaction(InteractionForm::Agent, None));

        // With 2 tabs, `q!` closes the tab (does not reach the controller's /q).
        let result = dispatch(
            &mut state,
            &harness.ctx(),
            "command.exec",
            &json!({"command": "q!"}),
        )
        .unwrap();
        assert_eq!(result["quit"], false);
        assert!(!state.should_quit);
        assert_eq!(state.tabs.len(), 1);

        // On the last tab, `q!` quits the app.
        let result = dispatch(
            &mut state,
            &harness.ctx(),
            "command.exec",
            &json!({"command": "q!"}),
        )
        .unwrap();
        assert_eq!(result["quit"], true);
        assert!(state.should_quit);
    }

    #[test]
    fn command_exec_qa_quits_with_multiple_tabs() {
        use super::super::tab::PaneKind;
        let harness = TestHarness::new();
        let mut state = TuiState::new();
        state
            .tabs
            .create_tab(PaneKind::Interaction(InteractionForm::Agent, None));
        assert_eq!(state.tabs.len(), 2);

        let result = dispatch(
            &mut state,
            &harness.ctx(),
            "command.exec",
            &json!({"command": "qa"}),
        )
        .unwrap();
        assert_eq!(result["quit"], true);
        assert!(state.should_quit);
    }

    // -- Phase 3: introspection & config (#302) --

    #[test]
    fn rpc_discover_lists_every_dispatchable_method() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();
        let result = dispatch(&mut state, &harness.ctx(), "rpc.discover", &json!({})).unwrap();

        assert_eq!(result["api_version"], RPC_API_VERSION);
        let methods = result["methods"].as_array().unwrap();
        assert_eq!(methods.len(), METHODS.len());

        let names: Vec<&str> = methods
            .iter()
            .map(|m| m["method"].as_str().unwrap())
            .collect();
        for expected in [
            "state.get",
            "config.get",
            "config.set",
            "keys.feed",
            "rpc.discover",
        ] {
            assert!(names.contains(&expected), "missing method: {expected}");
        }

        // Every method's params_schema must parse as JSON (not just be an
        // opaque string) and every dispatchable name must round-trip.
        for method in methods {
            assert!(method["params_schema"].is_object());
            let name = method["method"].as_str().unwrap();
            let dispatched = dispatch(&mut state, &harness.ctx(), name, &json!({}));
            // Not all methods succeed with empty params (e.g. input.send
            // requires 'text'), but none should be "method not found".
            if let Err(e) = dispatched {
                assert_ne!(
                    e.code, -32601,
                    "{name} listed by rpc.discover but not dispatchable"
                );
            }
        }
    }

    #[test]
    fn unknown_method_hints_at_rpc_discover() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();
        let err = dispatch(&mut state, &harness.ctx(), "bogus.method", &json!({})).unwrap_err();
        assert_eq!(err.code, -32601);
        assert!(err.message.contains("rpc.discover"));
    }

    #[test]
    fn commands_list_includes_builtins_and_lua_commands() {
        struct FakeEngineWithCommands;
        impl ScriptingEnginePort for FakeEngineWithCommands {
            fn emit_event(
                &self,
                _event: quorum_domain::scripting::ScriptEventType,
                _data: quorum_domain::scripting::ScriptEventData,
            ) -> Result<quorum_application::EventOutcome, quorum_application::ScriptError>
            {
                Ok(quorum_application::EventOutcome::Continue)
            }
            fn load_script(
                &self,
                _path: &std::path::Path,
            ) -> Result<(), quorum_application::ScriptError> {
                Ok(())
            }
            fn is_available(&self) -> bool {
                true
            }
            fn registered_keymaps(
                &self,
            ) -> Vec<(String, String, quorum_application::KeymapAction)> {
                Vec::new()
            }
            fn execute_callback(
                &self,
                _callback_id: u64,
            ) -> Result<(), quorum_application::ScriptError> {
                Ok(())
            }
            fn registered_commands(&self) -> Vec<(String, String, String, u64)> {
                vec![(
                    "hello".to_string(),
                    "Say hello".to_string(),
                    "/hello <name>".to_string(),
                    1,
                )]
            }
        }

        let mut harness = TestHarness::new();
        harness.engine = Arc::new(FakeEngineWithCommands);
        let mut state = TuiState::new();

        let result = dispatch(&mut state, &harness.ctx(), "commands.list", &json!({})).unwrap();
        let commands = result["commands"].as_array().unwrap();

        let builtin = commands
            .iter()
            .find(|c| c["name"] == "q")
            .expect("builtin 'q' command missing");
        assert_eq!(builtin["source"], "builtin");

        let lua = commands
            .iter()
            .find(|c| c["name"] == "hello")
            .expect("lua 'hello' command missing");
        assert_eq!(lua["source"], "lua");
        assert_eq!(lua["description"], "Say hello");
    }

    #[test]
    fn keymaps_list_includes_builtins() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();
        let result = dispatch(&mut state, &harness.ctx(), "keymaps.list", &json!({})).unwrap();
        let keymaps = result["keymaps"].as_array().unwrap();
        assert!(!keymaps.is_empty());
        let quit = keymaps
            .iter()
            .find(|k| k["mode"] == "global" && k["key"] == "Ctrl+c")
            .expect("global Ctrl+c binding missing");
        assert_eq!(quit["action"], "quit");
        assert_eq!(quit["source"], "builtin");
    }

    #[test]
    fn config_keys_lists_known_keys_with_metadata() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();
        let result = dispatch(&mut state, &harness.ctx(), "config.keys", &json!({})).unwrap();
        let keys = result["keys"].as_array().unwrap();
        assert_eq!(keys.len(), quorum_domain::known_keys().len());
        let strategy = keys
            .iter()
            .find(|k| k["key"] == "agent.strategy")
            .expect("agent.strategy missing from config.keys");
        assert_eq!(strategy["mutable"], true);
        assert!(
            strategy["valid_values"]
                .as_array()
                .unwrap()
                .contains(&json!("debate"))
        );
    }

    #[test]
    fn config_get_returns_current_value() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();
        let result = dispatch(
            &mut state,
            &harness.ctx(),
            "config.get",
            &json!({"key": "agent.strategy"}),
        )
        .unwrap();
        assert_eq!(result["value"], "quorum");
    }

    #[test]
    fn config_get_unknown_key_hints_at_config_keys() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();
        let err = dispatch(
            &mut state,
            &harness.ctx(),
            "config.get",
            &json!({"key": "nonexistent.key"}),
        )
        .unwrap_err();
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("config.keys"));
    }

    #[test]
    fn config_set_then_get_round_trips() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();

        let set_result = dispatch(
            &mut state,
            &harness.ctx(),
            "config.set",
            &json!({"key": "agent.strategy", "value": "debate"}),
        )
        .unwrap();
        assert_eq!(set_result["ok"], true);
        assert_eq!(set_result["value"], "debate");

        let get_result = dispatch(
            &mut state,
            &harness.ctx(),
            "config.get",
            &json!({"key": "agent.strategy"}),
        )
        .unwrap();
        assert_eq!(get_result["value"], "debate");
    }

    #[test]
    fn config_set_string_list_round_trips() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();

        dispatch(
            &mut state,
            &harness.ctx(),
            "config.set",
            &json!({"key": "models.review", "value": ["claude-opus-4.5", "gpt-5.3-codex"]}),
        )
        .unwrap();

        let get_result = dispatch(
            &mut state,
            &harness.ctx(),
            "config.get",
            &json!({"key": "models.review"}),
        )
        .unwrap();
        assert_eq!(
            get_result["value"],
            json!(["claude-opus-4.5", "gpt-5.3-codex"])
        );
    }

    #[test]
    fn config_set_invalid_value_is_rejected() {
        let harness = TestHarness::new();
        let mut state = TuiState::new();
        let err = dispatch(
            &mut state,
            &harness.ctx(),
            "config.set",
            &json!({"key": "agent.strategy", "value": "not-a-strategy"}),
        )
        .unwrap_err();
        assert_eq!(err.code, -32602);
    }
}
