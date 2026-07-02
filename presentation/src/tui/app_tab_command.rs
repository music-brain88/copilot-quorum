//! Tab command handling (local commands that don't need controller round-trip).

use super::event::TuiCommand;
use super::state::{DisplayMessage, TuiState};
use super::tab::PaneKind;
use quorum_domain::interaction::InteractionForm;
use tokio::sync::mpsc;

/// Outcome of a quit-family command (`:q`, `:qa`, ...).
pub(super) enum QuitOutcome {
    /// Not a quit-family command.
    NotQuit,
    /// Quit the whole app (caller sets `should_quit`).
    QuitApp,
    /// Closed the active tab; flash message to display.
    TabClosed(String),
}

/// Handle quit-family commands with Vim-style tab-aware semantics:
/// `:q`/`:quit` close the active tab when multiple tabs are open and quit the
/// app on the last one; `:qa`/`:qall`/`quitall`/`exit` always quit the app.
/// A trailing bang (`:q!`) is accepted and behaves the same — there is no
/// unsaved-buffer state to discard.
///
/// Shared by the TUI command path (`KeyAction::SubmitCommand`) and the Remote
/// Control API (`command.exec`) so the two never diverge.
pub(super) fn handle_quit_command(
    state: &mut TuiState,
    cmd: &str,
    cmd_tx: &mpsc::UnboundedSender<TuiCommand>,
) -> QuitOutcome {
    let normalized = cmd.trim();
    let normalized = normalized.strip_suffix('!').unwrap_or(normalized);
    match normalized {
        "qa" | "qall" | "quitall" | "exit" => QuitOutcome::QuitApp,
        "q" | "quit" => match close_active_tab(state, cmd_tx) {
            Some(flash) => QuitOutcome::TabClosed(flash),
            // Last tab — nothing left to close, quit the app.
            None => QuitOutcome::QuitApp,
        },
        _ => QuitOutcome::NotQuit,
    }
}

/// Try to close the active tab. On success, resyncs the controller's active
/// interaction with the new active tab and returns the flash message.
/// Returns None when this is the last tab (which cannot be closed).
fn close_active_tab(
    state: &mut TuiState,
    cmd_tx: &mpsc::UnboundedSender<TuiCommand>,
) -> Option<String> {
    // Capture the interaction bound to the tab we're about to close, so we can
    // cancel its in-flight agent once it's gone (issue #282). A placeholder tab
    // (id = None) has no agent to cancel.
    let closing_id = match state.tabs.active_pane().kind {
        PaneKind::Interaction(_, Some(id)) => Some(id),
        PaneKind::Interaction(_, None) => None,
    };
    if !state.tabs.close_active() {
        return None;
    }
    if let Some(id) = closing_id {
        let _ = cmd_tx.send(TuiCommand::CancelInteraction(id));
    }
    // Sync active_interaction_id with the new active tab
    if let PaneKind::Interaction(_, Some(id)) = state.tabs.active_pane().kind {
        let _ = cmd_tx.send(TuiCommand::ActivateInteraction(id));
    }
    Some(format!("Tab closed ({} remaining)", state.tabs.len()))
}

