//! `DebateStrategyExecutor` — adversarial discussion (#314, RFC #313 Phase A).
//!
//! Unlike [`QuorumStrategyExecutor`](super::quorum_strategy::QuorumStrategyExecutor)'s
//! equal peers, Debate assigns fixed opposing roles and runs turn-based rounds:
//!
//! ```text
//! Position assignment (proponent = roster[0], opponent = roster[1])
//!   → Round 1: proponent opens, opponent attacks
//!   → Round 2..max_rounds: proponent defends, opponent attacks, interjectors weigh in
//!     (each round ends with a moderator checkpoint — may settle early)
//!   → Moderator's settling checkpoint IS the synthesis
//! ```
//!
//! Turns are inherently sequential (each depends on the prior turn's content), so
//! unlike Quorum's fan-out phases, this does not parallelize model calls — and does
//! not stream per-chunk output (a straightforward follow-up, not required for #314's
//! acceptance criteria).

use super::strategy_executor::StrategyExecutor;
use super::types::{RunQuorumError, RunQuorumInput};
use crate::ports::llm_gateway::LlmGateway;
use crate::ports::progress::ProgressNotifier;
use async_trait::async_trait;
use quorum_domain::quorum::parsing::parse_debate_verdict;
use quorum_domain::{
    DebateConfig, DebatePromptTemplate, Model, ModelResponse, OrchestrationStrategy, PeerReview,
    Phase, QuorumResult, SynthesisResult,
};
use std::sync::Arc;
use tracing::{info, warn};

/// Adversarial discussion: fixed proponent/opponent roles attack and defend a
/// position across rounds, with an optional third-party interjector, until a
/// moderator settles the debate (or `max_rounds` is exhausted).
pub struct DebateStrategyExecutor;

impl DebateStrategyExecutor {
    pub fn new() -> Self {
        Self
    }

    /// Roster of debating models. `DebateConfig.models` is the primary source
    /// (it carries its own 3-model default specifically to seed interjection);
    /// falls back to `ModelConfig.participants` only if left empty.
    fn roster(&self, config: &DebateConfig, input: &RunQuorumInput) -> Vec<Model> {
        if config.models.is_empty() {
            input.models.participants.clone()
        } else {
            config.models.clone()
        }
    }

    async fn query(
        gateway: &Arc<dyn LlmGateway>,
        model: &Model,
        system_prompt: &str,
        prompt: &str,
    ) -> Result<String, RunQuorumError> {
        let session = gateway
            .create_session_with_system_prompt(model, system_prompt)
            .await?;
        Ok(session.send(prompt).await?)
    }

