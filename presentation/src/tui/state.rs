//! TUI application state
//!
//! Manages the global state of the TUI application:
//! - Current mode
//! - Input buffer
//! - Message history
//! - Scroll position
//! - Consensus level
//! - Event queue for state updates

use super::event::TuiEvent;
use super::mode::Mode;
use quorum_domain::ConsensusLevel;
use std::collections::VecDeque;

/// Application state
#[derive(Debug, Clone)]
pub struct AppState {
    /// Current mode
    pub mode: Mode,
    /// Input buffer (for Insert/Command mode)
    pub input: String,
    /// Cursor position in input buffer
    pub cursor_pos: usize,
    /// Message history
    pub messages: Vec<Message>,
    /// Scroll offset in message history
    pub scroll_offset: usize,
    /// Current consensus level
    pub consensus_level: ConsensusLevel,
    /// Whether to show help overlay
    pub show_help: bool,
    /// Pending confirmation prompt
    pub confirm_prompt: Option<String>,
    /// Error message to display
    pub error_message: Option<String>,
    /// Agent status
    pub agent_status: AgentStatus,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            input: String::new(),
            cursor_pos: 0,
            messages: Vec::new(),
            scroll_offset: 0,
            consensus_level: ConsensusLevel::Solo,
            show_help: false,
            confirm_prompt: None,
            error_message: None,
            agent_status: AgentStatus::Idle,
        }
    }
}

impl AppState {
    /// Create a new application state
    pub fn new() -> Self {
        Self::default()
    }

    /// Set mode
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        if mode == Mode::Normal {
            self.input.clear();
            self.cursor_pos = 0;
        }
    }

    /// Insert character at cursor position
    pub fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    /// Delete character before cursor
    pub fn delete_char(&mut self) {
        if self.cursor_pos > 0 {
            let char_before = self.input[..self.cursor_pos]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.input.remove(self.cursor_pos - char_before);
            self.cursor_pos -= char_before;
        }
    }

    /// Move cursor left
    pub fn cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            let char_before = self.input[..self.cursor_pos]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos -= char_before;
        }
    }

    /// Move cursor right
    pub fn cursor_right(&mut self) {
        if self.cursor_pos < self.input.len() {
            let char_at = self.input[self.cursor_pos..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos += char_at;
        }
    }

    /// Move cursor to start
    pub fn cursor_start(&mut self) {
        self.cursor_pos = 0;
    }

    /// Move cursor to end
    pub fn cursor_end(&mut self) {
        self.cursor_pos = self.input.len();
    }

    /// Add message to history
    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    /// Clear input buffer
    pub fn clear_input(&mut self) {
        self.input.clear();
        self.cursor_pos = 0;
    }

    /// Get current input
    pub fn get_input(&self) -> &str {
        &self.input
    }

    /// Scroll up
    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    /// Scroll down
    pub fn scroll_down(&mut self) {
        self.scroll_offset += 1;
    }

    /// Toggle consensus level
    pub fn toggle_consensus(&mut self) {
        self.consensus_level = match self.consensus_level {
            ConsensusLevel::Solo => ConsensusLevel::Ensemble,
            ConsensusLevel::Ensemble => ConsensusLevel::Solo,
        };
    }

    /// Set consensus level
    pub fn set_consensus_level(&mut self, level: ConsensusLevel) {
        self.consensus_level = level;
    }

    /// Show help
    pub fn show_help(&mut self) {
        self.show_help = true;
    }

    /// Hide help
    pub fn hide_help(&mut self) {
        self.show_help = false;
    }

    /// Set confirmation prompt
    pub fn set_confirm_prompt(&mut self, prompt: String) {
        self.confirm_prompt = Some(prompt);
        self.set_mode(Mode::Confirm);
    }

    /// Clear confirmation prompt
    pub fn clear_confirm_prompt(&mut self) {
        self.confirm_prompt = None;
    }

    /// Set error message
    pub fn set_error(&mut self, error: String) {
        self.error_message = Some(error);
    }

    /// Clear error message
    pub fn clear_error(&mut self) {
        self.error_message = None;
    }

    /// Set agent status
    pub fn set_agent_status(&mut self, status: AgentStatus) {
        self.agent_status = status;
    }
}

/// Message in the history
#[derive(Debug, Clone)]
pub struct Message {
    /// Message role (User, Assistant, System)
    pub role: MessageRole,
    /// Message content
    pub content: String,
    /// Timestamp
    pub timestamp: std::time::SystemTime,
}

impl Message {
    /// Create a user message
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
            timestamp: std::time::SystemTime::now(),
        }
    }

    /// Create an assistant message
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            timestamp: std::time::SystemTime::now(),
        }
    }

    /// Create a system message
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
            timestamp: std::time::SystemTime::now(),
        }
    }
}

/// Message role
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

