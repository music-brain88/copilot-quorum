//! Consensus level definitions for the Quorum system.
//!
//! Defines [`ConsensusLevel`] which determines the level of multi-model consensus:
//! - Solo: Single primary model, consensus only when needed
//! - Ensemble: Multiple models always participate in discussion and consensus
//!
//! Inspired by Cassandra's ConsistencyLevel:
//! - `ConsistencyLevel::ONE` → `ConsensusLevel::Solo`
//! - `ConsistencyLevel::QUORUM` → `ConsensusLevel::Ensemble`

use serde::{Deserialize, Serialize};
use std::fmt;

/// Consensus level for the Quorum system — the single user-facing mode axis.
///
/// # Solo vs Ensemble
///
/// - **Solo** (default): Single model driven, quick execution.
///   Uses `/discuss` for ad-hoc multi-model consultation.
///   Quorum Consensus is used only for plan/action review.
///
/// - **Ensemble**: Multi-model driven for all decisions.
///   Multiple models generate plans independently and vote.
///   Inspired by ML ensemble learning for improved accuracy.
///
/// # Cassandra Analogy
///
/// Just as Cassandra's ConsistencyLevel controls how many replicas must
/// acknowledge a read/write, ConsensusLevel controls how many models
/// participate in decision-making.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ConsensusLevel {
    /// Solo: Single model driven, consensus only when needed
    /// (Plan → Review → Execute)
    #[default]
    Solo,
    /// Ensemble: Multi-model driven, always Discussion + Consensus
    Ensemble,
}

impl ConsensusLevel {
    /// Derive the planning approach from this consensus level.
    ///
    /// - Solo → Single (one model creates the plan)
    /// - Ensemble → Ensemble (multiple models create plans + vote)
    pub fn planning_approach(&self) -> PlanningApproach {
        match self {
            ConsensusLevel::Solo => PlanningApproach::Single,
            ConsensusLevel::Ensemble => PlanningApproach::Ensemble,
        }
    }

    /// Get a human-readable description of this level
    pub fn description(&self) -> &'static str {
        match self {
            ConsensusLevel::Solo => {
                "Solo: Single model driven (Plan → Review → Execute)"
            }
            ConsensusLevel::Ensemble => {
                "Ensemble: Multi-model ensemble planning + voting"
            }
        }
    }

    /// Get a short description for display
    pub fn short_description(&self) -> &'static str {
        match self {
            ConsensusLevel::Solo => "Solo mode",
            ConsensusLevel::Ensemble => "Ensemble mode",
        }
    }

    /// Check if this is ensemble level
    pub fn is_ensemble(&self) -> bool {
        matches!(self, ConsensusLevel::Ensemble)
    }
}

impl fmt::Display for ConsensusLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConsensusLevel::Solo => write!(f, "solo"),
            ConsensusLevel::Ensemble => write!(f, "ensemble"),
        }
    }
}

impl std::str::FromStr for ConsensusLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "solo" | "s" => Ok(ConsensusLevel::Solo),
            "ensemble" | "ens" | "e" => Ok(ConsensusLevel::Ensemble),
            _ => Err(format!("Invalid ConsensusLevel: {}", s)),
        }
    }
}

/// Planning approach derived from [`ConsensusLevel`].
///
/// This is not user-facing — it is automatically determined by the consensus level.
///
/// - `Single`: One model (decision_model) creates the plan
/// - `Ensemble`: Multiple models (review_models) create plans independently, then vote
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlanningApproach {
    /// Single model generates the plan (derived from Solo)
    #[default]
    Single,
    /// Multiple models generate plans independently, then vote (derived from Ensemble)
    Ensemble,
}

impl PlanningApproach {
    /// Check if this is ensemble planning
    pub fn is_ensemble(&self) -> bool {
        matches!(self, PlanningApproach::Ensemble)
    }
}

impl fmt::Display for PlanningApproach {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanningApproach::Single => write!(f, "single"),
            PlanningApproach::Ensemble => write!(f, "ensemble"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", ConsensusLevel::Solo), "solo");
        assert_eq!(format!("{}", ConsensusLevel::Ensemble), "ensemble");
    }

    #[test]
    fn test_default() {
        assert_eq!(ConsensusLevel::default(), ConsensusLevel::Solo);
    }

    #[test]
    fn test_from_str() {
        assert_eq!(
            "solo".parse::<ConsensusLevel>().ok(),
            Some(ConsensusLevel::Solo)
        );
        assert_eq!(
            "s".parse::<ConsensusLevel>().ok(),
            Some(ConsensusLevel::Solo)
        );
        assert_eq!(
            "ensemble".parse::<ConsensusLevel>().ok(),
            Some(ConsensusLevel::Ensemble)
        );
        assert_eq!(
            "ens".parse::<ConsensusLevel>().ok(),
            Some(ConsensusLevel::Ensemble)
        );
        assert_eq!(
            "e".parse::<ConsensusLevel>().ok(),
            Some(ConsensusLevel::Ensemble)
        );
        assert!("unknown".parse::<ConsensusLevel>().is_err());
    }

    #[test]
    fn test_is_ensemble() {
        assert!(!ConsensusLevel::Solo.is_ensemble());
        assert!(ConsensusLevel::Ensemble.is_ensemble());
    }

    #[test]
    fn test_planning_approach() {
        assert_eq!(
            ConsensusLevel::Solo.planning_approach(),
            PlanningApproach::Single
        );
        assert_eq!(
            ConsensusLevel::Ensemble.planning_approach(),
            PlanningApproach::Ensemble
        );
    }

    #[test]
    fn test_planning_approach_is_ensemble() {
        assert!(!PlanningApproach::Single.is_ensemble());
        assert!(PlanningApproach::Ensemble.is_ensemble());
    }
}