/// Handle tab-related commands locally (no controller round-trip).
/// Returns Some(flash_message) if a tab command was handled, None otherwise.
pub(super) fn handle_tab_command(
    state: &mut TuiState,
    cmd: &str,
    cmd_tx: &mpsc::UnboundedSender<TuiCommand>,
) -> Option<String> {
    let trimmed = cmd.trim();
    let normalized = trimmed.strip_prefix(':').unwrap_or(trimmed);

    if normalized == "ask"
        || normalized.starts_with("ask ")
        || normalized == "discuss"
        || normalized.starts_with("discuss ")
        || normalized == "agent"
        || normalized.starts_with("agent ")
    {
        let (cmd_name, rest) = normalized.split_once(' ').unwrap_or((normalized, ""));
        if let Ok(form) = cmd_name.parse::<InteractionForm>() {
            if rest.is_empty() {
                return Some(format!("Usage: {} <query>", cmd_name));
            }
            let query = rest.trim().to_string();

            // Fix A: Create placeholder tab immediately so the user sees it
            // without waiting for the controller to process SpawnInteraction.
            // The real InteractionId is bound later when InteractionSpawned arrives.
            let kind = PaneKind::Interaction(form, None);
            state.tabs.create_tab(kind);
            state.tabs.active_pane_mut().set_title_if_empty(&query);

            let _ = cmd_tx.send(TuiCommand::SpawnInteraction {
                form,
                query,
                context_mode_override: None,
            });
            return Some(format!("Spawning {}...", cmd_name));
        }
    }

    if trimmed == "tabs" {
        // List all tabs
        let summary = state.tabs.tab_list_summary();
        state.push_message(DisplayMessage::system(summary.join("\n")));
        return Some(format!("{} tab(s) open", state.tabs.len()));
    }

    if trimmed == "tabclose" {
        return Some(match close_active_tab(state, cmd_tx) {
            Some(flash) => flash,
            None => "Cannot close last tab".into(),
        });
    }

    if trimmed == "tabnew" || trimmed.starts_with("tabnew ") {
        let arg = trimmed.strip_prefix("tabnew").unwrap().trim();
        let kind = if arg.is_empty() {
            PaneKind::Interaction(InteractionForm::Agent, None)
        } else {
            match arg.parse::<InteractionForm>() {
                Ok(form) => PaneKind::Interaction(form, None),
                Err(_) => {
                    return Some(format!("Unknown form: {}. Use agent/ask/discuss", arg));
                }
            }
        };
        state.tabs.create_tab(kind);
        return Some(format!("New tab: {}", kind.label()));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (
        TuiState,
        mpsc::UnboundedSender<TuiCommand>,
        mpsc::UnboundedReceiver<TuiCommand>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        (TuiState::new(), tx, rx)
    }

    #[test]
    fn quit_on_last_tab_quits_app() {
        let (mut state, tx, _rx) = setup();
        assert!(matches!(
            handle_quit_command(&mut state, "q", &tx),
            QuitOutcome::QuitApp
        ));
    }

    #[test]
    fn quit_with_multiple_tabs_closes_tab() {
        let (mut state, tx, _rx) = setup();
        state
            .tabs
            .create_tab(PaneKind::Interaction(InteractionForm::Agent, None));
        assert_eq!(state.tabs.len(), 2);
        assert!(matches!(
            handle_quit_command(&mut state, "quit", &tx),
            QuitOutcome::TabClosed(_)
        ));
        assert_eq!(state.tabs.len(), 1);
    }

    #[test]
    fn bang_variant_is_tab_aware() {
        let (mut state, tx, _rx) = setup();
        state
            .tabs
            .create_tab(PaneKind::Interaction(InteractionForm::Ask, None));
        assert!(matches!(
            handle_quit_command(&mut state, "q!", &tx),
            QuitOutcome::TabClosed(_)
        ));
        // Last tab — bang quits the app.
        assert!(matches!(
            handle_quit_command(&mut state, "q!", &tx),
            QuitOutcome::QuitApp
        ));
    }

    #[test]
    fn qa_always_quits_app() {
        let (mut state, tx, _rx) = setup();
        state
            .tabs
            .create_tab(PaneKind::Interaction(InteractionForm::Agent, None));
        for cmd in ["qa", "qall", "quitall", "exit", "qa!"] {
            assert!(matches!(
                handle_quit_command(&mut state, cmd, &tx),
                QuitOutcome::QuitApp
            ));
        }
        // No tab was closed along the way.
        assert_eq!(state.tabs.len(), 2);
    }

    #[test]
    fn closing_bound_tab_cancels_its_interaction() {
        use quorum_domain::interaction::InteractionId;
        let (mut state, tx, mut rx) = setup();
        // Active tab is a bound interaction; another tab exists so it can close.
        state.tabs.create_tab(PaneKind::Interaction(
            InteractionForm::Agent,
            Some(InteractionId(42)),
        ));
        assert!(matches!(
            handle_quit_command(&mut state, "q", &tx),
            QuitOutcome::TabClosed(_)
        ));
        // The closed tab's agent must be cancelled.
        assert!(matches!(
            rx.try_recv(),
            Ok(TuiCommand::CancelInteraction(InteractionId(42)))
        ));
    }

    #[test]
    fn closing_placeholder_tab_sends_no_cancel() {
        let (mut state, tx, mut rx) = setup();
        // Active tab is a placeholder (id = None) — nothing to cancel.
        state
            .tabs
            .create_tab(PaneKind::Interaction(InteractionForm::Agent, None));
        assert!(matches!(
            handle_quit_command(&mut state, "q", &tx),
            QuitOutcome::TabClosed(_)
        ));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn non_quit_commands_pass_through() {
        let (mut state, tx, _rx) = setup();
        for cmd in ["tabclose", "solo", "quite", "q2"] {
            assert!(matches!(
                handle_quit_command(&mut state, cmd, &tx),
                QuitOutcome::NotQuit
            ));
        }
    }
}
