//! TUI application state
//!
//! Single source of truth for everything the TUI renders.
//! Updated by TuiPresenter (UiEvent → state) and TuiProgressBridge (progress → state).

use super::mode::InputMode;
use quorum_domain::{AgentPhase, ConsensusLevel, InteractionType, PhaseScope};

/// Central TUI state — owned by the TuiApp select! loop
pub struct TuiState {
    // -- Mode --
    pub mode: InputMode,

    // -- Input buffer --
    pub input: String,
    pub cursor_pos: usize,

    // -- Command buffer (for : mode) --
    pub command_input: String,
    pub command_cursor: usize,

    // -- Conversation --
    pub messages: Vec<DisplayMessage>,
    pub streaming_text: String,
    pub scroll_offset: usize,
    pub auto_scroll: bool,

    // -- Progress --
    pub progress: ProgressState,

    // -- Config display --
    pub consensus_level: ConsensusLevel,
    pub phase_scope: PhaseScope,
    pub interaction_type: InteractionType,
    pub model_name: String,

    // -- Overlay --
    pub show_help: bool,
    pub flash_message: Option<(String, std::time::Instant)>,

    // -- HiL --
    pub hil_prompt: Option<HilPrompt>,

    // -- TUI config --
    pub tui_config: TuiInputConfig,

    // -- Lifecycle --
    pub should_quit: bool,
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            mode: InputMode::default(),
            input: String::new(),
            cursor_pos: 0,
            command_input: String::new(),
            command_cursor: 0,
            messages: Vec::new(),
            streaming_text: String::new(),
            scroll_offset: 0,
            auto_scroll: true,
            progress: ProgressState::default(),
            consensus_level: ConsensusLevel::Solo,
            phase_scope: PhaseScope::Full,
            interaction_type: InteractionType::Ask,
            model_name: String::new(),
            show_help: false,
            flash_message: None,
            hil_prompt: None,
            tui_config: TuiInputConfig::default(),
            should_quit: false,
        }
    }
}

impl TuiState {
    pub fn new() -> Self {
        Self::default()
    }

    // -- Input editing --

    pub fn insert_char(&mut self, c: char) {
        let cursor = self.active_cursor();
        self.active_input_mut().insert(cursor, c);
        *self.active_cursor_mut() += c.len_utf8();
    }

    pub fn delete_char(&mut self) {
        let cursor = self.active_cursor();
        if cursor > 0 {
            let input = self.active_input_mut();
            let prev_char_len = input[..cursor]
                .chars()
                .next_back()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            input.remove(cursor - prev_char_len);
            *self.active_cursor_mut() -= prev_char_len;
        }
    }

    pub fn cursor_left(&mut self) {
        let cursor = self.active_cursor();
        if cursor > 0 {
            let input = self.active_input();
            let prev_char_len = input[..cursor]
                .chars()
                .next_back()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            *self.active_cursor_mut() -= prev_char_len;
        }
    }

