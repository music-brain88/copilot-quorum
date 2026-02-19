//! Run Quorum use case
//!
//! Orchestrates the full Quorum discussion flow.

use crate::ports::llm_gateway::{GatewayError, LlmGateway};
use crate::ports::progress::{NoProgress, ProgressNotifier};
use quorum_domain::{
    Model, ModelConfig, ModelResponse, PeerReview, Phase, PromptTemplate, Question, QuorumResult,
    SynthesisResult,
};
use std::sync::Arc;
use thiserror::Error;
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

/// Errors that can occur during Quorum execution
#[derive(Error, Debug)]
pub enum RunQuorumError {
    #[error("No models configured")]
    NoModels,

    #[error("All models failed to respond")]
    AllModelsFailed,

    #[error("Synthesis failed: {0}")]
    SynthesisFailed(String),

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
    /// Whether to include peer review phase
    pub enable_review: bool,
}

impl RunQuorumInput {
    pub fn new(question: impl Into<Question>, models: ModelConfig) -> Self {
        Self {
            question: question.into(),
            models,
            enable_review: true,
        }
    }

    pub fn without_review(mut self) -> Self {
        self.enable_review = false;
        self
    }
}

/// Use case for running a Quorum discussion
pub struct RunQuorumUseCase<G: LlmGateway + 'static> {
    gateway: Arc<G>,
}

