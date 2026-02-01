//! Orchestration domain entities for the Quorum (合議) system.
//!
//! This module contains the core entities that manage a Quorum session where
//! multiple LLM models collaborate to answer a question through:
//! 1. **Initial Query** - All models independently answer the question
//! 2. **Peer Review** - Models review each other's responses (optional)
//! 3. **Synthesis** - A moderator model synthesizes all responses into a final answer
//!
//! # Key Types
//!
//! - [`Phase`] - Represents the current phase of a Quorum run
//! - [`QuorumConfig`] - Configuration for which models participate and how
//! - [`QuorumRun`] - Tracks the state of a single Quorum session

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
    /// Creates a new QuorumConfig with the specified models.
    ///
    /// Review phase is enabled by default. The first model will be used as
    /// the moderator unless explicitly set via [`with_moderator`](Self::with_moderator).
    pub fn new(models: Vec<Model>) -> Self {
        Self {
            models,
            ..Default::default()
        }
    }

    /// Sets a specific model to act as the moderator for synthesis.
    ///
    /// The moderator is responsible for combining all responses into a final answer.
    pub fn with_moderator(mut self, model: Model) -> Self {
        self.moderator = Some(model);
        self
    }

    /// Disables the peer review phase for faster execution.
    ///
    /// When disabled, the Quorum will go directly from Initial Query to Synthesis.
    pub fn without_review(mut self) -> Self {
        self.enable_review = false;
        self
    }

    /// Returns the moderator model, defaulting to the first participating model.
    pub fn get_moderator(&self) -> Option<&Model> {
        self.moderator.as_ref().or(self.models.first())
    }

    /// Validates that the configuration is usable.
    ///
    /// # Errors
    ///
    /// Returns an error if no models are configured.
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
    /// Creates a new QuorumRun with the given ID, question, and configuration.
    ///
    /// The run starts with no active phase; use [`set_phase`](Self::set_phase) to begin execution.
    pub fn new(id: impl Into<String>, question: Question, config: QuorumConfig) -> Self {
        Self {
            id: id.into(),
            question,
            config,
            current_phase: None,
        }
    }

    /// Returns the unique identifier for this Quorum run.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the question being discussed by the models.
    pub fn question(&self) -> &Question {
        &self.question
    }

    /// Returns the configuration for this run.
    pub fn config(&self) -> &QuorumConfig {
        &self.config
    }

    /// Returns the current execution phase, if any.
    pub fn current_phase(&self) -> Option<&Phase> {
        self.current_phase.as_ref()
    }

    /// Advances the run to the specified phase.
    pub fn set_phase(&mut self, phase: Phase) {
        self.current_phase = Some(phase);
    }

    /// Returns the sequence of phases that will be executed.
    ///
    /// If review is disabled in config, returns `[Initial, Synthesis]`.
    /// Otherwise returns `[Initial, Review, Synthesis]`.
    pub fn phases(&self) -> Vec<Phase> {
        if self.config.enable_review {
            vec![Phase::Initial, Phase::Review, Phase::Synthesis]
        } else {
            vec![Phase::Initial, Phase::Synthesis]
        }
    }
}
