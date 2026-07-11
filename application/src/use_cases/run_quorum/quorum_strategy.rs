//! `QuorumStrategyExecutor` — the standard Quorum discussion flow.
//!
//! Initial Query → Peer Review (optional) → Synthesis. Extracted from what used to be
//! `RunQuorumUseCase`'s body verbatim (#314); behavior is unchanged, only `gateway`
//! moved from a struct field to a per-call parameter (see [`StrategyExecutor`]).

use super::strategy_executor::StrategyExecutor;
use super::types::{RunQuorumError, RunQuorumInput};
use crate::ports::llm_gateway::{GatewayError, LlmGateway, StreamObserver};
use crate::ports::progress::ProgressNotifier;
use async_trait::async_trait;
use quorum_domain::{
    Model, ModelResponse, PeerReview, Phase, PromptTemplate, QuorumResult, StreamContext,
    SynthesisResult,
};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

/// Equal-peer discussion: all models answer independently, review each other
/// anonymously, and a moderator synthesizes the final answer.
pub struct QuorumStrategyExecutor;

impl QuorumStrategyExecutor {
    pub fn new() -> Self {
        Self
    }

    /// Phase 1: Query all models in parallel with real-time streaming
    async fn phase_initial(
        &self,
        gateway: &Arc<dyn LlmGateway>,
        input: &RunQuorumInput,
        progress: &dyn ProgressNotifier,
    ) -> Result<Vec<ModelResponse>, RunQuorumError> {
        info!("Phase 1: Initial Query");
        progress.on_phase_start(&Phase::Initial, input.models.participants.len());

        let (agg_tx, mut agg_rx) = mpsc::unbounded_channel::<(String, String)>();
        let mut join_set = JoinSet::new();

        for model in &input.models.participants {
            let gateway = Arc::clone(gateway);
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
        gateway: &Arc<dyn LlmGateway>,
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

            let gateway = Arc::clone(gateway);
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
        gateway: &Arc<dyn LlmGateway>,
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
            gateway.as_ref(),
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

impl Default for QuorumStrategyExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StrategyExecutor for QuorumStrategyExecutor {
    fn name(&self) -> &'static str {
        "quorum"
    }

    fn phases(&self) -> Vec<Phase> {
        vec![Phase::Initial, Phase::Review, Phase::Synthesis]
    }

    async fn execute(
        &self,
        input: &RunQuorumInput,
        gateway: Arc<dyn LlmGateway>,
        progress: &dyn ProgressNotifier,
    ) -> Result<QuorumResult, RunQuorumError> {
        // Phase 1: Initial Query
        let responses = self.phase_initial(&gateway, input, progress).await?;

        // Check if we have any successful responses
        let successful_responses: Vec<_> = responses.iter().filter(|r| r.success).collect();
        if successful_responses.is_empty() {
            return Err(RunQuorumError::AllModelsFailed);
        }

        // Phase 2: Peer Review (optional)
        let reviews = if input.enable_review && successful_responses.len() > 1 {
            self.phase_review(&gateway, input, &responses, progress)
                .await?
        } else {
            debug!("Skipping peer review phase");
            vec![]
        };

        // Phase 3: Synthesis
        let synthesis = self
            .phase_synthesis(&gateway, input, &responses, &reviews, progress)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::progress::NoProgress;
    use crate::use_cases::run_quorum::test_support::ScriptedGateway;
    use quorum_domain::ModelConfig;

    fn input(models: ModelConfig) -> RunQuorumInput {
        RunQuorumInput::new("What is the best caching strategy?", models)
    }

    #[tokio::test]
    async fn quorum_flow_runs_initial_review_and_synthesis() {
        let models = ModelConfig::default()
            .with_participants(vec![Model::ClaudeSonnet45, Model::Gpt53Codex])
            .with_moderator(Model::ClaudeSonnet45);

        let gateway = ScriptedGateway::new();
        gateway.respond(Model::ClaudeSonnet45, "Use a write-through cache.");
        gateway.respond(Model::Gpt53Codex, "Use a write-behind cache.");
        gateway.respond(Model::ClaudeSonnet45, "The other response has merit too.");
        gateway.respond(Model::Gpt53Codex, "Write-through is safer.");
        gateway.respond(Model::ClaudeSonnet45, "Final synthesis: use write-through.");

        let result = QuorumStrategyExecutor::new()
            .execute(&input(models), Arc::new(gateway), &NoProgress)
            .await
            .unwrap();

        assert_eq!(result.responses.len(), 2);
        assert_eq!(result.reviews.len(), 2);
        assert_eq!(
            result.synthesis.conclusion,
            "Final synthesis: use write-through."
        );
    }

    #[tokio::test]
    async fn quorum_flow_skips_review_when_disabled() {
        let models = ModelConfig::default()
            .with_participants(vec![Model::ClaudeSonnet45, Model::Gpt53Codex])
            .with_moderator(Model::ClaudeSonnet45);

        let gateway = ScriptedGateway::new();
        gateway.respond(Model::ClaudeSonnet45, "Use a write-through cache.");
        gateway.respond(Model::Gpt53Codex, "Use a write-behind cache.");
        gateway.respond(Model::ClaudeSonnet45, "Final synthesis: use write-through.");

        let result = QuorumStrategyExecutor::new()
            .execute(
                &input(models).without_review(),
                Arc::new(gateway),
                &NoProgress,
            )
            .await
            .unwrap();

        assert_eq!(result.responses.len(), 2);
        assert!(result.reviews.is_empty());
    }

    #[tokio::test]
    async fn quorum_flow_fails_when_all_models_fail() {
        let models = ModelConfig::default().with_participants(vec![Model::ClaudeSonnet45]);

        // No scripted response — the single participant's call fails.
        let gateway = ScriptedGateway::new();

        let err = QuorumStrategyExecutor::new()
            .execute(&input(models), Arc::new(gateway), &NoProgress)
            .await
            .unwrap_err();

        assert!(matches!(err, RunQuorumError::AllModelsFailed));
    }
}
