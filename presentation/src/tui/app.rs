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
use super::event::{HilKind, HilRequest, RoutedTuiEvent, TuiCommand, TuiEvent};
use super::layout::TuiLayoutConfig;
use super::mode::{self, InputMode, KeyAction};
use super::presenter::TuiPresenter;
use super::progress::TuiProgressBridge;
use super::state::{
    DisplayMessage, EnsembleProgress, HilPrompt, QuorumStatus, TaskProgress, TaskSummary,
    ToolExecutionDisplay, ToolExecutionDisplayStatus, TuiInputConfig, TuiState,
};
use super::surface::SurfaceId;
use super::tab::PaneKind;
use super::widgets::{
    MainLayout, header::HeaderWidget, input::InputWidget, status_bar::StatusBarWidget,
    tab_bar::TabBarWidget,
};

/// Side-effect that requires main loop intervention (e.g. terminal suspend)
enum SideEffect {
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
    AgentController, CommandAction, ContextLoaderPort, ConversationLogger, LlmGateway,
    NoConversationLogger, ToolExecutorPort, ToolSchemaPort, UiEvent,
};
use quorum_domain::core::string::truncate;
use quorum_domain::interaction::{InteractionForm, InteractionId};
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
}

impl TuiApp {
    /// Create a new TUI application wired to the controller
    pub fn new(
        gateway: Arc<dyn LlmGateway>,
        tool_executor: Arc<dyn ToolExecutorPort>,
        tool_schema: Arc<dyn ToolSchemaPort>,
        context_loader: Arc<dyn ContextLoaderPort>,
        config: QuorumConfig,
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
        config: QuorumConfig,
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

        let controller_handle = tokio::spawn(controller_task(controller, cmd_rx, progress_tx));

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
            content_registry: std::cell::RefCell::new(Self::build_default_registry()),
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
            self.layout_config.preset,
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
            // Render
            terminal.draw(|frame| {
                self.render(frame, &state);
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
                    self.apply_routed_tui_event(&mut state, routed);
                }

                // HiL requests
                Some(hil_request) = self.hil_rx.recv() => {
                    self.handle_hil_request(&mut state, hil_request);
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

    /// Build the default content registry with all built-in renderers.
    fn build_default_registry() -> ContentRegistry {
        use super::widgets::{
            conversation::ConversationRenderer, progress_panel::ProgressRenderer,
            tool_log::ToolLogRenderer,
        };

        ContentRegistry::new()
            .register(Box::new(ConversationRenderer))
            .register(Box::new(ProgressRenderer))
            .register(Box::new(ToolLogRenderer))
    }

    /// Render all widgets via registry-driven dispatch.
    fn render(&self, frame: &mut ratatui::Frame, state: &TuiState) {
        use super::surface::SurfaceLayout;

        let pane_surfaces = state.route.required_pane_surfaces();
        let show_tab_bar = state.tabs.len() > 1;
        let layout = MainLayout::compute_with_layout(
            frame.area(),
            state.input_line_count() as u16,
            state.tui_config.max_input_height,
            show_tab_bar,
            state.layout_config.preset,
            state.layout_config.flex_threshold,
            pane_surfaces.len(),
        );

        // Build surface layout from the computed main layout + pane surfaces
        let surfaces = SurfaceLayout::from_main_layout(&layout, &pane_surfaces);

        // Fixed chrome
        frame.render_widget(HeaderWidget::new(state), layout.header);
        if let Some(tab_bar_area) = surfaces.area_for(&SurfaceId::TabBar) {
            frame.render_widget(TabBarWidget::new(&state.tabs), tab_bar_area);
        }
        frame.render_widget(InputWidget::new(state), layout.input);
        frame.render_widget(StatusBarWidget::new(state), layout.status_bar);

        // Registry-driven content dispatch
        let registry = self.content_registry.borrow();
        for entry in state.route.entries() {
            if let Some(area) = surfaces.area_for(&entry.surface)
                && let Some(renderer) = registry.get(&entry.content)
            {
                renderer.render_content(state, area, frame.buffer_mut());
            }
        }

        // Dynamic overlays (rendered on top, only when visible)
        if state.show_help {
            let help_area = MainLayout::centered_overlay(70, 70, frame.area());
            frame.render_widget(ratatui::widgets::Clear, help_area);
            self.render_help(frame, help_area);
        }

        if state.hil_prompt.is_some() {
            let modal_area = MainLayout::centered_overlay(60, 50, frame.area());
            frame.render_widget(ratatui::widgets::Clear, modal_area);
            self.render_hil_modal(frame, modal_area, state);
        }
    }

    fn render_help(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        use ratatui::style::{Color, Modifier, Style};
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

        let lines = vec![
            Line::from(Span::styled(
                "Keyboard Shortcuts",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Normal Mode:"),
            Line::from("  i      Enter Insert mode"),
            Line::from("  :      Enter Command mode"),
            Line::from("  s      Switch to Solo mode"),
            Line::from("  e      Switch to Ensemble mode"),
            Line::from("  f      Toggle Fast scope"),
            Line::from("  a      Ask (prefill :ask )"),
            Line::from("  d      Discuss (prefill :discuss )"),
            Line::from("  j/k    Scroll down/up"),
            Line::from("  gg/G   Scroll to top/bottom"),
            Line::from("  gt/gT  Next/prev tab"),
            Line::from("  ?      Toggle this help"),
            Line::from("  Ctrl+C Quit"),
            Line::from(""),
            Line::from("  I      Open $EDITOR (with current input)"),
            Line::from(""),
            Line::from("Insert Mode:"),
            Line::from("  Enter        Send message"),
            Line::from("  Shift+Enter  Insert newline (multiline)"),
            Line::from("  Esc        Return to Normal"),
            Line::from(""),
            Line::from("Commands (:command):"),
            Line::from("  :q       Quit"),
            Line::from("  :help    Show help"),
            Line::from("  :solo    Switch to Solo mode"),
            Line::from("  :ens     Switch to Ensemble mode"),
            Line::from("  :fast    Toggle fast mode"),
            Line::from("  :ask <question>   Ask (lightweight Q&A)"),
            Line::from("  :discuss <question> Discuss (quorum discussion)"),
            Line::from("  :config  Show configuration"),
            Line::from("  :clear   Clear history"),
            Line::from("  :tabnew [form]  New tab (agent/ask/discuss)"),
            Line::from("  :tabclose       Close tab"),
            Line::from("  :tabs           List tabs"),
            Line::from(""),
            Line::from(Span::styled(
                "Press ? or Esc to close",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Help ")
            .style(Style::default().fg(Color::Cyan));

        frame.render_widget(
            Paragraph::new(lines).block(block).wrap(Wrap { trim: true }),
            area,
        );
    }

    fn render_hil_modal(
        &self,
        frame: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
        state: &TuiState,
    ) {
        use ratatui::style::{Color, Modifier, Style};
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

        let hil = state.hil_prompt.as_ref().unwrap();
        let mut lines = vec![
            Line::from(Span::styled(
                &hil.title,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(format!("Objective: {}", hil.objective)),
            Line::from(""),
        ];

        for (i, task) in hil.tasks.iter().enumerate() {
            lines.push(Line::from(format!("  {}. {}", i + 1, task)));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(&*hil.message));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "y: approve  n: reject  Esc: reject",
            Style::default().fg(Color::DarkGray),
        )));

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Human Intervention ")
            .style(Style::default().fg(Color::Yellow));

        frame.render_widget(
            Paragraph::new(lines).block(block).wrap(Wrap { trim: true }),
            area,
        );
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
                    self.handle_hil_key(state, key);
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

                let action = mode::handle_key_event(state.mode, key, state.pending_key);
                if let KeyAction::PendingKey(c) = action {
                    state.pending_key = Some(c);
                    return None;
                }
                state.pending_key = None;
                self.handle_action(state, action)
            }
            crossterm::event::Event::Resize(_, _) => None,
            _ => None,
        }
    }

    /// Handle a semantic key action.
    /// Returns a `SideEffect` if the main loop needs to perform a terminal-level action.
    fn handle_action(&self, state: &mut TuiState, action: KeyAction) -> Option<SideEffect> {
        match action {
            KeyAction::None => {}

            // Mode transitions
            KeyAction::EnterInsert => state.mode = InputMode::Insert,
            KeyAction::EnterCommand => {
                state.mode = InputMode::Command;
                state.command_input.clear();
                state.command_cursor = 0;
            }
            KeyAction::ExitToNormal => state.mode = InputMode::Normal,

            // Text editing
            KeyAction::InsertChar(c) => state.insert_char(c),
            KeyAction::InsertNewline => state.insert_newline(),
            KeyAction::DeleteChar => state.delete_char(),
            KeyAction::CursorLeft => state.cursor_left(),
            KeyAction::CursorRight => state.cursor_right(),
            KeyAction::CursorHome => state.cursor_home(),
            KeyAction::CursorEnd => state.cursor_end(),

            // Submit
            KeyAction::SubmitInput => {
                let input = state.take_input();
                if !input.is_empty() {
                    state.tabs.active_pane_mut().set_title_if_empty(&input);
                    state.push_message(DisplayMessage::user(&input));
                    let interaction_id = Self::active_interaction_id(state);
                    let _ = self.cmd_tx.send(TuiCommand::ProcessRequest {
                        interaction_id,
                        request: input,
                    });
                }
            }
            KeyAction::SubmitCommand => {
                let cmd = state.take_command();
                state.mode = InputMode::Normal;
                if !cmd.is_empty() {
                    if cmd == "q" || cmd == "quit" || cmd == "exit" {
                        state.should_quit = true;
                    } else if let Some(flash) = self.handle_tab_command(state, &cmd) {
                        state.set_flash(flash);
                    } else {
                        let interaction_id = Self::active_interaction_id(state);
                        let _ = self.cmd_tx.send(TuiCommand::HandleCommand {
                            interaction_id,
                            command: cmd,
                        });
                    }
                }
            }

            // Quick commands
            KeyAction::SwitchSolo => {
                let interaction_id = Self::active_interaction_id(state);
                let _ = self.cmd_tx.send(TuiCommand::HandleCommand {
                    interaction_id,
                    command: "solo".into(),
                });
            }
            KeyAction::SwitchEnsemble => {
                let interaction_id = Self::active_interaction_id(state);
                let _ = self.cmd_tx.send(TuiCommand::HandleCommand {
                    interaction_id,
                    command: "ens".into(),
                });
            }
            KeyAction::ToggleFast => {
                let interaction_id = Self::active_interaction_id(state);
                let _ = self.cmd_tx.send(TuiCommand::HandleCommand {
                    interaction_id,
                    command: "fast".into(),
                });
            }
            KeyAction::SwitchAsk => {
                // Enter command mode with "ask " pre-filled
                state.mode = InputMode::Command;
                state.command_input = "ask ".into();
                state.command_cursor = state.command_input.len();
            }
            KeyAction::SwitchDiscuss => {
                // Enter command mode with "discuss " pre-filled
                state.mode = InputMode::Command;
                state.command_input = "discuss ".into();
                state.command_cursor = state.command_input.len();
            }

            // Scrolling
            KeyAction::ScrollUp => state.scroll_up(),
            KeyAction::ScrollDown => state.scroll_down(),
            KeyAction::ScrollToTop => state.scroll_to_top(),
            KeyAction::ScrollToBottom => state.scroll_to_bottom(),

            // Tabs
            KeyAction::NextTab => {
                state.tabs.next_tab();
                if let PaneKind::Interaction(_, Some(id)) = state.tabs.active_pane().kind {
                    let _ = self.cmd_tx.send(TuiCommand::ActivateInteraction(id));
                }
                state.set_flash(format!(
                    "Tab {}/{}",
                    state.tabs.active_index() + 1,
                    state.tabs.len()
                ));
            }
            KeyAction::PrevTab => {
                state.tabs.prev_tab();
                if let PaneKind::Interaction(_, Some(id)) = state.tabs.active_pane().kind {
                    let _ = self.cmd_tx.send(TuiCommand::ActivateInteraction(id));
                }
                state.set_flash(format!(
                    "Tab {}/{}",
                    state.tabs.active_index() + 1,
                    state.tabs.len()
                ));
            }

            // PendingKey is handled in handle_terminal_event before reaching here
            KeyAction::PendingKey(_) => {}

            // Editor — requires terminal suspend, handled by main loop
            KeyAction::LaunchEditor => {
                return Some(SideEffect::LaunchEditor);
            }

            // Application
            KeyAction::Quit => state.should_quit = true,
            KeyAction::ShowHelp => state.show_help = !state.show_help,
            KeyAction::ToggleConsensus => {
                // Handled by command
                let _ = self.cmd_tx.send(TuiCommand::HandleCommand {
                    interaction_id: Self::active_interaction_id(state),
                    command: "toggle_consensus".into(),
                });
            }
        }
        None
    }

    fn active_interaction_id(state: &TuiState) -> Option<InteractionId> {
        match state.tabs.active_pane().kind {
            PaneKind::Interaction(_, Some(id)) => Some(id),
            PaneKind::Interaction(_, None) => None,
        }
    }

    /// Apply a routed TuiEvent to the appropriate interaction pane
    fn apply_routed_tui_event(&self, state: &mut TuiState, routed: RoutedTuiEvent) {
        if let Some(id) = routed.interaction_id
            && state.tabs.find_tab_index_by_interaction(id).is_some()
        {
            self.apply_tui_event_to_interaction(state, id, routed.event);
            return;
        }
        // Fallback to active pane (global event or untargeted)
        self.apply_tui_event(state, routed.event);
    }

    /// Apply event to a specific interaction pane — the single source of truth
    /// for all TuiEvent → state mapping.
    ///
    /// Called directly by [`apply_routed_tui_event`] for targeted events, and
    /// indirectly by [`apply_tui_event`] for untargeted (active-pane) events.
    fn apply_tui_event_to_interaction(
        &self,
        state: &mut TuiState,
        id: InteractionId,
        event: TuiEvent,
    ) {
        match event {
            TuiEvent::StreamChunk(chunk) => {
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                    pane.conversation.streaming_text.push_str(&chunk);
                    if pane.conversation.auto_scroll {
                        pane.conversation.scroll_offset = 0;
                    }
                }
            }
            TuiEvent::StreamEnd => {
                state.finalize_stream_for(id);
            }
            TuiEvent::PhaseChange { phase, name } => {
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                    let progress = &mut pane.progress;
                    progress.current_phase = Some(phase);
                    progress.phase_name = name;
                }
            }
            TuiEvent::TaskStart {
                description,
                index,
                total,
            } => {
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                    let progress = &mut pane.progress;
                    let completed_tasks = progress
                        .task_progress
                        .as_ref()
                        .map(|tp| tp.completed_tasks.clone())
                        .unwrap_or_default();
                    progress.task_progress = Some(TaskProgress {
                        current_index: index,
                        total,
                        description: description.clone(),
                        completed_tasks,
                        active_tool_executions: Vec::new(),
                    });
                }
                state.push_message_to(
                    id,
                    DisplayMessage::system(format!(
                        "Executing Task {}/{}: {}",
                        index, total, description
                    )),
                );
            }
            TuiEvent::TaskComplete {
                description,
                success,
                index,
                total: _,
                output,
            } => {
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                    let progress = &mut pane.progress;
                    let active_execs = if let Some(ref mut tp) = progress.task_progress {
                        std::mem::take(&mut tp.active_tool_executions)
                    } else {
                        Vec::new()
                    };
                    if let Some(ref mut tp) = progress.task_progress {
                        tp.completed_tasks.push(TaskSummary {
                            index,
                            description: description.clone(),
                            success,
                            output: output.clone(),
                            duration_ms: None,
                            tool_executions: active_execs,
                        });
                    }
                }

                let tool_exec_lines = if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                    if let Some(ref tp) = pane.progress.task_progress {
                        tp.completed_tasks
                            .last()
                            .map(|summary| {
                                summary
                                    .tool_executions
                                    .iter()
                                    .map(|exec| {
                                        let (icon, dur) = match &exec.state {
                                            ToolExecutionDisplayStatus::Completed { .. } => {
                                                let d = exec
                                                    .duration_ms
                                                    .map(|ms| {
                                                        if ms < 1000 {
                                                            format!("{}ms", ms)
                                                        } else {
                                                            format!("{:.1}s", ms as f64 / 1000.0)
                                                        }
                                                    })
                                                    .unwrap_or_default();
                                                ("✓", d)
                                            }
                                            ToolExecutionDisplayStatus::Error { message } => {
                                                ("✗", truncate(message, 40))
                                            }
                                            _ => ("…", String::new()),
                                        };
                                        format!("  {} {} ({})", icon, exec.tool_name, dur)
                                    })
                                    .collect::<Vec<_>>()
                                    .join(
                                        "
",
                                    )
                            })
                            .unwrap_or_default()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };

                let status = if success { "✓" } else { "✗" };
                let mut msg = if let Some(ref out) = output {
                    let extracted = extract_response_text(out);
                    if extracted.is_empty() {
                        format!("Task {} {} {}", index, status, description)
                    } else {
                        format!(
                            "Task {} {} {}\n  Output: {}",
                            index, status, description, extracted
                        )
                    }
                } else {
                    format!("Task {} {} {}", index, status, description)
                };
                if !tool_exec_lines.is_empty() {
                    msg.push('\n');
                    msg.push_str(&tool_exec_lines);
                }
                state.push_message_to(id, DisplayMessage::system(msg));
            }
            TuiEvent::InteractionCompleted {
                parent_id,
                result_text,
            } => {
                let target_id = parent_id.unwrap_or(id);
                state.push_message_to(target_id, DisplayMessage::system(result_text));
                state.set_flash("Child interaction completed");
            }
            TuiEvent::QuorumStart { phase, model_count } => {
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                    pane.progress.quorum_status = Some(QuorumStatus {
                        phase,
                        total: model_count,
                        completed: 0,
                        approved: 0,
                    });
                }
            }
            TuiEvent::QuorumModelVote { model: _, approved } => {
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id)
                    && let Some(ref mut qs) = pane.progress.quorum_status
                {
                    qs.completed += 1;
                    if approved {
                        qs.approved += 1;
                    }
                }
            }
            TuiEvent::QuorumComplete {
                phase,
                approved,
                feedback: _,
            } => {
                let status = if approved { "APPROVED" } else { "REJECTED" };
                state.set_flash(format!("{}: {}", phase, status));
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                    pane.progress.quorum_status = None;
                }
            }
            TuiEvent::PlanRevision { revision, feedback } => {
                state.push_message_to(
                    id,
                    DisplayMessage::system(format!("Plan revision #{}: {}", revision, feedback)),
                );
            }
            TuiEvent::EnsembleStart(count) => {
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                    pane.progress.ensemble_progress = Some(EnsembleProgress {
                        total_models: count,
                        plans_generated: 0,
                        models_completed: Vec::new(),
                        models_failed: Vec::new(),
                        voting_started: false,
                        plan_count: None,
                        selected: None,
                    });
                }
            }
            TuiEvent::EnsemblePlanGenerated(model) => {
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id)
                    && let Some(ref mut ep) = pane.progress.ensemble_progress
                {
                    ep.plans_generated += 1;
                    ep.models_completed.push(model);
                }
            }
            TuiEvent::EnsembleVotingStart(plan_count) => {
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id)
                    && let Some(ref mut ep) = pane.progress.ensemble_progress
                {
                    ep.voting_started = true;
                    ep.plan_count = Some(plan_count);
                }
            }
            TuiEvent::EnsembleModelFailed { model, error } => {
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id)
                    && let Some(ref mut ep) = pane.progress.ensemble_progress
                {
                    ep.models_failed.push((model, error));
                }
            }
            TuiEvent::EnsembleComplete {
                selected_model,
                score,
            } => {
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id)
                    && let Some(ref mut ep) = pane.progress.ensemble_progress
                {
                    ep.selected = Some((selected_model.clone(), score));
                }
                state.push_message_to(
                    id,
                    DisplayMessage::system(format!(
                        "Selected plan from {} (score: {:.1}/10)",
                        selected_model, score
                    )),
                );
            }
            TuiEvent::EnsembleFallback(reason) => {
                state.push_message_to(
                    id,
                    DisplayMessage::system(format!("Ensemble failed, solo fallback: {}", reason)),
                );
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                    pane.progress.ensemble_progress = None;
                }
            }
            TuiEvent::ModelStreamStart { model, context: _ } => {
                use super::content::ContentSlot;
                use super::state::{ModelStreamState, ModelStreamStatus};
                use super::widgets::model_stream::ModelStreamRenderer;

                // Register dynamic route + renderer for this model
                state.route.set_route(
                    ContentSlot::ModelStream(model.clone()),
                    SurfaceId::DynamicPane(model.clone()),
                );
                self.content_registry
                    .borrow_mut()
                    .register_mut(Box::new(ModelStreamRenderer::new(model.clone())));

                if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                    pane.progress.model_streams.insert(
                        model.clone(),
                        ModelStreamState {
                            model_name: model,
                            streaming_text: String::new(),
                            status: ModelStreamStatus::Streaming,
                            score: None,
                            duration_ms: None,
                        },
                    );
                }
            }
            TuiEvent::ModelStreamChunk { model, chunk } => {
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id)
                    && let Some(ms) = pane.progress.model_streams.get_mut(&model)
                {
                    ms.streaming_text.push_str(&chunk);
                }
            }
            TuiEvent::ModelStreamEnd(model) => {
                use super::state::ModelStreamStatus;
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id)
                    && let Some(ms) = pane.progress.model_streams.get_mut(&model)
                {
                    ms.status = ModelStreamStatus::Complete;
                }
            }
            TuiEvent::EnsembleVoteScore { model, score } => {
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id)
                    && let Some(ms) = pane.progress.model_streams.get_mut(&model)
                {
                    ms.score = Some(score);
                }
            }
            TuiEvent::AgentStarting => {
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                    let progress = &mut pane.progress;
                    progress.is_running = true;
                    progress.quorum_status = None;
                    progress.task_progress = None;
                    progress.ensemble_progress = None;
                }
            }
            TuiEvent::AgentResult {
                success,
                summary: _,
            } => {
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                    let progress = &mut pane.progress;
                    progress.is_running = false;
                    progress.current_phase = None;
                }
                if success {
                    state.set_flash("Agent completed successfully");
                } else {
                    state.set_flash("Agent completed with issues");
                }
            }
            TuiEvent::AgentError(msg) => {
                if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                    pane.progress.is_running = false;
                }
                state.set_flash(msg);
            }
            TuiEvent::Flash(msg) => {
                state.set_flash(msg);
            }
            TuiEvent::HistoryCleared => {}
            TuiEvent::Exit => {
                state.should_quit = true;
            }
            TuiEvent::ToolExecutionUpdate {
                task_index: _,
                execution_id,
                tool_name,
                state: exec_state,
                duration_ms,
                args_preview,
            } => {
                use super::event::ToolExecutionDisplayState;

                // Build flash message outside the mutable borrow scope
                let flash_msg = if let ToolExecutionDisplayState::Error { ref message } = exec_state
                {
                    Some(format!("Tool error: {} - {}", tool_name, message))
                } else {
                    None
                };

                if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                    // Auto-initialize task_progress for non-agent contexts
                    // (gather_context, run_ask) that don't emit TaskStart
                    let tp = pane
                        .progress
                        .task_progress
                        .get_or_insert_with(|| TaskProgress {
                            current_index: 0,
                            total: 0,
                            description: String::new(),
                            completed_tasks: Vec::new(),
                            active_tool_executions: Vec::new(),
                        });

                    let display_status = match exec_state {
                        ToolExecutionDisplayState::Pending => ToolExecutionDisplayStatus::Pending,
                        ToolExecutionDisplayState::Running => ToolExecutionDisplayStatus::Running,
                        ToolExecutionDisplayState::Completed { preview } => {
                            ToolExecutionDisplayStatus::Completed { preview }
                        }
                        ToolExecutionDisplayState::Error { message } => {
                            ToolExecutionDisplayStatus::Error { message }
                        }
                    };

                    if let Some(existing) = tp
                        .active_tool_executions
                        .iter_mut()
                        .find(|e| e.execution_id == execution_id)
                    {
                        existing.state = display_status;
                        existing.duration_ms = duration_ms;
                    } else {
                        tp.active_tool_executions.push(ToolExecutionDisplay {
                            execution_id,
                            tool_name,
                            state: display_status,
                            duration_ms,
                            args_preview,
                        });
                    }
                }

                if let Some(msg) = flash_msg {
                    state.set_flash(msg);
                }
            }
            // Config/mode events handled by presenter already
            TuiEvent::Welcome { .. }
            | TuiEvent::ConfigDisplay(_)
            | TuiEvent::ModeChanged { .. }
            | TuiEvent::ScopeChanged(_)
            | TuiEvent::StrategyChanged(_)
            | TuiEvent::CommandError(_) => {}
        }
    }

    /// Apply a TuiEvent (from progress bridge or presenter) to state.
    ///
    /// Delegates to [`apply_tui_event_to_interaction`] when the active pane has an
    /// interaction id. When no interaction is active, only global events are handled.
    fn apply_tui_event(&self, state: &mut TuiState, event: TuiEvent) {
        if let Some(id) = Self::active_interaction_id(state) {
            self.apply_tui_event_to_interaction(state, id, event);
            return;
        }
        // No active interaction — handle global events only
        match event {
            TuiEvent::Flash(msg) => state.set_flash(msg),
            TuiEvent::Exit => {
                state.should_quit = true;
            }
            TuiEvent::HistoryCleared => {}
            TuiEvent::Welcome { .. }
            | TuiEvent::ConfigDisplay(_)
            | TuiEvent::ModeChanged { .. }
            | TuiEvent::ScopeChanged(_)
            | TuiEvent::StrategyChanged(_)
            | TuiEvent::CommandError(_) => {}
            _ => { /* dropped — no active interaction to target */ }
        }
    }

    /// Handle HiL request — show modal, store response channel
    fn handle_hil_request(&self, state: &mut TuiState, request: HilRequest) {
        let (title, objective, tasks, message) = match &request.kind {
            HilKind::PlanIntervention {
                request: _req,
                plan,
                review_history,
            } => {
                let rev_count = review_history.iter().filter(|r| !r.approved).count();
                (
                    "Plan Requires Human Intervention".to_string(),
                    plan.objective.clone(),
                    plan.tasks.iter().map(|t| t.description.clone()).collect(),
                    format!(
                        "Revision limit ({}) exceeded. Approve or reject?",
                        rev_count
                    ),
                )
            }
            HilKind::ExecutionConfirmation { request: _, plan } => (
                "Ready to Execute Plan".to_string(),
                plan.objective.clone(),
                plan.tasks.iter().map(|t| t.description.clone()).collect(),
                "Approve execution?".to_string(),
            ),
        };

        state.hil_prompt = Some(HilPrompt {
            title,
            objective,
            tasks,
            message,
        });

        // Store the response sender — will be consumed when user presses y/n
        *self.pending_hil_tx.lock().unwrap() = Some(request.response_tx);
    }

    /// Handle key press while HiL modal is shown
    fn handle_hil_key(&self, state: &mut TuiState, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                state.hil_prompt = None;
                state.set_flash("Plan approved");
                self.send_hil_response(HumanDecision::Approve);
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                state.hil_prompt = None;
                state.set_flash("Plan rejected");
                self.send_hil_response(HumanDecision::Reject);
            }
            _ => {}
        }
    }

    /// Send the stored HiL response (consumes the oneshot sender)
    fn send_hil_response(&self, decision: HumanDecision) {
        if let Some(tx) = self.pending_hil_tx.lock().unwrap().take() {
            let _ = tx.send(decision);
        }
    }

    /// Handle tab-related commands locally (no controller round-trip).
    /// Returns Some(flash_message) if a tab command was handled, None otherwise.
    fn handle_tab_command(&self, state: &mut TuiState, cmd: &str) -> Option<String> {
        let trimmed = cmd.trim();
        let normalized = trimmed.strip_prefix(':').unwrap_or(trimmed);

        if normalized == "ask"
            || normalized.starts_with("ask ")
            || normalized == "discuss"
            || normalized.starts_with("discuss ")
            || normalized == "agent"
            || normalized.starts_with("agent ")
        {
            let (cmd_name, rest) = normalized.split_once(' ').unwrap_or((normalized, ""));
            if let Ok(form) = cmd_name.parse::<InteractionForm>() {
                if rest.is_empty() {
                    return Some(format!("Usage: {} <query>", cmd_name));
                }
                let query = rest.trim().to_string();

                // Fix A: Create placeholder tab immediately so the user sees it
                // without waiting for the controller to process SpawnInteraction.
                // The real InteractionId is bound later when InteractionSpawned arrives.
                let kind = PaneKind::Interaction(form, None);
                state.tabs.create_tab(kind);
                state.tabs.active_pane_mut().set_title_if_empty(&query);

                let _ = self.cmd_tx.send(TuiCommand::SpawnInteraction {
                    form,
                    query,
                    context_mode_override: None,
                });
                return Some(format!("Spawning {}...", cmd_name));
            }
        }

        if trimmed == "tabs" {
            // List all tabs
            let summary = state.tabs.tab_list_summary();
            state.push_message(DisplayMessage::system(summary.join("\n")));
            return Some(format!("{} tab(s) open", state.tabs.len()));
        }

        if trimmed == "tabclose" {
            if state.tabs.close_active() {
                // Sync active_interaction_id with the new active tab
                if let PaneKind::Interaction(_, Some(id)) = state.tabs.active_pane().kind {
                    let _ = self.cmd_tx.send(TuiCommand::ActivateInteraction(id));
                }
                return Some(format!("Tab closed ({} remaining)", state.tabs.len()));
            } else {
                return Some("Cannot close last tab".into());
            }
        }

        if trimmed == "tabnew" || trimmed.starts_with("tabnew ") {
            let arg = trimmed.strip_prefix("tabnew").unwrap().trim();
            let kind = if arg.is_empty() {
                PaneKind::Interaction(quorum_domain::interaction::InteractionForm::Agent, None)
            } else {
                match arg.parse::<quorum_domain::interaction::InteractionForm>() {
                    Ok(form) => PaneKind::Interaction(form, None),
                    Err(_) => {
                        return Some(format!("Unknown form: {}. Use agent/ask/discuss", arg));
                    }
                }
            };
            state.tabs.create_tab(kind);
            return Some(format!("New tab: {}", kind.label()));
        }

        None
    }
}