impl MessageRole {
    /// Get the display name
    pub fn display(&self) -> &'static str {
        match self {
            Self::User => "You",
            Self::Assistant => "Agent",
            Self::System => "System",
        }
    }

    /// Get the color for this role
    pub fn color(&self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            Self::User => Color::Cyan,
            Self::Assistant => Color::Green,
            Self::System => Color::Yellow,
        }
    }
}

/// Agent execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    /// Agent is idle
    Idle,
    /// Agent is thinking/planning
    Thinking,
    /// Agent is executing tools
    Executing,
    /// Agent is waiting for review
    WaitingForReview,
    /// Agent completed successfully
    Completed,
    /// Agent failed
    Failed,
}

impl AgentStatus {
    /// Get the display name
    pub fn display(&self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Thinking => "Thinking...",
            Self::Executing => "Executing...",
            Self::WaitingForReview => "Waiting for Review",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
        }
    }

    /// Get the color for this status
    pub fn color(&self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            Self::Idle => Color::Gray,
            Self::Thinking => Color::Blue,
            Self::Executing => Color::Yellow,
            Self::WaitingForReview => Color::Magenta,
            Self::Completed => Color::Green,
            Self::Failed => Color::Red,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_input_operations() {
        let mut state = AppState::new();
        
        state.insert_char('h');
        state.insert_char('e');
        state.insert_char('l');
        state.insert_char('l');
        state.insert_char('o');
        
        assert_eq!(state.get_input(), "hello");
        assert_eq!(state.cursor_pos, 5);
        
        state.delete_char();
        assert_eq!(state.get_input(), "hell");
        assert_eq!(state.cursor_pos, 4);
    }

    #[test]
    fn test_state_cursor_movement() {
        let mut state = AppState::new();
        state.input = "hello".to_string();
        state.cursor_pos = 5;
        
        state.cursor_left();
        assert_eq!(state.cursor_pos, 4);
        
        state.cursor_start();
        assert_eq!(state.cursor_pos, 0);
        
        state.cursor_end();
        assert_eq!(state.cursor_pos, 5);
        
        state.cursor_right();
        assert_eq!(state.cursor_pos, 5); // Can't go beyond end
    }

    #[test]
    fn test_state_mode_switching() {
        let mut state = AppState::new();
        state.input = "test".to_string();
        
        assert_eq!(state.mode, Mode::Normal);
        
        state.set_mode(Mode::Insert);
        assert_eq!(state.mode, Mode::Insert);
        
        state.set_mode(Mode::Normal);
        assert_eq!(state.mode, Mode::Normal);
        assert!(state.input.is_empty()); // Input cleared on Normal mode
    }

    #[test]
    fn test_consensus_toggle() {
        let mut state = AppState::new();
        
        assert_eq!(state.consensus_level, ConsensusLevel::Solo);
        
        state.toggle_consensus();
        assert_eq!(state.consensus_level, ConsensusLevel::Ensemble);
        
        state.toggle_consensus();
        assert_eq!(state.consensus_level, ConsensusLevel::Solo);
    }

    #[test]
    fn test_message_creation() {
        let msg = Message::user("test");
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.content, "test");
        
        let msg = Message::assistant("response");
        assert_eq!(msg.role, MessageRole::Assistant);
        assert_eq!(msg.content, "response");
    }
}

/// TUI-specific state (for presenter, progress, HIL)
///
/// This is a separate state object used by TUI adapters to:
/// 1. Queue events for rendering
/// 2. Track current mode (Normal/HumanIntervention)
/// 3. Maintain rendering state separate from AppState
#[derive(Debug, Clone)]
pub struct TuiState {
    /// Event queue (for future rendering)
    event_queue: VecDeque<TuiEvent>,
    /// Current TUI mode
    mode: TuiMode,
    /// Message history (for widgets)
    pub messages: Vec<MessageEntry>,
    /// Input buffer
    pub input: String,
    /// Show help overlay
    pub show_help: bool,
    /// Progress state
    pub progress: ProgressState,
}

/// Message entry for widget rendering
#[derive(Debug, Clone)]
pub struct MessageEntry {
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

/// Progress state for widget rendering
#[derive(Debug, Clone, Default)]
pub struct ProgressState {
    pub current_phase: Option<String>,
    pub current_status: String,
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            event_queue: VecDeque::new(),
            mode: TuiMode::Normal,
            messages: Vec::new(),
            input: String::new(),
            show_help: false,
            progress: ProgressState::default(),
        }
    }
}

impl TuiState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Emit a TUI event (add to queue)
    pub fn emit(&mut self, event: TuiEvent) {
        self.event_queue.push_back(event);
    }

    /// Poll next event from queue
    pub fn poll_event(&mut self) -> Option<TuiEvent> {
        self.event_queue.pop_front()
    }

    /// Set TUI mode
    pub fn set_mode(&mut self, mode: TuiMode) {
        self.mode = mode;
    }

    /// Get current TUI mode
    pub fn mode(&self) -> TuiMode {
        self.mode
    }
}

/// TUI-specific mode (for presenter/progress/HIL)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiMode {
    /// Normal operation
    Normal,
    /// Human intervention required
    HumanIntervention,
}
