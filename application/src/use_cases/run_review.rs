//! Run Review use case (#300, RFC Discussion #304 D2)
//!
//! Headless multi-model Quorum review of a diff/PR: reuses the structured
//! vote parsing from `run_agent::review` (`query_model_for_review` +
//! `parse_review_response`) for the per-model vote phase, and the
//! `RunQuorumUseCase::phase_synthesis` pattern (moderator synthesizes votes +
//! feedback into one review) for the synthesis phase. Read-only: no tool use,
//! no HiL.

use super::run_agent::review::query_model_for_review;
use crate::ports::agent_progress::AgentProgressNotifier;
use crate::ports::event_publisher::{AppEvent, EventPublisher, NoEventPublisher};
use crate::ports::llm_gateway::{GatewayError, LlmGateway};
use quorum_domain::quorum::{QuorumResultPayload, QuorumTopic, Vote, VoteResult};
use quorum_domain::{ModelConfig, ReviewPromptTemplate, SynthesisResult};
use std::sync::Arc;
use thiserror::Error;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::warn;

/// Errors that can occur during a headless review
#[derive(Error, Debug)]
pub enum RunReviewError {
    #[error("No review models configured")]
    NoModels,
    #[error("All review models failed to respond")]
    QuorumFailed,
    #[error("Gateway error: {0}")]
    GatewayError(#[from] GatewayError),
    #[error("Cancelled")]
    Cancelled,
}

/// Input for the RunReview use case
#[derive(Debug, Clone)]
pub struct RunReviewInput {
    /// The review material — diff, with optional PR context / focus already
    /// embedded (see [`ReviewPromptTemplate::build_material`]).
    pub material: String,
    /// Model configuration (`review` votes, `moderator` synthesizes)
    pub models: ModelConfig,
}

impl RunReviewInput {
    pub fn new(material: impl Into<String>, models: ModelConfig) -> Self {
        Self {
            material: material.into(),
            models,
        }
    }
}

/// Output of a completed review
#[derive(Debug, Clone)]
pub struct RunReviewOutput {
    /// Whether the quorum approved (majority of cast votes)
    pub approved: bool,
    /// All individual votes (including abstain / model_error)
    pub votes: Vec<Vote>,
    /// The moderator's synthesized review
    pub synthesis: SynthesisResult,
}

/// Use case for running a headless PR/diff review
#[derive(Clone)]
pub struct RunReviewUseCase {
    gateway: Arc<dyn LlmGateway>,
    event_publisher: Arc<dyn EventPublisher>,
    cancellation_token: Option<CancellationToken>,
}

impl RunReviewUseCase {
    pub fn new(gateway: Arc<dyn LlmGateway>) -> Self {
        Self {
            gateway,
            event_publisher: Arc::new(NoEventPublisher),
            cancellation_token: None,
        }
    }

    pub fn with_event_publisher(mut self, publisher: Arc<dyn EventPublisher>) -> Self {
        self.event_publisher = publisher;
        self
    }

    pub fn with_cancellation(mut self, token: CancellationToken) -> Self {
        self.cancellation_token = Some(token);
        self
    }

