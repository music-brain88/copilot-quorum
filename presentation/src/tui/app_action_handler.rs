//! KeyAction handling — maps semantic key actions to state changes and commands.

use super::command_completion;
use super::content::ContentRegistry;
use super::event::TuiCommand;
use super::mode::{CompletionDirection, InputMode, KeyAction, VisualDirection};
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
            state.command_completion = None;
        }
        KeyAction::ExitToNormal => {
            // Esc during an active completion session cancels the
            // completion (restoring what was typed before Tab) and stays in
            // Command mode — mirrors vim's wildmenu Esc behavior (#326).
            if state.mode == InputMode::Command
                && let Some(completion) = state.command_completion.take()
            {
                state.command_input = completion.original_input;
                state.command_cursor = state.command_input.len();
            } else {
                state.mode = InputMode::Normal;
                state.visual_selection = None;
            }
        }

        // Text editing
        KeyAction::InsertChar(c) => {
            state.command_completion = None;
            state.insert_char(c);
        }
        KeyAction::InsertNewline => state.insert_newline(),
        KeyAction::DeleteChar => {
            state.command_completion = None;
            state.delete_char();
        }
        KeyAction::CursorLeft => state.cursor_left(),
        KeyAction::CursorRight => state.cursor_right(),
        KeyAction::CursorHome => state.cursor_home(),
        KeyAction::CursorEnd => state.cursor_end(),

        // Submit
        KeyAction::SubmitInput => {
            let input = state.take_input();
            if !input.is_empty() {
                state.tabs.active_pane_mut().set_title_if_empty(&input);
                match state.tabs.active_pane().kind {
                    // Bound tab: run the request against its interaction.
                    PaneKind::Interaction(_, Some(id)) => {
                        state.push_message(DisplayMessage::user(&input));
                        let _ = cmd_tx.send(TuiCommand::ProcessRequest {
                            interaction_id: Some(id),
                            request: input,
                        });
                    }
                    // Placeholder tab (`:tabnew`, or the survivor after a bound
                    // tab was closed): spawn a fresh interaction and bind it to
                    // THIS tab so the response renders here — instead of the
                    // controller falling back to its `active_interaction_id`,
                    // which may still point at a closed tab (#283).
                    //
                    // The spawn path echoes the user message via
                    // InteractionSpawned, so don't push it locally here (that
                    // would duplicate it).
                    PaneKind::Interaction(form, None) => {
                        let _ = cmd_tx.send(TuiCommand::SpawnInteraction {
                            form,
                            query: input,
                            context_mode_override: None,
                        });
                    }
                }
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
            state.command_completion = None;
        }
        KeyAction::SwitchDiscuss => {
            // Enter command mode with "discuss " pre-filled
            state.mode = InputMode::Command;
            state.command_input = "discuss ".into();
            state.command_cursor = state.command_input.len();
            state.command_completion = None;
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
        KeyAction::CloseTabOrQuit => {
            // Tab-aware quit (`:q`): close the active tab, or quit on the last
            // one. Shares the exact command-path helper so the keymap action and
            // `:q` never diverge (#284).
            use super::app_tab_command::QuitOutcome;
            match super::app_tab_command::handle_quit_command(state, "q", cmd_tx) {
                QuitOutcome::QuitApp => state.should_quit = true,
                QuitOutcome::TabClosed(flash) => state.set_flash(flash),
                QuitOutcome::NotQuit => {}
            }
        }
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

        KeyAction::CommandComplete(direction) => {
            complete_command(state, scripting_engine, direction);
        }
    }
    None
}

