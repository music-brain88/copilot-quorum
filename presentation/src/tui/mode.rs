//! Vim-like modal input system
//!
//! Defines input modes and key-to-action dispatching.
//! #68 implements the core modes; #69 will add more Normal-mode bindings.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Application input mode (vim-like)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    /// Navigation and single-key commands
    Normal,
    /// Text input — typing goes into the input buffer
    #[default]
    Insert,
    /// Command-line mode (`:` prefix)
    Command,
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
    InsertNewline,
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

    // -- Tabs (Normal mode, via g prefix) --
    NextTab,
    PrevTab,

    // -- Pending key (prefix key awaiting next input) --
    PendingKey(char),

    // -- Quick commands (Normal mode) --
    SwitchSolo,
    SwitchEnsemble,
    ToggleFast,
    SwitchAsk,
    SwitchDiscuss,

    // -- Editor --
    LaunchEditor,

    // -- Application --
    Quit,
    ShowHelp,
    ToggleConsensus,

    // -- Lua scripting --
    LuaCallback(u64),
}

/// A table of custom keybindings registered from Lua scripts.
///
/// Maps `(InputMode, KeyCode, KeyModifiers)` to `KeyAction`.
/// Checked **before** built-in bindings in `handle_key_event`.
pub struct CustomKeymap {
    entries: Vec<(InputMode, KeyCode, KeyModifiers, KeyAction)>,
}

impl CustomKeymap {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Build a custom keymap from scripting engine keymap registrations.
    ///
    /// Each entry is `(mode_name, key_descriptor, action)` from the engine.
    /// Entries with unparseable key descriptors or unknown modes are silently skipped.
    pub fn from_registered(
        keymaps: &[(String, String, quorum_application::KeymapAction)],
    ) -> Self {
        let mut entries = Vec::new();
        for (mode_name, key_desc, action) in keymaps {
            let mode = match mode_name.as_str() {
                "normal" => InputMode::Normal,
                "insert" => InputMode::Insert,
                "command" => InputMode::Command,
                _ => continue,
            };
            let Some((key_code, modifiers)) = parse_key_descriptor(key_desc) else {
                continue;
            };
            let key_action = match action {
                quorum_application::KeymapAction::Builtin(name) => {
                    builtin_action_by_name(name)
                }
                quorum_application::KeymapAction::LuaCallback(id) => {
                    KeyAction::LuaCallback(*id)
                }
            };
            entries.push((mode, key_code, modifiers, key_action));
        }
        Self { entries }
    }

    /// Look up a custom binding for the given mode + key event.
    pub fn lookup(&self, mode: InputMode, key: &KeyEvent) -> Option<&KeyAction> {
        self.entries.iter().find_map(|(m, code, mods, action)| {
            if *m == mode && *code == key.code && key.modifiers.contains(*mods) {
                Some(action)
            } else {
                None
            }
        })
    }

}

/// Map a built-in action name (from Lua) to a `KeyAction`.
fn builtin_action_by_name(name: &str) -> KeyAction {
    match name {
        "enter_insert" => KeyAction::EnterInsert,
        "enter_command" => KeyAction::EnterCommand,
        "exit_to_normal" => KeyAction::ExitToNormal,
        "submit_input" => KeyAction::SubmitInput,
        "submit_command" => KeyAction::SubmitCommand,
        "scroll_up" => KeyAction::ScrollUp,
        "scroll_down" => KeyAction::ScrollDown,
        "scroll_to_top" => KeyAction::ScrollToTop,
        "scroll_to_bottom" => KeyAction::ScrollToBottom,
        "next_tab" => KeyAction::NextTab,
        "prev_tab" => KeyAction::PrevTab,
        "switch_solo" => KeyAction::SwitchSolo,
        "switch_ensemble" => KeyAction::SwitchEnsemble,
        "toggle_fast" => KeyAction::ToggleFast,
        "switch_ask" => KeyAction::SwitchAsk,
        "switch_discuss" => KeyAction::SwitchDiscuss,
        "launch_editor" => KeyAction::LaunchEditor,
        "quit" => KeyAction::Quit,
        "show_help" => KeyAction::ShowHelp,
        "insert_newline" => KeyAction::InsertNewline,
        _ => KeyAction::None,
    }
}

