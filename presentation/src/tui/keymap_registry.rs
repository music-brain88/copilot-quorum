//! Single source of truth for built-in keybindings, read by the Remote
//! Control API's `keymaps.list` (#302).
//!
//! Mirrors the bindings documented in the Help overlay (`app_render::
//! help_lines`), scoped the same way: notable actions, not every raw
//! text-editing key (arrow keys / Backspace / plain character insertion
//! are intentionally omitted, matching what the Help overlay already
//! chooses to show a human).
//!
//! `key` uses the Lua keymap descriptor syntax (`mode::parse_key_descriptor`)
//! where the binding is a single keypress. Multi-key sequences (`"gg"`,
//! `"yy"`) are not parseable descriptors — they document a Normal-mode
//! prefix chord, not a single `keys.feed` entry.

/// Metadata for one built-in keybinding.
#[derive(Debug, Clone, Copy)]
pub struct KeymapInfo {
    /// Mode the binding applies in, or `"global"` for bindings active in
    /// every mode (currently only Ctrl+C).
    pub mode: &'static str,
    /// Human-readable key descriptor (see module docs re: chords).
    pub key: &'static str,
    /// Action name, matching `mode::builtin_action_by_name` where the
    /// binding maps to a single built-in action.
    pub action: &'static str,
    /// One-line description.
    pub description: &'static str,
}

/// All built-in keybindings, in the order they should be presented.
pub fn builtin_keymaps() -> &'static [KeymapInfo] {
    KEYMAPS
}

static KEYMAPS: &[KeymapInfo] = &[
    KeymapInfo {
        mode: "global",
        key: "Ctrl+c",
        action: "quit",
        description: "Quit the app",
    },
    KeymapInfo {
        mode: "normal",
        key: "i",
        action: "enter_insert",
        description: "Enter Insert mode",
    },
    KeymapInfo {
        mode: "normal",
        key: ":",
        action: "enter_command",
        description: "Enter Command mode",
    },
    KeymapInfo {
        mode: "normal",
        key: "I",
        action: "launch_editor",
        description: "Open $EDITOR with the current input",
    },
    KeymapInfo {
        mode: "normal",
        key: "s",
        action: "switch_solo",
        description: "Switch to Solo mode",
    },
    KeymapInfo {
        mode: "normal",
        key: "e",
        action: "switch_ensemble",
        description: "Switch to Ensemble mode",
    },
    KeymapInfo {
        mode: "normal",
        key: "f",
        action: "toggle_fast",
        description: "Toggle Fast phase scope",
    },
    KeymapInfo {
        mode: "normal",
        key: "a",
        action: "switch_ask",
        description: "Prefill \":ask \" in Command mode",
    },
    KeymapInfo {
        mode: "normal",
        key: "d",
        action: "switch_discuss",
        description: "Prefill \":discuss \" in Command mode",
    },
    KeymapInfo {
        mode: "normal",
        key: "j",
        action: "scroll_down",
        description: "Scroll down",
    },
    KeymapInfo {
        mode: "normal",
        key: "Down",
        action: "scroll_down",
        description: "Scroll down",
    },
    KeymapInfo {
        mode: "normal",
        key: "k",
        action: "scroll_up",
        description: "Scroll up",
    },
    KeymapInfo {
        mode: "normal",
        key: "Up",
        action: "scroll_up",
        description: "Scroll up",
    },
    KeymapInfo {
        mode: "normal",
        key: "gg",
        action: "scroll_to_top",
        description: "Scroll to top (g prefix chord)",
    },
    KeymapInfo {
        mode: "normal",
        key: "gt",
        action: "next_tab",
        description: "Next tab (g prefix chord)",
    },
    KeymapInfo {
        mode: "normal",
        key: "gT",
        action: "prev_tab",
        description: "Previous tab (g prefix chord)",
    },
    KeymapInfo {
        mode: "normal",
        key: "G",
        action: "scroll_to_bottom",
        description: "Scroll to bottom",
    },
    KeymapInfo {
        mode: "normal",
        key: "yy",
        action: "yank_recent",
        description: "Yank the most recent message in the focused pane (y prefix chord)",
    },
    KeymapInfo {
        mode: "normal",
        key: "ya",
        action: "yank_all",
        description: "Yank the full content of the focused pane (y prefix chord)",
    },
    KeymapInfo {
        mode: "normal",
        key: "Y",
        action: "yank_last_assistant",
        description: "Yank the last Assistant response",
    },
    KeymapInfo {
        mode: "normal",
        key: "v",
        action: "enter_visual",
        description: "Enter Visual mode",
    },
    KeymapInfo {
        mode: "normal",
        key: "Ctrl+w",
        action: "cycle_focus",
        description: "Cycle which content slot has yank focus",
    },
    KeymapInfo {
        mode: "normal",
        key: "?",
        action: "show_help",
        description: "Toggle the help overlay",
    },
    KeymapInfo {
        mode: "visual",
        key: "h/j/k/l",
        action: "visual_extend",
        description: "Extend the selection",
    },
    KeymapInfo {
        mode: "visual",
        key: "w/b",
        action: "visual_extend",
        description: "Word-wise extend",
    },
    KeymapInfo {
        mode: "visual",
        key: "Home/End",
        action: "visual_extend",
        description: "Jump to line start/end",
    },
    KeymapInfo {
        mode: "visual",
        key: "y",
        action: "visual_yank",
        description: "Yank the selection and copy to clipboard",
    },
    KeymapInfo {
        mode: "visual",
        key: "Enter",
        action: "visual_yank",
        description: "Yank the selection and copy to clipboard",
    },
    KeymapInfo {
        mode: "visual",
        key: "Esc",
        action: "exit_to_normal",
        description: "Exit to Normal mode",
    },
    KeymapInfo {
        mode: "insert",
        key: "Enter",
        action: "submit_input",
        description: "Send the message",
    },
    KeymapInfo {
        mode: "insert",
        key: "Shift+Enter",
        action: "insert_newline",
        description: "Insert a newline (multiline input)",
    },
    KeymapInfo {
        mode: "insert",
        key: "Esc",
        action: "exit_to_normal",
        description: "Return to Normal mode",
    },
    KeymapInfo {
        mode: "command",
        key: "Enter",
        action: "submit_command",
        description: "Run the command",
    },
    KeymapInfo {
        mode: "command",
        key: "Esc",
        action: "exit_to_normal",
        description: "Cancel and return to Normal mode",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_keymaps_not_empty() {
        assert!(!builtin_keymaps().is_empty());
    }

    #[test]
    fn every_entry_has_a_description() {
        for km in builtin_keymaps() {
            assert!(!km.description.is_empty(), "{} has no description", km.key);
        }
    }

    #[test]
    fn every_mode_is_a_known_name() {
        for km in builtin_keymaps() {
            assert!(
                ["global", "normal", "visual", "insert", "command"].contains(&km.mode),
                "unknown mode: {}",
                km.mode
            );
        }
    }
}
