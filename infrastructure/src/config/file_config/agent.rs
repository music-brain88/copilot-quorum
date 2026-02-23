//! Agent configuration from TOML (`[agent]` section)

use quorum_domain::agent::validation::{ConfigIssue, ConfigIssueCode, Severity};
use quorum_domain::{ConsensusLevel, HilMode, PhaseScope};
use serde::{Deserialize, Serialize};

/// Raw agent configuration from TOML
///
/// # Example
///
/// ```toml
/// [agent]
/// consensus_level = "solo"                 # "solo" or "ensemble"
/// phase_scope = "full"                     # "full", "fast", "plan-only"
/// strategy = "quorum"                      # "quorum" or "debate"
/// hil_mode = "interactive"                 # "interactive", "auto_reject", "auto_approve"
/// max_plan_revisions = 3
/// ```
///
/// Model settings are in `[models]` section, not here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileAgentConfig {
    /// Maximum plan revisions before human intervention
    pub max_plan_revisions: usize,
    /// Human-in-the-loop mode (interactive, auto_reject, auto_approve)
    pub hil_mode: String,
    /// Consensus level: "solo" or "ensemble"
    pub consensus_level: String,
    /// Phase scope: "full", "fast", "plan-only"
    pub phase_scope: String,
    /// Orchestration strategy: "quorum" or "debate"
    pub strategy: String,
}

impl Default for FileAgentConfig {
    fn default() -> Self {
        Self {
            max_plan_revisions: 3,
            hil_mode: "interactive".to_string(),
            consensus_level: "solo".to_string(),
            phase_scope: "full".to_string(),
            strategy: "quorum".to_string(),
        }
    }
}

impl FileAgentConfig {
    /// Parse hil_mode string into HilMode enum, returning warnings on failure.
    pub fn parse_hil_mode(&self) -> (HilMode, Vec<ConfigIssue>) {
        match self.hil_mode.parse::<HilMode>() {
            Ok(mode) => (mode, vec![]),
            Err(_) => {
                let issue = ConfigIssue {
                    severity: Severity::Warning,
                    code: ConfigIssueCode::InvalidEnumValue {
                        field: "agent.hil_mode".to_string(),
                        value: self.hil_mode.clone(),
                        valid_values: vec![
                            "interactive".to_string(),
                            "auto_reject".to_string(),
                            "auto_approve".to_string(),
                        ],
                    },
                    message: format!(
                        "agent.hil_mode: unknown value '{}', falling back to 'interactive'",
                        self.hil_mode
                    ),
                };
                (HilMode::default(), vec![issue])
            }
        }
    }

    /// Parse consensus_level string into ConsensusLevel enum
    ///
    /// Accepts: "solo", "s", "ensemble", "ens", "e"
    pub fn parse_consensus_level(&self) -> (ConsensusLevel, Vec<ConfigIssue>) {
        match self.consensus_level.parse::<ConsensusLevel>() {
            Ok(level) => (level, vec![]),
            Err(_) => {
                let issue = ConfigIssue {
                    severity: Severity::Warning,
                    code: ConfigIssueCode::InvalidEnumValue {
                        field: "agent.consensus_level".to_string(),
                        value: self.consensus_level.clone(),
                        valid_values: vec!["solo".to_string(), "ensemble".to_string()],
                    },
                    message: format!(
                        "agent.consensus_level: unknown value '{}', falling back to 'solo'",
                        self.consensus_level
                    ),
                };
                (ConsensusLevel::default(), vec![issue])
            }
        }
    }

    /// Parse phase_scope string into PhaseScope enum
    ///
    /// Accepts: "full", "fast", "plan-only", "plan"
    pub fn parse_phase_scope(&self) -> (PhaseScope, Vec<ConfigIssue>) {
        match self.phase_scope.parse::<PhaseScope>() {
            Ok(scope) => (scope, vec![]),
            Err(_) => {
                let issue = ConfigIssue {
                    severity: Severity::Warning,
                    code: ConfigIssueCode::InvalidEnumValue {
                        field: "agent.phase_scope".to_string(),
                        value: self.phase_scope.clone(),
                        valid_values: vec![
                            "full".to_string(),
                            "fast".to_string(),
                            "plan-only".to_string(),
                        ],
                    },
                    message: format!(
                        "agent.phase_scope: unknown value '{}', falling back to 'full'",
                        self.phase_scope
                    ),
                };
                (PhaseScope::default(), vec![issue])
            }
        }
    }

