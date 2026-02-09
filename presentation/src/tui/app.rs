//! TUI application entry point
//!
//! Implements the main TUI loop with Actor pattern for state management:
//! - TuiApp: Main loop (terminal rendering, event handling)
//! - controller_task: Background task to update AppState

use super::event::Event;
use super::mode::Mode;
use super::state::AppState;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, EventStream},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::stream::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use std::io;
use tokio::sync::mpsc;

/// Main TUI application
pub struct TuiApp {
    /// Terminal backend
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    /// Event receiver from controller
    event_rx: mpsc::UnboundedReceiver<Event>,
    /// Command sender to controller
    cmd_tx: mpsc::UnboundedSender<Command>,
}

/// Commands sent from UI to controller
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Command {
    /// Submit user input
    Submit(String),
    /// Execute command
    ExecuteCommand(String),
    /// Change mode
    SetMode(Mode),
    /// Toggle consensus level
    ToggleConsensus,
    /// Show help
    ShowHelp,
    /// Hide help
    HideHelp,
    /// Confirm action
    Confirm,
    /// Cancel action
    Cancel,
    /// Quit application
    Quit,
}

impl TuiApp {
    /// Create a new TUI application
    pub fn new() -> io::Result<Self> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;

        // Create channels
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        // Spawn controller task
        let _controller_handle = tokio::spawn(controller_task(cmd_rx, event_tx.clone()));

        // Spawn terminal event reader
        tokio::spawn(terminal_event_task(event_tx.clone()));

        Ok(Self {
            terminal,
            event_rx,
            cmd_tx,
        })
    }

    /// Run the TUI application
    pub async fn run(&mut self) -> io::Result<()> {
        let mut state = AppState::new();

        loop {
            // Render
            self.terminal.draw(|frame| {
                // TODO: Implement proper rendering with widgets
                // For now, just render placeholder
                use ratatui::widgets::{Block, Borders, Paragraph};
                let block = Block::default()
                    .title("Copilot Quorum TUI")
                    .borders(Borders::ALL);
                let text = format!(
                    "Mode: {:?}\nInput: {}\nConsensus: {:?}",
                    state.mode, state.input, state.consensus_level
                );
                let paragraph = Paragraph::new(text).block(block);
                frame.render_widget(paragraph, frame.area());
            })?;

            // Handle events
            if let Some(event) = self.event_rx.recv().await {
                match event {
                    Event::Key(key) => {
                        if !self.handle_key_event(&mut state, key) {
                            break; // Quit requested
                        }
                    }
                    Event::Resize(_, _) => {
                        // Terminal will auto-resize on next draw
                    }
                    Event::UiEvent(ui_event) => {
                        // Handle application UI event
                        self.handle_ui_event(&mut state, ui_event);
                    }
                    Event::Error(err) => {
                        state.set_error(err);
                    }
                    Event::Tick => {
                        // Periodic update (no-op for now)
                    }
                    Event::Mouse(_) => {
                        // Ignore mouse events for now
                    }
                }
            }
        }

        Ok(())
    }

    /// Handle key event
    fn handle_key_event(&mut self, state: &mut AppState, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};

        // Check for quit shortcuts
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return false; // Quit
        }

        match state.mode {
            Mode::Normal => self.handle_normal_mode(state, key),
            Mode::Insert => self.handle_insert_mode(state, key),
            Mode::Command => self.handle_command_mode(state, key),
            Mode::Confirm => self.handle_confirm_mode(state, key),
        }
    }

    /// Handle Normal mode keys
    fn handle_normal_mode(&mut self, state: &mut AppState, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Char('i') => {
                state.set_mode(Mode::Insert);
            }
            KeyCode::Char(':') => {
                state.set_mode(Mode::Command);
            }
            KeyCode::Char('?') => {
                state.show_help();
            }
            KeyCode::Char('q') => {
                return false; // Quit
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.scroll_up();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                state.scroll_down();
            }
            KeyCode::Char('e') => {
                // Toggle ensemble mode
                state.toggle_consensus();
                let _ = self.cmd_tx.send(Command::ToggleConsensus);
            }
            _ => {}
        }

        true
    }

    /// Handle Insert mode keys
    fn handle_insert_mode(&mut self, state: &mut AppState, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Esc => {
                state.set_mode(Mode::Normal);
            }
            KeyCode::Enter => {
                let input = state.get_input().to_string();
                if !input.is_empty() {
                    let _ = self.cmd_tx.send(Command::Submit(input));
                    state.clear_input();
                }
                state.set_mode(Mode::Normal);
            }
            KeyCode::Backspace => {
                state.delete_char();
            }
            KeyCode::Left => {
                state.cursor_left();
            }
            KeyCode::Right => {
                state.cursor_right();
            }
            KeyCode::Home => {
                state.cursor_start();
            }
            KeyCode::End => {
                state.cursor_end();
            }
            KeyCode::Char(c) => {
                state.insert_char(c);
            }
            _ => {}
        }

        true
    }

    /// Handle Command mode keys
    fn handle_command_mode(&mut self, state: &mut AppState, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Esc => {
                state.set_mode(Mode::Normal);
            }
            KeyCode::Enter => {
                let command = state.get_input().to_string();
                if !command.is_empty() {
                    // Special handling for quit commands
                    if command == "q" || command == "quit" {
                        return false;
                    }
                    let _ = self.cmd_tx.send(Command::ExecuteCommand(command));
                    state.clear_input();
                }
                state.set_mode(Mode::Normal);
            }
            KeyCode::Backspace => {
                state.delete_char();
            }
            KeyCode::Left => {
                state.cursor_left();
            }
            KeyCode::Right => {
                state.cursor_right();
            }
            KeyCode::Home => {
                state.cursor_start();
            }
            KeyCode::End => {
                state.cursor_end();
            }
            KeyCode::Char(c) => {
                state.insert_char(c);
            }
            _ => {}
        }

        true
    }

    /// Handle Confirm mode keys
    fn handle_confirm_mode(&mut self, state: &mut AppState, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let _ = self.cmd_tx.send(Command::Confirm);
                state.clear_confirm_prompt();
                state.set_mode(Mode::Normal);
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                let _ = self.cmd_tx.send(Command::Cancel);
                state.clear_confirm_prompt();
                state.set_mode(Mode::Normal);
            }
            _ => {}
        }

        true
    }

    /// Handle application UI event
    fn handle_ui_event(
        &mut self,
        _state: &mut AppState,
        _ui_event: quorum_application::UiEvent,
    ) {
        // Convert UiEvent to TuiEvent and update state
        // This is handled by TuiPresenter/TuiProgressReporter in the background
        // Here we just acknowledge that events can flow through
    }
}

