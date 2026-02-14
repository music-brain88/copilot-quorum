//! Session mode — runtime-mutable orchestration settings.
//!
//! [`SessionMode`] groups the runtime-mutable axes that TUI commands
//! (`/solo`, `/ens`, `/fast`, `/strategy`) can toggle during a session.

use crate::agent::validation::{ConfigIssue, ConfigIssueCode, Severity};
use crate::orchestration::mode::{ConsensusLevel, PlanningApproach};
use crate::orchestration::scope::PhaseScope;
use crate::orchestration::strategy::OrchestrationStrategy;
use serde::{Deserialize, Serialize};

/// Runtime-mutable orchestration mode.
///
/// Groups the three orthogonal axes that can change during a TUI session:
/// - `consensus_level` — Solo or Ensemble (toggled via `/solo`, `/ens`)
/// - `phase_scope` — Full, Fast, or PlanOnly (toggled via `/fast`, `/scope`)
/// - `strategy` — Quorum or Debate (toggled via `/strategy`)
///
/// # Design Note: Flat Structure
///
/// A `ConsensusMode::Solo` / `Ensemble { strategy }` enum would prevent the
/// invalid state "Solo + Debate", but the flat structure is intentional:
/// TUI users switching `/solo` → `/ens` expect their previous strategy setting
/// to be preserved. The flat structure makes this natural.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMode {
    pub consensus_level: ConsensusLevel,
    pub phase_scope: PhaseScope,
    pub strategy: OrchestrationStrategy,
}

impl Default for SessionMode {
    fn default() -> Self {
        Self {
            consensus_level: ConsensusLevel::Solo,
            phase_scope: PhaseScope::Full,
            strategy: OrchestrationStrategy::default(),
        }
    }
}

impl SessionMode {
    /// Create a new SessionMode with the given consensus level.
    pub fn new(consensus_level: ConsensusLevel) -> Self {
        Self {
            consensus_level,
            ..Default::default()
        }
    }

    // ==================== Builder Methods ====================

    pub fn with_consensus_level(mut self, level: ConsensusLevel) -> Self {
        self.consensus_level = level;
        self
    }

    pub fn with_phase_scope(mut self, scope: PhaseScope) -> Self {
        self.phase_scope = scope;
        self
    }

    pub fn with_strategy(mut self, strategy: OrchestrationStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    // ==================== PhaseScope Delegation ====================

    /// Whether this mode includes the plan review phase.
    pub fn includes_plan_review(&self) -> bool {
        self.phase_scope.includes_plan_review()
    }

    /// Whether this mode includes the task execution phase.
    pub fn includes_execution(&self) -> bool {
        self.phase_scope.includes_execution()
    }

    /// Whether this mode includes the action review phase.
    pub fn includes_action_review(&self) -> bool {
        self.phase_scope.includes_action_review()
    }

    /// Whether this mode requires explicit execution confirmation.
    pub fn requires_execution_confirmation(&self) -> bool {
        self.phase_scope.requires_execution_confirmation()
    }

    /// Get the planning approach derived from the consensus level.
    pub fn planning_approach(&self) -> PlanningApproach {
        self.consensus_level.planning_approach()
    }

    // ==================== Validation ====================

    /// Validate the combination of `consensus_level × phase_scope × strategy`.
    ///
    /// Returns a list of issues. An empty list means the combination is valid.
    pub fn validate_combination(&self) -> Vec<ConfigIssue> {
        let mut issues = Vec::new();
        let is_debate = matches!(self.strategy, OrchestrationStrategy::Debate(_));

        if is_debate {
            if self.consensus_level == ConsensusLevel::Solo {
                issues.push(ConfigIssue {
                    severity: Severity::Error,
                    code: ConfigIssueCode::SoloWithDebate,
                    message: "Solo mode with Debate strategy is invalid: \
                              a single model cannot debate with itself"
                        .to_string(),
                });
            } else {
                issues.push(ConfigIssue {
                    severity: Severity::Warning,
                    code: ConfigIssueCode::DebateNotImplemented,
                    message: "Debate strategy is not yet implemented \
                              (no StrategyExecutor available)"
                        .to_string(),
                });
            }
        }

        if self.consensus_level == ConsensusLevel::Ensemble && self.phase_scope == PhaseScope::Fast
        {
            issues.push(ConfigIssue {
                severity: Severity::Warning,
                code: ConfigIssueCode::EnsembleWithFast,
                message: "Ensemble mode with Fast scope skips review phases, \
                          reducing the value of multi-model consensus"
                    .to_string(),
            });
        }

        issues
    }

    /// Check whether any issues are errors (i.e. fatal).
    pub fn has_errors(issues: &[ConfigIssue]) -> bool {
        issues.iter().any(|i| i.severity == Severity::Error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        let mode = SessionMode::default();
        assert_eq!(mode.consensus_level, ConsensusLevel::Solo);
        assert_eq!(mode.phase_scope, PhaseScope::Full);
        assert_eq!(mode.strategy, OrchestrationStrategy::default());
    }

    #[test]
    fn test_builder() {
        let mode = SessionMode::default()
            .with_consensus_level(ConsensusLevel::Ensemble)
            .with_phase_scope(PhaseScope::Fast);
        assert_eq!(mode.consensus_level, ConsensusLevel::Ensemble);
        assert_eq!(mode.phase_scope, PhaseScope::Fast);
    }

    #[test]
    fn test_phase_scope_delegation() {
        let full = SessionMode::default().with_phase_scope(PhaseScope::Full);
        assert!(full.includes_plan_review());
        assert!(full.includes_execution());
        assert!(full.includes_action_review());
        assert!(full.requires_execution_confirmation());

        let fast = SessionMode::default().with_phase_scope(PhaseScope::Fast);
        assert!(!fast.includes_plan_review());
        assert!(fast.includes_execution());
        assert!(!fast.includes_action_review());
        assert!(!fast.requires_execution_confirmation());

        let plan_only = SessionMode::default().with_phase_scope(PhaseScope::PlanOnly);
        assert!(!plan_only.includes_execution());
    }

    #[test]
    fn test_planning_approach() {
        let solo = SessionMode::default();
        assert!(!solo.planning_approach().is_ensemble());

        let ensemble = SessionMode::default().with_consensus_level(ConsensusLevel::Ensemble);
        assert!(ensemble.planning_approach().is_ensemble());
    }
}