impl<G: LlmGateway + 'static> RunQuorumUseCase<G> {
    pub fn new(gateway: Arc<G>) -> Self {
        Self { gateway }
    }

    /// Execute the use case with default (no-op) progress
    pub async fn execute(&self, input: RunQuorumInput) -> Result<QuorumResult, RunQuorumError> {
        self.execute_with_progress(input, &NoProgress).await
    }

    /// Execute the use case with progress callbacks
    pub async fn execute_with_progress(
        &self,
        input: RunQuorumInput,
        progress: &dyn ProgressNotifier,
    ) -> Result<QuorumResult, RunQuorumError> {
        if input.models.participants.is_empty() {
            return Err(RunQuorumError::NoModels);
        }

        info!(
            "Starting Quorum with {} models",
            input.models.participants.len()
        );

        // Phase 1: Initial Query
        let responses = self.phase_initial(&input, progress).await?;

        // Check if we have any successful responses
        let successful_responses: Vec<_> = responses.iter().filter(|r| r.success).collect();
        if successful_responses.is_empty() {
            return Err(RunQuorumError::AllModelsFailed);
        }

        // Phase 2: Peer Review (optional)
        let reviews = if input.enable_review && successful_responses.len() > 1 {
            self.phase_review(&input, &responses, progress).await?
        } else {
            debug!("Skipping peer review phase");
            vec![]
        };

        // Phase 3: Synthesis
        let synthesis = self
            .phase_synthesis(&input, &responses, &reviews, progress)
            .await?;

        Ok(QuorumResult::new(
            input.question.content(),
            input.models.participants.iter().map(|m| m.to_string()).collect(),
            responses,
            reviews,
            synthesis,
        ))
    }

    /// Phase 1: Query all models in parallel
    async fn phase_initial(
        &self,
        input: &RunQuorumInput,
        progress: &dyn ProgressNotifier,
    ) -> Result<Vec<ModelResponse>, RunQuorumError> {
        info!("Phase 1: Initial Query");
        progress.on_phase_start(&Phase::Initial, input.models.participants.len());

        let mut join_set = JoinSet::new();

        for model in &input.models.participants {
            let gateway = Arc::clone(&self.gateway);
            let model = model.clone();
            let question = input.question.content().to_string();

            join_set.spawn(async move {
                let result = Self::query_model(&gateway, &model, &question).await;
                (model, result)
            });
        }

        let mut responses = Vec::new();

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok((model, Ok(content))) => {
                    info!("Model {} responded successfully", model);
                    progress.on_task_complete(&Phase::Initial, &model, true);
                    responses.push(ModelResponse::success(model.to_string(), content));
                }
                Ok((model, Err(e))) => {
                    warn!("Model {} failed: {}", model, e);
                    progress.on_task_complete(&Phase::Initial, &model, false);
                    responses.push(ModelResponse::failure(model.to_string(), e.to_string()));
                }
                Err(e) => {
                    warn!("Task join error: {}", e);
                }
            }
        }

        progress.on_phase_complete(&Phase::Initial);
        Ok(responses)
    }

    /// Phase 2: Each model reviews others' responses
    async fn phase_review(
        &self,
        input: &RunQuorumInput,
        responses: &[ModelResponse],
        progress: &dyn ProgressNotifier,
    ) -> Result<Vec<PeerReview>, RunQuorumError> {
        info!("Phase 2: Peer Review");

        let successful_responses: Vec<_> = responses.iter().filter(|r| r.success).collect();
        progress.on_phase_start(&Phase::Review, successful_responses.len());

        let mut join_set = JoinSet::new();

        // Prepare anonymized responses
        let anonymized: Vec<(String, String)> = successful_responses
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let id = format!("Response {}", (b'A' + i as u8) as char);
                (id, r.content.clone())
            })
            .collect();

        for (i, response) in successful_responses.iter().enumerate() {
            // Each model reviews all OTHER responses
            let other_responses: Vec<_> = anonymized
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, r)| r.clone())
                .collect();

            if other_responses.is_empty() {
                continue;
            }

            let gateway = Arc::clone(&self.gateway);
            let model: Model = response.model.parse().unwrap();
            let question = input.question.content().to_string();

            join_set.spawn(async move {
                let result =
                    Self::review_responses(&gateway, &model, &question, &other_responses).await;
                (model, other_responses, result)
            });
        }

        let mut reviews = Vec::new();

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok((model, reviewed, Ok(review_content))) => {
                    info!("Model {} completed review", model);
                    progress.on_task_complete(&Phase::Review, &model, true);

                    // Create a review entry for each reviewed response
                    for (id, _) in reviewed {
                        reviews.push(PeerReview::new(
                            model.to_string(),
                            id,
                            review_content.clone(),
                        ));
                    }
                }
                Ok((model, _, Err(e))) => {
                    warn!("Model {} review failed: {}", model, e);
                    progress.on_task_complete(&Phase::Review, &model, false);
                }
                Err(e) => {
                    warn!("Task join error: {}", e);
                }
            }
        }

        progress.on_phase_complete(&Phase::Review);
        Ok(reviews)
    }

    /// Phase 3: Synthesize all responses and reviews
    async fn phase_synthesis(
        &self,
        input: &RunQuorumInput,
        responses: &[ModelResponse],
        reviews: &[PeerReview],
        progress: &dyn ProgressNotifier,
    ) -> Result<SynthesisResult, RunQuorumError> {
        info!("Phase 3: Synthesis");
        progress.on_phase_start(&Phase::Synthesis, 1);

        let moderator = input.models.moderator.clone();

        let successful_responses: Vec<(String, String)> = responses
            .iter()
            .filter(|r| r.success)
            .map(|r| (r.model.clone(), r.content.clone()))
            .collect();

        let review_pairs: Vec<(String, String)> = reviews
            .iter()
            .map(|r| (r.reviewer.clone(), r.content.clone()))
            .collect();

        let synthesis_content = Self::synthesize(
            &self.gateway,
            &moderator,
            input.question.content(),
            &successful_responses,
            &review_pairs,
        )
        .await?;

        progress.on_task_complete(&Phase::Synthesis, &moderator, true);
        progress.on_phase_complete(&Phase::Synthesis);

        Ok(SynthesisResult::new(
            moderator.to_string(),
            synthesis_content,
        ))
    }

    /// Query a single model
    async fn query_model(
        gateway: &G,
        model: &Model,
        question: &str,
    ) -> Result<String, GatewayError> {
        let session = gateway
            .create_session_with_system_prompt(model, PromptTemplate::initial_system())
            .await?;

        let prompt = PromptTemplate::initial_query(question);
        session.send(&prompt).await
    }

    /// Have a model review other responses
    async fn review_responses(
        gateway: &G,
        model: &Model,
        question: &str,
        responses: &[(String, String)],
    ) -> Result<String, GatewayError> {
        let session = gateway
            .create_session_with_system_prompt(model, PromptTemplate::review_system())
            .await?;

        let prompt = PromptTemplate::review_prompt(question, responses);
        session.send(&prompt).await
    }

    /// Synthesize all responses and reviews
    async fn synthesize(
        gateway: &G,
        moderator: &Model,
        question: &str,
        responses: &[(String, String)],
        reviews: &[(String, String)],
    ) -> Result<String, GatewayError> {
        let session = gateway
            .create_session_with_system_prompt(moderator, PromptTemplate::synthesis_system())
            .await?;

        let prompt = PromptTemplate::synthesis_prompt(question, responses, reviews);
        session.send(&prompt).await
    }
}
