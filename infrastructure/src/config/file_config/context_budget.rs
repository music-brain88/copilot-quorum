//! Context budget configuration from TOML (`[context_budget]` section)

use quorum_domain::ContextBudget;
use quorum_domain::agent::validation::{ConfigIssue, ConfigIssueCode, Severity};
use serde::{Deserialize, Serialize};

/// Context budget configuration from TOML.
///
/// Controls how much task result context is retained between task executions.
///
/// # Example
///
/// ```toml
/// [context_budget]
/// max_entry_bytes = 20000
/// max_total_bytes = 60000
/// recent_full_count = 3
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileContextBudgetConfig {
    /// Maximum bytes for a single task result (head+tail truncated).
    pub max_entry_bytes: usize,
    /// Maximum bytes for the entire previous_results buffer.
    pub max_total_bytes: usize,
    /// Number of recent task results to keep in full (older are summarized).
    pub recent_full_count: usize,
}

impl Default for FileContextBudgetConfig {
    fn default() -> Self {
        let budget = ContextBudget::default();
        Self {
            max_entry_bytes: budget.max_entry_bytes(),
            max_total_bytes: budget.max_total_bytes(),
            recent_full_count: budget.recent_full_count(),
        }
    }
}

impl FileContextBudgetConfig {
    /// Convert to domain `ContextBudget`, returning validation issues.
    ///
    /// If the values violate constraints, falls back to `ContextBudget::default()`
    /// and returns warnings describing the issues.
    pub fn to_context_budget(&self) -> (ContextBudget, Vec<ConfigIssue>) {
        match ContextBudget::try_new(
            self.max_entry_bytes,
            self.max_total_bytes,
            self.recent_full_count,
        ) {
            Ok(budget) => (budget, vec![]),
            Err(errors) => {
                let issues = errors
                    .into_iter()
                    .map(|msg| ConfigIssue {
                        severity: Severity::Warning,
                        code: ConfigIssueCode::InvalidConstraint {
                            field: "context_budget".to_string(),
                        },
                        message: msg,
                    })
                    .collect();
                (ContextBudget::default(), issues)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quorum_domain::agent::validation::ConfigIssueCode;

    #[test]
    fn test_context_budget_config_default() {
        let config = FileContextBudgetConfig::default();
        assert_eq!(config.max_entry_bytes, 20_000);
        assert_eq!(config.max_total_bytes, 60_000);
        assert_eq!(config.recent_full_count, 3);
    }

    #[test]
    fn test_context_budget_config_deserialize() {
        let toml_str = r#"
[context_budget]
max_entry_bytes = 10000
max_total_bytes = 30000
recent_full_count = 2
"#;
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.context_budget.max_entry_bytes, 10_000);
        assert_eq!(config.context_budget.max_total_bytes, 30_000);
        assert_eq!(config.context_budget.recent_full_count, 2);
    }

    #[test]
    fn test_context_budget_config_to_domain() {
        let config = FileContextBudgetConfig::default();
        let (budget, issues) = config.to_context_budget();
        assert!(issues.is_empty());
        assert_eq!(budget.max_entry_bytes(), 20_000);
    }

    #[test]
    fn test_context_budget_config_validation_falls_back_to_default() {
        let config = FileContextBudgetConfig {
            max_entry_bytes: 50_000,
            max_total_bytes: 10_000, // Less than entry — invalid
            recent_full_count: 0,    // Less than 1 — invalid
        };
        let (budget, issues) = config.to_context_budget();
        assert_eq!(issues.len(), 2);
        assert!(
            issues
                .iter()
                .all(|i| matches!(&i.code, ConfigIssueCode::InvalidConstraint { .. }))
        );
        // Should fall back to default
        assert_eq!(budget, ContextBudget::default());
    }

    #[test]
    fn test_context_budget_missing_uses_defaults() {
        // Empty config should use defaults
        let config: super::super::FileConfig = toml::from_str("").unwrap();
        assert_eq!(config.context_budget.max_entry_bytes, 20_000);
    }
}