/// Parse a human-readable key descriptor string into (KeyCode, KeyModifiers).
///
/// Supports:
/// - Simple keys: `"j"`, `"Esc"`, `"Enter"`, `"Space"`, `"Tab"`
/// - Modified keys: `"Ctrl+s"`, `"Shift+Enter"`, `"Ctrl+Shift+p"`
/// - Special keys: `"Backspace"`, `"Delete"`, `"Up"`, `"Down"`, `"F1"`–`"F12"`
pub fn parse_key_descriptor(desc: &str) -> Option<(KeyCode, KeyModifiers)> {
    if desc.is_empty() {
        return None;
    }

    let parts: Vec<&str> = desc.split('+').collect();
    let (key_str, mod_parts) = parts.split_last()?;

    if key_str.is_empty() {
        return None;
    }

    let mut modifiers = KeyModifiers::NONE;
    for part in mod_parts {
        match *part {
            "Ctrl" => modifiers |= KeyModifiers::CONTROL,
            "Shift" => modifiers |= KeyModifiers::SHIFT,
            "Alt" => modifiers |= KeyModifiers::ALT,
            _ => return None,
        }
    }

    let key_code = match *key_str {
        "Esc" => KeyCode::Esc,
        "Enter" => KeyCode::Enter,
        "Tab" => KeyCode::Tab,
        "Backspace" => KeyCode::Backspace,
        "Delete" => KeyCode::Delete,
        "Space" => KeyCode::Char(' '),
        "Up" => KeyCode::Up,
        "Down" => KeyCode::Down,
        "Left" => KeyCode::Left,
        "Right" => KeyCode::Right,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        s if s.starts_with('F') && s.len() >= 2 => {
            let num = s[1..].parse::<u8>().ok()?;
            if (1..=12).contains(&num) {
                KeyCode::F(num)
            } else {
                return None;
            }
        }
        s if s.len() == 1 => KeyCode::Char(s.chars().next().unwrap()),
        _ => return None,
    };

    Some((key_code, modifiers))
}

/// Map a key event + current mode to a semantic action.
///
/// `pending_key` carries the prefix key from a previous keystroke (e.g. `g`).
pub fn handle_key_event(mode: InputMode, key: KeyEvent, pending_key: Option<char>) -> KeyAction {
    // Global: Ctrl+C always quits
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return KeyAction::Quit;
    }

    match mode {
        InputMode::Normal => handle_normal(key, pending_key),
        InputMode::Insert => handle_insert(key),
        InputMode::Command => handle_command(key),
    }
}

fn handle_normal(key: KeyEvent, pending_key: Option<char>) -> KeyAction {
    // Handle pending `g` prefix
    if pending_key == Some('g') {
        return match key.code {
            KeyCode::Char('g') => KeyAction::ScrollToTop, // gg
            KeyCode::Char('t') => KeyAction::NextTab,     // gt
            KeyCode::Char('T') => KeyAction::PrevTab,     // gT
            _ => KeyAction::None,                         // unknown g-combo, discard
        };
    }

    match key.code {
        // Mode transitions
        KeyCode::Char('i') => KeyAction::EnterInsert,
        KeyCode::Char(':') => KeyAction::EnterCommand,

        // $EDITOR delegation
        KeyCode::Char('I') => KeyAction::LaunchEditor,

        // Consensus level
        KeyCode::Char('s') => KeyAction::SwitchSolo,
        KeyCode::Char('e') => KeyAction::SwitchEnsemble,
        KeyCode::Char('f') => KeyAction::ToggleFast,

        // Interaction type
        KeyCode::Char('a') => KeyAction::SwitchAsk,
        KeyCode::Char('d') => KeyAction::SwitchDiscuss,

        // Scrolling
        KeyCode::Char('j') | KeyCode::Down => KeyAction::ScrollDown,
        KeyCode::Char('k') | KeyCode::Up => KeyAction::ScrollUp,
        KeyCode::Char('g') => KeyAction::PendingKey('g'), // g prefix
        KeyCode::Char('G') => KeyAction::ScrollToBottom,

        // Help
        KeyCode::Char('?') => KeyAction::ShowHelp,

        _ => KeyAction::None,
    }
}