/// Background controller task (Actor)
///
/// Owns the AgentController and processes commands from the TUI event loop.
async fn controller_task(
    mut controller: AgentController,
    mut cmd_rx: mpsc::UnboundedReceiver<TuiCommand>,
    progress_tx: mpsc::UnboundedSender<RoutedTuiEvent>,
) {
    // Send welcome on startup
    controller.send_welcome();

    let mut tasks = tokio::task::JoinSet::new();

    loop {
        tokio::select! {
            biased;

            // Handle completed tasks (spawns + inline executions)
            Some(res) = tasks.join_next() => {
                match res {
                    Ok(completion) => controller.finalize(completion),
                    Err(e) => {
                        // Task panic or cancellation
                        if e.is_cancelled() {
                            // ignore
                        } else {
                            let _ = progress_tx.send(RoutedTuiEvent::global(TuiEvent::Flash(
                                format!("Task panicked: {}", e)
                            )));
                        }
                    }
                }
            }

            // Handle commands
            cmd_opt = cmd_rx.recv() => {
                let cmd = match cmd_opt {
                    Some(c) => c,
                    None => break, // Channel closed
                };

                match cmd {
                    TuiCommand::ProcessRequest { interaction_id, request } => {
                        let iid = interaction_id.unwrap_or_else(|| controller.active_interaction_id());
                        let (clean_query, full_query) = controller.prepare_inline(&request);
                        let context = controller.build_spawn_context();
                        let tx = progress_tx.clone();
                        tasks.spawn(async move {
                            let progress = TuiProgressBridge::for_interaction(tx, iid);
                            context.execute(None, InteractionForm::Agent, clean_query, full_query, &progress).await
                        });
                    }
                    TuiCommand::HandleCommand { interaction_id, command } => {
                        if command == "__welcome" {
                             continue;
                        }
                        if command.starts_with("__") {
                            continue;
                        }

                        let cmd_str = format!("/{}", command);

                        let iid = interaction_id.unwrap_or_else(|| controller.active_interaction_id());
                        let progress = TuiProgressBridge::for_interaction(progress_tx.clone(), iid);

                        match controller.handle_command(&cmd_str, &progress).await {
                            CommandAction::Exit => {
                                break;
                            }
                            CommandAction::Continue => {}
                            CommandAction::Execute { form, query } => {
                                let (clean_query, full_query) = controller.prepare_inline(&query);
                                let context = controller.build_spawn_context();
                                let tx = progress_tx.clone();
                                tasks.spawn(async move {
                                    let progress = TuiProgressBridge::for_interaction(tx, iid);
                                    context.execute(None, form, clean_query, full_query, &progress).await
                                });
                            }
                        }
                    }
                    TuiCommand::SetVerbose(verbose) => {
                        controller.set_verbose(verbose);
                    }
                    TuiCommand::SetCancellation(token) => {
                        controller.set_cancellation(token);
                    }
                    TuiCommand::SetReferenceResolver(resolver) => {
                        controller.set_reference_resolver(resolver);
                    }
                    TuiCommand::SpawnInteraction {
                        form,
                        query,
                        context_mode_override,
                    } => {
                        match controller.prepare_spawn(form, &query, context_mode_override) {
                            Ok((child_id, clean_query, full_query)) => {
                                let context = controller.build_spawn_context();
                                let tx = progress_tx.clone();

                                tasks.spawn(async move {
                                    let progress = TuiProgressBridge::for_interaction(tx, child_id);
                                    context.execute(Some(child_id), form, clean_query, full_query, &progress).await
                                });
                            }
                            Err(e) => {
                                let _ = progress_tx.send(RoutedTuiEvent::global(TuiEvent::Flash(
                                    format!("Failed to prepare spawn: {}", e)
                                )));
                            }
                        }
                    }
                    TuiCommand::ActivateInteraction(id) => {
                        controller.set_active_interaction(id);
                    }
                    TuiCommand::Quit => {
                        break;
                    }
                }
            }
        }
    }
}

