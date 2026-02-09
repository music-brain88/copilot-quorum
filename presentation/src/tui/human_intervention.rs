//! TUI Human Intervention — HumanInterventionPort via oneshot channels
//!
//! Instead of blocking on stdin, sends an HilRequest through a channel
//! to the TUI event loop, which shows a modal and sends back the decision.

use super::event::{HilKind, HilRequest};
use async_trait::async_trait;
use quorum_application::ports::human_intervention::{
    HumanInterventionError, HumanInterventionPort,
};
use quorum_domain::{HumanDecision, Plan, ReviewRound};
use tokio::sync::{mpsc, oneshot};

/// Channel-based HumanInterventionPort for TUI
///
/// Sends intervention requests to the TUI main loop via mpsc,
/// then awaits the decision on a oneshot channel.
pub struct TuiHumanIntervention {
    hil_tx: mpsc::UnboundedSender<HilRequest>,
}

impl TuiHumanIntervention {
    pub fn new(hil_tx: mpsc::UnboundedSender<HilRequest>) -> Self {
        Self { hil_tx }
    }
}

#[async_trait]
impl HumanInterventionPort for TuiHumanIntervention {
    async fn request_intervention(
        &self,
        request: &str,
        plan: &Plan,
        review_history: &[ReviewRound],
    ) -> Result<HumanDecision, HumanInterventionError> {
        let (response_tx, response_rx) = oneshot::channel();

        let hil_request = HilRequest {
            kind: HilKind::PlanIntervention {
                request: request.to_string(),
                plan: plan.clone(),
                review_history: review_history.to_vec(),
            },
            response_tx,
        };

        self.hil_tx
            .send(hil_request)
            .map_err(|_| HumanInterventionError::IoError("TUI channel closed".to_string()))?;

        response_rx.await.map_err(|_| {
            HumanInterventionError::IoError("TUI response channel dropped".to_string())
        })
    }

    async fn request_execution_confirmation(
        &self,
        request: &str,
        plan: &Plan,
    ) -> Result<HumanDecision, HumanInterventionError> {
        let (response_tx, response_rx) = oneshot::channel();

        let hil_request = HilRequest {
            kind: HilKind::ExecutionConfirmation {
                request: request.to_string(),
                plan: plan.clone(),
            },
            response_tx,
        };

        self.hil_tx
            .send(hil_request)
            .map_err(|_| HumanInterventionError::IoError("TUI channel closed".to_string()))?;

        response_rx.await.map_err(|_| {
            HumanInterventionError::IoError("TUI response channel dropped".to_string())
        })
    }
}