fn handle_insert(key: KeyEvent) -> KeyAction {
    match key.code {
        KeyCode::Esc => KeyAction::ExitToNormal,
        // Shift+Enter (primary, needs kitty keyboard protocol) or
        // Alt+Enter (fallback for terminals without keyboard enhancement)
        KeyCode::Enter
            if key.modifiers.contains(KeyModifiers::SHIFT)
                || key.modifiers.contains(KeyModifiers::ALT) =>
        {
            KeyAction::InsertNewline
        }
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
            assert_eq!(handle_key_event(mode, key, None), KeyAction::Quit);
        }
    }

    #[test]
    fn test_normal_mode_transitions() {
        let key_i = KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Normal, key_i, None),
            KeyAction::EnterInsert
        );

        // `a` is now SwitchAsk, not EnterInsert
        let key_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Normal, key_a, None),
            KeyAction::SwitchAsk
        );

        let key_colon = KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Normal, key_colon, None),
            KeyAction::EnterCommand
        );
    }

    #[test]
    fn test_normal_scrolling() {
        let key_j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Normal, key_j, None),
            KeyAction::ScrollDown
        );

        let key_k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Normal, key_k, None),
            KeyAction::ScrollUp
        );
    }

    #[test]
    fn test_insert_mode_typing() {
        let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Insert, key, None),
            KeyAction::InsertChar('x')
        );
    }

    #[test]
    fn test_insert_esc_exits() {
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Insert, key, None),
            KeyAction::ExitToNormal
        );
    }

    #[test]
    fn test_insert_enter_submits() {
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Insert, key, None),
            KeyAction::SubmitInput
        );
    }

    #[test]
    fn test_insert_shift_enter_newline() {
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
        assert_eq!(
            handle_key_event(InputMode::Insert, key, None),
            KeyAction::InsertNewline
        );
    }

    #[test]
    fn test_insert_alt_enter_newline_fallback() {
        // Alt+Enter is kept as fallback for terminals without kitty protocol
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
        assert_eq!(
            handle_key_event(InputMode::Insert, key, None),
            KeyAction::InsertNewline
        );
    }

    #[test]
    fn test_normal_shift_i_launches_editor() {
        let key = KeyEvent::new(KeyCode::Char('I'), KeyModifiers::SHIFT);
        assert_eq!(
            handle_key_event(InputMode::Normal, key, None),
            KeyAction::LaunchEditor
        );
    }

    #[test]
    fn test_command_enter_submits() {
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Command, key, None),
            KeyAction::SubmitCommand
        );
    }

    #[test]
    fn test_command_esc_exits() {
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Command, key, None),
            KeyAction::ExitToNormal
        );
    }

    #[test]
    fn test_normal_quick_commands() {
        let cases = vec![
            ('s', KeyAction::SwitchSolo),
            ('e', KeyAction::SwitchEnsemble),
            ('f', KeyAction::ToggleFast),
            ('a', KeyAction::SwitchAsk),
            ('d', KeyAction::SwitchDiscuss),
        ];
        for (ch, expected) in cases {
            let key = KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE);
            assert_eq!(
                handle_key_event(InputMode::Normal, key, None),
                expected,
                "Normal mode '{}' should map to {:?}",
                ch,
                expected,
            );
        }
    }

    #[test]
    fn test_g_prefix_gg_scroll_to_top() {
        // First `g` press → PendingKey
        let key_g = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Normal, key_g, None),
            KeyAction::PendingKey('g')
        );
        // Second `g` with pending → ScrollToTop
        let key_g2 = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Normal, key_g2, Some('g')),
            KeyAction::ScrollToTop
        );
    }

    #[test]
    fn test_g_prefix_gt_next_tab() {
        let key_t = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Normal, key_t, Some('g')),
            KeyAction::NextTab
        );
    }

    #[test]
    fn test_g_prefix_g_shift_t_prev_tab() {
        let key_shift_t = KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT);
        assert_eq!(
            handle_key_event(InputMode::Normal, key_shift_t, Some('g')),
            KeyAction::PrevTab
        );
    }

    #[test]
    fn test_g_prefix_unknown_discards() {
        let key_x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(InputMode::Normal, key_x, Some('g')),
            KeyAction::None
        );
    }

    #[test]
    fn test_big_g_scroll_to_bottom() {
        let key_big_g = KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT);
        assert_eq!(
            handle_key_event(InputMode::Normal, key_big_g, None),
            KeyAction::ScrollToBottom
        );
    }

    // -- Key descriptor parser tests --

    #[test]
    fn test_parse_simple_char() {
        assert_eq!(
            parse_key_descriptor("j"),
            Some((KeyCode::Char('j'), KeyModifiers::NONE))
        );
    }

    #[test]
    fn test_parse_ctrl_modifier() {
        assert_eq!(
            parse_key_descriptor("Ctrl+s"),
            Some((KeyCode::Char('s'), KeyModifiers::CONTROL))
        );
    }

    #[test]
    fn test_parse_shift_enter() {
        assert_eq!(
            parse_key_descriptor("Shift+Enter"),
            Some((KeyCode::Enter, KeyModifiers::SHIFT))
        );
    }

    #[test]
    fn test_parse_esc() {
        assert_eq!(
            parse_key_descriptor("Esc"),
            Some((KeyCode::Esc, KeyModifiers::NONE))
        );
    }

    #[test]
    fn test_parse_f_keys() {
        assert_eq!(
            parse_key_descriptor("F1"),
            Some((KeyCode::F(1), KeyModifiers::NONE))
        );
    }

    #[test]
    fn test_parse_ctrl_shift_combo() {
        assert_eq!(
            parse_key_descriptor("Ctrl+Shift+p"),
            Some((KeyCode::Char('p'), KeyModifiers::CONTROL | KeyModifiers::SHIFT))
        );
    }

    #[test]
    fn test_parse_special_keys() {
        assert_eq!(
            parse_key_descriptor("Space"),
            Some((KeyCode::Char(' '), KeyModifiers::NONE))
        );
        assert_eq!(
            parse_key_descriptor("Tab"),
            Some((KeyCode::Tab, KeyModifiers::NONE))
        );
        assert_eq!(
            parse_key_descriptor("Backspace"),
            Some((KeyCode::Backspace, KeyModifiers::NONE))
        );
    }

    #[test]
    fn test_parse_arrow_keys() {
        assert_eq!(
            parse_key_descriptor("Up"),
            Some((KeyCode::Up, KeyModifiers::NONE))
        );
        assert_eq!(
            parse_key_descriptor("Down"),
            Some((KeyCode::Down, KeyModifiers::NONE))
        );
    }

    #[test]
    fn test_parse_invalid_returns_none() {
        assert_eq!(parse_key_descriptor(""), None);
        assert_eq!(parse_key_descriptor("Ctrl+"), None);
        assert_eq!(parse_key_descriptor("gibberish"), None);
    }

    #[test]
    fn test_builtin_action_by_name() {
        assert_eq!(builtin_action_by_name("submit_input"), KeyAction::SubmitInput);
        assert_eq!(builtin_action_by_name("quit"), KeyAction::Quit);
        assert_eq!(builtin_action_by_name("unknown"), KeyAction::None);
    }
}
