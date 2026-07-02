//! KeyAction handling — maps semantic key actions to state changes and commands.

use super::content::ContentRegistry;
use super::event::TuiCommand;
use super::mode::{InputMode, KeyAction, VisualDirection};
use super::state::{DisplayMessage, TuiState, VisualSelection, YankMode, content_slot_label};
use super::tab::PaneKind;
use std::cell::RefCell;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Side-effect that requires main loop intervention (e.g. terminal suspend)
pub(super) use super::app::SideEffect;

/// Handle a semantic key action.
/// Returns a `SideEffect` if the main loop needs to perform a terminal-level action.
pub(super) fn handle_action(
    state: &mut TuiState,
    action: KeyAction,
    cmd_tx: &mpsc::UnboundedSender<TuiCommand>,
    scripting_engine: &Arc<dyn quorum_application::ScriptingEnginePort>,
    clipboard: &Arc<dyn quorum_application::ClipboardPort>,
    content_registry: &RefCell<ContentRegistry>,
) -> Option<SideEffect> {
    match action {
        KeyAction::None => {}

        // Mode transitions
        KeyAction::EnterInsert => state.mode = InputMode::Insert,
        KeyAction::EnterCommand => {
            state.mode = InputMode::Command;
            state.command_input.clear();
            state.command_cursor = 0;
        }
        KeyAction::ExitToNormal => {
            state.mode = InputMode::Normal;
            state.visual_selection = None;
        }

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
                let interaction_id = state.active_interaction_id();
                let _ = cmd_tx.send(TuiCommand::ProcessRequest {
                    interaction_id,
                    request: input,
                });
            }
        }
        KeyAction::SubmitCommand => {
            // Trim so `:q ` (trailing space) matches the same as `:q`,
            // mirroring the Remote Control API (remote.rs command_exec).
            let cmd = state.take_command().trim().to_string();
            state.mode = InputMode::Normal;
            if !cmd.is_empty() {
                use super::app_tab_command::QuitOutcome;
                match super::app_tab_command::handle_quit_command(state, &cmd, cmd_tx) {
                    QuitOutcome::QuitApp => state.should_quit = true,
                    QuitOutcome::TabClosed(flash) => state.set_flash(flash),
                    QuitOutcome::NotQuit => {
                        if let Some(flash) =
                            super::app_tab_command::handle_tab_command(state, &cmd, cmd_tx)
                        {
                            state.set_flash(flash);
                        } else {
                            let interaction_id = state.active_interaction_id();
                            let _ = cmd_tx.send(TuiCommand::HandleCommand {
                                interaction_id,
                                command: cmd,
                            });
                        }
                    }
                }
            }
        }

        // Quick commands
        KeyAction::SwitchSolo => {
            let interaction_id = state.active_interaction_id();
            let _ = cmd_tx.send(TuiCommand::HandleCommand {
                interaction_id,
                command: "solo".into(),
            });
        }
        KeyAction::SwitchEnsemble => {
            let interaction_id = state.active_interaction_id();
            let _ = cmd_tx.send(TuiCommand::HandleCommand {
                interaction_id,
                command: "ens".into(),
            });
        }
        KeyAction::ToggleFast => {
            let interaction_id = state.active_interaction_id();
            let _ = cmd_tx.send(TuiCommand::HandleCommand {
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
                let _ = cmd_tx.send(TuiCommand::ActivateInteraction(id));
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
                let _ = cmd_tx.send(TuiCommand::ActivateInteraction(id));
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
        KeyAction::ShowHelp => state.toggle_help(),
        KeyAction::ToggleConsensus => {
            // Handled by command
            let _ = cmd_tx.send(TuiCommand::HandleCommand {
                interaction_id: state.active_interaction_id(),
                command: "toggle_consensus".into(),
            });
        }

        // Lua callback — execute via scripting engine
        KeyAction::LuaCallback(id) => {
            if let Err(e) = execute_lua_callback(scripting_engine, id) {
                state.set_flash(format!("Lua error: {}", e));
            }
        }

        // -- Yank / Visual --
        KeyAction::YankRecent => yank(state, clipboard, content_registry, YankMode::Recent),
        KeyAction::YankAll => yank(state, clipboard, content_registry, YankMode::All),
        KeyAction::YankLastAssistant => {
            yank(state, clipboard, content_registry, YankMode::LastAssistant)
        }
        KeyAction::EnterVisual => {
            state.mode = InputMode::Visual;
            state.visual_selection = Some(VisualSelection {
                anchor_line: 0,
                cursor_line: 0,
            });
        }
        KeyAction::VisualExtend(dir) => apply_visual_extend(state, dir),
        KeyAction::VisualYank => {
            let text = extract_visual_selection(state, content_registry);
            match clipboard.write(&text) {
                Ok(()) => state.set_flash(format!("Yanked {} chars", text.chars().count())),
                Err(e) => state.set_flash(format!("Clipboard error: {}", e.message)),
            }
            state.mode = InputMode::Normal;
            state.visual_selection = None;
        }
        KeyAction::CycleFocus => {
            state.cycle_focus();
            state.set_flash(format!(
                "Focus: {}",
                content_slot_label(&state.focused_slot)
            ));
        }
    }
    None
}

/// Shared yank helper — extract text from the focused slot, write to clipboard,
/// and set a flash message reporting the outcome.
fn yank(
    state: &mut TuiState,
    clipboard: &Arc<dyn quorum_application::ClipboardPort>,
    content_registry: &RefCell<ContentRegistry>,
    mode: YankMode,
) {
    let registry = content_registry.borrow();
    let text = match state.extract_focused_text(&registry, mode) {
        Some(t) => t,
        None => {
            state.set_flash("Nothing to yank (no renderer for focused slot)");
            return;
        }
    };
    drop(registry);
    if text.is_empty() {
        state.set_flash("Nothing to yank (empty)");
        return;
    }
    match clipboard.write(&text) {
        Ok(()) => state.set_flash(format!("Yanked {} chars", text.chars().count())),
        Err(e) => state.set_flash(format!("Clipboard error: {}", e.message)),
    }
}

/// Apply a Visual-mode direction to the cursor line of the current selection.
fn apply_visual_extend(state: &mut TuiState, dir: VisualDirection) {
    let Some(ref mut sel) = state.visual_selection else {
        return;
    };
    match dir {
        VisualDirection::Up | VisualDirection::WordLeft => {
            sel.cursor_line = sel.cursor_line.saturating_sub(1);
        }
        VisualDirection::Down | VisualDirection::WordRight => {
            sel.cursor_line = sel.cursor_line.saturating_add(1);
        }
        // LineStart / LineEnd are no-ops in the MVP line-based model.
        VisualDirection::LineStart | VisualDirection::LineEnd => {}
    }
}

/// Slice the focused slot's full text by Visual selection line range.
fn extract_visual_selection(
    state: &TuiState,
    content_registry: &RefCell<ContentRegistry>,
) -> String {
    let Some(sel) = state.visual_selection else {
        return String::new();
    };
    let registry = content_registry.borrow();
    let Some(renderer) = registry.get(&state.focused_slot) else {
        return String::new();
    };
    let full = renderer.get_text_content(state);
    let (start, end) = sel.range();
    full.lines()
        .enumerate()
        .filter(|(i, _)| *i >= start && *i <= end)
        .map(|(_, l)| l)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Execute a Lua callback by its ID through the scripting engine.
fn execute_lua_callback(
    scripting_engine: &Arc<dyn quorum_application::ScriptingEnginePort>,
    callback_id: u64,
) -> Result<(), String> {
    scripting_engine
        .execute_callback(callback_id)
        .map_err(|e| e.message)
}
