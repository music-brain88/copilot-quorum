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
    AgentController, ContextLoaderPort, ConversationLogger, LlmGateway, NoConversationLogger,
    ToolExecutorPort, ToolSchemaPort, TuiAccessorPort, UiEvent,
};
use quorum_domain::{ConsensusLevel, HumanDecision, Model};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use super::human_intervention::TuiHumanIntervention;

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
        )
    }

    /// Create a new TUI application with a conversation logger.
    pub fn new_with_logger(
        gateway: Arc<dyn LlmGateway>,
        tool_executor: Arc<dyn ToolExecutorPort>,
        tool_schema: Arc<dyn ToolSchemaPort>,
        context_loader: Arc<dyn ContextLoaderPort>,
        config: Arc<Mutex<QuorumConfig>>,
        conversation_logger: Arc<dyn ConversationLogger>,
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

        // Controller (runs in background task)
        let controller = AgentController::new(
            gateway,
            tool_executor,
            tool_schema,
            context_loader,
            config,
            human_intervention,
            ui_tx,
        )
        .with_conversation_logger(conversation_logger);

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
        }
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
        state.tui_config = self.tui_config.clone();
        state.layout_config = self.layout_config.clone();
        state.route = super::route::RouteTable::from_preset_and_overrides(
            self.layout_config.preset.clone(),
            &self.layout_config.route_overrides,
        );
        let mut event_stream = EventStream::new();
        let mut tick = tokio::time::interval(Duration::from_millis(250));

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

    /// Handle a terminal (crossterm) event.
    /// Returns a `SideEffect` if the main loop needs to perform a terminal-level action.
    fn handle_terminal_event(
        &self,
        state: &mut TuiState,
        event: crossterm::event::Event,
    ) -> Option<SideEffect> {
        match event {
            crossterm::event::Event::Key(key) => {
                // If HiL modal is showing, handle y/n/Esc
                if state.hil_prompt.is_some() {
                    super::app_hil::handle_hil_key(state, &self.pending_hil_tx, key);
                    return None;
                }

                // If help is showing, Esc or ? closes it
                if state.show_help {
                    match key.code {
                        crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('?') => {
                            state.show_help = false;
                            return None;
                        }
                        _ => {}
                    }
                }

                // Check custom keymap (from Lua) before built-in bindings
                if let Some(custom_action) = self.custom_keymap.lookup(state.mode, &key) {
                    state.pending_key = None;
                    return super::app_action_handler::handle_action(
                        state,
                        custom_action.clone(),
                        &self.cmd_tx,
                        &self.scripting_engine,
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
                    &self.cmd_tx,
                    &self.scripting_engine,
                )
            }
            crossterm::event::Event::Resize(_, _) => None,
            _ => None,
        }
    }
}
