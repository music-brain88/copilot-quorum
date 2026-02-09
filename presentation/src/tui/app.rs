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

use super::event::{HilKind, HilRequest, TuiCommand, TuiEvent};
use super::mode::{self, InputMode, KeyAction};
use super::presenter::TuiPresenter;
use super::progress::TuiProgressBridge;
use super::state::{DisplayMessage, HilPrompt, QuorumStatus, ToolLogEntry, TuiState};
use super::widgets::{
    MainLayout, conversation::ConversationWidget, header::HeaderWidget, input::InputWidget,
    progress_panel::ProgressPanelWidget, status_bar::StatusBarWidget,
};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, EventStream},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures::stream::StreamExt;
use quorum_application::{
    AgentController, CommandAction, ContextLoaderPort, LlmGateway, ToolExecutorPort, UiEvent,
};
use quorum_domain::{AgentConfig, ConsensusLevel, HumanDecision, Model};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use super::human_intervention::TuiHumanIntervention;

/// Main TUI application
pub struct TuiApp<
    G: LlmGateway + 'static,
    T: ToolExecutorPort + 'static,
    C: ContextLoaderPort + 'static,
> {
    // -- Actor channels --
    cmd_tx: mpsc::UnboundedSender<TuiCommand>,
    ui_rx: mpsc::UnboundedReceiver<UiEvent>,
    tui_event_rx: mpsc::UnboundedReceiver<TuiEvent>,
    hil_rx: mpsc::UnboundedReceiver<HilRequest>,

    // -- Presenter (applies UiEvents to state) --
    presenter: TuiPresenter,

    // -- Pending HiL response sender --
    pending_hil_tx: Arc<Mutex<Option<oneshot::Sender<HumanDecision>>>>,

    // -- Controller task handle --
    _controller_handle: tokio::task::JoinHandle<()>,

    // -- Type witness for generics --
    _phantom: std::marker::PhantomData<(G, T, C)>,
}

