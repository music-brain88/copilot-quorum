//! TUI application state
//!
//! Single source of truth for everything the TUI renders.
//! Updated by TuiPresenter (UiEvent → state) and TuiProgressBridge (progress → state).

use super::mode::InputMode;
use super::tab::TabManager;
use quorum_domain::{AgentPhase, ConsensusLevel, PhaseScope};

/// Central TUI state — owned by the TuiApp select! loop
pub struct TuiState {
    // -- Mode --
    pub mode: InputMode,

    // -- Command buffer (for : mode) — global, not per-tab --
    pub command_input: String,
    pub command_cursor: usize,

    // -- Tabs (own per-pane input, messages, streaming, scroll, progress) --
    pub tabs: TabManager,

    // -- Pending key (for g prefix in Normal mode) --
    pub pending_key: Option<char>,

    // -- Config display --
    pub consensus_level: ConsensusLevel,
    pub phase_scope: PhaseScope,
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
            command_input: String::new(),
            command_cursor: 0,
            tabs: TabManager::new(),
            pending_key: None,
            consensus_level: ConsensusLevel::Solo,
            phase_scope: PhaseScope::Full,
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
        let len = self.active_input().len();
        if cursor < len {
            let input = self.active_input();
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
            _ => &self.tabs.active_pane().input,
        };
        input.lines().count().max(1) + if input.ends_with('\n') { 1 } else { 0 }
    }

    /// Take the current input buffer contents and clear it
    pub fn take_input(&mut self) -> String {
        let pane = self.tabs.active_pane_mut();
        pane.cursor_pos = 0;
        std::mem::take(&mut pane.input)
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
            _ => &self.tabs.active_pane().input,
        }
    }

    fn active_input_mut(&mut self) -> &mut String {
        match self.mode {
            InputMode::Command => &mut self.command_input,
            _ => &mut self.tabs.active_pane_mut().input,
        }
    }

    fn active_cursor(&self) -> usize {
        match self.mode {
            InputMode::Command => self.command_cursor,
            _ => self.tabs.active_pane().cursor_pos,
        }
    }

    fn active_cursor_mut(&mut self) -> &mut usize {
        match self.mode {
            InputMode::Command => &mut self.command_cursor,
            _ => &mut self.tabs.active_pane_mut().cursor_pos,
        }
    }

    // -- Messages --

    pub fn push_message(&mut self, msg: DisplayMessage) {
        let pane = self.tabs.active_pane_mut();
        pane.messages.push(msg);
        if pane.auto_scroll {
            pane.scroll_offset = 0;
            // auto_scroll stays true
        }
    }

    /// Push a message to a specific interaction pane
    pub fn push_message_to(&mut self, id: quorum_domain::InteractionId, msg: DisplayMessage) {
        if let Some(pane) = self.tabs.pane_for_interaction_mut(id) {
            pane.messages.push(msg);
            if pane.auto_scroll {
                pane.scroll_offset = 0;
            }
        }
    }

    /// Finalize streaming text into a message
    pub fn finalize_stream(&mut self) {
        let pane = self.tabs.active_pane_mut();
        if !pane.streaming_text.is_empty() {
            let text = std::mem::take(&mut pane.streaming_text);
            let msg = DisplayMessage::assistant(text);
            pane.messages.push(msg);
            if pane.auto_scroll {
                pane.scroll_offset = 0;
            }
        }
    }

    /// Finalize streaming text for a specific interaction
    pub fn finalize_stream_for(&mut self, id: quorum_domain::InteractionId) {
        if let Some(pane) = self.tabs.pane_for_interaction_mut(id)
            && !pane.streaming_text.is_empty()
        {
            let text = std::mem::take(&mut pane.streaming_text);
            let msg = DisplayMessage::assistant(text);
            pane.messages.push(msg);
            if pane.auto_scroll {
                pane.scroll_offset = 0;
            }
        }
    }

    // -- Scrolling --

    pub fn scroll_up(&mut self) {
        let pane = self.tabs.active_pane_mut();
        pane.auto_scroll = false;
        pane.scroll_offset = pane.scroll_offset.saturating_add(1);
    }

    pub fn scroll_down(&mut self) {
        let pane = self.tabs.active_pane_mut();
        if pane.scroll_offset > 0 {
            pane.scroll_offset = pane.scroll_offset.saturating_sub(1);
        } else {
            pane.auto_scroll = true;
        }
    }

    pub fn scroll_to_top(&mut self) {
        let pane = self.tabs.active_pane_mut();
        pane.auto_scroll = false;
        pane.scroll_offset = usize::MAX; // Will be clamped during render
    }

    pub fn scroll_to_bottom(&mut self) {
        let pane = self.tabs.active_pane_mut();
        pane.scroll_offset = 0;
        pane.auto_scroll = true;
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
    pub quorum_status: Option<QuorumStatus>,
    pub task_progress: Option<TaskProgress>,
    pub ensemble_progress: Option<EnsembleProgress>,
    pub is_running: bool,
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
    /// Tool executions for the currently active task
    pub active_tool_executions: Vec<ToolExecutionDisplay>,
}

