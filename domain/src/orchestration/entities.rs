//! Orchestration domain entities

use crate::core::model::Model;
use crate::core::question::Question;
use serde::{Deserialize, Serialize};

/// Phase of a Quorum run
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Phase {
    /// Initial query phase - all models answer the question
    Initial,
    /// Peer review phase - models review each other's responses
    Review,
    /// Synthesis phase - moderator synthesizes all responses
    Synthesis,
}

impl Phase {
    pub fn as_str(&self) -> &str {
        match self {
            Phase::Initial => "initial",
            Phase::Review => "review",
            Phase::Synthesis => "synthesis",
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Phase::Initial => "Initial Query",
            Phase::Review => "Peer Review",
            Phase::Synthesis => "Synthesis",
        }
    }
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Configuration for a Quorum run (Entity)
#[derive(Debug, Clone)]
pub struct QuorumConfig {
    /// Models to participate in the discussion
    pub models: Vec<Model>,
    /// Model to use for final synthesis (defaults to first model)
    pub moderator: Option<Model>,
    /// Whether to include peer review phase
    pub enable_review: bool,
}

impl Default for QuorumConfig {
    fn default() -> Self {
        Self {
            models: Model::default_models(),
            moderator: None,
            enable_review: true,
        }
    }
}

impl QuorumConfig {
    pub fn new(models: Vec<Model>) -> Self {
        Self {
            models,
            ..Default::default()
        }
    }

    pub fn with_moderator(mut self, model: Model) -> Self {
        self.moderator = Some(model);
        self
    }

    pub fn without_review(mut self) -> Self {
        self.enable_review = false;
        self
    }

    /// Get the moderator model, defaulting to the first model
    pub fn get_moderator(&self) -> Option<&Model> {
        self.moderator.as_ref().or(self.models.first())
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.models.is_empty() {
            return Err("At least one model is required");
        }
        Ok(())
    }
}

/// Represents a single Quorum run (Entity)
///
/// Tracks the state and progress of a Quorum discussion.
#[derive(Debug, Clone)]
pub struct QuorumRun {
    id: String,
    question: Question,
    config: QuorumConfig,
    current_phase: Option<Phase>,
}

impl QuorumRun {
    pub fn new(id: impl Into<String>, question: Question, config: QuorumConfig) -> Self {
        Self {
            id: id.into(),
            question,
            config,
            current_phase: None,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn question(&self) -> &Question {
        &self.question
    }

    pub fn config(&self) -> &QuorumConfig {
        &self.config
    }

    pub fn current_phase(&self) -> Option<&Phase> {
        self.current_phase.as_ref()
    }

    pub fn set_phase(&mut self, phase: Phase) {
        self.current_phase = Some(phase);
    }

    /// Get the phases that will be executed based on config
    pub fn phases(&self) -> Vec<Phase> {
        if self.config.enable_review {
            vec![Phase::Initial, Phase::Review, Phase::Synthesis]
        } else {
            vec![Phase::Initial, Phase::Synthesis]
        }
    }
}
