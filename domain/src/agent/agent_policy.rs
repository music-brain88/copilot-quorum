//! Agent policy — domain-level behavioral constraints.
//!
//! [`AgentPolicy`] captures the static rules that govern agent behavior,
//! particularly around Human-in-the-Loop (HiL) intervention.
//! These are domain policies that constrain [`AgentState`](super::entities::AgentState)
//! transitions.

use super::entities::HilMode;
use serde::{Deserialize, Serialize};

/// Action determined by HiL policy evaluation.
///
/// Returned by [`AgentPolicy::hil_action`] when the plan revision limit is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HilAction {
    /// Not yet at the revision limit — continue normally.
    Continue,
    /// Interactive mode: request human intervention via port.
    RequestIntervention,
    /// Auto-reject mode: abort the agent.
    Abort,
    /// Auto-approve mode: force-approve the last plan.
    ForceApprove,
}

/// Agent behavioral policy — static domain constraints.
///
/// These settings constrain the agent's state machine transitions and
/// determine when/how human intervention is triggered.
///
/// Only the Agent buffer needs this; Ask and Discuss buffers don't have
/// plan revision or HiL logic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPolicy {
    /// Human-in-the-loop mode for handling plan revision limits.
    pub hil_mode: HilMode,
    /// Whether to require plan review (always true by design, but explicit).
    pub require_plan_review: bool,
    /// Whether to require final review after execution.
    pub require_final_review: bool,
    /// Maximum number of plan revisions before HiL triggers.
    pub max_plan_revisions: usize,
}

impl Default for AgentPolicy {
    fn default() -> Self {
        Self {
            hil_mode: HilMode::Interactive,
            require_plan_review: true,
            require_final_review: false,
            max_plan_revisions: 3,
        }
    }
}

impl AgentPolicy {
    // ==================== Builder Methods ====================

    pub fn with_hil_mode(mut self, mode: HilMode) -> Self {
        self.hil_mode = mode;
        self
    }

    pub fn with_require_plan_review(mut self, require: bool) -> Self {
        self.require_plan_review = require;
        self
    }

    pub fn with_require_final_review(mut self, require: bool) -> Self {
        self.require_final_review = require;
        self
    }

    pub fn with_max_plan_revisions(mut self, max: usize) -> Self {
        self.max_plan_revisions = max;
        self
    }

    /// Determine the HiL action given the current plan revision count.
    ///
    /// This encodes the domain rule: "if revision count >= limit, act based on hil_mode".
    pub fn hil_action(&self, plan_revision_count: usize) -> HilAction {
        if plan_revision_count < self.max_plan_revisions {
            return HilAction::Continue;
        }
        match self.hil_mode {
            HilMode::Interactive => HilAction::RequestIntervention,
            HilMode::AutoReject => HilAction::Abort,
            HilMode::AutoApprove => HilAction::ForceApprove,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        let policy = AgentPolicy::default();
        assert_eq!(policy.hil_mode, HilMode::Interactive);
        assert!(policy.require_plan_review);
        assert!(!policy.require_final_review);
        assert_eq!(policy.max_plan_revisions, 3);
    }

    #[test]
    fn test_builder() {
        let policy = AgentPolicy::default()
            .with_hil_mode(HilMode::AutoReject)
            .with_max_plan_revisions(5)
            .with_require_final_review(true);

        assert_eq!(policy.hil_mode, HilMode::AutoReject);
        assert_eq!(policy.max_plan_revisions, 5);
        assert!(policy.require_final_review);
    }

    #[test]
    fn test_hil_action_continue() {
        let policy = AgentPolicy::default(); // max_plan_revisions = 3
        assert_eq!(policy.hil_action(0), HilAction::Continue);
        assert_eq!(policy.hil_action(1), HilAction::Continue);
        assert_eq!(policy.hil_action(2), HilAction::Continue);
    }

    #[test]
    fn test_hil_action_interactive() {
        let policy = AgentPolicy::default().with_hil_mode(HilMode::Interactive);
        assert_eq!(policy.hil_action(3), HilAction::RequestIntervention);
        assert_eq!(policy.hil_action(5), HilAction::RequestIntervention);
    }

    #[test]
    fn test_hil_action_auto_reject() {
        let policy = AgentPolicy::default().with_hil_mode(HilMode::AutoReject);
        assert_eq!(policy.hil_action(3), HilAction::Abort);
    }

    #[test]
    fn test_hil_action_auto_approve() {
        let policy = AgentPolicy::default().with_hil_mode(HilMode::AutoApprove);
        assert_eq!(policy.hil_action(3), HilAction::ForceApprove);
    }
}