/// Summary of a completed task
#[derive(Debug, Clone)]
pub struct TaskSummary {
    pub index: usize,
    pub description: String,
    pub success: bool,
    pub output: Option<String>,
    /// Duration of the task in milliseconds
    pub duration_ms: Option<u64>,
    /// Tool executions performed during this task
    pub tool_executions: Vec<ToolExecutionDisplay>,
}

/// Display state for a single tool execution within a task
#[derive(Debug, Clone)]
pub struct ToolExecutionDisplay {
    pub execution_id: String,
    pub tool_name: String,
    pub state: ToolExecutionDisplayStatus,
    pub duration_ms: Option<u64>,
    pub args_preview: Option<String>,
}

/// Status of a tool execution for display
#[derive(Debug, Clone)]
pub enum ToolExecutionDisplayStatus {
    Pending,
    Running,
    Completed { preview: String },
    Error { message: String },
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
    use crate::tui::tab::PaneKind;
    use quorum_domain::interaction::{InteractionForm, InteractionId};

    #[test]
    fn test_input_editing() {
        let mut state = TuiState::new();
        state.mode = InputMode::Insert;

        state.insert_char('h');
        state.insert_char('i');
        assert_eq!(state.tabs.active_pane().input, "hi");
        assert_eq!(state.tabs.active_pane().cursor_pos, 2);

        state.delete_char();
        assert_eq!(state.tabs.active_pane().input, "h");
        assert_eq!(state.tabs.active_pane().cursor_pos, 1);
    }

    #[test]
    fn test_command_buffer_separate() {
        let mut state = TuiState::new();

        // Type in insert mode
        state.mode = InputMode::Insert;
        state.insert_char('a');
        assert_eq!(state.tabs.active_pane().input, "a");

        // Switch to command mode - separate buffer
        state.mode = InputMode::Command;
        state.insert_char('q');
        assert_eq!(state.command_input, "q");
        assert_eq!(state.tabs.active_pane().input, "a"); // Unchanged
    }

    #[test]
    fn test_take_input_clears() {
        let mut state = TuiState::new();
        state.tabs.active_pane_mut().input = "hello".into();
        state.tabs.active_pane_mut().cursor_pos = 5;

        let taken = state.take_input();
        assert_eq!(taken, "hello");
        assert!(state.tabs.active_pane().input.is_empty());
        assert_eq!(state.tabs.active_pane().cursor_pos, 0);
    }

    #[test]
    fn test_scroll_behavior() {
        let mut state = TuiState::new();
        assert!(state.tabs.active_pane().auto_scroll);

        state.scroll_up();
        assert!(!state.tabs.active_pane().auto_scroll);
        assert_eq!(state.tabs.active_pane().scroll_offset, 1);

        state.scroll_to_bottom();
        assert!(state.tabs.active_pane().auto_scroll);
        assert_eq!(state.tabs.active_pane().scroll_offset, 0);
    }

    #[test]
    fn test_finalize_stream() {
        let mut state = TuiState::new();
        state.tabs.active_pane_mut().streaming_text = "Hello world".into();

        state.finalize_stream();
        assert!(state.tabs.active_pane().streaming_text.is_empty());
        assert_eq!(state.tabs.active_pane().messages.len(), 1);
        assert_eq!(state.tabs.active_pane().messages[0].content, "Hello world");
        assert_eq!(
            state.tabs.active_pane().messages[0].role,
            MessageRole::Assistant
        );
    }

