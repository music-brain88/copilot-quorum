//! Input/error types for the RunQuorum use case.

use crate::ports::llm_gateway::GatewayError;
use quorum_domain::{ModelConfig, OrchestrationStrategy, Question};
use thiserror::Error;

/// Errors that can occur during Quorum execution
#[derive(Error, Debug)]
pub enum RunQuorumError {
    #[error("No models configured")]
    NoModels,

    #[error("All models failed to respond")]
    AllModelsFailed,

    #[error("Synthesis failed: {0}")]
    SynthesisFailed(String),

    #[error("Debate strategy requires at least 2 models, got {0}")]
    NotEnoughModelsForDebate(usize),

    #[error("Gateway error: {0}")]
    GatewayError(#[from] GatewayError),
}

/// Input for the RunQuorum use case
#[derive(Debug, Clone)]
pub struct RunQuorumInput {
    /// The question to ask
    pub question: Question,
    /// Model configuration (`participants` join the discussion, `moderator` synthesizes)
    pub models: ModelConfig,
    /// Whether to include peer review phase (Quorum strategy only)
    pub enable_review: bool,
    /// Which orchestration strategy drives this discussion
    pub strategy: OrchestrationStrategy,
}

impl RunQuorumInput {
    pub fn new(question: impl Into<Question>, models: ModelConfig) -> Self {
        Self {
            question: question.into(),
            models,
            enable_review: true,
            strategy: OrchestrationStrategy::default(),
        }
    }

    pub fn without_review(mut self) -> Self {
        self.enable_review = false;
        self
    }

    pub fn with_strategy(mut self, strategy: OrchestrationStrategy) -> Self {
        self.strategy = strategy;
        self
    }
}