impl<G: LlmGateway + 'static, T: ToolExecutorPort + 'static, C: ContextLoaderPort + 'static>
    TuiApp<G, T, C>
{
    /// Create a new TUI application wired to the controller
    pub fn new(
        gateway: Arc<G>,
        tool_executor: Arc<T>,
        context_loader: Arc<C>,
        config: AgentConfig,
    ) -> Self {
        // Channels
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<TuiCommand>();
        let (ui_tx, ui_rx) = mpsc::unbounded_channel::<UiEvent>();
        let (tui_event_tx, tui_event_rx) = mpsc::unbounded_channel::<TuiEvent>();
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
            context_loader,
            config,
            human_intervention,
            ui_tx,
        );

        let controller_handle = tokio::spawn(controller_task(controller, cmd_rx, progress_tx));

        Self {
            cmd_tx,
            ui_rx,
            tui_event_rx,
            hil_rx,
            presenter,
            pending_hil_tx: Arc::new(Mutex::new(None)),
            _controller_handle: controller_handle,
            _phantom: std::marker::PhantomData,
        }
    }

    // -- Builder methods (delegate to controller via commands) --

    pub fn with_verbose(self, _verbose: bool) -> Self {
        // TODO: send SetVerbose command to controller
        self
    }

    pub fn with_cancellation(self, _token: CancellationToken) -> Self {
        // TODO: send cancellation to controller
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

    /// Run the TUI main loop
    pub async fn run(&mut self) -> io::Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Install panic hook to restore terminal
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
            original_hook(info);
        }));

        let mut state = TuiState::new();
        let mut event_stream = EventStream::new();
        let mut tick = tokio::time::interval(Duration::from_millis(250));

        // Send welcome
        let _ = self
            .cmd_tx
            .send(TuiCommand::HandleCommand("__welcome".into()));

        loop {
            // Render
            terminal.draw(|frame| {
                self.render(frame, &state);
            })?;

            if state.should_quit {
                break;
            }

            // select! on all event sources
            tokio::select! {
                // Terminal events (keyboard, mouse, resize)
                Some(Ok(term_event)) = event_stream.next() => {
                    self.handle_terminal_event(&mut state, term_event);
                }

                // UiEvents from controller (via AgentController → ui_tx)
                Some(ui_event) = self.ui_rx.recv() => {
                    self.presenter.apply(&mut state, &ui_event);
                }

                // TuiEvents from progress bridge / presenter
                Some(tui_event) = self.tui_event_rx.recv() => {
                    self.apply_tui_event(&mut state, tui_event);
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
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        Ok(())
    }

    /// Render all widgets
    fn render(&self, frame: &mut ratatui::Frame, state: &TuiState) {
        let layout = MainLayout::compute(frame.area());

        frame.render_widget(HeaderWidget::new(state), layout.header);
        frame.render_widget(ConversationWidget::new(state), layout.conversation);
        frame.render_widget(ProgressPanelWidget::new(state), layout.progress);
        frame.render_widget(InputWidget::new(state), layout.input);
        frame.render_widget(StatusBarWidget::new(state), layout.status_bar);

        // Help overlay
        if state.show_help {
            let help_area = MainLayout::centered_overlay(70, 70, frame.area());
            frame.render_widget(ratatui::widgets::Clear, help_area);
            self.render_help(frame, help_area);
        }

        // HiL modal
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
            Line::from("  i/a    Enter Insert mode"),
            Line::from("  :      Enter Command mode"),
            Line::from("  s      Switch to Solo mode"),
            Line::from("  e      Switch to Ensemble mode"),
            Line::from("  f      Toggle Fast scope"),
            Line::from("  d      Start Quorum Discussion"),
            Line::from("  j/k    Scroll down/up"),
            Line::from("  g/G    Scroll to top/bottom"),
            Line::from("  ?      Toggle this help"),
            Line::from("  Ctrl+C Quit"),
            Line::from(""),
            Line::from("Insert Mode:"),
            Line::from("  Enter  Send message"),
            Line::from("  Esc    Return to Normal"),
            Line::from(""),
            Line::from("Commands (:command):"),
            Line::from("  :q     Quit"),
            Line::from("  :help  Show help"),
            Line::from("  :solo  Switch to Solo mode"),
            Line::from("  :ens   Switch to Ensemble mode"),
            Line::from("  :fast  Toggle fast mode"),
            Line::from("  :config Show configuration"),
            Line::from("  :clear Clear history"),
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

    /// Handle a terminal (crossterm) event
    fn handle_terminal_event(&self, state: &mut TuiState, event: crossterm::event::Event) {
        match event {
            crossterm::event::Event::Key(key) => {
                // If HiL modal is showing, handle y/n/Esc
                if state.hil_prompt.is_some() {
                    self.handle_hil_key(state, key);
                    return;
                }

                // If help is showing, Esc or ? closes it
                if state.show_help {
                    match key.code {
                        crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('?') => {
                            state.show_help = false;
                            return;
                        }
                        _ => {}
                    }
                }

                let action = mode::handle_key_event(state.mode, key);
                self.handle_action(state, action);
            }
            crossterm::event::Event::Resize(_, _) => {
                // Terminal auto-resizes on next draw
            }
            _ => {}
        }
    }

    /// Handle a semantic key action
    fn handle_action(&self, state: &mut TuiState, action: KeyAction) {
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
            KeyAction::DeleteChar => state.delete_char(),
            KeyAction::CursorLeft => state.cursor_left(),
            KeyAction::CursorRight => state.cursor_right(),
            KeyAction::CursorHome => state.cursor_home(),
            KeyAction::CursorEnd => state.cursor_end(),

            // Submit
            KeyAction::SubmitInput => {
                let input = state.take_input();
                if !input.is_empty() {
                    state.push_message(DisplayMessage::user(&input));
                    let _ = self.cmd_tx.send(TuiCommand::ProcessRequest(input));
                }
            }
            KeyAction::SubmitCommand => {
                let cmd = state.take_command();
                state.mode = InputMode::Normal;
                if !cmd.is_empty() {
                    if cmd == "q" || cmd == "quit" || cmd == "exit" {
                        state.should_quit = true;
                    } else {
                        let _ = self.cmd_tx.send(TuiCommand::HandleCommand(cmd));
                    }
                }
            }

            // Quick commands
            KeyAction::SwitchSolo => {
                let _ = self.cmd_tx.send(TuiCommand::HandleCommand("solo".into()));
            }
            KeyAction::SwitchEnsemble => {
                let _ = self.cmd_tx.send(TuiCommand::HandleCommand("ens".into()));
            }
            KeyAction::ToggleFast => {
                let _ = self.cmd_tx.send(TuiCommand::HandleCommand("fast".into()));
            }
            KeyAction::StartDiscuss => {
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

            // Application
            KeyAction::Quit => state.should_quit = true,
            KeyAction::ShowHelp => state.show_help = !state.show_help,
            KeyAction::ToggleConsensus => {
                // Handled by command
                let _ = self
                    .cmd_tx
                    .send(TuiCommand::HandleCommand("toggle_consensus".into()));
            }
        }
    }

    /// Apply a TuiEvent (from progress bridge or presenter) to state
    fn apply_tui_event(&self, state: &mut TuiState, event: TuiEvent) {
        match event {
            TuiEvent::StreamChunk(chunk) => {
                state.streaming_text.push_str(&chunk);
                if state.auto_scroll {
                    state.scroll_to_bottom();
                }
            }
            TuiEvent::StreamEnd => {
                state.finalize_stream();
            }
            TuiEvent::PhaseChange { phase, name } => {
                state.progress.current_phase = Some(phase);
                state.progress.phase_name = name;
                state.progress.current_tool = None;
            }
            TuiEvent::TaskStart(desc) => {
                state.set_flash(format!("Task: {}", desc));
            }
            TuiEvent::TaskComplete {
                description,
                success,
            } => {
                let status = if success { "✓" } else { "✗" };
                state.set_flash(format!("{} {}", status, description));
            }
            TuiEvent::ToolCall { tool_name, args: _ } => {
                state.progress.current_tool = Some(tool_name.clone());
                state.progress.tool_log.push(ToolLogEntry {
                    tool_name,
                    success: None,
                });
            }
            TuiEvent::ToolResult { tool_name, success } => {
                state.progress.current_tool = None;
                // Update the last matching tool log entry
                if let Some(entry) = state
                    .progress
                    .tool_log
                    .iter_mut()
                    .rev()
                    .find(|e| e.tool_name == tool_name && e.success.is_none())
                {
                    entry.success = Some(success);
                }
            }
            TuiEvent::ToolError { tool_name, message } => {
                state.progress.current_tool = None;
                if let Some(entry) = state
                    .progress
                    .tool_log
                    .iter_mut()
                    .rev()
                    .find(|e| e.tool_name == tool_name && e.success.is_none())
                {
                    entry.success = Some(false);
                }
                state.set_flash(format!("Tool error: {} - {}", tool_name, message));
            }
            TuiEvent::QuorumStart { phase, model_count } => {
                state.progress.quorum_status = Some(QuorumStatus {
                    phase,
                    total: model_count,
                    completed: 0,
                    approved: 0,
                });
            }
            TuiEvent::QuorumModelVote { model: _, approved } => {
                if let Some(ref mut qs) = state.progress.quorum_status {
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
                state.progress.quorum_status = None;
            }
            TuiEvent::PlanRevision { revision, feedback } => {
                state.messages.push(DisplayMessage::system(format!(
                    "Plan revision #{}: {}",
                    revision, feedback
                )));
            }
            TuiEvent::EnsembleStart(count) => {
                state.set_flash(format!("Ensemble: {} models planning...", count));
            }
            TuiEvent::EnsemblePlanGenerated(model) => {
                state.set_flash(format!("Plan generated: {}", model));
            }
            TuiEvent::EnsembleComplete {
                selected_model,
                score,
            } => {
                state.set_flash(format!(
                    "Selected: {} (score: {:.1})",
                    selected_model, score
                ));
            }
            TuiEvent::AgentStarting => {
                state.progress.is_running = true;
                state.progress.tool_log.clear();
                state.progress.quorum_status = None;
            }
            TuiEvent::AgentResult {
                success,
                summary: _,
            } => {
                state.progress.is_running = false;
                state.progress.current_phase = None;
                state.progress.current_tool = None;
                if success {
                    state.set_flash("Agent completed successfully");
                } else {
                    state.set_flash("Agent completed with issues");
                }
            }
            TuiEvent::AgentError(msg) => {
                state.progress.is_running = false;
                state.set_flash(msg);
            }
            TuiEvent::Flash(msg) => {
                state.set_flash(msg);
            }
            TuiEvent::HistoryCleared => {
                // Already handled by presenter
            }
            TuiEvent::Exit => {
                state.should_quit = true;
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
}

/// Background controller task (Actor)
///
/// Owns the AgentController and processes commands from the TUI event loop.
async fn controller_task<
    G: LlmGateway + 'static,
    T: ToolExecutorPort + 'static,
    C: ContextLoaderPort + 'static,
>(
    mut controller: AgentController<G, T, C>,
    mut cmd_rx: mpsc::UnboundedReceiver<TuiCommand>,
    progress_tx: mpsc::UnboundedSender<TuiEvent>,
) {
    // Send welcome on startup
    controller.send_welcome();

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            TuiCommand::ProcessRequest(request) => {
                let progress = TuiProgressBridge::new(progress_tx.clone());
                controller.process_request(&request, &progress).await;
            }
            TuiCommand::HandleCommand(command) => {
                if command == "__welcome" {
                    // Already sent welcome above, skip
                    continue;
                }
                if command.starts_with("__") {
                    // Internal commands, skip
                    continue;
                }
                // Prefix with / for the controller's command parser
                let cmd_str = format!("/{}", command);
                match controller.handle_command(&cmd_str).await {
                    CommandAction::Exit => {
                        break;
                    }
                    CommandAction::Continue => {}
                }
            }
            TuiCommand::Quit => {
                break;
            }
        }
    }
}