    pub fn cursor_right(&mut self) {
        let cursor = self.active_cursor();
        let input = self.active_input();
        if cursor < input.len() {
            let next_char_len = input[cursor..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            *self.active_cursor_mut() += next_char_len;
        }
    }

    pub fn cursor_home(&mut self) {
        *self.active_cursor_mut() = 0;
    }

    pub fn cursor_end(&mut self) {
        let len = self.active_input().len();
        *self.active_cursor_mut() = len;
    }

    /// Insert a newline at the current cursor position
    pub fn insert_newline(&mut self) {
        let cursor = self.active_cursor();
        self.active_input_mut().insert(cursor, '\n');
        *self.active_cursor_mut() += 1;
    }

    /// Count the number of lines in the current input buffer
    pub fn input_line_count(&self) -> usize {
        let input = match self.mode {
            InputMode::Command => &self.command_input,
            _ => &self.input,
        };
        input.lines().count().max(1) + if input.ends_with('\n') { 1 } else { 0 }
    }

    /// Take the current input buffer contents and clear it
    pub fn take_input(&mut self) -> String {
        self.cursor_pos = 0;
        std::mem::take(&mut self.input)
    }

    /// Take the command buffer contents and clear it
    pub fn take_command(&mut self) -> String {
        self.command_cursor = 0;
        std::mem::take(&mut self.command_input)
    }

    // -- Active buffer helpers (routes to input or command based on mode) --

    fn active_input(&self) -> &str {
        match self.mode {
            InputMode::Command => &self.command_input,
            _ => &self.input,
        }
    }

    fn active_input_mut(&mut self) -> &mut String {
        match self.mode {
            InputMode::Command => &mut self.command_input,
            _ => &mut self.input,
        }
    }

    fn active_cursor(&self) -> usize {
        match self.mode {
            InputMode::Command => self.command_cursor,
            _ => self.cursor_pos,
        }
    }

    fn active_cursor_mut(&mut self) -> &mut usize {
        match self.mode {
            InputMode::Command => &mut self.command_cursor,
            _ => &mut self.cursor_pos,
        }
    }

    // -- Messages --

    pub fn push_message(&mut self, msg: DisplayMessage) {
        self.messages.push(msg);
        if self.auto_scroll {
            self.scroll_to_bottom();
        }
    }

    /// Finalize streaming text into a message
    pub fn finalize_stream(&mut self) {
        if !self.streaming_text.is_empty() {
            let text = std::mem::take(&mut self.streaming_text);
            self.push_message(DisplayMessage::assistant(text));
        }
    }

    // -- Scrolling --

    pub fn scroll_up(&mut self) {
        self.auto_scroll = false;
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    pub fn scroll_down(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset = self.scroll_offset.saturating_sub(1);
        } else {
            self.auto_scroll = true;
        }
    }

    pub fn scroll_to_top(&mut self) {
        self.auto_scroll = false;
        self.scroll_offset = usize::MAX; // Will be clamped during render
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }

    // -- Flash messages --

    pub fn set_flash(&mut self, msg: impl Into<String>) {
        self.flash_message = Some((msg.into(), std::time::Instant::now()));
    }

    /// Clear flash if older than the given duration
    pub fn expire_flash(&mut self, max_age: std::time::Duration) {
        if let Some((_, created)) = &self.flash_message
            && created.elapsed() > max_age
        {
            self.flash_message = None;
        }
    }
}

/// A single message in the conversation panel
#[derive(Debug, Clone)]
pub struct DisplayMessage {
    pub role: MessageRole,
    pub content: String,
}

impl DisplayMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

impl MessageRole {
    pub fn label(&self) -> &'static str {
        match self {
            Self::User => "You",
            Self::Assistant => "Agent",
            Self::System => "System",
        }
    }

    pub fn color(&self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            Self::User => Color::Cyan,
            Self::Assistant => Color::Green,
            Self::System => Color::Yellow,
        }
    }
}

/// Progress panel state
#[derive(Debug, Clone, Default)]
pub struct ProgressState {
    pub current_phase: Option<AgentPhase>,
    pub phase_name: String,
    pub current_tool: Option<String>,
    pub tool_log: Vec<ToolLogEntry>,
    pub quorum_status: Option<QuorumStatus>,
    pub task_progress: Option<TaskProgress>,
    pub ensemble_progress: Option<EnsembleProgress>,
    pub is_running: bool,
}

#[derive(Debug, Clone)]
pub struct ToolLogEntry {
    pub tool_name: String,
    pub success: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct QuorumStatus {
    pub phase: String,
    pub total: usize,
    pub completed: usize,
    pub approved: usize,
}

/// Task execution progress (shown during Executing phase)
#[derive(Debug, Clone)]
pub struct TaskProgress {
    pub current_index: usize,
    pub total: usize,
    pub description: String,
    pub completed_tasks: Vec<TaskSummary>,
}

/// Summary of a completed task
#[derive(Debug, Clone)]
pub struct TaskSummary {
    pub index: usize,
    pub description: String,
    pub success: bool,
}

/// Ensemble planning progress
#[derive(Debug, Clone)]
pub struct EnsembleProgress {
    pub total_models: usize,
    pub plans_generated: usize,
    pub models_completed: Vec<String>,
    pub models_failed: Vec<(String, String)>,
    pub voting_started: bool,
    pub plan_count: Option<usize>,
    pub selected: Option<(String, f64)>,
}

/// TUI input configuration (presentation-layer view)
///
/// This is the presentation-layer equivalent of `FileTuiInputConfig`.
/// Values are typically populated from infrastructure config at startup.
#[derive(Debug, Clone)]
pub struct TuiInputConfig {
    /// Maximum height for the input area in text lines (default: 8)
    pub max_input_height: u16,
    /// Whether to show context header in $EDITOR temp file
    pub context_header: bool,
}

impl Default for TuiInputConfig {
    fn default() -> Self {
        Self {
            max_input_height: 8,
            context_header: true,
        }
    }
}

/// Human intervention prompt data
#[derive(Debug, Clone)]
pub struct HilPrompt {
    pub title: String,
    pub objective: String,
    pub tasks: Vec<String>,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_editing() {
        let mut state = TuiState::new();
        state.mode = InputMode::Insert;

        state.insert_char('h');
        state.insert_char('i');
        assert_eq!(state.input, "hi");
        assert_eq!(state.cursor_pos, 2);

        state.delete_char();
        assert_eq!(state.input, "h");
        assert_eq!(state.cursor_pos, 1);
    }

