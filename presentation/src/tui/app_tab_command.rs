//! Tab command handling (local commands that don't need controller round-trip).

use super::event::TuiCommand;
use super::state::{DisplayMessage, TuiState};
use super::tab::PaneKind;
use quorum_domain::interaction::InteractionForm;
use tokio::sync::mpsc;

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
        if state.tabs.close_active() {
            // Sync active_interaction_id with the new active tab
            if let PaneKind::Interaction(_, Some(id)) = state.tabs.active_pane().kind {
                let _ = cmd_tx.send(TuiCommand::ActivateInteraction(id));
            }
            return Some(format!("Tab closed ({} remaining)", state.tabs.len()));
        } else {
            return Some("Cannot close last tab".into());
        }
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