    /// Parse strategy string into strategy name, returning warnings on failure.
    ///
    /// Returns "quorum" or "debate". Used by CLI to configure OrchestrationStrategy.
    pub fn parse_strategy(&self) -> (&str, Vec<ConfigIssue>) {
        match self.strategy.to_lowercase().as_str() {
            "quorum" => ("quorum", vec![]),
            "debate" => ("debate", vec![]),
            _ => {
                let issue = ConfigIssue {
                    severity: Severity::Warning,
                    code: ConfigIssueCode::InvalidEnumValue {
                        field: "agent.strategy".to_string(),
                        value: self.strategy.clone(),
                        valid_values: vec!["quorum".to_string(), "debate".to_string()],
                    },
                    message: format!(
                        "agent.strategy: unknown value '{}', falling back to 'quorum'",
                        self.strategy
                    ),
                };
                ("quorum", vec![issue])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quorum_domain::agent::validation::ConfigIssueCode;

    #[test]
    fn test_agent_config_deserialize() {
        let toml_str = r#"
[agent]
max_plan_revisions = 5
hil_mode = "auto_reject"
consensus_level = "ensemble"
phase_scope = "fast"
strategy = "debate"
"#;
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.agent.max_plan_revisions, 5);
        assert_eq!(config.agent.hil_mode, "auto_reject");
        assert_eq!(config.agent.consensus_level, "ensemble");
        assert_eq!(
            config.agent.parse_consensus_level().0,
            ConsensusLevel::Ensemble
        );
        assert_eq!(config.agent.phase_scope, "fast");
        assert_eq!(config.agent.parse_phase_scope().0, PhaseScope::Fast);
        assert_eq!(config.agent.parse_strategy().0, "debate");
    }

    #[test]
    fn test_agent_config_consensus_level_deserialize() {
        // Test "solo" (default)
        let toml_str = r#"
[agent]
consensus_level = "solo"
"#;
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.agent.consensus_level, "solo");
        assert_eq!(config.agent.parse_consensus_level().0, ConsensusLevel::Solo);

        // Test "ensemble"
        let toml_str = r#"
[agent]
consensus_level = "ensemble"
"#;
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.agent.consensus_level, "ensemble");
        assert_eq!(
            config.agent.parse_consensus_level().0,
            ConsensusLevel::Ensemble
        );

        // Test alias "ens" -> Ensemble
        let toml_str = r#"
[agent]
consensus_level = "ens"
"#;
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.agent.parse_consensus_level().0,
            ConsensusLevel::Ensemble
        );
    }

    #[test]
    fn test_agent_config_phase_scope_deserialize() {
        let toml_str = r#"
[agent]
phase_scope = "plan-only"
"#;
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.agent.parse_phase_scope().0, PhaseScope::PlanOnly);
    }

    #[test]
    fn test_validate_typo_hil_mode_warns() {
        let toml_str = r#"
[agent]
hil_mode = "typo"
"#;
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        let issues = config.validate();
        assert!(issues.iter().any(|i| matches!(
            &i.code,
            ConfigIssueCode::InvalidEnumValue { field, .. } if field == "agent.hil_mode"
        )));
        // Typo should be a warning, not an error
        assert!(issues.iter().all(|i| i.severity == Severity::Warning));
    }

    #[test]
    fn test_validate_typo_consensus_level_warns() {
        let toml_str = r#"
[agent]
consensus_level = "typo"
"#;
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        let issues = config.validate();
        assert!(issues.iter().any(|i| matches!(
            &i.code,
            ConfigIssueCode::InvalidEnumValue { field, .. } if field == "agent.consensus_level"
        )));
    }

    #[test]
    fn test_validate_typo_strategy_warns() {
        let toml_str = r#"
[agent]
strategy = "typo"
"#;
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        let issues = config.validate();
        assert!(issues.iter().any(|i| matches!(
            &i.code,
            ConfigIssueCode::InvalidEnumValue { field, .. } if field == "agent.strategy"
        )));
    }
}
