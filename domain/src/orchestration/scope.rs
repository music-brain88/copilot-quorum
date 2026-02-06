//! Phase scope definitions for the Quorum system.
//!
//! [`PhaseScope`] is an orthogonal option to [`ConsensusLevel`] that controls
//! which phases of execution are included in a run.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Phase scope — controls which execution phases are included.
///
/// This is orthogonal to [`ConsensusLevel`]: you can combine any scope with
/// any consensus level. For example, `Solo + Fast` skips reviews for speed,
/// while `Ensemble + PlanOnly` generates multi-model plans without executing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PhaseScope {
    /// Full execution: all phases included (default)
    #[default]
    Full,
    /// Fast execution: skip review phases
    Fast,
    /// Plan only: generate plan but don't execute
    PlanOnly,
}

impl PhaseScope {
    /// Whether this scope includes the plan review phase
    pub fn includes_plan_review(&self) -> bool {
        matches!(self, PhaseScope::Full)
    }

    /// Whether this scope includes the task execution phase
    pub fn includes_execution(&self) -> bool {
        matches!(self, PhaseScope::Full | PhaseScope::Fast)
    }

    /// Whether this scope includes the action review phase
    pub fn includes_action_review(&self) -> bool {
        matches!(self, PhaseScope::Full)
    }
}

impl fmt::Display for PhaseScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PhaseScope::Full => write!(f, "full"),
            PhaseScope::Fast => write!(f, "fast"),
            PhaseScope::PlanOnly => write!(f, "plan-only"),
        }
    }
}

impl std::str::FromStr for PhaseScope {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "full" => Ok(PhaseScope::Full),
            "fast" => Ok(PhaseScope::Fast),
            "plan-only" | "planonly" | "plan" => Ok(PhaseScope::PlanOnly),
            _ => Err(format!("Invalid PhaseScope: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        assert_eq!(PhaseScope::default(), PhaseScope::Full);
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", PhaseScope::Full), "full");
        assert_eq!(format!("{}", PhaseScope::Fast), "fast");
        assert_eq!(format!("{}", PhaseScope::PlanOnly), "plan-only");
    }

    #[test]
    fn test_from_str() {
        assert_eq!(
            "full".parse::<PhaseScope>().ok(),
            Some(PhaseScope::Full)
        );
        assert_eq!(
            "fast".parse::<PhaseScope>().ok(),
            Some(PhaseScope::Fast)
        );
        assert_eq!(
            "plan-only".parse::<PhaseScope>().ok(),
            Some(PhaseScope::PlanOnly)
        );
        assert_eq!(
            "plan".parse::<PhaseScope>().ok(),
            Some(PhaseScope::PlanOnly)
        );
        assert!("invalid".parse::<PhaseScope>().is_err());
    }

    #[test]
    fn test_includes_plan_review() {
        assert!(PhaseScope::Full.includes_plan_review());
        assert!(!PhaseScope::Fast.includes_plan_review());
        assert!(!PhaseScope::PlanOnly.includes_plan_review());
    }

    #[test]
    fn test_includes_execution() {
        assert!(PhaseScope::Full.includes_execution());
        assert!(PhaseScope::Fast.includes_execution());
        assert!(!PhaseScope::PlanOnly.includes_execution());
    }

    #[test]
    fn test_includes_action_review() {
        assert!(PhaseScope::Full.includes_action_review());
        assert!(!PhaseScope::Fast.includes_action_review());
        assert!(!PhaseScope::PlanOnly.includes_action_review());
    }
}
