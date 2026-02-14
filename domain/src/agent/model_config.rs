//! Role-based model configuration.
//!
//! [`ModelConfig`] groups the model selections for different agent phases.
//! This is a static value object — once created, models don't change at runtime.

use crate::core::model::Model;
use serde::{Deserialize, Serialize};

/// Role-based model configuration.
///
/// Different phases of agent execution have different requirements:
/// - **Exploration**: Cheap model for context gathering + low-risk tools
/// - **Decision**: High-performance model for planning + high-risk decisions
/// - **Review**: Multiple high-performance models for quality judgments
///
/// # Example
///
/// ```
/// use quorum_domain::agent::model_config::ModelConfig;
/// use quorum_domain::Model;
///
/// let config = ModelConfig::default()
///     .with_decision(Model::ClaudeOpus45)
///     .with_review(vec![Model::ClaudeOpus45, Model::Gpt52Codex]);
///
/// assert_eq!(config.decision, Model::ClaudeOpus45);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Model for exploration: context gathering + low-risk tool execution.
    pub exploration: Model,
    /// Model for decisions: planning + high-risk tool execution.
    pub decision: Model,
    /// Models for review phases: plan review, action review, final review.
    pub review: Vec<Model>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            exploration: Model::ClaudeHaiku45,
            decision: Model::ClaudeSonnet45,
            review: vec![Model::ClaudeSonnet45, Model::Gpt52Codex],
        }
    }
}

impl ModelConfig {
    // ==================== Builder Methods ====================

    pub fn with_exploration(mut self, model: Model) -> Self {
        self.exploration = model;
        self
    }

    pub fn with_decision(mut self, model: Model) -> Self {
        self.decision = model;
        self
    }

    pub fn with_review(mut self, models: Vec<Model>) -> Self {
        self.review = models;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        let config = ModelConfig::default();
        assert_eq!(config.exploration, Model::ClaudeHaiku45);
        assert_eq!(config.decision, Model::ClaudeSonnet45);
        assert_eq!(config.review.len(), 2);
    }

    #[test]
    fn test_builder() {
        let config = ModelConfig::default()
            .with_exploration(Model::ClaudeSonnet45)
            .with_decision(Model::ClaudeOpus45)
            .with_review(vec![Model::ClaudeOpus45]);

        assert_eq!(config.exploration, Model::ClaudeSonnet45);
        assert_eq!(config.decision, Model::ClaudeOpus45);
        assert_eq!(config.review, vec![Model::ClaudeOpus45]);
    }
}
