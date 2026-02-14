//! Run Ask use case
//!
//! Lightweight single-model Q&A without the full agent lifecycle.
//! Ask buffers are Solo-fixed with no plan/review cycle.
//!
//! # Design (Issue #127)
//!
//! Ask uses only [`ModelConfig`] and [`ExecutionParams`] — no
//! [`SessionMode`] or [`AgentPolicy`].

use crate::config::ExecutionParams;
use crate::ports::llm_gateway::{GatewayError, LlmGateway};
use quorum_domain::agent::model_config::ModelConfig;
use std::sync::Arc;
use thiserror::Error;
use tracing::info;

/// Errors that can occur during Ask execution
#[derive(Error, Debug)]
pub enum RunAskError {
    #[error("Empty question")]
    EmptyQuestion,

    #[error("Gateway error: {0}")]
    GatewayError(#[from] GatewayError),
}

/// Input for the RunAsk use case.
///
/// Deliberately excludes [`SessionMode`] and [`AgentPolicy`]:
/// Ask is always Solo, no plan/review cycle.
///
/// # Config Necessity (Issue #127)
///
/// | Type | Required |
/// |------|----------|
/// | SessionMode | No (Solo fixed) |
/// | ModelConfig | Yes |
/// | AgentPolicy | No |
/// | ExecutionParams | Yes |
#[derive(Debug, Clone)]
pub struct RunAskInput {
    /// The question to ask
    pub question: String,
    /// Model configuration (uses `exploration` model for Ask)
    pub models: ModelConfig,
    /// Execution parameters (working_dir, limits)
    pub execution: ExecutionParams,
}

impl RunAskInput {
    pub fn new(
        question: impl Into<String>,
        models: ModelConfig,
        execution: ExecutionParams,
    ) -> Self {
        Self {
            question: question.into(),
            models,
            execution,
        }
    }
}

/// Output from the RunAsk use case
#[derive(Debug, Clone)]
pub struct RunAskOutput {
    /// The model's answer
    pub answer: String,
    /// Which model answered
    pub model: String,
}

/// Use case for running a lightweight Ask query.
///
/// Uses the `exploration` model from [`ModelConfig`] for single-turn Q&A.
/// No planning, no review, no tool execution (Phase A).
pub struct RunAskUseCase<G: LlmGateway + 'static> {
    gateway: Arc<G>,
}

impl<G: LlmGateway + 'static> RunAskUseCase<G> {
    pub fn new(gateway: Arc<G>) -> Self {
        Self { gateway }
    }

    /// Execute the Ask query.
    pub async fn execute(&self, input: RunAskInput) -> Result<RunAskOutput, RunAskError> {
        if input.question.trim().is_empty() {
            return Err(RunAskError::EmptyQuestion);
        }

        let model = &input.models.exploration;
        info!("Ask: querying {} with question", model);

        let session = self
            .gateway
            .create_session_with_system_prompt(
                model,
                "You are a helpful assistant. Answer the user's question concisely and accurately.",
            )
            .await?;

        let answer = session.send(&input.question).await?;

        Ok(RunAskOutput {
            answer,
            model: model.to_string(),
        })
    }
}
