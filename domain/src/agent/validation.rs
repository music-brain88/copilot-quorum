//! Configuration combination validation for [`SessionMode`].
//!
//! Validates the orthogonal configuration axes:
//! [`ConsensusLevel`] × [`PhaseScope`] × [`OrchestrationStrategy`]
//!
//! Some combinations are invalid or unsupported. This module detects them
//! and returns structured issues with severity levels.
//!
//! # Examples
//!
//! ```
//! use quorum_domain::SessionMode;
//! use quorum_domain::agent::validation::Severity;
//!
//! let mode = SessionMode::default(); // Solo + Full + Quorum
//! let issues = mode.validate_combination();
//! assert!(issues.is_empty()); // Valid combination
//! ```

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
}

/// A detected issue in the configuration combination.
#[derive(Debug, Clone)]
pub struct ConfigIssue {
    pub severity: Severity,
    pub code: ConfigIssueCode,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::mode::ConsensusLevel;
    use crate::orchestration::scope::PhaseScope;
    use crate::orchestration::session_mode::SessionMode;
    use crate::orchestration::strategy::{DebateConfig, OrchestrationStrategy};

    // ==================== Helper ====================

    fn make_mode(
        level: ConsensusLevel,
        scope: PhaseScope,
        strategy: OrchestrationStrategy,
    ) -> SessionMode {
        SessionMode {
            consensus_level: level,
            phase_scope: scope,
            strategy,
        }
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
            make_mode(ConsensusLevel::Solo, PhaseScope::Full, quorum()).validate_combination();
        assert!(issues.is_empty());
    }

    #[test]
    fn solo_fast_quorum_is_valid() {
        let issues =
            make_mode(ConsensusLevel::Solo, PhaseScope::Fast, quorum()).validate_combination();
        assert!(issues.is_empty());
    }

    #[test]
    fn solo_plan_only_quorum_is_valid() {
        let issues =
            make_mode(ConsensusLevel::Solo, PhaseScope::PlanOnly, quorum()).validate_combination();
        assert!(issues.is_empty());
    }

    #[test]
    fn ensemble_full_quorum_is_valid() {
        let issues =
            make_mode(ConsensusLevel::Ensemble, PhaseScope::Full, quorum()).validate_combination();
        assert!(issues.is_empty());
    }

    // ==================== Ensemble + PlanOnly + Quorum (valid) ====================

    #[test]
    fn ensemble_plan_only_quorum_is_valid() {
        let issues = make_mode(ConsensusLevel::Ensemble, PhaseScope::PlanOnly, quorum())
            .validate_combination();
        assert!(issues.is_empty());
    }

    // ==================== Warning-only combinations ====================

    #[test]
    fn ensemble_fast_quorum_warns() {
        let issues =
            make_mode(ConsensusLevel::Ensemble, PhaseScope::Fast, quorum()).validate_combination();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Warning);
        assert_eq!(issues[0].code, ConfigIssueCode::EnsembleWithFast);
    }

    #[test]
    fn ensemble_full_debate_warns() {
        let issues =
            make_mode(ConsensusLevel::Ensemble, PhaseScope::Full, debate()).validate_combination();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Warning);
        assert_eq!(issues[0].code, ConfigIssueCode::DebateNotImplemented);
    }

    #[test]
    fn ensemble_plan_only_debate_warns() {
        let issues = make_mode(ConsensusLevel::Ensemble, PhaseScope::PlanOnly, debate())
            .validate_combination();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, ConfigIssueCode::DebateNotImplemented);
    }

    #[test]
    fn ensemble_fast_debate_warns_both() {
        let issues =
            make_mode(ConsensusLevel::Ensemble, PhaseScope::Fast, debate()).validate_combination();
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
            make_mode(ConsensusLevel::Solo, PhaseScope::Full, debate()).validate_combination();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Error);
        assert_eq!(issues[0].code, ConfigIssueCode::SoloWithDebate);
    }

    #[test]
    fn solo_fast_debate_is_error() {
        let issues =
            make_mode(ConsensusLevel::Solo, PhaseScope::Fast, debate()).validate_combination();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Error);
        assert_eq!(issues[0].code, ConfigIssueCode::SoloWithDebate);
    }

    #[test]
    fn solo_plan_only_debate_is_error() {
        let issues =
            make_mode(ConsensusLevel::Solo, PhaseScope::PlanOnly, debate()).validate_combination();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Error);
        assert_eq!(issues[0].code, ConfigIssueCode::SoloWithDebate);
    }

    // ==================== has_errors helper ====================

    #[test]
    fn has_errors_returns_true_for_errors() {
        let issues =
            make_mode(ConsensusLevel::Solo, PhaseScope::Full, debate()).validate_combination();
        assert!(SessionMode::has_errors(&issues));
    }

    #[test]
    fn has_errors_returns_false_for_warnings_only() {
        let issues =
            make_mode(ConsensusLevel::Ensemble, PhaseScope::Fast, quorum()).validate_combination();
        assert!(!SessionMode::has_errors(&issues));
    }

    #[test]
    fn has_errors_returns_false_for_empty() {
        let issues: Vec<ConfigIssue> = vec![];
        assert!(!SessionMode::has_errors(&issues));
    }
}
