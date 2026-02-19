//! Role-based model configuration.
//!
//! [`ModelConfig`] groups the model selections for different agent phases.
//! This is a static value object — once created, models don't change at runtime.

use crate::core::model::Model;
use serde::{Deserialize, Serialize};

/// Role-based model configuration.
///
/// Different interactions and phases have different model requirements:
///
/// ## Agent Roles
/// - **Exploration**: Cheap model for context gathering + low-risk tools
/// - **Decision**: High-performance model for planning + high-risk decisions
/// - **Review**: Multiple high-performance models for quality judgments
///
/// ## Interaction Roles
/// - **Participants**: Models participating in Quorum Discussion
/// - **Moderator**: Model for Quorum Synthesis
/// - **Ask**: Model for lightweight Q&A
///
/// # Example
///
/// ```
/// use quorum_domain::agent::model_config::ModelConfig;
/// use quorum_domain::Model;
///
/// let config = ModelConfig::default()
///     .with_decision(Model::ClaudeOpus45)
///     .with_participants(vec![Model::ClaudeOpus45, Model::Gpt52Codex]);
///
/// assert_eq!(config.decision, Model::ClaudeOpus45);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelConfig {
    // ==================== Agent Roles ====================
    /// Model for exploration: context gathering + low-risk tool execution.
    pub exploration: Model,
    /// Model for decisions: planning + high-risk tool execution.
    pub decision: Model,
    /// Models for review phases: plan review, action review, final review.
    pub review: Vec<Model>,

    // ==================== Interaction Roles ====================
    /// Models participating in Quorum Discussion.
    pub participants: Vec<Model>,
    /// Model for Quorum Synthesis (moderator).
    pub moderator: Model,
    /// Model for Ask (lightweight Q&A) interaction.
    pub ask: Model,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            exploration: Model::ClaudeHaiku45,
            decision: Model::ClaudeSonnet45,
            review: vec![Model::ClaudeSonnet45, Model::Gpt52Codex],
            participants: vec![Model::ClaudeSonnet45, Model::Gpt52Codex],
            moderator: Model::ClaudeSonnet45,
            ask: Model::ClaudeSonnet45,
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

    pub fn with_participants(mut self, models: Vec<Model>) -> Self {
        self.participants = models;
        self
    }

    pub fn with_moderator(mut self, model: Model) -> Self {
        self.moderator = model;
        self
    }

    pub fn with_ask(mut self, model: Model) -> Self {
        self.ask = model;
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
        // Interaction roles
        assert_eq!(config.participants.len(), 2);
        assert_eq!(config.moderator, Model::ClaudeSonnet45);
        assert_eq!(config.ask, Model::ClaudeSonnet45);
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

    #[test]
    fn test_interaction_role_builders() {
        let config = ModelConfig::default()
            .with_participants(vec![Model::ClaudeOpus45, Model::Gemini3Pro])
            .with_moderator(Model::ClaudeOpus45)
            .with_ask(Model::Gpt52Codex);

        assert_eq!(
            config.participants,
            vec![Model::ClaudeOpus45, Model::Gemini3Pro]
        );
        assert_eq!(config.moderator, Model::ClaudeOpus45);
        assert_eq!(config.ask, Model::Gpt52Codex);
    }
}
