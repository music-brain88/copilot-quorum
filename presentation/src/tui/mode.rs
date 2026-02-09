//! TUI mode system (vim-like mode switching)
//!
//! Defines the mode-based interaction model:
//! - Normal mode: Navigation and commands
//! - Insert mode: Text input
//! - Command mode: Execute commands

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Application mode (vim-like)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Normal mode - navigation and commands
    Normal,
    /// Insert mode - text input
    Insert,
    /// Command mode - execute commands (like `:` in vim)
    Command,
    /// Confirm mode - yes/no prompts
    Confirm,
}

/// REPL mode (for compatibility with widgets)
pub type ReplMode = Mode;

impl Default for Mode {
    fn default() -> Self {
        Self::Normal
    }
}

impl Mode {
    /// Get the mode indicator string for status line
    pub fn indicator(&self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Command => "COMMAND",
            Self::Confirm => "CONFIRM",
        }
    }

    /// Get the mode color for status line
    pub fn color(&self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            Self::Normal => Color::Blue,
            Self::Insert => Color::Green,
            Self::Command => Color::Yellow,
            Self::Confirm => Color::Magenta,
        }
    }
}

/// User action derived from key events
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Enter insert mode
    EnterInsert,
    /// Enter command mode
    EnterCommand,
    /// Exit current mode to normal
    ExitToNormal,
    /// Submit current input (Enter in Insert/Command mode)
    Submit,
    /// Cancel current operation (Esc)
    Cancel,
    /// Quit application
    Quit,
    /// Insert character (in Insert/Command mode)
    InsertChar(char),
    /// Delete character (Backspace)
    DeleteChar,
    /// Move cursor left
    CursorLeft,
    /// Move cursor right
    CursorRight,
    /// Move to start of line
    CursorStart,
    /// Move to end of line
    CursorEnd,
    /// Scroll up
    ScrollUp,
    /// Scroll down
    ScrollDown,
    /// Toggle consensus level (Solo/Ensemble)
    ToggleConsensus,
    /// Switch to Solo mode
    SwitchToSolo,
    /// Switch to Ensemble mode
    SwitchToEnsemble,
    /// Trigger Quorum Discussion
    TriggerDiscussion,
    /// Show help
    ShowHelp,
    /// Confirm (Yes)
    ConfirmYes,
    /// Reject (No)
    ConfirmNo,
    /// No action
    None,
}

/// Key event handler - maps key events to actions based on current mode
pub struct KeyHandler;

impl KeyHandler {
    /// Handle key event in the given mode
    pub fn handle(mode: Mode, key: KeyEvent) -> Action {
        match mode {
            Mode::Normal => Self::handle_normal(key),
            Mode::Insert => Self::handle_insert(key),
            Mode::Command => Self::handle_command(key),
            Mode::Confirm => Self::handle_confirm(key),
        }
    }

    fn handle_normal(key: KeyEvent) -> Action {
        match (key.code, key.modifiers) {
            // Mode switches
            (KeyCode::Char('i'), KeyModifiers::NONE) => Action::EnterInsert,
            (KeyCode::Char(':'), KeyModifiers::NONE) => Action::EnterCommand,
            
            // Quit
            (KeyCode::Char('q'), KeyModifiers::NONE) => Action::Quit,
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => Action::Quit,
            
            // Navigation
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, _) => Action::ScrollUp,
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, _) => Action::ScrollDown,
            
            // Consensus level shortcuts
            (KeyCode::Char('s'), KeyModifiers::NONE) => Action::SwitchToSolo,
            (KeyCode::Char('e'), KeyModifiers::NONE) => Action::SwitchToEnsemble,
            (KeyCode::Char('t'), KeyModifiers::NONE) => Action::ToggleConsensus,
            (KeyCode::Char('d'), KeyModifiers::NONE) => Action::TriggerDiscussion,
            
            // Help
            (KeyCode::Char('?'), KeyModifiers::NONE) => Action::ShowHelp,
            