    pub async fn execute(
        &self,
        input: RunReviewInput,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<RunReviewOutput, RunReviewError> {
        let models = &input.models.review;
        if models.is_empty() {
            return Err(RunReviewError::NoModels);
        }

        progress.on_quorum_start("pr_review", models.len());

        let prompt = ReviewPromptTemplate::review_prompt(&input.material);

        let mut join_set = JoinSet::new();
        for model in models {
            let gateway = Arc::clone(&self.gateway);
            let model = model.clone();
            let prompt = prompt.clone();

            join_set.spawn(async move {
                let result = query_model_for_review(gateway.as_ref(), &model, &prompt).await;
                (model, result)
            });
        }

        let mut votes = Vec::new();

        loop {
            let result = if let Some(ref token) = self.cancellation_token {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => {
                        join_set.abort_all();
                        return Err(RunReviewError::Cancelled);
                    }
                    result = join_set.join_next() => result,
                }
            } else {
                join_set.join_next().await
            };

            let Some(result) = result else {
                break;
            };

            match result {
                Ok((model, Ok(response))) => {
                    let (approved, feedback) =
                        quorum_domain::quorum::parse_review_response(&response);
                    progress.on_quorum_model_complete(&model, approved);
                    votes.push(if approved {
                        Vote::approve(model.to_string(), feedback)
                    } else {
                        Vote::reject(model.to_string(), feedback)
                    });
                }
                Ok((model, Err(e))) => {
                    warn!("Model {} failed to review: {}", model, e);
                    progress.on_quorum_model_complete(&model, false);
                    votes.push(Vote::model_error(model.to_string(), e.to_string()));
                }
                Err(e) => {
                    warn!("Task join error: {}", e);
                }
            }
        }

        if !votes.iter().any(Vote::is_cast) {
            return Err(RunReviewError::QuorumFailed);
        }

        let vote_result = VoteResult::from_votes(votes);

        progress.on_quorum_complete_with_votes(
            "pr_review",
            vote_result.passed,
            &vote_result.votes,
            vote_result.aggregated_feedback.as_deref(),
        );

        // Synthesis phase: moderator combines material + votes into one review.
        let moderator = input.models.moderator.clone();
        let synthesis_prompt =
            ReviewPromptTemplate::synthesis_prompt(&input.material, &vote_result.votes);
        let session = self
            .gateway
            .create_text_only_session(&moderator, ReviewPromptTemplate::synthesis_system())
            .await?;
        let synthesis_content = session.send(&synthesis_prompt).await?;
        let synthesis = SynthesisResult::new(moderator.to_string(), synthesis_content);

        self.event_publisher
            .publish(AppEvent::QuorumResult(Box::new(
                QuorumResultPayload::new(QuorumTopic::PrReview, None, &vote_result)
                    .with_synthesis(synthesis.clone()),
            )));

        Ok(RunReviewOutput {
            approved: vote_result.passed,
            votes: vote_result.votes,
            synthesis,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::event_publisher::AppEvent;
    use crate::ports::llm_gateway::LlmSession;
    use async_trait::async_trait;
    use quorum_domain::Model;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct ScriptedSession {
        model: Model,
        response: String,
    }

    #[async_trait]
    impl LlmSession for ScriptedSession {
        fn model(&self) -> &Model {
            &self.model
        }

        async fn send(&self, _content: &str) -> Result<String, GatewayError> {
            Ok(self.response.clone())
        }
    }

    struct ScriptedGateway {
        // Sessions returned in order, keyed by call index across the whole test.
        sessions: Mutex<VecDeque<Box<dyn LlmSession>>>,
    }

    impl ScriptedGateway {
        fn new(sessions: Vec<Box<dyn LlmSession>>) -> Self {
            Self {
                sessions: Mutex::new(VecDeque::from(sessions)),
            }
        }
    }

    #[async_trait]
    impl LlmGateway for ScriptedGateway {
        async fn create_session(
            &self,
            _model: &Model,
        ) -> Result<Box<dyn LlmSession>, GatewayError> {
            self.sessions
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| GatewayError::Other("no more sessions".to_string()))
        }

        async fn create_session_with_system_prompt(
            &self,
            model: &Model,
            _system_prompt: &str,
        ) -> Result<Box<dyn LlmSession>, GatewayError> {
            self.create_session(model).await
        }

        async fn available_models(&self) -> Result<Vec<Model>, GatewayError> {
            Ok(vec![Model::default()])
        }
    }

    fn model_config(review: Vec<Model>, moderator: Model) -> ModelConfig {
        ModelConfig::default()
            .with_review(review)
            .with_moderator(moderator)
    }

    #[tokio::test]
    async fn test_execute_approves_on_majority() {
        let gateway = Arc::new(ScriptedGateway::new(vec![
            Box::new(ScriptedSession {
                model: Model::ClaudeSonnet45,
                response: "APPROVE. Looks safe.".into(),
            }),
            Box::new(ScriptedSession {
                model: Model::Gpt53Codex,
                response: "APPROVE. LGTM.".into(),
            }),
            // Moderator synthesis session
            Box::new(ScriptedSession {
                model: Model::ClaudeSonnet45,
                response: "Both reviewers approved. Merge it.".into(),
            }),
        ]));
        let use_case = RunReviewUseCase::new(gateway);
        let input = RunReviewInput::new(
            "diff --git a b",
            model_config(
                vec![Model::ClaudeSonnet45, Model::Gpt53Codex],
                Model::ClaudeSonnet45,
            ),
        );

        let output = use_case
            .execute(input, &crate::ports::agent_progress::NoAgentProgress)
            .await
            .unwrap();

        assert!(output.approved);
        assert_eq!(output.votes.len(), 2);
        assert_eq!(
            output.synthesis.conclusion,
            "Both reviewers approved. Merge it."
        );
    }

    #[tokio::test]
    async fn test_execute_rejects_on_majority_reject() {
        let gateway = Arc::new(ScriptedGateway::new(vec![
            Box::new(ScriptedSession {
                model: Model::ClaudeSonnet45,
                response: "REJECT. Missing tests.".into(),
            }),
            Box::new(ScriptedSession {
                model: Model::Gpt53Codex,
                response: "REJECT. Security issue.".into(),
            }),
            Box::new(ScriptedSession {
                model: Model::ClaudeSonnet45,
                response: "Both reviewers rejected. Needs work.".into(),
            }),
        ]));
        let use_case = RunReviewUseCase::new(gateway);
        let input = RunReviewInput::new(
            "diff --git a b",
            model_config(
                vec![Model::ClaudeSonnet45, Model::Gpt53Codex],
                Model::ClaudeSonnet45,
            ),
        );

        let output = use_case
            .execute(input, &crate::ports::agent_progress::NoAgentProgress)
            .await
            .unwrap();

        assert!(!output.approved);
        assert_eq!(output.votes.iter().filter(|v| v.is_reject()).count(), 2);
    }

    #[tokio::test]
    async fn test_execute_no_models_configured() {
        let gateway = Arc::new(ScriptedGateway::new(vec![]));
        let use_case = RunReviewUseCase::new(gateway);
        let input = RunReviewInput::new("diff --git a b", model_config(vec![], Model::default()));

        let err = use_case
            .execute(input, &crate::ports::agent_progress::NoAgentProgress)
            .await
            .unwrap_err();
        assert!(matches!(err, RunReviewError::NoModels));
    }

    #[tokio::test]
    async fn test_execute_all_models_fail_is_quorum_failed() {
        // No sessions queued at all → every review query fails with a gateway error.
        let gateway = Arc::new(ScriptedGateway::new(vec![]));
        let use_case = RunReviewUseCase::new(gateway);
        let input = RunReviewInput::new(
            "diff --git a b",
            model_config(vec![Model::ClaudeSonnet45], Model::ClaudeSonnet45),
        );

        let err = use_case
            .execute(input, &crate::ports::agent_progress::NoAgentProgress)
            .await
            .unwrap_err();
        assert!(matches!(err, RunReviewError::QuorumFailed));
    }

    #[tokio::test]
    async fn test_execute_publishes_quorum_result_with_synthesis() {
        struct RecordingPublisher {
            events: Mutex<Vec<AppEvent>>,
        }
        impl EventPublisher for RecordingPublisher {
            fn publish(&self, event: AppEvent) {
                self.events.lock().unwrap().push(event);
            }
        }

        let gateway = Arc::new(ScriptedGateway::new(vec![
            Box::new(ScriptedSession {
                model: Model::ClaudeSonnet45,
                response: "APPROVE.".into(),
            }),
            Box::new(ScriptedSession {
                model: Model::ClaudeSonnet45,
                response: "Approved unanimously.".into(),
            }),
        ]));
        let publisher = Arc::new(RecordingPublisher {
            events: Mutex::new(Vec::new()),
        });
        let use_case = RunReviewUseCase::new(gateway).with_event_publisher(publisher.clone());
        let input = RunReviewInput::new(
            "diff --git a b",
            model_config(vec![Model::ClaudeSonnet45], Model::ClaudeSonnet45),
        );

        use_case
            .execute(input, &crate::ports::agent_progress::NoAgentProgress)
            .await
            .unwrap();

        let events = publisher.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        let AppEvent::QuorumResult(payload) = &events[0] else {
            panic!("expected QuorumResult, got {:?}", events[0]);
        };
        assert_eq!(payload.topic, QuorumTopic::PrReview);
        assert!(payload.synthesis.is_some());
    }
}
