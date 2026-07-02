//! Human-in-the-Loop (HiL) modal handling.

use super::event::{HilKind, HilRequest};
use super::state::{HilPrompt, TuiState};
use quorum_domain::HumanDecision;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

/// Handle HiL request — show modal, store response channel.
pub(super) fn handle_hil_request(
    state: &mut TuiState,
    pending_hil_tx: &Arc<Mutex<Option<oneshot::Sender<HumanDecision>>>>,
    request: HilRequest,
) {
    let (title, objective, tasks, message) = match &request.kind {
        HilKind::PlanIntervention {
            request: _req,
            plan,
            review_history,
        } => {
            let rev_count = review_history.iter().filter(|r| !r.approved).count();
            (
                "Plan Requires Human Intervention".to_string(),
                plan.objective.clone(),
                plan.tasks.iter().map(|t| t.description.clone()).collect(),
                format!(
                    "Revision limit ({}) exceeded. Approve or reject?",
                    rev_count
                ),
            )
        }
        HilKind::ExecutionConfirmation { request: _, plan } => (
            "Ready to Execute Plan".to_string(),
            plan.objective.clone(),
            plan.tasks.iter().map(|t| t.description.clone()).collect(),
            "Approve execution?".to_string(),
        ),
    };

    state.hil_prompt = Some(HilPrompt {
        title,
        objective,
        tasks,
        message,
    });

    // Store the response sender — will be consumed when user presses y/n
    *pending_hil_tx.lock().unwrap() = Some(request.response_tx);
}

/// Handle key press while HiL modal is shown.
///
/// Decision keys (y/n/Esc) answer the prompt. All other keys are resolved
/// through the Normal-mode keymap and scroll actions are delegated to the
/// conversation pane, so the user can read the Quorum review feedback
/// behind the modal before deciding (#269).
pub(super) fn handle_hil_key(
    state: &mut TuiState,
    pending_hil_tx: &Arc<Mutex<Option<oneshot::Sender<HumanDecision>>>>,
    key: crossterm::event::KeyEvent,
) {
    use super::mode::{self, InputMode, KeyAction};
    use crossterm::event::KeyCode;
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            state.pending_key = None;
            state.hil_prompt = None;
            state.set_flash("Plan approved");
            send_hil_response(pending_hil_tx, HumanDecision::Approve);
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            state.pending_key = None;
            state.hil_prompt = None;
            state.set_flash("Plan rejected");
            send_hil_response(pending_hil_tx, HumanDecision::Reject);
        }
        _ => {
            // Delegate scroll keys (j/k/gg/G/arrows) to the conversation pane.
            let action = mode::handle_key_event(InputMode::Normal, key, state.pending_key);
            if let KeyAction::PendingKey(c) = action {
                state.pending_key = Some(c);
                return;
            }
            state.pending_key = None;
            match action {
                KeyAction::ScrollUp => state.scroll_up(),
                KeyAction::ScrollDown => state.scroll_down(),
                KeyAction::ScrollToTop => state.scroll_to_top(),
                KeyAction::ScrollToBottom => state.scroll_to_bottom(),
                _ => {}
            }
        }
    }
}

/// Send the stored HiL response (consumes the oneshot sender).
fn send_hil_response(
    pending_hil_tx: &Arc<Mutex<Option<oneshot::Sender<HumanDecision>>>>,
    decision: HumanDecision,
) {
    if let Some(tx) = pending_hil_tx.lock().unwrap().take() {
        let _ = tx.send(decision);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn state_with_modal() -> TuiState {
        let mut state = TuiState::new();
        state.hil_prompt = Some(HilPrompt {
            title: "Plan Requires Human Intervention".into(),
            objective: "Test objective".into(),
            tasks: vec!["task 1".into()],
            message: "Approve or reject?".into(),
        });
        state
    }

    fn hil_tx() -> Arc<Mutex<Option<oneshot::Sender<HumanDecision>>>> {
        let (tx, _rx) = oneshot::channel();
        Arc::new(Mutex::new(Some(tx)))
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn scroll_keys_delegate_to_conversation_while_modal_shown() {
        let mut state = state_with_modal();
        let tx = hil_tx();

        handle_hil_key(&mut state, &tx, key(KeyCode::Char('k')));
        assert_eq!(state.tabs.active_pane().conversation.scroll_offset, 1);
        handle_hil_key(&mut state, &tx, key(KeyCode::Char('k')));
        assert_eq!(state.tabs.active_pane().conversation.scroll_offset, 2);
        handle_hil_key(&mut state, &tx, key(KeyCode::Char('j')));
        assert_eq!(state.tabs.active_pane().conversation.scroll_offset, 1);

        // Arrow keys work too
        handle_hil_key(&mut state, &tx, key(KeyCode::Up));
        assert_eq!(state.tabs.active_pane().conversation.scroll_offset, 2);
        handle_hil_key(&mut state, &tx, key(KeyCode::Down));
        assert_eq!(state.tabs.active_pane().conversation.scroll_offset, 1);

        // Modal stays open, no decision consumed
        assert!(state.hil_prompt.is_some());
        assert!(tx.lock().unwrap().is_some());
    }

    #[test]
    fn gg_scrolls_to_top_and_g_scrolls_to_bottom() {
        let mut state = state_with_modal();
        let tx = hil_tx();

        // gg → scroll to top (via pending key)
        handle_hil_key(&mut state, &tx, key(KeyCode::Char('g')));
        assert_eq!(state.pending_key, Some('g'));
        assert!(state.hil_prompt.is_some());
        handle_hil_key(&mut state, &tx, key(KeyCode::Char('g')));
        assert_eq!(state.pending_key, None);
        assert_eq!(
            state.tabs.active_pane().conversation.scroll_offset,
            usize::MAX
        );

        // G → scroll to bottom
        handle_hil_key(&mut state, &tx, key(KeyCode::Char('G')));
        assert_eq!(state.tabs.active_pane().conversation.scroll_offset, 0);

        assert!(state.hil_prompt.is_some());
        assert!(tx.lock().unwrap().is_some());
    }

    #[test]
    fn decision_keys_still_answer_the_prompt() {
        let mut state = state_with_modal();
        let (tx, rx) = oneshot::channel();
        let tx = Arc::new(Mutex::new(Some(tx)));

        handle_hil_key(&mut state, &tx, key(KeyCode::Char('y')));
        assert!(state.hil_prompt.is_none());
        assert!(tx.lock().unwrap().is_none());
        assert!(matches!(rx.blocking_recv(), Ok(HumanDecision::Approve)));
    }

    #[test]
    fn decision_key_clears_stale_pending_key() {
        let mut state = state_with_modal();
        let tx = hil_tx();

        handle_hil_key(&mut state, &tx, key(KeyCode::Char('g')));
        assert_eq!(state.pending_key, Some('g'));
        handle_hil_key(&mut state, &tx, key(KeyCode::Char('n')));
        assert_eq!(state.pending_key, None);
        assert!(state.hil_prompt.is_none());
    }

    #[test]
    fn non_scroll_keys_do_not_leak_into_other_actions() {
        let mut state = state_with_modal();
        let tx = hil_tx();

        // `i` would enter Insert mode in Normal mode — must be ignored here
        let mode_before = state.mode;
        handle_hil_key(&mut state, &tx, key(KeyCode::Char('i')));
        assert_eq!(state.mode, mode_before);
        assert!(state.hil_prompt.is_some());
        assert!(tx.lock().unwrap().is_some());
    }
}