            _ => Action::None,
        }
    }

    fn handle_insert(key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => Action::ExitToNormal,
            KeyCode::Enter => Action::Submit,
            KeyCode::Char(c) => Action::InsertChar(c),
            KeyCode::Backspace => Action::DeleteChar,
            KeyCode::Left => Action::CursorLeft,
            KeyCode::Right => Action::CursorRight,
            KeyCode::Home => Action::CursorStart,
            KeyCode::End => Action::CursorEnd,
            _ => Action::None,
        }
    }

    fn handle_command(key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => Action::Cancel,
            KeyCode::Enter => Action::Submit,
            KeyCode::Char(c) => Action::InsertChar(c),
            KeyCode::Backspace => Action::DeleteChar,
            KeyCode::Left => Action::CursorLeft,
            KeyCode::Right => Action::CursorRight,
            KeyCode::Home => Action::CursorStart,
            KeyCode::End => Action::CursorEnd,
            _ => Action::None,
        }
    }

    fn handle_confirm(key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => Action::ConfirmYes,
            KeyCode::Char('n') | KeyCode::Char('N') => Action::ConfirmNo,
            KeyCode::Esc => Action::Cancel,
            KeyCode::Enter => Action::ConfirmYes, // Default to Yes on Enter
            _ => Action::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_default() {
        assert_eq!(Mode::default(), Mode::Normal);
    }

    #[test]
    fn test_mode_indicator() {
        assert_eq!(Mode::Normal.indicator(), "NORMAL");
        assert_eq!(Mode::Insert.indicator(), "INSERT");
        assert_eq!(Mode::Command.indicator(), "COMMAND");
        assert_eq!(Mode::Confirm.indicator(), "CONFIRM");
    }

    #[test]
    fn test_mode_color() {
        use ratatui::style::Color;
        assert_eq!(Mode::Normal.color(), Color::Blue);
        assert_eq!(Mode::Insert.color(), Color::Green);
        assert_eq!(Mode::Command.color(), Color::Yellow);
        assert_eq!(Mode::Confirm.color(), Color::Magenta);
    }

    #[test]
    fn test_normal_mode_key_handling() {
        // Mode switches
        let key = KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Normal, key), Action::EnterInsert);

        let key = KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Normal, key), Action::EnterCommand);

        // Quit commands
        let key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Normal, key), Action::Quit);

        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(KeyHandler::handle(Mode::Normal, key), Action::Quit);

        // Navigation
        let key = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Normal, key), Action::ScrollUp);

        let key = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Normal, key), Action::ScrollUp);

        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Normal, key), Action::ScrollDown);

        let key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Normal, key), Action::ScrollDown);

        // Consensus level shortcuts
        let key = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Normal, key), Action::SwitchToSolo);

        let key = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Normal, key), Action::SwitchToEnsemble);

        let key = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Normal, key), Action::ToggleConsensus);

        let key = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Normal, key), Action::TriggerDiscussion);

        // Help
        let key = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Normal, key), Action::ShowHelp);

        // Unknown key
        let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Normal, key), Action::None);
    }

    #[test]
    fn test_insert_mode_key_handling() {
        // Exit to normal
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Insert, key), Action::ExitToNormal);

        // Submit
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Insert, key), Action::Submit);

        // Character insertion
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Insert, key), Action::InsertChar('a'));

        let key = KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Insert, key), Action::InsertChar('Z'));

        // Editing
        let key = KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Insert, key), Action::DeleteChar);

        // Cursor movement
        let key = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Insert, key), Action::CursorLeft);

        let key = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Insert, key), Action::CursorRight);

        let key = KeyEvent::new(KeyCode::Home, KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Insert, key), Action::CursorStart);

        let key = KeyEvent::new(KeyCode::End, KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Insert, key), Action::CursorEnd);

        // Unknown key
        let key = KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Insert, key), Action::None);
    }

    #[test]
    fn test_command_mode_key_handling() {
        // Cancel
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Command, key), Action::Cancel);

        // Submit
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Command, key), Action::Submit);

        // Character insertion
        let key = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Command, key), Action::InsertChar('h'));

        // Editing
        let key = KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Command, key), Action::DeleteChar);

        // Cursor movement (same as Insert mode)
        let key = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Command, key), Action::CursorLeft);

        let key = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Command, key), Action::CursorRight);
    }

    #[test]
    fn test_confirm_mode_key_handling() {
        // Yes confirmations
        let key = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Confirm, key), Action::ConfirmYes);

        let key = KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Confirm, key), Action::ConfirmYes);

        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Confirm, key), Action::ConfirmYes);

        // No confirmations
        let key = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Confirm, key), Action::ConfirmNo);

        let key = KeyEvent::new(KeyCode::Char('N'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Confirm, key), Action::ConfirmNo);

        // Cancel
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Confirm, key), Action::Cancel);

        // Unknown key
        let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(KeyHandler::handle(Mode::Confirm, key), Action::None);
    }

    #[test]
    fn test_action_equality() {
        assert_eq!(Action::EnterInsert, Action::EnterInsert);
        assert_ne!(Action::EnterInsert, Action::EnterCommand);
        assert_eq!(Action::InsertChar('a'), Action::InsertChar('a'));
        assert_ne!(Action::InsertChar('a'), Action::InsertChar('b'));
    }

    #[test]
    fn test_mode_transitions() {
        // Normal -> Insert -> Normal
        let mut mode = Mode::Normal;
        let action = KeyHandler::handle(mode, KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert_eq!(action, Action::EnterInsert);
        
        mode = Mode::Insert;
        let action = KeyHandler::handle(mode, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(action, Action::ExitToNormal);

        // Normal -> Command -> Normal
        mode = Mode::Normal;
        let action = KeyHandler::handle(mode, KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
        assert_eq!(action, Action::EnterCommand);
        
        mode = Mode::Command;
        let action = KeyHandler::handle(mode, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(action, Action::Cancel);
    }
}