    fn render_transcript(turns: &[(String, String)]) -> String {
        turns
            .iter()
            .map(|(label, content)| format!("--- {} ---\n{}", label, content))
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

impl Default for DebateStrategyExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StrategyExecutor for DebateStrategyExecutor {
    fn name(&self) -> &'static str {
        "debate"
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
        // Dispatch to this executor only happens for the Debate variant — see
        // RunQuorumUseCase's exhaustive match. Any other variant here is a caller bug.
        let OrchestrationStrategy::Debate(config) = &input.strategy else {
            unreachable!("DebateStrategyExecutor invoked for a non-Debate strategy")
        };

        let roster = self.roster(config, input);
        if roster.len() < 2 {
            return Err(RunQuorumError::NotEnoughModelsForDebate(roster.len()));
        }
        let proponent = roster[0].clone();
        let opponent = roster[1].clone();
        let interjectors: Vec<Model> = if config.allow_interjection && roster.len() > 2 {
            roster[2..].to_vec()
        } else {
            vec![]
        };
        let moderator = config
            .moderator
            .clone()
            .unwrap_or_else(|| input.models.moderator.clone());
        let question = input.question.content();
        let max_rounds = config.max_rounds.max(1);

        info!(
            "Debate: proponent={}, opponent={}, interjectors={}, moderator={}, max_rounds={}",
            proponent,
            opponent,
            interjectors.len(),
            moderator,
            max_rounds
        );

        let mut transcript: Vec<(String, String)> = Vec::new();
        let mut model_responses: Vec<ModelResponse> = Vec::new();
        let mut peer_reviews: Vec<PeerReview> = Vec::new();

        // Phase: Initial — position assignment + opening statement
        progress.on_phase_start(&Phase::Initial, 1);
        let opening = match Self::query(
            &gateway,
            &proponent,
            &DebatePromptTemplate::proponent_system(config.intensity),
            &DebatePromptTemplate::proponent_opening_prompt(question),
        )
        .await
        {
            Ok(text) => {
                progress.on_task_complete(&Phase::Initial, &proponent, true);
                text
            }
            Err(e) => {
                progress.on_task_complete(&Phase::Initial, &proponent, false);
                return Err(e);
            }
        };
        model_responses.push(ModelResponse::success(
            proponent.to_string(),
            opening.clone(),
        ));
        transcript.push(("Proponent (opening)".to_string(), opening));
        progress.on_phase_complete(&Phase::Initial);

        // Phase: Review — attack/defense rounds, interjections, moderator checkpoints
        let review_task_estimate = max_rounds * (2 + interjectors.len());
        progress.on_phase_start(&Phase::Review, review_task_estimate);

        let mut settled_synthesis: Option<SynthesisResult> = None;

        for round in 1..=max_rounds {
            if round > 1 {
                let defense_prompt = DebatePromptTemplate::proponent_defense_prompt(
                    question,
                    &Self::render_transcript(&transcript),
                );
                let defense = match Self::query(
                    &gateway,
                    &proponent,
                    &DebatePromptTemplate::proponent_system(config.intensity),
                    &defense_prompt,
                )
                .await
                {
                    Ok(text) => {
                        progress.on_task_complete(&Phase::Review, &proponent, true);
                        text
                    }
                    Err(e) => {
                        progress.on_task_complete(&Phase::Review, &proponent, false);
                        return Err(e);
                    }
                };
                model_responses.push(ModelResponse::success(
                    proponent.to_string(),
                    defense.clone(),
                ));
                transcript.push((format!("Proponent (round {} defense)", round), defense));
            }

            let attack_prompt = DebatePromptTemplate::opponent_attack_prompt(
                question,
                &Self::render_transcript(&transcript),
            );
            let attack_text = match Self::query(
                &gateway,
                &opponent,
                &DebatePromptTemplate::opponent_system(config.intensity),
                &attack_prompt,
            )
            .await
            {
                Ok(text) => {
                    progress.on_task_complete(&Phase::Review, &opponent, true);
                    text
                }
                Err(e) => {
                    progress.on_task_complete(&Phase::Review, &opponent, false);
                    return Err(e);
                }
            };
            peer_reviews.push(PeerReview::new(
                opponent.to_string(),
                format!("Round {} proposal", round),
                attack_text.clone(),
            ));
            transcript.push((format!("Opponent (round {} attack)", round), attack_text));

            for interjector in &interjectors {
                let note_prompt = DebatePromptTemplate::interjector_prompt(
                    question,
                    &Self::render_transcript(&transcript),
                );
                match Self::query(
                    &gateway,
                    interjector,
                    DebatePromptTemplate::interjector_system(),
                    &note_prompt,
                )
                .await
                {
                    Ok(text) => {
                        progress.on_task_complete(&Phase::Review, interjector, true);
                        peer_reviews.push(PeerReview::new(
                            interjector.to_string(),
                            format!("Round {} proposal", round),
                            text.clone(),
                        ));
                        transcript.push((
                            format!("Interjection ({}, round {})", interjector, round),
                            text,
                        ));
                    }
                    Err(e) => {
                        // An interjector is a sideline voice, not load-bearing — log and
                        // continue rather than aborting the whole debate over it.
                        warn!(
                            "Interjector {} failed in round {}: {}",
                            interjector, round, e
                        );
                        progress.on_task_complete(&Phase::Review, interjector, false);
                    }
                }
            }

            let is_final_round = round == max_rounds;
            let checkpoint_prompt = DebatePromptTemplate::moderator_checkpoint_prompt(
                question,
                &Self::render_transcript(&transcript),
                round,
                max_rounds,
                is_final_round,
            );
            let checkpoint_text = match Self::query(
                &gateway,
                &moderator,
                DebatePromptTemplate::moderator_system(),
                &checkpoint_prompt,
            )
            .await
            {
                Ok(text) => {
                    progress.on_task_complete(&Phase::Review, &moderator, true);
                    text
                }
                Err(e) => {
                    progress.on_task_complete(&Phase::Review, &moderator, false);
                    return Err(e);
                }
            };

            let (verdict_settled, body) = parse_debate_verdict(&checkpoint_text);
            if verdict_settled || is_final_round {
                settled_synthesis = Some(SynthesisResult::new(moderator.to_string(), body));
                break;
            }
            transcript.push((format!("Moderator (round {} note)", round), body));
        }

        progress.on_phase_complete(&Phase::Review);

        // The final round always forces a settle, so the loop never exits without one.
        let synthesis = settled_synthesis.expect("debate loop always produces a synthesis");

        progress.on_phase_start(&Phase::Synthesis, 1);
        progress.on_task_complete(&Phase::Synthesis, &moderator, true);
        progress.on_phase_complete(&Phase::Synthesis);

        Ok(QuorumResult::new(
            input.question.content(),
            roster.iter().map(|m| m.to_string()).collect(),
            model_responses,
            peer_reviews,
            synthesis,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::progress::NoProgress;
    use crate::use_cases::run_quorum::test_support::ScriptedGateway;
    use quorum_domain::{DebateIntensity, ModelConfig};

    fn base_input(strategy: OrchestrationStrategy) -> RunQuorumInput {
        RunQuorumInput::new(
            "Should the cache be write-through or write-behind?",
            ModelConfig::default(),
        )
        .with_strategy(strategy)
    }

    fn debate_config(models: Vec<Model>, max_rounds: usize) -> DebateConfig {
        DebateConfig {
            models,
            moderator: Some(Model::ClaudeSonnet45),
            intensity: DebateIntensity::Strong,
            allow_interjection: false,
            max_rounds,
        }
    }

    #[tokio::test]
    async fn debate_settles_early_when_moderator_agrees() {
        let config = debate_config(vec![Model::Gpt53Codex, Model::Gemini31Pro], 5);
        let input = base_input(OrchestrationStrategy::Debate(config));

        let gateway = ScriptedGateway::new();
        gateway.respond(Model::Gpt53Codex, "Write-through keeps consistency simple.");
        gateway.respond(Model::Gemini31Pro, "Write-behind has better write latency.");
        gateway.respond(
            Model::ClaudeSonnet45,
            "VERDICT: SETTLED\n\nWrite-through wins for correctness-critical systems.",
        );

        let result = DebateStrategyExecutor::new()
            .execute(&input, Arc::new(gateway), &NoProgress)
            .await
            .unwrap();

        assert_eq!(result.responses.len(), 1); // opening only, settled before round 2's defense
        assert_eq!(result.reviews.len(), 1); // opponent's round-1 attack
        assert_eq!(
            result.synthesis.conclusion,
            "Write-through wins for correctness-critical systems."
        );
    }

    #[tokio::test]
    async fn debate_forces_settlement_at_max_rounds() {
        let config = debate_config(vec![Model::Gpt53Codex, Model::Gemini31Pro], 2);
        let input = base_input(OrchestrationStrategy::Debate(config));

        let gateway = ScriptedGateway::new();
        gateway.respond(Model::Gpt53Codex, "Opening: write-through.");
        gateway.respond(Model::Gemini31Pro, "Attack round 1: latency matters more.");
        gateway.respond(
            Model::ClaudeSonnet45,
            "VERDICT: CONTINUE\n\nStill contested: latency vs consistency.",
        );
        gateway.respond(
            Model::Gpt53Codex,
            "Defense round 2: consistency still wins.",
        );
        gateway.respond(Model::Gemini31Pro, "Attack round 2: but writes pile up.");
        gateway.respond(
            Model::ClaudeSonnet45,
            "VERDICT: CONTINUE\n\nNo real resolution, but rounds are up.",
        );

        let result = DebateStrategyExecutor::new()
            .execute(&input, Arc::new(gateway), &NoProgress)
            .await
            .unwrap();

        // Forced settle at round 2 despite the moderator saying CONTINUE.
        assert_eq!(result.responses.len(), 2); // opening + round-2 defense
        assert_eq!(result.reviews.len(), 2); // round-1 + round-2 attacks
        assert_eq!(
            result.synthesis.conclusion,
            "No real resolution, but rounds are up."
        );
    }

    #[tokio::test]
    async fn debate_allows_interjection_with_three_plus_models() {
        let config = DebateConfig {
            allow_interjection: true,
            ..debate_config(
                vec![Model::Gpt53Codex, Model::Gemini31Pro, Model::ClaudeHaiku45],
                1,
            )
        };
        let input = base_input(OrchestrationStrategy::Debate(config));

        let gateway = ScriptedGateway::new();
        gateway.respond(Model::Gpt53Codex, "Opening: write-through.");
        gateway.respond(Model::Gemini31Pro, "Attack: latency matters more.");
        gateway.respond(
            Model::ClaudeHaiku45,
            "Interjection: both ignore cold-start cost.",
        );
        gateway.respond(
            Model::ClaudeSonnet45,
            "VERDICT: SETTLED\n\nWrite-through, with a cache warmer.",
        );

        let result = DebateStrategyExecutor::new()
            .execute(&input, Arc::new(gateway), &NoProgress)
            .await
            .unwrap();

        assert_eq!(result.reviews.len(), 2); // opponent attack + interjector note
        assert!(
            result
                .reviews
                .iter()
                .any(|r| r.reviewer == Model::ClaudeHaiku45.to_string())
        );
    }

    #[tokio::test]
    async fn debate_rejects_single_model_roster() {
        let config = debate_config(vec![Model::Gpt53Codex], 3);
        let input = base_input(OrchestrationStrategy::Debate(config));
        let gateway = ScriptedGateway::new();

        let err = DebateStrategyExecutor::new()
            .execute(&input, Arc::new(gateway), &NoProgress)
            .await
            .unwrap_err();

        assert!(matches!(err, RunQuorumError::NotEnoughModelsForDebate(1)));
    }
}
