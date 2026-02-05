use std::fmt;

/// Orchestration mode for the Quorum system
///
/// # Solo vs Ensemble
///
/// The primary mode distinction is between Solo and Ensemble:
///
/// - **Solo** (Agent): Single model driven, quick execution
///   - Uses `/discuss` for ad-hoc multi-model consultation
///   - Default mode for simple tasks
///
/// - **Ensemble** (Quorum): Multi-model driven for all decisions
///   - Inspired by ML ensemble learning
///   - Combines perspectives for improved accuracy
///   - Best for complex design and architecture decisions
///
/// # Mode Aliases
///
/// - `solo` = `agent` (single model, autonomous task execution)
/// - `ensemble` = `quorum` (multi-model discussion)
/// - `ens` = `ensemble` (short form)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OrchestrationMode {
    /// Solo mode: Single model driven, autonomous task execution
    /// (Plan -> Review -> Execute)
    #[default]
    Agent,
    /// Ensemble mode: Multi-model consensus for all decisions
    Quorum,
    /// Fast mode: Single model, no review (high speed)
    Fast,
    /// Debate mode: Inter-model discussion
    Debate,
    /// Plan mode: Plan creation only, no execution
    Plan,
}

impl OrchestrationMode {
    /// Check if this is a solo-style mode (single primary model)
    pub fn is_solo(&self) -> bool {
        matches!(self, OrchestrationMode::Agent | OrchestrationMode::Fast)
    }

    /// Check if this is an ensemble-style mode (multiple models)
    pub fn is_ensemble(&self) -> bool {
        matches!(self, OrchestrationMode::Quorum | OrchestrationMode::Debate)
    }
}

impl fmt::Display for OrchestrationMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrchestrationMode::Agent => write!(f, "agent"),
            OrchestrationMode::Quorum => write!(f, "quorum"),
            OrchestrationMode::Fast => write!(f, "fast"),
            OrchestrationMode::Debate => write!(f, "debate"),
            OrchestrationMode::Plan => write!(f, "plan"),
        }
    }
}

impl std::str::FromStr for OrchestrationMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            // Solo modes
            "agent" | "a" | "solo" | "s" => Ok(OrchestrationMode::Agent),
            "fast" | "f" => Ok(OrchestrationMode::Fast),
            // Ensemble modes
            "quorum" | "q" | "ensemble" | "ens" | "e" => Ok(OrchestrationMode::Quorum),
            "debate" | "d" => Ok(OrchestrationMode::Debate),
            // Other
            "plan" | "p" => Ok(OrchestrationMode::Plan),
            _ => Err(format!("Invalid OrchestrationMode: {}", s)),
        }
    }
}

impl OrchestrationMode {
    /// Get a human-readable description of this mode
    pub fn description(&self) -> &'static str {
        match self {
            OrchestrationMode::Agent => {
                "Solo: Autonomous task execution (Plan -> Review -> Execute)"
            }
            OrchestrationMode::Quorum => "Ensemble: Multi-model Quorum Discussion",
            OrchestrationMode::Fast => "Solo: Single model immediate response (No review)",
            OrchestrationMode::Debate => "Ensemble: Inter-model debate discussion",
            OrchestrationMode::Plan => "Plan creation only (No execution)",
        }
    }

    /// Get a short description for display
    pub fn short_description(&self) -> &'static str {
        match self {
            OrchestrationMode::Agent => "Solo mode",
            OrchestrationMode::Quorum => "Ensemble mode",
            OrchestrationMode::Fast => "Fast mode",
            OrchestrationMode::Debate => "Debate mode",
            OrchestrationMode::Plan => "Plan mode",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", OrchestrationMode::Agent), "agent");
        assert_eq!(format!("{}", OrchestrationMode::Quorum), "quorum");
    }

    #[test]
    fn test_from_str_legacy() {
        assert_eq!(
            "agent".parse::<OrchestrationMode>().ok(),
            Some(OrchestrationMode::Agent)
        );
        assert_eq!(
            "a".parse::<OrchestrationMode>().ok(),
            Some(OrchestrationMode::Agent)
        );
        assert_eq!(
            "Quorum".parse::<OrchestrationMode>().ok(),
            Some(OrchestrationMode::Quorum)
        );
        assert!("unknown".parse::<OrchestrationMode>().is_err());
    }

    #[test]
    fn test_from_str_solo_ensemble() {
        // Solo aliases
        assert_eq!(
            "solo".parse::<OrchestrationMode>().ok(),
            Some(OrchestrationMode::Agent)
        );
        assert_eq!(
            "s".parse::<OrchestrationMode>().ok(),
            Some(OrchestrationMode::Agent)
        );

        // Ensemble aliases
        assert_eq!(
            "ensemble".parse::<OrchestrationMode>().ok(),
            Some(OrchestrationMode::Quorum)
        );
        assert_eq!(
            "ens".parse::<OrchestrationMode>().ok(),
            Some(OrchestrationMode::Quorum)
        );
        assert_eq!(
            "e".parse::<OrchestrationMode>().ok(),
            Some(OrchestrationMode::Quorum)
        );
    }

    #[test]
    fn test_is_solo_ensemble() {
        assert!(OrchestrationMode::Agent.is_solo());
        assert!(OrchestrationMode::Fast.is_solo());
        assert!(!OrchestrationMode::Quorum.is_solo());

        assert!(OrchestrationMode::Quorum.is_ensemble());
        assert!(OrchestrationMode::Debate.is_ensemble());
        assert!(!OrchestrationMode::Agent.is_ensemble());
    }
}