/// Extract the meaningful LLM analysis text from task output.
///
/// Task output contains interleaved tool results and LLM text separated by `\n---\n`.
/// This function filters out tool result sections (lines starting with `[tool_name]:`)
/// and returns the last LLM text block, which is typically the final analysis/summary.
fn extract_response_text(output: &str) -> String {
    let sections: Vec<&str> = output.split("\n---\n").collect();

    // Find the last section that isn't a tool result
    sections
        .iter()
        .rev()
        .find(|section| {
            let trimmed = section.trim();
            !trimmed.is_empty()
                && !trimmed
                    .lines()
                    .next()
                    .is_some_and(|first| first.contains("]:") && first.starts_with('['))
        })
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_plain_text() {
        let output = "The code is well-structured and follows best practices.";
        assert_eq!(extract_response_text(output), output);
    }

    #[test]
    fn test_extract_filters_tool_results() {
        let output = "[read_file]: contents of foo.rs\n---\nThe code looks clean.";
        assert_eq!(extract_response_text(output), "The code looks clean.");
    }

    #[test]
    fn test_extract_returns_last_llm_block() {
        let output =
            "Initial analysis\n---\n[grep_search]: found 3 matches\n---\nFinal summary here.";
        assert_eq!(extract_response_text(output), "Final summary here.");
    }

    #[test]
    fn test_extract_empty_output() {
        assert_eq!(extract_response_text(""), String::new());
    }

    #[test]
    fn test_extract_only_tool_results() {
        let output = "[read_file]: file contents\n---\n[grep_search]: matches";
        assert_eq!(extract_response_text(output), String::new());
    }

    #[test]
    fn test_extract_preserves_long_text() {
        let long_text = "A".repeat(12000);
        let result = extract_response_text(&long_text);
        assert_eq!(result.len(), 12000);
    }

    #[test]
    fn test_extract_ignores_brackets_mid_line() {
        // Text that has brackets but not at start of line
        let output = "The function returns [Ok] or [Err]: both are valid.";
        assert_eq!(extract_response_text(output), output);
    }
}