    #[test]
    fn test_push_message_to_interaction() {
        let mut state = TuiState::new();
        let id = InteractionId(9);
        state
            .tabs
            .create_tab(PaneKind::Interaction(InteractionForm::Ask, Some(id)));
        state.tabs.prev_tab(); // ensure active pane is not the target

        state.push_message_to(id, DisplayMessage::system("hello"));

        let index = state.tabs.find_tab_index_by_interaction(id).unwrap();
        assert_eq!(state.tabs.tabs()[index].pane.messages.len(), 1);
        assert_eq!(state.tabs.tabs()[index].pane.messages[0].content, "hello");
    }

    #[test]
    fn test_finalize_stream_for_interaction() {
        let mut state = TuiState::new();
        let id = InteractionId(10);
        state
            .tabs
            .create_tab(PaneKind::Interaction(InteractionForm::Discuss, Some(id)));

        if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
            pane.streaming_text = "stream text".into();
        }

        state.finalize_stream_for(id);

        let pane = state.tabs.pane_for_interaction_mut(id).unwrap();
        assert!(pane.streaming_text.is_empty());
        assert_eq!(pane.messages.len(), 1);
        assert_eq!(pane.messages[0].content, "stream text");
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
        assert_eq!(state.tabs.active_pane().input, "ab\nc");
        assert_eq!(state.tabs.active_pane().cursor_pos, 4);
    }

    #[test]
    fn test_insert_newline_mid_text() {
        let mut state = TuiState::new();
        state.mode = InputMode::Insert;
        state.tabs.active_pane_mut().input = "hello world".into();
        state.tabs.active_pane_mut().cursor_pos = 5;
        state.insert_newline();
        assert_eq!(state.tabs.active_pane().input, "hello\n world");
        assert_eq!(state.tabs.active_pane().cursor_pos, 6);
    }

    #[test]
    fn test_input_line_count() {
        let mut state = TuiState::new();
        state.mode = InputMode::Insert;

        // Empty input → 1 line
        assert_eq!(state.input_line_count(), 1);

        state.tabs.active_pane_mut().input = "hello".into();
        assert_eq!(state.input_line_count(), 1);

        state.tabs.active_pane_mut().input = "hello\nworld".into();
        assert_eq!(state.input_line_count(), 2);

        state.tabs.active_pane_mut().input = "a\nb\nc".into();
        assert_eq!(state.input_line_count(), 3);

        // Trailing newline = extra empty line
        state.tabs.active_pane_mut().input = "hello\n".into();
        assert_eq!(state.input_line_count(), 2);
    }

    #[test]
    fn test_delete_char_across_newline() {
        let mut state = TuiState::new();
        state.mode = InputMode::Insert;
        state.tabs.active_pane_mut().input = "ab\nc".into();
        state.tabs.active_pane_mut().cursor_pos = 3; // at '\n' + 1 = before 'c'
        // "ab\nc" → bytes: a(0) b(1) \n(2) c(3)
        // cursor_pos = 3 means cursor is before 'c'
        state.delete_char(); // should delete '\n'
        assert_eq!(state.tabs.active_pane().input, "abc");
        assert_eq!(state.tabs.active_pane().cursor_pos, 2);
    }

    #[test]
    fn test_cursor_movement() {
        let mut state = TuiState::new();
        state.mode = InputMode::Insert;
        state.tabs.active_pane_mut().input = "abc".into();
        state.tabs.active_pane_mut().cursor_pos = 3;

        state.cursor_left();
        assert_eq!(state.tabs.active_pane().cursor_pos, 2);

        state.cursor_home();
        assert_eq!(state.tabs.active_pane().cursor_pos, 0);

        state.cursor_end();
        assert_eq!(state.tabs.active_pane().cursor_pos, 3);

        state.cursor_right(); // Already at end
        assert_eq!(state.tabs.active_pane().cursor_pos, 3);
    }
}
