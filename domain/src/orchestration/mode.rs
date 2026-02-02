use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OrchestrationMode {
    #[default]
    Agent, // Plan -> Review -> Execute (Default)
    Quorum, // Multi-model consensus (/council)
    Fast,   // Single model, no review (High speed)
    Debate, // Model vs Model discussion
    Plan,   // Plan creation only, no execution
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
            "agent" | "a" => Ok(OrchestrationMode::Agent),
            "quorum" | "q" => Ok(OrchestrationMode::Quorum),
            "fast" | "f" => Ok(OrchestrationMode::Fast),
            "debate" | "d" => Ok(OrchestrationMode::Debate),
            "plan" | "p" => Ok(OrchestrationMode::Plan),
            _ => Err(format!("Invalid OrchestrationMode: {}", s)),
        }
    }
}

impl OrchestrationMode {
    pub fn description(&self) -> &'static str {
        match self {
            OrchestrationMode::Agent => "Autonomous task execution (Plan -> Review -> Execute)",
            OrchestrationMode::Quorum => "Multi-model consensus",
            OrchestrationMode::Fast => "Single model immediate response (No review)",
            OrchestrationMode::Debate => "Inter-model discussion",
            OrchestrationMode::Plan => "Plan creation only (No execution)",
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
    fn test_from_str() {
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
}
