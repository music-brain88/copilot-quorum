//! Role-based model configuration from TOML (`[models]` section)

use quorum_domain::Model;
use quorum_domain::agent::validation::{ConfigIssue, ConfigIssueCode, Severity};
use serde::{Deserialize, Serialize};

/// Role-based model configuration from TOML
///
/// # Example
///
/// ```toml
/// [models]
/// exploration = "gpt-5.2-codex"           # Context gathering + low-risk tools
/// decision = "claude-sonnet-4.5"          # Planning + high-risk tools
/// review = ["claude-opus-4.5", "gpt-5.2-codex", "gemini-3-pro-preview"]
/// participants = ["claude-opus-4.5", "gpt-5.2-codex", "gemini-3-pro-preview"]
/// moderator = "claude-opus-4.5"           # Quorum Synthesis
/// ask = "claude-sonnet-4.5"               # Ask (Q&A) interaction
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileModelsConfig {
    /// Model for exploration: context gathering + low-risk tools
    pub exploration: Option<String>,
    /// Model for decisions: planning + high-risk tools
    pub decision: Option<String>,
    /// Models for review phases
    pub review: Option<Vec<String>>,
    /// Models for Quorum Discussion participants
    pub participants: Option<Vec<String>>,
    /// Model for Quorum Synthesis (moderator)
    pub moderator: Option<String>,
    /// Model for Ask (Q&A) interaction
    pub ask: Option<String>,
}

impl FileModelsConfig {
    /// Parse a single model string, collecting issues for empty names.
    fn parse_single_model(
        field: &str,
        value: Option<&String>,
    ) -> (Option<Model>, Vec<ConfigIssue>) {
        let mut issues = Vec::new();
        match value {
            None => (None, issues),
            Some(s) if s.trim().is_empty() => {
                issues.push(ConfigIssue {
                    severity: Severity::Error,
                    code: ConfigIssueCode::EmptyModelName {
                        field: field.to_string(),
                    },
                    message: format!("models.{}: model name cannot be empty", field),
                });
                (None, issues)
            }
            Some(s) => {
                // Model::from_str is infallible; unknown names become Custom(...)
                let model: Model = s.parse().unwrap();
                (Some(model), issues)
            }
        }
    }

    /// Parse a model list, collecting issues for empty names.
    fn parse_model_list(
        field: &str,
        values: Option<&Vec<String>>,
    ) -> (Option<Vec<Model>>, Vec<ConfigIssue>) {
        let mut issues = Vec::new();
        match values {
            None => (None, issues),
            Some(strings) => {
                let mut models = Vec::new();
                for s in strings {
                    if s.trim().is_empty() {
                        issues.push(ConfigIssue {
                            severity: Severity::Error,
                            code: ConfigIssueCode::EmptyModelName {
                                field: field.to_string(),
                            },
                            message: format!(
                                "models.{}: model name cannot be empty in list",
                                field
                            ),
                        });
                    } else {
                        let model: Model = s.parse().unwrap();
                        models.push(model);
                    }
                }
                (Some(models), issues)
            }
        }
    }

    /// Parse exploration model string into Model enum
    pub fn parse_exploration(&self) -> (Option<Model>, Vec<ConfigIssue>) {
        Self::parse_single_model("exploration", self.exploration.as_ref())
    }

    /// Parse decision model string into Model enum
    pub fn parse_decision(&self) -> (Option<Model>, Vec<ConfigIssue>) {
        Self::parse_single_model("decision", self.decision.as_ref())
    }

    /// Parse review model strings into `Vec<Model>`
    pub fn parse_review(&self) -> (Option<Vec<Model>>, Vec<ConfigIssue>) {
        Self::parse_model_list("review", self.review.as_ref())
    }

    /// Parse participants model strings into `Vec<Model>`
    pub fn parse_participants(&self) -> (Option<Vec<Model>>, Vec<ConfigIssue>) {
        Self::parse_model_list("participants", self.participants.as_ref())
    }

    /// Parse moderator model string into Model enum
    pub fn parse_moderator(&self) -> (Option<Model>, Vec<ConfigIssue>) {
        Self::parse_single_model("moderator", self.moderator.as_ref())
    }

    /// Parse ask model string into Model enum
    pub fn parse_ask(&self) -> (Option<Model>, Vec<ConfigIssue>) {
        Self::parse_single_model("ask", self.ask.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quorum_domain::agent::validation::ConfigIssueCode;

    #[test]
    fn test_models_config_defaults() {
        let config = FileModelsConfig::default();
        assert!(config.exploration.is_none());
        assert!(config.decision.is_none());
        assert!(config.review.is_none());
        assert!(config.participants.is_none());
        assert!(config.moderator.is_none());
        assert!(config.ask.is_none());
    }

    #[test]
    fn test_models_config_deserialize() {
        let toml_str = r#"
[models]
exploration = "gpt-5.2-codex"
decision = "claude-sonnet-4.5"
review = ["claude-sonnet-4.5", "gpt-5.2-codex"]
"#;
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.models.parse_exploration().0, Some(Model::Gpt52Codex));
        assert_eq!(
            config.models.parse_decision().0,
            Some(Model::ClaudeSonnet45)
        );
        let review = config.models.parse_review().0.unwrap();
        assert_eq!(review.len(), 2);
        assert!(review.contains(&Model::ClaudeSonnet45));
        assert!(review.contains(&Model::Gpt52Codex));
    }

    #[test]
    fn test_models_config_partial() {
        let toml_str = r#"
[models]
decision = "claude-opus-4.5"
"#;
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        assert!(config.models.exploration.is_none());
        assert_eq!(config.models.parse_decision().0, Some(Model::ClaudeOpus45));
        assert!(config.models.review.is_none());
        assert!(config.models.participants.is_none());
        assert!(config.models.moderator.is_none());
        assert!(config.models.ask.is_none());
    }

    #[test]
    fn test_models_config_interaction_roles() {
        let toml_str = r#"
[models]
participants = ["claude-opus-4.5", "gpt-5.2-codex"]
moderator = "claude-opus-4.5"
ask = "claude-sonnet-4.5"
"#;
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        let participants = config.models.parse_participants().0.unwrap();
        assert_eq!(participants.len(), 2);
        assert!(participants.contains(&Model::ClaudeOpus45));
        assert!(participants.contains(&Model::Gpt52Codex));
        assert_eq!(config.models.parse_moderator().0, Some(Model::ClaudeOpus45));
        assert_eq!(config.models.parse_ask().0, Some(Model::ClaudeSonnet45));
    }

    #[test]
    fn test_validate_empty_model_name() {
        let toml_str = r#"
[models]
review = ["gpt-5.2-codex", ""]
"#;
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        let issues = config.validate();
        assert!(!issues.is_empty());
        assert!(issues.iter().any(|i| matches!(
            &i.code,
            ConfigIssueCode::EmptyModelName { field } if field == "review"
        )));
    }

    #[test]
    fn test_validate_empty_participants_name() {
        let toml_str = r#"
[models]
participants = ["gpt-5.2-codex", ""]
"#;
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        let issues = config.validate();
        assert!(!issues.is_empty());
        assert!(issues.iter().any(|i| matches!(
            &i.code,
            ConfigIssueCode::EmptyModelName { field } if field == "participants"
        )));
    }
}