impl Drop for TuiApp {
    fn drop(&mut self) {
        // Restore terminal
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        let _ = self.terminal.show_cursor();
    }
}

impl Default for TuiApp {
    fn default() -> Self {
        Self::new().expect("Failed to initialize TUI")
    }
}

/// Controller task (Actor pattern)
///
/// Runs in background to:
/// 1. Receive commands from UI
/// 2. Update application state
/// 3. Send events back to UI
async fn controller_task(
    mut cmd_rx: mpsc::UnboundedReceiver<Command>,
    event_tx: mpsc::UnboundedSender<Event>,
) {
    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            Command::Submit(input) => {
                // Handle user input submission
                // TODO: Integrate with RunQuorumUseCase / RunAgentUseCase
                // For now, just echo back as event
                let _ = event_tx.send(Event::Error(format!("Submitted: {}", input)));
            }
            Command::ExecuteCommand(_command) => {
                // Handle command execution
                // TODO: Parse and execute command (e.g., /solo, /ens, /help)
            }
            Command::SetMode(_mode) => {
                // Mode changes are handled directly in UI
            }
            Command::ToggleConsensus => {
                // TODO: Toggle consensus level and notify
            }
            Command::ShowHelp => {
                // TODO: Show help
            }
            Command::HideHelp => {
                // TODO: Hide help
            }
            Command::Confirm => {
                // TODO: Handle confirmation
            }
            Command::Cancel => {
                // TODO: Handle cancellation
            }
            Command::Quit => {
                break;
            }
        }
    }
}

/// Terminal event reader task
///
/// Reads terminal events (key, mouse, resize) and forwards to main loop
async fn terminal_event_task(event_tx: mpsc::UnboundedSender<Event>) {
    let mut reader = EventStream::new();

    while let Some(Ok(terminal_event)) = reader.next().await {
        let event: Event = terminal_event.into();
        if event_tx.send(event).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_clone() {
        let cmd = Command::Submit("test".to_string());
        let _cloned = cmd.clone();
    }

    #[test]
    fn test_command_debug() {
        let cmd = Command::Quit;
        let debug_str = format!("{:?}", cmd);
        assert!(debug_str.contains("Quit"));
    }
}
