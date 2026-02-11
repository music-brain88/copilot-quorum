//! Configuration combination validation for AgentConfig.
//!
//! Validates the orthogonal configuration axes:
//! [`ConsensusLevel`] × [`PhaseScope`] × [`OrchestrationStrategy`]
//! × [`InteractionType`] × [`ContextMode`]
//!
//! Some combinations are invalid or unsupported. This module detects them
//! and returns structured issues with severity levels.
//!
//! # Examples
//!
//! ```
//! use quorum_domain::agent::{AgentConfig, ConfigIssue};
//! use quorum_domain::agent::validation::Severity;
//!
//! let config = AgentConfig::default(); // Solo + Full + Quorum + Ask + Shared
//! let issues = config.validate_combination();
//! assert!(issues.is_empty()); // Valid combination
//! ```

use super::entities::AgentConfig;
use crate::orchestration::interaction::InteractionType;
use crate::orchestration::mode::ConsensusLevel;
use crate::orchestration::scope::PhaseScope;
use crate::orchestration::strategy::OrchestrationStrategy;

/// Severity level of a configuration issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Fatal: the configuration cannot work at all.
    Error,
    /// Non-fatal: the configuration works but may not behave as expected.
    Warning,
}

/// Identifies a specific configuration issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigIssueCode {
    /// Solo + Debate: a single model cannot debate with itself.
    SoloWithDebate,
    /// Debate strategy has no StrategyExecutor implementation yet.
    DebateNotImplemented,
    /// Ensemble + Fast: review phases are skipped, reducing Ensemble's value.
    EnsembleWithFast,
    /// Solo + Ask with non-default orchestration: orchestration has no effect.
    AskWithOrchestration,
}

/// A detected issue in the AgentConfig combination.
#[derive(Debug, Clone)]
pub struct ConfigIssue {
    pub severity: Severity,
    pub code: ConfigIssueCode,
    pub message: String,
}

impl AgentConfig {
    /// Validate the combination of `consensus_level` × `phase_scope` × `orchestration_strategy`.
    ///
    /// Returns a list of issues. An empty list means the combination is valid.
    pub fn validate_combination(&self) -> Vec<ConfigIssue> {
        let mut issues = Vec::new();
        let is_debate = matches!(
            self.orchestration_strategy,
            OrchestrationStrategy::Debate(_)
        );

        if is_debate {
            if self.consensus_level == ConsensusLevel::Solo {
                // Solo + Debate is an error (debate with one model is impossible).
                // Don't also emit DebateNotImplemented — the error subsumes it.
                issues.push(ConfigIssue {
                    severity: Severity::Error,
                    code: ConfigIssueCode::SoloWithDebate,
                    message: "Solo mode with Debate strategy is invalid: \
                              a single model cannot debate with itself"
                        .to_string(),
                });
            } else {
                // Ensemble + Debate: warn that it's not implemented yet
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

        // Solo + Ask + non-default orchestration: orchestration has no effect
        if self.consensus_level == ConsensusLevel::Solo
            && self.interaction_type == InteractionType::Ask
            && is_debate
        {
            // Solo + Ask + Debate is already caught by SoloWithDebate (Error).
            // Only warn if not already an error for the same reason.
            if !issues
                .iter()
                .any(|i| i.code == ConfigIssueCode::SoloWithDebate)
            {
                issues.push(ConfigIssue {
                    severity: Severity::Warning,
                    code: ConfigIssueCode::AskWithOrchestration,
                    message: "Solo + Ask mode does not use orchestration; \
                              the orchestration strategy setting has no effect"
                        .to_string(),
                });
            }
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
    use crate::orchestration::interaction::{ContextMode, InteractionType};
    use crate::orchestration::strategy::{DebateConfig, OrchestrationStrategy};

    // ==================== Helper ====================

    fn make_config(
        level: ConsensusLevel,
        scope: PhaseScope,
        strategy: OrchestrationStrategy,
    ) -> AgentConfig {
        AgentConfig::default()
            .with_consensus_level(level)
            .with_phase_scope(scope)
            .with_orchestration_strategy(strategy)
    }

    fn make_full_config(
        level: ConsensusLevel,
        scope: PhaseScope,
        strategy: OrchestrationStrategy,
        interaction: InteractionType,
        context: ContextMode,
    ) -> AgentConfig {
        make_config(level, scope, strategy)
            .with_interaction_type(interaction)
            .with_context_mode(context)
    }

    fn quorum() -> OrchestrationStrategy {
        OrchestrationStrategy::default()
    }

    fn debate() -> OrchestrationStrategy {
        OrchestrationStrategy::Debate(DebateConfig::default())
    }

    // ==================== Valid combinations (0 issues) ====================

    #[test]
    fn solo_full_quorum_is_valid() {
        let issues =
            make_config(ConsensusLevel::Solo, PhaseScope::Full, quorum()).validate_combination();
        assert!(issues.is_empty());
    }

    #[test]
    fn solo_fast_quorum_is_valid() {
        let issues =
            make_config(ConsensusLevel::Solo, PhaseScope::Fast, quorum()).validate_combination();
        assert!(issues.is_empty());
    }

    #[test]
    fn solo_plan_only_quorum_is_valid() {
        let issues = make_config(ConsensusLevel::Solo, PhaseScope::PlanOnly, quorum())
            .validate_combination();
        assert!(issues.is_empty());
    }

    #[test]
    fn ensemble_full_quorum_is_valid() {
        let issues = make_config(ConsensusLevel::Ensemble, PhaseScope::Full, quorum())
            .validate_combination();
        assert!(issues.is_empty());
    }

    // ==================== Ensemble + PlanOnly + Quorum (valid) ====================

    #[test]
    fn ensemble_plan_only_quorum_is_valid() {
        let issues = make_config(ConsensusLevel::Ensemble, PhaseScope::PlanOnly, quorum())
            .validate_combination();
        assert!(issues.is_empty());
    }

    // ==================== Warning-only combinations ====================

    #[test]
    fn ensemble_fast_quorum_warns() {
        let issues = make_config(ConsensusLevel::Ensemble, PhaseScope::Fast, quorum())
            .validate_combination();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Warning);
        assert_eq!(issues[0].code, ConfigIssueCode::EnsembleWithFast);
    }

    #[test]
    fn ensemble_full_debate_warns() {
        let issues = make_config(ConsensusLevel::Ensemble, PhaseScope::Full, debate())
            .validate_combination();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Warning);
        assert_eq!(issues[0].code, ConfigIssueCode::DebateNotImplemented);
    }

    #[test]
    fn ensemble_plan_only_debate_warns() {
        let issues = make_config(ConsensusLevel::Ensemble, PhaseScope::PlanOnly, debate())
            .validate_combination();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, ConfigIssueCode::DebateNotImplemented);
    }

    #[test]
    fn ensemble_fast_debate_warns_both() {
        let issues = make_config(ConsensusLevel::Ensemble, PhaseScope::Fast, debate())
            .validate_combination();
        assert_eq!(issues.len(), 2);
        let codes: Vec<_> = issues.iter().map(|i| i.code).collect();
        assert!(codes.contains(&ConfigIssueCode::DebateNotImplemented));
        assert!(codes.contains(&ConfigIssueCode::EnsembleWithFast));
        // Both are warnings
        assert!(issues.iter().all(|i| i.severity == Severity::Warning));
    }

    // ==================== Error combinations (Solo + Debate) ====================

    #[test]
    fn solo_full_debate_is_error() {
        let issues =
            make_config(ConsensusLevel::Solo, PhaseScope::Full, debate()).validate_combination();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Error);
        assert_eq!(issues[0].code, ConfigIssueCode::SoloWithDebate);
    }

