//! Quorum consensus configuration from TOML (`[quorum]` section)
//!
//! The quorum configuration controls how multi-model consensus works.
//! This is the core of the Quorum system - inspired by distributed systems
//! where consensus ensures reliability.
//!
//! Example configuration:
//!
//! ```toml
//! [quorum]
//! rule = "majority"
//! min_models = 2
//!
//! [quorum.discussion]
//! models = ["claude-sonnet-4.5", "gpt-5.2-codex", "gemini-3-pro"]
//! moderator = "claude-opus-4.5"
//! enable_peer_review = true
//! ```

use quorum_domain::{Model, QuorumRule};
use serde::{Deserialize, Serialize};

/// Quorum consensus configuration
///
/// Controls how Quorum Consensus (voting for approval) works.
///
/// # Example
///
/// ```toml
/// [quorum]
/// rule = "majority"           # or "unanimous", "atleast:2", "75%"
/// min_models = 2              # minimum models required for valid consensus
/// moderator = "claude-opus-4.5"
/// enable_peer_review = true
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FileQuorumConfig {
    /// Consensus rule: "majority", "unanimous", "atleast:N", "N%"
    pub rule: String,
    /// Minimum number of models required for valid consensus
    pub min_models: usize,
    /// Moderator model for synthesis
    pub moderator: Option<String>,
    /// Enable peer review phase
    pub enable_peer_review: bool,
}

impl Default for FileQuorumConfig {
    fn default() -> Self {
        Self {
            rule: "majority".to_string(),
            min_models: 2,
            moderator: None,
            enable_peer_review: true,
        }
    }
}

impl FileQuorumConfig {
    /// Parse the rule string into QuorumRule enum
    pub fn parse_rule(&self) -> QuorumRule {
        self.rule.parse().unwrap_or_default()
    }

    /// Parse moderator into Model enum
    pub fn parse_moderator(&self) -> Option<Model> {
        self.moderator.as_ref().and_then(|s| s.parse().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quorum_domain::agent::validation::ConfigIssueCode;

    #[test]
    fn test_quorum_config_default() {
        let config = FileQuorumConfig::default();
        assert_eq!(config.rule, "majority");
        assert_eq!(config.min_models, 2);
        assert!(config.enable_peer_review);
        assert!(config.moderator.is_none());
    }

    #[test]
    fn test_quorum_config_deserialize() {
        let toml_str = r#"
[quorum]
rule = "unanimous"
min_models = 3
moderator = "claude-opus-4.5"
enable_peer_review = false
"#;
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.quorum.rule, "unanimous");
        assert_eq!(config.quorum.min_models, 3);
        assert!(!config.quorum.enable_peer_review);

        let moderator = config.quorum.parse_moderator().unwrap();
        assert_eq!(moderator, Model::ClaudeOpus45);
    }

    #[test]
    fn test_quorum_config_parse_rule() {
        let mut config = FileQuorumConfig::default();

        config.rule = "majority".to_string();
        assert_eq!(config.parse_rule(), QuorumRule::Majority);

        config.rule = "unanimous".to_string();
        assert_eq!(config.parse_rule(), QuorumRule::Unanimous);

        config.rule = "atleast:2".to_string();
        assert_eq!(config.parse_rule(), QuorumRule::AtLeast(2));

        config.rule = "75%".to_string();
        assert_eq!(config.parse_rule(), QuorumRule::Percentage(75));
    }

    #[test]
    fn test_validate_dead_quorum_section() {
        let toml_str = r#"
[quorum]
rule = "unanimous"
min_models = 3
"#;
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        let issues = config.validate();
        assert!(issues.iter().any(|i| matches!(
            &i.code,
            ConfigIssueCode::DeadSection { section } if section == "quorum"
        )));
    }

    #[test]
    fn test_validate_default_quorum_no_dead_warning() {
        // Default [quorum] values should NOT trigger a dead section warning
        let config = super::super::FileConfig::default();
        let issues = config.validate();
        assert!(
            !issues
                .iter()
                .any(|i| matches!(&i.code, ConfigIssueCode::DeadSection { .. }))
        );
    }
}
