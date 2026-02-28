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
pub(super) fn handle_hil_key(
    state: &mut TuiState,
    pending_hil_tx: &Arc<Mutex<Option<oneshot::Sender<HumanDecision>>>>,
    key: crossterm::event::KeyEvent,
) {
    use crossterm::event::KeyCode;
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            state.hil_prompt = None;
            state.set_flash("Plan approved");
            send_hil_response(pending_hil_tx, HumanDecision::Approve);
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            state.hil_prompt = None;
            state.set_flash("Plan rejected");
            send_hil_response(pending_hil_tx, HumanDecision::Reject);
        }
        _ => {}
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