    #[test]
    fn test_command_buffer_separate() {
        let mut state = TuiState::new();

        // Type in insert mode
        state.mode = InputMode::Insert;
        state.insert_char('a');
        assert_eq!(state.input, "a");

        // Switch to command mode - separate buffer
        state.mode = InputMode::Command;
        state.insert_char('q');
        assert_eq!(state.command_input, "q");
        assert_eq!(state.input, "a"); // Unchanged
    }

    #[test]
    fn test_take_input_clears() {
        let mut state = TuiState::new();
        state.input = "hello".into();
        state.cursor_pos = 5;

        let taken = state.take_input();
        assert_eq!(taken, "hello");
        assert!(state.input.is_empty());
        assert_eq!(state.cursor_pos, 0);
    }

    #[test]
    fn test_scroll_behavior() {
        let mut state = TuiState::new();
        assert!(state.auto_scroll);

        state.scroll_up();
        assert!(!state.auto_scroll);
        assert_eq!(state.scroll_offset, 1);

        state.scroll_to_bottom();
        assert!(state.auto_scroll);
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn test_finalize_stream() {
        let mut state = TuiState::new();
        state.streaming_text = "Hello world".into();

        state.finalize_stream();
        assert!(state.streaming_text.is_empty());
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].content, "Hello world");
        assert_eq!(state.messages[0].role, MessageRole::Assistant);
    }

    #[test]
    fn test_flash_message() {
        let mut state = TuiState::new();
        state.set_flash("test");
        assert!(state.flash_message.is_some());

        // Should not expire immediately
        state.expire_flash(std::time::Duration::from_secs(5));
        assert!(state.flash_message.is_some());
    }

    #[test]
    fn test_message_constructors() {
        let msg = DisplayMessage::user("hello");
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.content, "hello");

        let msg = DisplayMessage::assistant("hi");
        assert_eq!(msg.role, MessageRole::Assistant);

        let msg = DisplayMessage::system("info");
        assert_eq!(msg.role, MessageRole::System);
    }

    #[test]
    fn test_insert_newline() {
        let mut state = TuiState::new();
        state.mode = InputMode::Insert;
        state.insert_char('a');
        state.insert_char('b');
        state.insert_newline();
        state.insert_char('c');
        assert_eq!(state.input, "ab\nc");
        assert_eq!(state.cursor_pos, 4);
    }

    #[test]
    fn test_insert_newline_mid_text() {
        let mut state = TuiState::new();
        state.mode = InputMode::Insert;
        state.input = "hello world".into();
        state.cursor_pos = 5;
        state.insert_newline();
        assert_eq!(state.input, "hello\n world");
        assert_eq!(state.cursor_pos, 6);
    }

    #[test]
    fn test_input_line_count() {
        let mut state = TuiState::new();
        state.mode = InputMode::Insert;

        // Empty input → 1 line
        assert_eq!(state.input_line_count(), 1);

        state.input = "hello".into();
        assert_eq!(state.input_line_count(), 1);

        state.input = "hello\nworld".into();
        assert_eq!(state.input_line_count(), 2);

        state.input = "a\nb\nc".into();
        assert_eq!(state.input_line_count(), 3);

        // Trailing newline = extra empty line
        state.input = "hello\n".into();
        assert_eq!(state.input_line_count(), 2);
    }

    #[test]
    fn test_delete_char_across_newline() {
        let mut state = TuiState::new();
        state.mode = InputMode::Insert;
        state.input = "ab\nc".into();
        state.cursor_pos = 3; // at '\n' + 1 = 'c' position... wait, let's be precise
        // "ab\nc" → bytes: a(0) b(1) \n(2) c(3)
        // cursor_pos = 3 means cursor is before 'c'
        state.delete_char(); // should delete '\n'
        assert_eq!(state.input, "abc");
        assert_eq!(state.cursor_pos, 2);
    }

    #[test]
    fn test_cursor_movement() {
        let mut state = TuiState::new();
        state.mode = InputMode::Insert;
        state.input = "abc".into();
        state.cursor_pos = 3;

        state.cursor_left();
        assert_eq!(state.cursor_pos, 2);

        state.cursor_home();
        assert_eq!(state.cursor_pos, 0);

        state.cursor_end();
        assert_eq!(state.cursor_pos, 3);

        state.cursor_right(); // Already at end
        assert_eq!(state.cursor_pos, 3);
    }
}
