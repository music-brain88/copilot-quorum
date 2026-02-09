//! Vim-like modal input system
//!
//! Defines input modes and key-to-action dispatching.
//! #68 implements the core modes; #69 will add more Normal-mode bindings.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Application input mode (vim-like)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Navigation and single-key commands
    Normal,
    /// Text input — typing goes into the input buffer
    Insert,
    /// Command-line mode (`:` prefix)
    Command,
}

impl Default for InputMode {
    fn default() -> Self {
        Self::Insert // Start in Insert for discoverability
    }
}

impl InputMode {
    /// Status-bar indicator text
    pub fn indicator(&self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "-- INSERT --",
            Self::Command => ":",
        }
    }

    /// Status-bar indicator color
    pub fn color(&self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            Self::Normal => Color::Blue,
            Self::Insert => Color::Green,
            Self::Command => Color::Yellow,
        }
    }
}

/// Semantic action produced by key dispatching
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyAction {
    /// No-op
    None,

    // -- Mode transitions --
    EnterInsert,
    EnterCommand,
    ExitToNormal,

    // -- Text editing (Insert / Command mode) --
    InsertChar(char),
    DeleteChar,
    CursorLeft,
    CursorRight,
    CursorHome,
    CursorEnd,

    // -- Submit --
    SubmitInput,
    SubmitCommand,

    // -- Scrolling (Normal mode) --
    ScrollUp,
    ScrollDown,
    ScrollToTop,
    ScrollToBottom,

    // -- Application --
    Quit,
    ShowHelp,
    ToggleConsensus,
}

/// Map a key event + current mode to a semantic action
pub fn handle_key_event(mode: InputMode, key: KeyEvent) -> KeyAction {
    // Global: Ctrl+C always quits
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return KeyAction::Quit;
    }

    match mode {
        InputMode::Normal => handle_normal(key),
        InputMode::Insert => handle_insert(key),
        InputMode::Command => handle_command(key),
    }
}

fn handle_normal(key: KeyEvent) -> KeyAction {
    match key.code {
        // Mode transitions
        KeyCode::Char('i') | KeyCode::Char('a') => KeyAction::EnterInsert,
        KeyCode::Char(':') => KeyAction::EnterCommand,

        // Scrolling
        KeyCode::Char('j') | KeyCode::Down => KeyAction::ScrollDown,
        KeyCode::Char('k') | KeyCode::Up => KeyAction::ScrollUp,
        KeyCode::Char('g') => KeyAction::ScrollToTop,
        KeyCode::Char('G') => KeyAction::ScrollToBottom,

        // Help
        KeyCode::Char('?') => KeyAction::ShowHelp,

        _ => KeyAction::None,
    }
}

fn handle_insert(key: KeyEvent) -> KeyAction {
    match key.code {
        KeyCode::Esc => KeyAction::ExitToNormal,
        KeyCode::Enter => KeyAction::SubmitInput,
        KeyCode::Backspace => KeyAction::DeleteChar,
        KeyCode::Left => KeyAction::CursorLeft,
        KeyCode::Right => KeyAction::CursorRight,
        KeyCode::Home => KeyAction::CursorHome,
        KeyCode::End => KeyAction::CursorEnd,
        KeyCode::Char(c) => KeyAction::InsertChar(c),
        _ => KeyAction::None,
    }
}

fn handle_command(key: KeyEvent) -> KeyAction {
    match key.code {
        KeyCode::Esc => KeyAction::ExitToNormal,
        KeyCode::Enter => KeyAction::SubmitCommand,
        KeyCode::Backspace => KeyAction::DeleteChar,
        KeyCode::Left => KeyAction::CursorLeft,
        KeyCode::Right => KeyAction::CursorRight,
        KeyCode::Home => KeyAction::CursorHome,
        KeyCode::End => KeyAction::CursorEnd,
        KeyCode::Char(c) => KeyAction::InsertChar(c),
        _ => KeyAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_mode_is_insert() {
        assert_eq!(InputMode::default(), InputMode::Insert);
    }

    #[test]
    fn test_ctrl_c_always_quits() {
        for mode in [InputMode::Normal, InputMode::Insert, InputMode::Command] {
            let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
            assert_eq!(handle_key_event(mode, key), KeyAction::Quit);
        }
    }

    #[test]
    fn test_normal_mode_transitions() {
        let key_i = KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Normal, key_i),
            KeyAction::EnterInsert
        );

        let key_colon = KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Normal, key_colon),
            KeyAction::EnterCommand
        );
    }

    #[test]
    fn test_normal_scrolling() {
        let key_j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Normal, key_j),
            KeyAction::ScrollDown
        );

        let key_k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Normal, key_k),
            KeyAction::ScrollUp
        );
    }

    #[test]
    fn test_insert_mode_typing() {
        let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Insert, key),
            KeyAction::InsertChar('x')
        );
    }

    #[test]
    fn test_insert_esc_exits() {
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Insert, key),
            KeyAction::ExitToNormal
        );
    }

    #[test]
    fn test_insert_enter_submits() {
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Insert, key),
            KeyAction::SubmitInput
        );
    }

    #[test]
    fn test_command_enter_submits() {
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Command, key),
            KeyAction::SubmitCommand
        );
    }

    #[test]
    fn test_command_esc_exits() {
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Command, key),
            KeyAction::ExitToNormal
        );
    }
}