/// Advance Command-mode wildmenu completion by one Tab/Shift+Tab press (#326).
/// No-op outside Command mode (Tab/BackTab only dispatch this action from
/// `handle_command()`, so this guard should never actually trigger).
fn complete_command(
    state: &mut TuiState,
    scripting_engine: &Arc<dyn quorum_application::ScriptingEnginePort>,
    direction: CompletionDirection,
) {
    if state.mode != InputMode::Command {
        return;
    }
    let lua_command_names: Vec<String> = scripting_engine
        .registered_commands()
        .into_iter()
        .map(|(name, _description, _usage, _callback_id)| name)
        .collect();

    match command_completion::advance(
        &state.command_input,
        state.command_completion.as_ref(),
        &lua_command_names,
        direction,
    ) {
        Some((new_input, new_completion)) => {
            state.command_input = new_input;
            state.command_cursor = state.command_input.len();
            state.command_completion = Some(new_completion);
        }
        None => {
            state.command_completion = None;
            state.set_flash("No matching command");
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use quorum_application::{NoClipboard, NoScriptingEngine};
    use quorum_domain::interaction::{InteractionForm, InteractionId};

    fn submit(state: &mut TuiState, cmd_tx: &mpsc::UnboundedSender<TuiCommand>) {
        run(state, KeyAction::SubmitInput, cmd_tx);
    }

    #[test]
    fn submit_on_bound_tab_processes_request() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = TuiState::new();
        let id = InteractionId(5);
        state
            .tabs
            .create_tab(PaneKind::Interaction(InteractionForm::Agent, Some(id)));
        state.tabs.active_pane_mut().input = "hello".into();

        submit(&mut state, &tx);

        match rx.try_recv() {
            Ok(TuiCommand::ProcessRequest {
                interaction_id,
                request,
            }) => {
                assert_eq!(interaction_id, Some(id));
                assert_eq!(request, "hello");
            }
            other => panic!("expected ProcessRequest, got {:?}", other.is_ok()),
        }
        // The user message is echoed locally for a bound tab.
        assert_eq!(state.tabs.active_pane().conversation.messages.len(), 1);
    }

    fn run(state: &mut TuiState, action: KeyAction, cmd_tx: &mpsc::UnboundedSender<TuiCommand>) {
        let scripting: Arc<dyn quorum_application::ScriptingEnginePort> =
            Arc::new(NoScriptingEngine);
        let clipboard: Arc<dyn quorum_application::ClipboardPort> = Arc::new(NoClipboard);
        let registry = RefCell::new(ContentRegistry::new());
        handle_action(state, action, cmd_tx, &scripting, &clipboard, &registry);
    }

    #[test]
    fn close_tab_or_quit_closes_tab_when_multiple_open() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = TuiState::new();
        state.tabs.create_tab(PaneKind::Interaction(
            InteractionForm::Agent,
            Some(InteractionId(3)),
        ));
        assert_eq!(state.tabs.len(), 2);

        run(&mut state, KeyAction::CloseTabOrQuit, &tx);

        // Tab-aware: closed the active tab, did NOT quit the app.
        assert_eq!(state.tabs.len(), 1);
        assert!(!state.should_quit);
        // And cancelled the closed tab's interaction (via the shared helper).
        assert!(matches!(
            rx.try_recv(),
            Ok(TuiCommand::CancelInteraction(InteractionId(3)))
        ));
    }

    #[test]
    fn close_tab_or_quit_quits_on_last_tab() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = TuiState::new();
        assert_eq!(state.tabs.len(), 1);

        run(&mut state, KeyAction::CloseTabOrQuit, &tx);

        assert!(state.should_quit);
    }

    #[test]
    fn quit_builtin_always_quits_whole_app() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = TuiState::new();
        state
            .tabs
            .create_tab(PaneKind::Interaction(InteractionForm::Agent, None));
        assert_eq!(state.tabs.len(), 2);

        run(&mut state, KeyAction::Quit, &tx);

        // `quit` is `:qa` — quits regardless of open tab count.
        assert!(state.should_quit);
    }

    #[test]
    fn submit_on_placeholder_tab_spawns_interaction() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = TuiState::new();
        // `:tabnew` style placeholder (id = None) becomes active.
        state
            .tabs
            .create_tab(PaneKind::Interaction(InteractionForm::Agent, None));
        state.tabs.active_pane_mut().input = "do the thing".into();

        submit(&mut state, &tx);

        match rx.try_recv() {
            Ok(TuiCommand::SpawnInteraction { form, query, .. }) => {
                assert_eq!(form, InteractionForm::Agent);
                assert_eq!(query, "do the thing");
            }
            other => panic!("expected SpawnInteraction, got ok={:?}", other.is_ok()),
        }
        // No local echo — the spawn path echoes via InteractionSpawned, so the
        // placeholder pane must not have pushed a duplicate user message.
        assert!(state.tabs.active_pane().conversation.messages.is_empty());
    }

    // -- Command-mode completion (#326) --

    fn enter_command(state: &mut TuiState, cmd_tx: &mpsc::UnboundedSender<TuiCommand>, text: &str) {
        run(state, KeyAction::EnterCommand, cmd_tx);
        for c in text.chars() {
            run(state, KeyAction::InsertChar(c), cmd_tx);
        }
    }

    #[test]
    fn tab_completes_command_name_to_longest_prefix() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = TuiState::new();
        enter_command(&mut state, &tx, "st");

        run(
            &mut state,
            KeyAction::CommandComplete(CompletionDirection::Forward),
            &tx,
        );

        assert_eq!(state.command_input, "strategy");
        assert!(state.command_completion.is_some());
    }

    #[test]
    fn tab_cycles_through_multiple_matches() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = TuiState::new();
        enter_command(&mut state, &tx, "q");

        run(
            &mut state,
            KeyAction::CommandComplete(CompletionDirection::Forward),
            &tx,
        );
        let first = state.command_input.clone();
        run(
            &mut state,
            KeyAction::CommandComplete(CompletionDirection::Forward),
            &tx,
        );
        let second = state.command_input.clone();

        assert_ne!(first, second);
    }

    #[test]
    fn shift_tab_cycles_backward() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = TuiState::new();
        enter_command(&mut state, &tx, "q");

        run(
            &mut state,
            KeyAction::CommandComplete(CompletionDirection::Backward),
            &tx,
        );

        let completion = state.command_completion.as_ref().unwrap();
        assert_eq!(completion.cycle_index, Some(completion.matches.len() - 1));
    }

    #[test]
    fn typing_after_tab_discards_completion_state() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = TuiState::new();
        enter_command(&mut state, &tx, "q");
        run(
            &mut state,
            KeyAction::CommandComplete(CompletionDirection::Forward),
            &tx,
        );
        assert!(state.command_completion.is_some());

        run(&mut state, KeyAction::InsertChar('a'), &tx);

        assert!(state.command_completion.is_none());
    }

    #[test]
    fn backspace_after_tab_discards_completion_state() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = TuiState::new();
        enter_command(&mut state, &tx, "q");
        run(
            &mut state,
            KeyAction::CommandComplete(CompletionDirection::Forward),
            &tx,
        );
        assert!(state.command_completion.is_some());

        run(&mut state, KeyAction::DeleteChar, &tx);

        assert!(state.command_completion.is_none());
    }

    #[test]
    fn esc_during_completion_restores_original_input_and_stays_in_command_mode() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = TuiState::new();
        enter_command(&mut state, &tx, "st");
        run(
            &mut state,
            KeyAction::CommandComplete(CompletionDirection::Forward),
            &tx,
        );
        assert_eq!(state.command_input, "strategy");

        run(&mut state, KeyAction::ExitToNormal, &tx);

        assert_eq!(state.command_input, "st");
        assert!(state.command_completion.is_none());
        assert_eq!(state.mode, InputMode::Command);
    }

    #[test]
    fn esc_without_completion_exits_to_normal_as_before() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = TuiState::new();
        enter_command(&mut state, &tx, "q");

        run(&mut state, KeyAction::ExitToNormal, &tx);

        assert_eq!(state.mode, InputMode::Normal);
    }

    #[test]
    fn tab_with_no_matches_sets_flash_and_leaves_input_untouched() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = TuiState::new();
        enter_command(&mut state, &tx, "zzz");

        run(
            &mut state,
            KeyAction::CommandComplete(CompletionDirection::Forward),
            &tx,
        );

        assert_eq!(state.command_input, "zzz");
        assert!(state.command_completion.is_none());
        assert!(state.flash_message.is_some());
    }
}
