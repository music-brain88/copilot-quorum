//! Raw TOML configuration data types
//!
//! These structs represent the exact structure of the TOML config file.
//! They are deserialized directly and use domain types where appropriate.

mod agent;
mod context_budget;
mod models;
mod output;
mod providers;
mod quorum;
mod repl;
mod tools;
mod tui;

pub use agent::FileAgentConfig;
pub use context_budget::FileContextBudgetConfig;
pub use models::FileModelsConfig;
pub use output::{FileOutputConfig, FileOutputFormat};
pub use providers::{FileAnthropicConfig, FileOpenAiConfig, FileProvidersConfig};
pub use quorum::FileQuorumConfig;
pub use repl::FileReplConfig;
pub use tools::{
    EnhancedToolConfig, FileBuiltinToolsConfig, FileCliToolsConfig, FileCustomToolConfig,
    FileCustomToolParameter, FileMcpServerConfig, FileMcpToolsConfig, FileToolsConfig,
    ToolProviderType,
};
pub use tui::{
    FileTuiConfig, FileTuiInputConfig, FileTuiLayoutConfig, FileTuiRoutesConfig,
    FileTuiSurfaceConfig, FileTuiSurfacesConfig,
};

use quorum_domain::agent::validation::{ConfigIssue, ConfigIssueCode, Severity};
use serde::{Deserialize, Serialize};

/// Complete file configuration (raw TOML structure)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileConfig {
    /// Role-based model selection
    pub models: FileModelsConfig,
    /// Quorum consensus settings
    pub quorum: FileQuorumConfig,
    /// Output settings
    pub output: FileOutputConfig,
    /// REPL settings
    pub repl: FileReplConfig,
    /// Agent settings
    pub agent: FileAgentConfig,
    /// Tools settings
    pub tools: FileToolsConfig,
    /// TUI settings
    pub tui: FileTuiConfig,
    /// Context budget settings
    pub context_budget: FileContextBudgetConfig,
    /// Provider settings (e.g. Bedrock credentials)
    /// This is separate from the agent/model config to allow flexible provider routing.
    pub providers: FileProvidersConfig,
}

impl FileConfig {
    /// Validate the entire configuration, returning all detected issues.
    ///
    /// This is the single entry point for config validation. It checks:
    /// 1. Empty model names across all model fields
    /// 2. Enum parse failures for agent fields (hil_mode, consensus_level, etc.)
    /// 3. Dead sections that are not wired into the application
    pub fn validate(&self) -> Vec<ConfigIssue> {
        let mut issues = Vec::new();

        // 1. Model parse validation (catches empty names)
        issues.extend(self.models.parse_exploration().1);
        issues.extend(self.models.parse_decision().1);
        issues.extend(self.models.parse_review().1);
        issues.extend(self.models.parse_participants().1);
        issues.extend(self.models.parse_moderator().1);
        issues.extend(self.models.parse_ask().1);

        // 2. Enum parse validation
        issues.extend(self.agent.parse_hil_mode().1);
        issues.extend(self.agent.parse_consensus_level().1);
        issues.extend(self.agent.parse_phase_scope().1);
        issues.extend(self.agent.parse_strategy().1);

        // 3. Context budget validation
        issues.extend(self.context_budget.to_context_budget().1);

        // 4. TUI layout preset validation
        {
            let valid = ["default", "minimal", "min", "wide", "stacked", "stack"];
            if !valid.contains(&self.tui.layout.preset.to_lowercase().as_str()) {
                issues.push(ConfigIssue {
                    severity: Severity::Warning,
                    code: ConfigIssueCode::InvalidEnumValue {
                        field: "tui.layout.preset".to_string(),
                        value: self.tui.layout.preset.clone(),
                        valid_values: vec![
                            "default".to_string(),
                            "minimal".to_string(),
                            "wide".to_string(),
                            "stacked".to_string(),
                        ],
                    },
                    message: format!(
                        "tui.layout.preset: unknown value '{}', falling back to 'default'",
                        self.tui.layout.preset
                    ),
                });
            }
        }

        // 5. Dead [quorum] section detection
        if self.quorum != FileQuorumConfig::default() {
            issues.push(ConfigIssue {
                severity: Severity::Warning,
                code: ConfigIssueCode::DeadSection {
                    section: "quorum".to_string(),
                },
                message: "[quorum] section is configured but not currently used by the application"
                    .to_string(),
            });
        }

        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quorum_domain::OutputFormat;

    #[test]
    fn test_deserialize_full_config() {
        let toml_str = r#"
[models]
exploration = "gpt-5.2-codex"
decision = "claude-sonnet-4.5"
review = ["claude-opus-4.5", "gpt-5.2-codex"]

[output]
format = "full"
color = false

[repl]
show_progress = false
history_file = "~/.local/share/quorum/history.txt"
"#;

        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.models.exploration, Some("gpt-5.2-codex".to_string()));
        assert_eq!(
            config.models.decision,
            Some("claude-sonnet-4.5".to_string())
        );
        assert_eq!(config.models.review.as_ref().unwrap().len(), 2);
        assert_eq!(config.output.format, Some(OutputFormat::Full));
        assert!(!config.output.color);
        assert!(!config.repl.show_progress);
    }

    #[test]
    fn test_deserialize_partial_config() {
        let toml_str = r#"
[models]
decision = "gpt-5.2-codex"
"#;

        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.models.parse_decision().0,
            Some(quorum_domain::Model::Gpt52Codex)
        );
        // Defaults should apply
        assert!(config.models.exploration.is_none());
        assert!(config.output.color);
        assert!(config.repl.show_progress);
    }

    #[test]
    fn test_default_config() {
        let config = FileConfig::default();
        assert!(config.models.exploration.is_none());
        assert!(config.models.decision.is_none());
        assert!(config.models.review.is_none());
        assert!(config.output.color);
        assert!(config.repl.show_progress);
    }

    #[test]
    fn test_validate_valid_config() {
        let config = FileConfig::default();
        assert!(config.validate().is_empty());
    }
}
