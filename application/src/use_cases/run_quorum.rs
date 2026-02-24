//! Run Quorum use case
//!
//! Orchestrates the full Quorum discussion flow.

use crate::ports::llm_gateway::{GatewayError, LlmGateway, StreamObserver};
use crate::ports::progress::{NoProgress, ProgressNotifier};
use quorum_domain::{
    Model, ModelConfig, ModelResponse, PeerReview, Phase, PromptTemplate, Question, QuorumResult,
    StreamContext, SynthesisResult,
};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
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
pub struct RunQuorumUseCase {
    gateway: Arc<dyn LlmGateway>,
}

impl RunQuorumUseCase {
    pub fn new(gateway: Arc<dyn LlmGateway>) -> Self {
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
            input
                .models
                .participants
                .iter()
                .map(|m| m.to_string())
                .collect(),
            responses,
            reviews,
            synthesis,
        ))
    }

    /// Phase 1: Query all models in parallel with real-time streaming
    async fn phase_initial(
        &self,
        input: &RunQuorumInput,
        progress: &dyn ProgressNotifier,
    ) -> Result<Vec<ModelResponse>, RunQuorumError> {
        info!("Phase 1: Initial Query");
        progress.on_phase_start(&Phase::Initial, input.models.participants.len());

        let (agg_tx, mut agg_rx) = mpsc::unbounded_channel::<(String, String)>();
        let mut join_set = JoinSet::new();

        for model in &input.models.participants {
            let gateway = Arc::clone(&self.gateway);
            let model = model.clone();
            let question = input.question.content().to_string();

            progress.on_model_stream_start(&model, &StreamContext::QuorumInitial);

            // Create observer for this model's streaming chunks
            let tx = agg_tx.clone();
            let model_name = model.to_string();
            let observer: StreamObserver = Arc::new(move |chunk: &str| {
                let _ = tx.send((model_name.clone(), chunk.to_string()));
            });

            join_set.spawn(async move {
                let result =
                    Self::query_model_streaming(gateway.as_ref(), &model, &question, observer)
                        .await;
                (model, result)
            });
        }
        drop(agg_tx);

        let mut responses = Vec::new();

        loop {
            tokio::select! {
                biased;
                Some((model, chunk)) = agg_rx.recv() => {
                    progress.on_model_stream_chunk(&model, &chunk);
                    continue;
                }
                result = join_set.join_next() => {
                    let Some(result) = result else { break };
                    match result {
                        Ok((model, Ok(content))) => {
                            info!("Model {} responded successfully", model);
                            progress.on_model_stream_end(&model.to_string());
                            progress.on_task_complete(&Phase::Initial, &model, true);
                            responses.push(ModelResponse::success(model.to_string(), content));
                        }
                        Ok((model, Err(e))) => {
                            warn!("Model {} failed: {}", model, e);
                            progress.on_model_stream_end(&model.to_string());
                            progress.on_task_complete(&Phase::Initial, &model, false);
                            responses.push(ModelResponse::failure(model.to_string(), e.to_string()));
                        }
                        Err(e) => {
                            warn!("Task join error: {}", e);
                        }
                    }
                }
            }
        }

        progress.on_phase_complete(&Phase::Initial);
        Ok(responses)
    }

    /// Phase 2: Each model reviews others' responses with streaming
    async fn phase_review(
        &self,
        input: &RunQuorumInput,
        responses: &[ModelResponse],
        progress: &dyn ProgressNotifier,
    ) -> Result<Vec<PeerReview>, RunQuorumError> {
        info!("Phase 2: Peer Review");

        let successful_responses: Vec<_> = responses.iter().filter(|r| r.success).collect();
        progress.on_phase_start(&Phase::Review, successful_responses.len());

        let (agg_tx, mut agg_rx) = mpsc::unbounded_channel::<(String, String)>();
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

            progress.on_model_stream_start(&model, &StreamContext::QuorumReview);

            let tx = agg_tx.clone();
            let model_name = model.to_string();
            let observer: StreamObserver = Arc::new(move |chunk: &str| {
                let _ = tx.send((model_name.clone(), chunk.to_string()));
            });

            join_set.spawn(async move {
                let result = Self::review_responses_streaming(
                    gateway.as_ref(),
                    &model,
                    &question,
                    &other_responses,
                    observer,
                )
                .await;
                (model, other_responses, result)
            });
        }
        drop(agg_tx);

        let mut reviews = Vec::new();

        loop {
            tokio::select! {
                biased;
                Some((model, chunk)) = agg_rx.recv() => {
                    progress.on_model_stream_chunk(&model, &chunk);
                    continue;
                }
                result = join_set.join_next() => {
                    let Some(result) = result else { break };
                    match result {
                        Ok((model, reviewed, Ok(review_content))) => {
                            info!("Model {} completed review", model);
                            progress.on_model_stream_end(&model.to_string());
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
                            progress.on_model_stream_end(&model.to_string());
                            progress.on_task_complete(&Phase::Review, &model, false);
                        }
                        Err(e) => {
                            warn!("Task join error: {}", e);
                        }
                    }
                }
            }
        }

        progress.on_phase_complete(&Phase::Review);
        Ok(reviews)
    }

    /// Phase 3: Synthesize all responses and reviews with streaming
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

        progress.on_model_stream_start(&moderator, &StreamContext::QuorumSynthesis);

        // Use a channel to relay synthesis chunks to progress in real-time
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let moderator_name = moderator.to_string();
        let observer: StreamObserver = Arc::new(move |chunk: &str| {
            let _ = tx.send(chunk.to_string());
        });

        let synthesis_future = Self::synthesize_streaming(
            self.gateway.as_ref(),
            &moderator,
            input.question.content(),
            &successful_responses,
            &review_pairs,
            observer,
        );
        tokio::pin!(synthesis_future);

        let synthesis_content = loop {
            tokio::select! {
                biased;
                Some(chunk) = rx.recv() => {
                    progress.on_model_stream_chunk(&moderator_name, &chunk);
                }
                result = &mut synthesis_future => {
                    // Drain remaining chunks
                    while let Ok(chunk) = rx.try_recv() {
                        progress.on_model_stream_chunk(&moderator_name, &chunk);
                    }
                    break result?;
                }
            }
        };

        progress.on_model_stream_end(&moderator.to_string());
        progress.on_task_complete(&Phase::Synthesis, &moderator, true);
        progress.on_phase_complete(&Phase::Synthesis);

        Ok(SynthesisResult::new(
            moderator.to_string(),
            synthesis_content,
        ))
    }

    /// Query a single model with streaming observer
    async fn query_model_streaming(
        gateway: &dyn LlmGateway,
        model: &Model,
        question: &str,
        observer: StreamObserver,
    ) -> Result<String, GatewayError> {
        let session = gateway
            .create_streaming_session(model, PromptTemplate::initial_system(), observer)
            .await?;

        let prompt = PromptTemplate::initial_query(question);
        session.send(&prompt).await
    }

    /// Have a model review other responses with streaming
    async fn review_responses_streaming(
        gateway: &dyn LlmGateway,
        model: &Model,
        question: &str,
        responses: &[(String, String)],
        observer: StreamObserver,
    ) -> Result<String, GatewayError> {
        let session = gateway
            .create_streaming_session(model, PromptTemplate::review_system(), observer)
            .await?;

        let prompt = PromptTemplate::review_prompt(question, responses);
        session.send(&prompt).await
    }

    /// Synthesize all responses and reviews with streaming observer
    async fn synthesize_streaming(
        gateway: &dyn LlmGateway,
        moderator: &Model,
        question: &str,
        responses: &[(String, String)],
        reviews: &[(String, String)],
        observer: StreamObserver,
    ) -> Result<String, GatewayError> {
        let session = gateway
            .create_streaming_session(moderator, PromptTemplate::synthesis_system(), observer)
            .await?;

        let prompt = PromptTemplate::synthesis_prompt(question, responses, reviews);
        session.send(&prompt).await
    }
}
