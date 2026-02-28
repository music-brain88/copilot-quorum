//! KeyAction handling — maps semantic key actions to state changes and commands.

use super::event::TuiCommand;
use super::mode::{InputMode, KeyAction};
use super::state::{DisplayMessage, TuiState};
use super::tab::PaneKind;
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
        KeyAction::ExitToNormal => state.mode = InputMode::Normal,

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
            let cmd = state.take_command();
            state.mode = InputMode::Normal;
            if !cmd.is_empty() {
                if cmd == "q" || cmd == "quit" || cmd == "exit" {
                    state.should_quit = true;
                } else if let Some(flash) =
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
        KeyAction::ShowHelp => state.show_help = !state.show_help,
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
    }
    None
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
