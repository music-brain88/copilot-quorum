//! TUI application — main loop with Actor pattern
//!
//! Architecture:
//! ```text
//! TuiApp (select! loop)                 controller_task (tokio::spawn)
//!   ├─ crossterm EventStream              ├─ cmd_rx.recv()
//!   ├─ ui_rx (UiEvent from controller)    ├─ controller.handle_command()
//!   ├─ tui_rx (TuiEvent from progress)    └─ controller.process_request()
//!   ├─ hil_rx (HilRequest)
//!   └─ tick_interval
//!        └── cmd_tx ──────────────────>──┘
//! ```

use super::content::ContentRegistry;
use super::editor::{self, EditorContext, EditorResult};
use super::event::{HilRequest, RoutedTuiEvent, TuiCommand};
use super::layout::TuiLayoutConfig;
use super::mode::{self, InputMode, KeyAction};
use super::presenter::TuiPresenter;
use super::state::{TuiInputConfig, TuiState};

/// Side-effect that requires main loop intervention (e.g. terminal suspend)
pub(super) enum SideEffect {
    LaunchEditor,
}

/// Outcome of awaiting a specific interaction via [`TuiApp::run_headless_until`].
#[derive(Debug, Clone)]
pub enum InteractionOutcome {
    /// The interaction completed and produced a structured result.
    Completed(InteractionResult),
    /// The interaction failed before producing a result (see the message for
    /// what a `*Error` `UiEvent` reported).
    Failed(String),
    /// The process was interrupted (`:q!`, Ctrl+C, SIGTERM) before completion.
    Interrupted,
}

/// Everything needed to process a terminal event outside `&self TuiApp`.
///
/// Borrowed from `TuiApp` via [`TuiApp::input_deps`] — lets the same key
/// dispatch run from the remote `keys.feed` handler and from unit tests
/// (a full `TuiApp` cannot be constructed in tests: it spawns a controller
/// task and needs a gateway).
pub(super) struct InputDeps<'a> {
    pub cmd_tx: &'a mpsc::UnboundedSender<TuiCommand>,
    pub pending_hil_tx: &'a Arc<Mutex<Option<oneshot::Sender<HumanDecision>>>>,
    pub custom_keymap: &'a mode::CustomKeymap,
    pub scripting_engine: &'a Arc<dyn quorum_application::ScriptingEnginePort>,
    pub clipboard: &'a Arc<dyn ClipboardPort>,
    pub content_registry: &'a std::cell::RefCell<ContentRegistry>,
}