    #[test]
    fn solo_fast_debate_is_error() {
        let issues =
            make_config(ConsensusLevel::Solo, PhaseScope::Fast, debate()).validate_combination();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Error);
        assert_eq!(issues[0].code, ConfigIssueCode::SoloWithDebate);
    }

    #[test]
    fn solo_plan_only_debate_is_error() {
        let issues = make_config(ConsensusLevel::Solo, PhaseScope::PlanOnly, debate())
            .validate_combination();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Error);
        assert_eq!(issues[0].code, ConfigIssueCode::SoloWithDebate);
    }

    // ==================== has_errors helper ====================

    #[test]
    fn has_errors_returns_true_for_errors() {
        let issues =
            make_config(ConsensusLevel::Solo, PhaseScope::Full, debate()).validate_combination();
        assert!(AgentConfig::has_errors(&issues));
    }

    #[test]
    fn has_errors_returns_false_for_warnings_only() {
        let issues = make_config(ConsensusLevel::Ensemble, PhaseScope::Fast, quorum())
            .validate_combination();
        assert!(!AgentConfig::has_errors(&issues));
    }

    #[test]
    fn has_errors_returns_false_for_empty() {
        let issues: Vec<ConfigIssue> = vec![];
        assert!(!AgentConfig::has_errors(&issues));
    }

    // ==================== InteractionType + ContextMode combinations ====================

    #[test]
    fn solo_ask_quorum_is_valid() {
        let issues = make_full_config(
            ConsensusLevel::Solo,
            PhaseScope::Full,
            quorum(),
            InteractionType::Ask,
            ContextMode::Shared,
        )
        .validate_combination();
        assert!(issues.is_empty());
    }

    #[test]
    fn solo_discuss_quorum_is_valid() {
        let issues = make_full_config(
            ConsensusLevel::Solo,
            PhaseScope::Full,
            quorum(),
            InteractionType::Discuss,
            ContextMode::Shared,
        )
        .validate_combination();
        assert!(issues.is_empty());
    }

    #[test]
    fn ensemble_ask_quorum_is_valid() {
        let issues = make_full_config(
            ConsensusLevel::Ensemble,
            PhaseScope::Full,
            quorum(),
            InteractionType::Ask,
            ContextMode::Shared,
        )
        .validate_combination();
        assert!(issues.is_empty());
    }

    #[test]
    fn ensemble_discuss_quorum_is_valid() {
        let issues = make_full_config(
            ConsensusLevel::Ensemble,
            PhaseScope::Full,
            quorum(),
            InteractionType::Discuss,
            ContextMode::Shared,
        )
        .validate_combination();
        assert!(issues.is_empty());
    }

    #[test]
    fn context_mode_does_not_affect_validation() {
        // Fresh context should not trigger any issues on its own
        let issues = make_full_config(
            ConsensusLevel::Solo,
            PhaseScope::Full,
            quorum(),
            InteractionType::Ask,
            ContextMode::Fresh,
        )
        .validate_combination();
        assert!(issues.is_empty());
    }

    #[test]
    fn solo_ask_debate_is_error_not_warning() {
        // Solo + Ask + Debate should be Error (SoloWithDebate), not AskWithOrchestration
        let issues = make_full_config(
            ConsensusLevel::Solo,
            PhaseScope::Full,
            debate(),
            InteractionType::Ask,
            ContextMode::Shared,
        )
        .validate_combination();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Error);
        assert_eq!(issues[0].code, ConfigIssueCode::SoloWithDebate);
    }

    #[test]
    fn default_config_includes_new_axes() {
        let config = AgentConfig::default();
        assert_eq!(config.interaction_type, InteractionType::Ask);
        assert_eq!(config.context_mode, ContextMode::Shared);
    }
}