/// Handle a terminal (crossterm) event — same code path for keyboard input
/// and remote `keys.feed` injection.
/// Returns a `SideEffect` if the main loop needs to perform a terminal-level action.
pub(super) fn dispatch_terminal_event(
    state: &mut TuiState,
    event: crossterm::event::Event,
    deps: &InputDeps<'_>,
) -> Option<SideEffect> {
    match event {
        crossterm::event::Event::Key(key) => {
            // If HiL modal is showing, handle y/n/Esc
            if state.hil_prompt.is_some() {
                super::app_hil::handle_hil_key(state, deps.pending_hil_tx, key);
                return None;
            }

            // If help is showing, handle close + scroll keys (other keys fall through)
            if state.show_help {
                use crossterm::event::KeyCode;
                let max_scroll = super::app_render::help_max_scroll(state.term_size);
                match key.code {
                    KeyCode::Esc | KeyCode::Char('?') => {
                        state.close_help();
                        return None;
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        state.help_scroll_down(max_scroll);
                        return None;
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        state.help_scroll_up();
                        return None;
                    }
                    KeyCode::Char('g') | KeyCode::Home => {
                        state.help_scroll_to_top();
                        return None;
                    }
                    KeyCode::Char('G') | KeyCode::End => {
                        state.help_scroll_to_bottom(max_scroll);
                        return None;
                    }
                    _ => {}
                }
            }

            // Check custom keymap (from Lua) before built-in bindings
            if let Some(custom_action) = deps.custom_keymap.lookup(state.mode, &key) {
                state.pending_key = None;
                return super::app_action_handler::handle_action(
                    state,
                    custom_action.clone(),
                    deps.cmd_tx,
                    deps.scripting_engine,
                    deps.clipboard,
                    deps.content_registry,
                );
            }

            let action = mode::handle_key_event(state.mode, key, state.pending_key);
            if let KeyAction::PendingKey(c) = action {
                state.pending_key = Some(c);
                return None;
            }
            state.pending_key = None;
            super::app_action_handler::handle_action(
                state,
                action,
                deps.cmd_tx,
                deps.scripting_engine,
                deps.clipboard,
                deps.content_registry,
            )
        }
        crossterm::event::Event::Resize(width, height) => {
            state.term_size = (width, height);
            None
        }
        _ => None,
    }
}
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, EventStream, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures::stream::StreamExt;
use quorum_application::QuorumConfig;
use quorum_application::{
    AgentController, ClipboardPort, ContextLoaderPort, ConversationLogger, LlmGateway, NoClipboard,
    NoConversationLogger, ToolExecutorPort, ToolSchemaPort, TuiAccessorPort, UiEvent,
};
use quorum_domain::{
    ConsensusLevel, HumanDecision, InteractionForm, InteractionId, InteractionResult, Model,
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use super::human_intervention::TuiHumanIntervention;

/// Terminal size assumed for `screen.capture` / `layout.get` when there is
/// no real terminal to query — the historical TTY-lookup-failure fallback
/// (#272) promoted to the explicit headless-mode default (#303, RFC #304 D3).
fn headless_terminal_size() -> ratatui::layout::Size {
    ratatui::layout::Size::new(80, 24)
}

/// Main TUI application
pub struct TuiApp {
    // -- Actor channels --
    cmd_tx: mpsc::UnboundedSender<TuiCommand>,
    ui_rx: mpsc::UnboundedReceiver<UiEvent>,
    tui_event_rx: mpsc::UnboundedReceiver<RoutedTuiEvent>,
    hil_rx: mpsc::UnboundedReceiver<HilRequest>,

    // -- Presenter (applies UiEvents to state) --
    presenter: TuiPresenter,

    // -- Pending HiL response sender --
    pending_hil_tx: Arc<Mutex<Option<oneshot::Sender<HumanDecision>>>>,

    // -- Controller task handle --
    _controller_handle: tokio::task::JoinHandle<()>,

    // -- TUI configuration --
    tui_config: TuiInputConfig,

    // -- Layout configuration --
    layout_config: TuiLayoutConfig,

    // -- Content registry (registry-driven rendering) --
    // RefCell for interior mutability: dynamic model stream renderers are
    // registered during event handling (&self) but consumed during render (&self).
    content_registry: std::cell::RefCell<ContentRegistry>,

    // -- Scripting engine (optional Lua runtime) --
    scripting_engine: Arc<dyn quorum_application::ScriptingEnginePort>,

    // -- Custom keybindings from Lua --
    custom_keymap: mode::CustomKeymap,

    // -- TUI accessor for Lua scripting --
    tui_accessor: Option<Arc<Mutex<dyn TuiAccessorPort>>>,

    // -- Clipboard (for yank/copy operations) --
    clipboard: Arc<dyn ClipboardPort>,

    // -- Remote control socket path (--listen) --
    listen_path: Option<std::path::PathBuf>,

    // -- Shared config, for the Remote Control API's config.* methods (#302) --
    // The same `Arc<Mutex<QuorumConfig>>` handed to `AgentController` below —
    // cloned before the move so `dispatch()` can reach `ConfigAccessorPort`
    // without a round-trip through the controller task.
    shared_config: Arc<Mutex<QuorumConfig>>,
}

impl TuiApp {
    /// Create a new TUI application wired to the controller
    pub fn new(
        gateway: Arc<dyn LlmGateway>,
        tool_executor: Arc<dyn ToolExecutorPort>,
        tool_schema: Arc<dyn ToolSchemaPort>,
        context_loader: Arc<dyn ContextLoaderPort>,
        config: Arc<Mutex<QuorumConfig>>,
    ) -> Self {
        Self::new_with_logger(
            gateway,
            tool_executor,
            tool_schema,
            context_loader,
            config,
            Arc::new(NoConversationLogger),
            None,
        )
    }

    /// Create a new TUI application with a conversation logger.
    ///
    /// `supervisor_reporter`, if given, is folded into the controller's
    /// event-publisher composite (Issue #309) — it must be threaded in here,
    /// before the controller is handed off to its background task below, since
    /// nothing else can reach `AgentController` directly afterwards (only via
    /// `TuiCommand`, and there's no command for this yet). Infra-agnostic:
    /// this crate never constructs the concrete adapter itself, only forwards
    /// whatever the DI-assembly layer (`cli/`) already built.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_logger(
        gateway: Arc<dyn LlmGateway>,
        tool_executor: Arc<dyn ToolExecutorPort>,
        tool_schema: Arc<dyn ToolSchemaPort>,
        context_loader: Arc<dyn ContextLoaderPort>,
        config: Arc<Mutex<QuorumConfig>>,
        conversation_logger: Arc<dyn ConversationLogger>,
        supervisor_reporter: Option<Arc<dyn quorum_application::EventPublisher>>,
    ) -> Self {
        // Channels
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<TuiCommand>();
        let (ui_tx, ui_rx) = mpsc::unbounded_channel::<UiEvent>();
        let (tui_event_tx, tui_event_rx) = mpsc::unbounded_channel::<RoutedTuiEvent>();
        let (hil_tx, hil_rx) = mpsc::unbounded_channel::<HilRequest>();

        // Human intervention port (sends to hil_rx)
        let human_intervention = Arc::new(TuiHumanIntervention::new(hil_tx));

        // Progress bridge (sends TuiEvents to tui_event_rx)
        let progress_tx = tui_event_tx.clone();

        // Presenter (applies UiEvents and emits TuiEvents)
        let presenter = TuiPresenter::new(tui_event_tx);

        // Cloned before the move into `AgentController` — the Remote Control
        // API's config.* methods (#302) read/write through this handle
        // instead of round-tripping through the controller task.
        let shared_config = Arc::clone(&config);

        // Controller (runs in background task)
        let mut controller = AgentController::new(
            gateway,
            tool_executor,
            tool_schema,
            context_loader,
            config,
            human_intervention,
            ui_tx,
        )
        .with_conversation_logger(conversation_logger);
        if let Some(reporter) = supervisor_reporter {
            controller = controller.with_event_subscriber(reporter);
        }

        let controller_handle = tokio::spawn(super::app_controller::controller_task(
            controller,
            cmd_rx,
            progress_tx,
        ));

        Self {
            cmd_tx,
            ui_rx,
            tui_event_rx,
            hil_rx,
            presenter,
            pending_hil_tx: Arc::new(Mutex::new(None)),
            _controller_handle: controller_handle,
            tui_config: TuiInputConfig::default(),
            layout_config: TuiLayoutConfig::default(),
            content_registry: std::cell::RefCell::new(super::app_render::build_default_registry()),
            scripting_engine: Arc::new(quorum_application::NoScriptingEngine),
            custom_keymap: mode::CustomKeymap::new(),
            tui_accessor: None,
            clipboard: Arc::new(NoClipboard),
            listen_path: None,
            shared_config,
        }
    }

    /// Set the clipboard adapter used for yank/copy operations.
    ///
    /// Defaults to `NoClipboard`, which returns an error on `write`.
    /// Pass an `ArboardClipboard` (or similar) to enable actual copying.
    pub fn with_clipboard(mut self, clipboard: Arc<dyn ClipboardPort>) -> Self {
        self.clipboard = clipboard;
        self
    }

    /// Expose a JSON-RPC remote control socket at `path` (see `tui::remote`).
    pub fn with_listen(mut self, path: std::path::PathBuf) -> Self {
        self.listen_path = Some(path);
        self
    }

    // -- Builder methods (delegate to controller via commands) --

    pub fn with_verbose(self, verbose: bool) -> Self {
        let _ = self.cmd_tx.send(TuiCommand::SetVerbose(verbose));
        self
    }

    pub fn with_cancellation(self, token: CancellationToken) -> Self {
        let _ = self.cmd_tx.send(TuiCommand::SetCancellation(token));
        self
    }

    pub fn with_consensus_level(self, _level: ConsensusLevel) -> Self {
        self
    }

    pub fn with_moderator(self, _model: Model) -> Self {
        self
    }

    pub fn with_working_dir(self, _dir: impl Into<String>) -> Self {
        self
    }

    pub fn with_final_review(self, _enable: bool) -> Self {
        self
    }

    pub fn with_reference_resolver(
        self,
        resolver: std::sync::Arc<dyn quorum_application::ReferenceResolverPort>,
    ) -> Self {
        let _ = self.cmd_tx.send(TuiCommand::SetReferenceResolver(resolver));
        self
    }

    /// Feed `request` to the root interaction as if it had been typed into
    /// the active pane, right after construction.
    ///
    /// Used to run the CLI's positional `QUESTION` as the first request in
    /// `--headless` mode (`copilot-quorum --headless --listen SOCK "..."`)
    /// instead of silently discarding it — with a real terminal, a `QUESTION`
    /// always takes the single-request path instead of the TUI, so this is
    /// only reachable in practice when `--headless` forced the TUI branch.
    /// Safe to queue before `run`/`run_headless` starts: the welcome message
    /// is already enqueued by the controller task at construction time, so
    /// it is always displayed first regardless of when this is sent.
    pub fn with_initial_request(self, request: impl Into<String>) -> Self {
        let _ = self.cmd_tx.send(TuiCommand::ProcessRequest {
            interaction_id: None,
            request: request.into(),
        });
        self
    }

    /// Spawn a brand new root-level interaction (no parent) and return its id
    /// once the controller task has created it.
    ///
    /// Used by headless entry points that start a fresh interaction rather
    /// than nesting under whichever interaction happens to be active (e.g.
    /// #300's `review` subcommand). Pair with [`Self::run_headless_until`] to
    /// await its completion.
    pub async fn spawn_root_interaction(
        &self,
        form: InteractionForm,
        label: impl Into<String>,
        material: impl Into<String>,
    ) -> io::Result<InteractionId> {
        let (respond_to, rx) = oneshot::channel();
        self.cmd_tx
            .send(TuiCommand::SpawnRootInteraction {
                form,
                label: label.into(),
                material: material.into(),
                respond_to,
            })
            .map_err(|_| io::Error::other("controller task unavailable"))?;
        rx.await
            .map_err(|_| io::Error::other("controller task dropped without responding"))
    }

    pub fn with_tui_config(mut self, config: TuiInputConfig) -> Self {
        self.tui_config = config;
        self
    }

    pub fn with_layout_config(mut self, config: TuiLayoutConfig) -> Self {
        self.layout_config = config;
        self
    }

    /// Set the scripting engine and build custom keymaps from its registrations.
    pub fn with_scripting_engine(
        mut self,
        engine: Arc<dyn quorum_application::ScriptingEnginePort>,
    ) -> Self {
        let keymaps = engine.registered_keymaps();
        self.custom_keymap = mode::CustomKeymap::from_registered(&keymaps);
        // Share scripting engine with the controller for Lua command dispatch
        let _ = self
            .cmd_tx
            .send(TuiCommand::SetScriptingEngine(engine.clone()));
        self.scripting_engine = engine;
        self
    }

    /// Set the TUI accessor for Lua scripting integration.
    pub fn with_tui_accessor(mut self, accessor: Arc<Mutex<dyn TuiAccessorPort>>) -> Self {
        self.tui_accessor = Some(accessor);
        self
    }

    /// Apply pending changes from the TUI accessor (Lua scripting) to TuiState.
    ///
    /// Called each frame before rendering. Drains the accessor's pending changes
    /// and applies route overrides, preset switches, content slot registrations,
    /// and text updates.
    fn apply_tui_changes(&self, state: &mut TuiState) {
        let accessor = match &self.tui_accessor {
            Some(a) => a,
            None => return,
        };

        // Lock briefly, drain all pending changes, then release the lock.
        let changes = {
            let mut acc = accessor.lock().unwrap();
            acc.take_pending_changes()
        };

        super::app_tui_changes::apply_pending_tui_changes(changes, state, &self.content_registry);
    }

    /// Run the TUI main loop
    pub async fn run(&mut self) -> io::Result<()> {
        // Validate the remote-control socket path (--listen) up front, before
        // touching the terminal. A too-long path otherwise fails deep in libc
        // bind() with the opaque "SUN_LEN" message *after* raw mode is enabled,
        // corrupting the screen; validating here surfaces a friendly error on a
        // clean terminal. (#272)
        if let Some(path) = &self.listen_path {
            super::remote::validate_socket_path(path)?;
        }

        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

        // Enable keyboard enhancement (kitty protocol) for Shift+Enter detection.
        // Silently ignored on terminals that don't support it.
        let keyboard_enhanced = execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )
        .is_ok();

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Install panic hook to restore terminal
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = execute!(
                io::stdout(),
                PopKeyboardEnhancementFlags,
                LeaveAlternateScreen,
                DisableMouseCapture
            );
            original_hook(info);
        }));

        let mut state = TuiState::new();
        if let Ok(size) = terminal.size() {
            state.term_size = (size.width, size.height);
        }
        state.tui_config = self.tui_config.clone();
        state.layout_config = self.layout_config.clone();
        state.route = super::route::RouteTable::from_preset_and_overrides(
            self.layout_config.preset.clone(),
            &self.layout_config.route_overrides,
        );
        let mut event_stream = EventStream::new();
        let mut tick = tokio::time::interval(Duration::from_millis(250));

        // Remote control socket (--listen). The receiver participates in the
        // select! loop below; without a listener the channel simply never
        // yields (all senders dropped → branch disabled).
        let (remote_tx, mut remote_rx) = mpsc::unbounded_channel::<super::remote::RemoteRequest>();
        if let Some(path) = &self.listen_path {
            super::remote::spawn_listener(path.clone(), remote_tx)?;
        } else {
            drop(remote_tx);
        }

        // Send welcome
        let _ = self.cmd_tx.send(TuiCommand::HandleCommand {
            interaction_id: None,
            command: "__welcome".into(),
        });

        loop {
            // Apply pending Lua scripting changes before rendering
            self.apply_tui_changes(&mut state);

            // Render
            terminal.draw(|frame| {
                super::app_render::render(frame, &state, &self.content_registry);
            })?;

            if state.should_quit {
                break;
            }

            // select! on all event sources
            // biased: prioritize ui_rx (InteractionSpawned, etc.) over tui_event_rx
            // to ensure tabs exist before progress events target them.
            tokio::select! {
                biased;

                // Terminal events (keyboard, mouse, resize)
                Some(Ok(term_event)) = event_stream.next() => {
                    if let Some(side_effect) = self.handle_terminal_event(&mut state, term_event) {
                        match side_effect {
                            SideEffect::LaunchEditor => {
                                Self::run_editor(&mut terminal, &mut state, keyboard_enhanced)?;
                            }
                        }
                    }
                }

                // UiEvents from controller (via AgentController → ui_tx)
                Some(ui_event) = self.ui_rx.recv() => {
                    self.presenter.apply(&mut state, &ui_event);
                }

                // TuiEvents from progress bridge / presenter
                Some(routed) = self.tui_event_rx.recv() => {
                    super::app_event_dispatch::apply_routed_tui_event(
                        &mut state,
                        &self.content_registry,
                        routed,
                    );
                }

                // HiL requests
                Some(hil_request) = self.hil_rx.recv() => {
                    super::app_hil::handle_hil_request(
                        &mut state,
                        &self.pending_hil_tx,
                        hil_request,
                    );
                }

                // Remote control requests (--listen socket)
                Some(remote_request) = remote_rx.recv() => {
                    let terminal_size = terminal
                        .size()
                        .unwrap_or_else(|_| headless_terminal_size());
                    let ctx = super::remote::RemoteContext {
                        deps: self.input_deps(),
                        terminal_size,
                        shared_config: &self.shared_config,
                    };
                    super::remote::handle_request(&mut state, &ctx, remote_request);
                }

                // Tick for flash expiry, spinner animation, etc.
                _ = tick.tick() => {
                    state.expire_flash(Duration::from_secs(5));
                }
            }
        }

        // Restore terminal
        disable_raw_mode()?;
        if keyboard_enhanced {
            let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
        }
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        Ok(())
    }

    /// Run the TUI event loop headless: no raw mode, no alternate screen, no
    /// `EventStream`, no `terminal.draw`. Input arrives only through the
    /// `--listen` socket; `screen.capture` still off-screen-renders the same
    /// `TuiState` (`remote_view::capture_screen`), so an external agent sees
    /// exactly what a TTY user would (#303, RFC #304 D3).
    ///
    /// Requires `--listen` — enforced by clap's `requires` on `--headless`,
    /// re-checked here since `TuiApp` can be constructed without going
    /// through CLI parsing. Exits on `:q!` / `:qa` (via `command.exec`) or on
    /// SIGINT/SIGTERM, since there is no keyboard to type a quit command.
    ///
    /// See [`Self::run_headless_until`] for the #300 sibling that awaits one
    /// specific interaction instead of running forever — used by the `review`
    /// subcommand, which doesn't take `--listen` at all.
    pub async fn run_headless(&mut self) -> io::Result<()> {
        let path = self.listen_path.clone().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "ヘッドレスモードには --listen が必須です(ソケットなしでは操作できなくなります)。\
                 例: copilot-quorum --headless --listen /tmp/quorum.sock",
            )
        })?;
        super::remote::validate_socket_path(&path)?;

        let mut state = TuiState::new();
        state.tui_config = self.tui_config.clone();
        state.layout_config = self.layout_config.clone();
        state.route = super::route::RouteTable::from_preset_and_overrides(
            self.layout_config.preset.clone(),
            &self.layout_config.route_overrides,
        );

        let mut tick = tokio::time::interval(Duration::from_millis(250));

        // Remote control socket (--listen). Unlike `run()`, this is not
        // optional here — headless with no listener would be unoperable,
        // which is why `--headless` requires `--listen` up front.
        let (remote_tx, mut remote_rx) = mpsc::unbounded_channel::<super::remote::RemoteRequest>();
        super::remote::spawn_listener(path, remote_tx)?;

        // No terminal to catch Ctrl+C as a raw-mode key event, so listen for
        // the process signals directly.
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

        // Send welcome
        let _ = self.cmd_tx.send(TuiCommand::HandleCommand {
            interaction_id: None,
            command: "__welcome".into(),
        });

        loop {
            // Apply pending Lua scripting changes (same as `run()`, minus the draw)
            self.apply_tui_changes(&mut state);

            if state.should_quit {
                break;
            }

            tokio::select! {
                biased;

                // UiEvents from controller (via AgentController → ui_tx)
                Some(ui_event) = self.ui_rx.recv() => {
                    self.presenter.apply(&mut state, &ui_event);
                }

                // TuiEvents from progress bridge / presenter
                Some(routed) = self.tui_event_rx.recv() => {
                    super::app_event_dispatch::apply_routed_tui_event(
                        &mut state,
                        &self.content_registry,
                        routed,
                    );
                }

                // HiL requests
                Some(hil_request) = self.hil_rx.recv() => {
                    super::app_hil::handle_hil_request(
                        &mut state,
                        &self.pending_hil_tx,
                        hil_request,
                    );
                }

                // Remote control requests (--listen socket)
                Some(remote_request) = remote_rx.recv() => {
                    let ctx = super::remote::RemoteContext {
                        deps: self.input_deps(),
                        terminal_size: headless_terminal_size(),
                        shared_config: &self.shared_config,
                    };
                    super::remote::handle_request(&mut state, &ctx, remote_request);
                }

                // Tick for flash expiry
                _ = tick.tick() => {
                    state.expire_flash(Duration::from_secs(5));
                }

                // Graceful shutdown — no keyboard, so these are the only
                // out-of-band ways to stop a headless process.
                _ = tokio::signal::ctrl_c() => {
                    break;
                }
                _ = sigterm.recv() => {
                    break;
                }
            }
        }

        Ok(())
    }

    /// Run the headless event loop until a specific interaction completes (or
    /// fails / the process is interrupted), returning its outcome instead of
    /// looping forever like [`Self::run_headless`].
    ///
    /// Used by #300's `review` subcommand: build a headless `TuiApp`, spawn a
    /// Review interaction via [`Self::spawn_root_interaction`], then await it
    /// here. Unlike `run_headless`, `--listen` is optional — `review` doesn't
    /// take a `--listen` flag at all, but a future caller that also wants the
    /// socket open (e.g. for `screen.capture` while a review runs) can still
    /// set one via [`Self::with_listen`].
    ///
    /// RFC #304 D2/D3 originally sketched this as comparing the awaited
    /// `interaction_id` against each `ui_event`'s id inline. In practice the
    /// success path needs the *structured* [`InteractionResult`], not just
    /// the notification text — so [`InteractionCompletedEvent`] now carries
    /// it, and this loop returns it directly. Failure is signaled by the
    /// relevant `*Error` `UiEvent` instead, since a headless caller only ever
    /// has one interaction in flight, so any error unambiguously belongs to it.
    pub async fn run_headless_until(
        &mut self,
        interaction_id: InteractionId,
    ) -> io::Result<InteractionOutcome> {
        use quorum_application::UiEvent;

        let mut state = TuiState::new();
        state.tui_config = self.tui_config.clone();
        state.layout_config = self.layout_config.clone();
        state.route = super::route::RouteTable::from_preset_and_overrides(
            self.layout_config.preset.clone(),
            &self.layout_config.route_overrides,
        );

        let mut tick = tokio::time::interval(Duration::from_millis(250));

        let (remote_tx, mut remote_rx) = mpsc::unbounded_channel::<super::remote::RemoteRequest>();
        if let Some(path) = self.listen_path.clone() {
            super::remote::validate_socket_path(&path)?;
            super::remote::spawn_listener(path, remote_tx)?;
        } else {
            drop(remote_tx);
        }

        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

        loop {
            self.apply_tui_changes(&mut state);

            if state.should_quit {
                return Ok(InteractionOutcome::Interrupted);
            }

            tokio::select! {
                biased;

                Some(ui_event) = self.ui_rx.recv() => {
                    let outcome = match &ui_event {
                        UiEvent::InteractionCompleted(event) if event.id == interaction_id => {
                            event.result.clone().map(InteractionOutcome::Completed)
                        }
                        UiEvent::AskError { error }
                        | UiEvent::QuorumError { error }
                        | UiEvent::ReviewError { error }
                        | UiEvent::InteractionSpawnError { error } => {
                            Some(InteractionOutcome::Failed(error.clone()))
                        }
                        UiEvent::AgentError(e) => Some(InteractionOutcome::Failed(e.error.clone())),
                        _ => None,
                    };
                    self.presenter.apply(&mut state, &ui_event);
                    if let Some(outcome) = outcome {
                        return Ok(outcome);
                    }
                }

                Some(routed) = self.tui_event_rx.recv() => {
                    super::app_event_dispatch::apply_routed_tui_event(
                        &mut state,
                        &self.content_registry,
                        routed,
                    );
                }

                Some(hil_request) = self.hil_rx.recv() => {
                    super::app_hil::handle_hil_request(
                        &mut state,
                        &self.pending_hil_tx,
                        hil_request,
                    );
                }

                Some(remote_request) = remote_rx.recv() => {
                    let ctx = super::remote::RemoteContext {
                        deps: self.input_deps(),
                        terminal_size: headless_terminal_size(),
                        shared_config: &self.shared_config,
                    };
                    super::remote::handle_request(&mut state, &ctx, remote_request);
                }

                _ = tick.tick() => {
                    state.expire_flash(Duration::from_secs(5));
                }

                _ = tokio::signal::ctrl_c() => {
                    return Ok(InteractionOutcome::Interrupted);
                }
                _ = sigterm.recv() => {
                    return Ok(InteractionOutcome::Interrupted);
                }
            }
        }
    }

    /// Suspend the TUI, launch $EDITOR, and resume.
    ///
    /// The current INSERT buffer content is passed as initial text.
    /// On save, the content replaces the INSERT buffer and mode switches to Insert.
    /// On cancel, nothing changes and mode stays Normal.
    fn run_editor(
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        state: &mut TuiState,
        keyboard_enhanced: bool,
    ) -> io::Result<()> {
        let initial_text = state.tabs.active_pane().input.clone();

        let context = EditorContext {
            consensus_level: format!("{}", state.consensus_level),
            phase_scope: format!("{}", state.phase_scope),
            strategy: "Quorum".to_string(),
        };

        // Suspend TUI
        disable_raw_mode()?;
        if keyboard_enhanced {
            let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
        }
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        // Launch editor (blocking)
        let result = editor::launch_editor_with_options(
            &initial_text,
            &context,
            state.tui_config.context_header,
        );

        // Resume TUI
        enable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            EnterAlternateScreen,
            EnableMouseCapture
        )?;
        if keyboard_enhanced {
            let _ = execute!(
                terminal.backend_mut(),
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            );
        }
        terminal.clear()?;

        // Apply result
        match result {
            EditorResult::Saved(text) => {
                let pane = state.tabs.active_pane_mut();
                pane.input = text;
                pane.cursor_pos = pane.input.len();
                state.mode = InputMode::Insert;
                state.set_flash("Editor: content loaded into input buffer");
            }
            EditorResult::Cancelled => {
                state.set_flash("Editor: cancelled");
            }
        }

        Ok(())
    }

    /// Borrow the dependencies needed for terminal-event dispatch.
    pub(super) fn input_deps(&self) -> InputDeps<'_> {
        InputDeps {
            cmd_tx: &self.cmd_tx,
            pending_hil_tx: &self.pending_hil_tx,
            custom_keymap: &self.custom_keymap,
            scripting_engine: &self.scripting_engine,
            clipboard: &self.clipboard,
            content_registry: &self.content_registry,
        }
    }

    /// Handle a terminal (crossterm) event.
    /// Returns a `SideEffect` if the main loop needs to perform a terminal-level action.
    fn handle_terminal_event(
        &self,
        state: &mut TuiState,
        event: crossterm::event::Event,
    ) -> Option<SideEffect> {
        dispatch_terminal_event(state, event, &self.input_deps())
    }
}
