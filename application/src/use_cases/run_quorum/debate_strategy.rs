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
use crate::ports::event_publisher::{AppEvent, EventPublisher};
use crate::ports::human_intervention::{HumanInterventionError, HumanInterventionPort};
use crate::ports::llm_gateway::LlmGateway;
use crate::ports::progress::ProgressNotifier;
use async_trait::async_trait;
use quorum_domain::quorum::parsing::{
    parse_debate_verdict, parse_decomposition_request, parse_divergence_check,
    parse_moderator_rulings, parse_opponent_rebuttals,
};
use quorum_domain::util::truncate_head_tail;
use quorum_domain::{
    DebateConfig, DebateIntensity, DebatePromptTemplate, HilMode, HumanDecision, Model,
    ModelResponse, Objection, ObjectionLedger, ObjectionStatus, OrchestrationStrategy, PeerReview,
    Phase, QuorumResult, QuorumResultPayload, QuorumTopic, SynthesisResult, Vote, VoteResult,
    VoteVerdict,
};
use std::sync::Arc;
use tracing::{info, warn};

/// Byte budget for the transcript summary handed to
/// `HumanInterventionPort::request_debate_escalation` — the port docs call
/// this a "condensed summary", and both the CLI and TUI render it inline
/// (a single modal line in the TUI), so the full `render_transcript` output
/// (unbounded, grows every round) would blow past what either surface can
/// reasonably display.
const TRANSCRIPT_SUMMARY_MAX_BYTES: usize = 2000;

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

    /// Query the opponent's attack for `round`, allowing at most one
    /// decomposition detour (#316's `DECOMPOSE_REQUEST:` protocol).
    ///
    /// If the opponent's response opens with a `DECOMPOSE_REQUEST:` line
    /// (see [`parse_decomposition_request`]), the proponent is asked to
    /// break the targeted claim into sub-claims via
    /// [`DebatePromptTemplate::proponent_decomposition_prompt`], that
    /// response is appended to `transcript`, and the opponent is re-queried
    /// against the updated transcript — all within the same round (`round`
    /// is only used for labeling/logging here; the caller's loop counter is
    /// never touched). If the re-query *also* opens with a
    /// `DECOMPOSE_REQUEST:`, the second request is ignored (one detour per
    /// round, enforced here rather than trusted to the model) — a warning
    /// is logged and the raw response is returned as-is for the caller to
    /// parse as a normal CLAIM/EVIDENCE/SEVERITY rebuttal (or treated as
    /// empty if it isn't one).
    #[allow(clippy::too_many_arguments)]
    async fn query_opponent_attack_with_decomposition(
        gateway: &Arc<dyn LlmGateway>,
        opponent: &Model,
        proponent: &Model,
        intensity: DebateIntensity,
        question: &str,
        transcript: &mut Vec<(String, String)>,
        round: usize,
        progress: &dyn ProgressNotifier,
    ) -> Result<String, RunQuorumError> {
        let attack_prompt = DebatePromptTemplate::opponent_attack_prompt(
            question,
            &Self::render_transcript(transcript),
        );
        let attack_text = match Self::query(
            gateway,
            opponent,
            &DebatePromptTemplate::opponent_system(intensity),
            &attack_prompt,
        )
        .await
        {
            Ok(text) => {
                progress.on_task_complete(&Phase::Review, opponent, true);
                text
            }
            Err(e) => {
                progress.on_task_complete(&Phase::Review, opponent, false);
                return Err(e);
            }
        };

        let Some(target) = parse_decomposition_request(&attack_text) else {
            return Ok(attack_text);
        };
        info!(
            "Opponent requested decomposition of \"{}\" in round {}",
            target, round
        );

        let decomposition_prompt = DebatePromptTemplate::proponent_decomposition_prompt(
            &target,
            &Self::render_transcript(transcript),
        );
        let decomposition_text = match Self::query(
            gateway,
            proponent,
            &DebatePromptTemplate::proponent_system(intensity),
            &decomposition_prompt,
        )
        .await
        {
            Ok(text) => {
                progress.on_task_complete(&Phase::Review, proponent, true);
                text
            }
            Err(e) => {
                progress.on_task_complete(&Phase::Review, proponent, false);
                return Err(e);
            }
        };
        transcript.push((
            format!("Proponent (round {} decomposition)", round),
            decomposition_text,
        ));

        let requery_prompt = DebatePromptTemplate::opponent_attack_prompt(
            question,
            &Self::render_transcript(transcript),
        );
        let requery_text = match Self::query(
            gateway,
            opponent,
            &DebatePromptTemplate::opponent_system(intensity),
            &requery_prompt,
        )
        .await
        {
            Ok(text) => {
                progress.on_task_complete(&Phase::Review, opponent, true);
                text
            }
            Err(e) => {
                progress.on_task_complete(&Phase::Review, opponent, false);
                return Err(e);
            }
        };

        if parse_decomposition_request(&requery_text).is_some() {
            warn!("decomposition request ignored — already used this round");
        }

        Ok(requery_text)
    }

    /// Decide what to do when a settle checkpoint (early verdict OR forced
    /// final round) is reached while critical/major objections are still
    /// unresolved.
    ///
    /// `can_continue` is `true` for a non-final round (a `Reject` decision
    /// declines the premature settle and the debate continues) and `false`
    /// at the final round (a `Reject` decision aborts the debate, since
    /// there is no next round to continue to) — see the caller's match on
    /// the returned `HumanDecision`.
    ///
    /// This mirrors `RunAgentUseCase::handle_human_intervention`
    /// (`run_agent/hil.rs`) so the three `HilMode` branches behave
    /// consistently across use cases:
    /// - `AutoReject` — fail-secure default, never silently settle.
    /// - `AutoApprove` — force the settlement through, loudly logged.
    /// - `Interactive` — defer to `HumanInterventionPort::request_debate_escalation`
    ///   if one is configured, otherwise fall back to `AutoReject`'s behavior.
    #[allow(clippy::too_many_arguments)]
    async fn handle_debate_escalation(
        hil_mode: HilMode,
        human_intervention: Option<&Arc<dyn HumanInterventionPort>>,
        question: &str,
        unresolved: &[Objection],
        transcript_summary: &str,
        can_continue: bool,
    ) -> Result<HumanDecision, RunQuorumError> {
        match hil_mode {
            HilMode::AutoReject => {
                warn!(
                    "Auto-rejecting debate settlement due to HilMode::AutoReject ({} unresolved critical/major objection(s))",
                    unresolved.len()
                );
                Ok(HumanDecision::Reject)
            }
            HilMode::AutoApprove => {
                warn!(
                    "Auto-approving debate settlement despite {} unresolved critical/major objection(s) (HilMode::AutoApprove) - use with caution!",
                    unresolved.len()
                );
                Ok(HumanDecision::Approve)
            }
            HilMode::Interactive => {
                if let Some(intervention) = human_intervention {
                    intervention
                        .request_debate_escalation(
                            question,
                            unresolved,
                            transcript_summary,
                            can_continue,
                        )
                        .await
                        .map_err(|e| match e {
                            HumanInterventionError::Cancelled => RunQuorumError::Cancelled,
                            _ => RunQuorumError::HumanInterventionFailed(e.to_string()),
                        })
                } else {
                    warn!(
                        "No human intervention handler configured for debate escalation, auto-rejecting ({} unresolved critical/major objection(s))",
                        unresolved.len()
                    );
                    Ok(HumanDecision::Reject)
                }
            }
        }
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
        event_publisher: Arc<dyn EventPublisher>,
        human_intervention: Option<Arc<dyn HumanInterventionPort>>,
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
        // Tracks the opponent's structured (CLAIM/EVIDENCE/SEVERITY) rebuttals
        // across rounds so the moderator can rule on each by stable ID instead
        // of re-deriving rebuttal identity from prose each checkpoint.
        let mut ledger = ObjectionLedger::new();
        // Guards the anti-mode-collapse "contrarian brief" (see below) so it
        // fires at most once per debate, even though it's only ever attempted
        // in round 1.
        let mut contrarian_brief_used = false;

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
        // Domain-computed outcome of the debate, independent of any vote
        // tally: flips true once a settle checkpoint is actually reached
        // (either a clean settle with no unresolved critical/major
        // objections, or a forced settle via `HumanDecision::Approve` at
        // escalation). The `Reject`/`Edit` escalation path returns an `Err`
        // before this point, so it never lingers `false` past the loop.
        let mut settled = false;

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

            let attack_text = Self::query_opponent_attack_with_decomposition(
                &gateway,
                &opponent,
                &proponent,
                config.intensity,
                question,
                &mut transcript,
                round,
                progress,
            )
            .await?;
            peer_reviews.push(PeerReview::new(
                opponent.to_string(),
                format!("Round {} proposal", round),
                attack_text.clone(),
            ));
            for (claim, evidence, severity) in parse_opponent_rebuttals(&attack_text) {
                ledger.add(round, claim, evidence, severity);
            }
            // Captured before `attack_text` is moved into `transcript` below —
            // used by the divergence check further down. Reading it back out
            // of `transcript` by index would be wrong when round 1's opponent
            // requested a decomposition: `query_opponent_attack_with_decomposition`
            // pushes an intermediate "Proponent (round 1 decomposition)" entry,
            // shifting the opponent's actual attack to a later index.
            let first_round_attack_text = (round == 1).then(|| attack_text.clone());
            transcript.push((format!("Opponent (round {} attack)", round), attack_text));

            // Anti-mode-collapse divergence check (#316): only once, right after
            // round 1's opponent attack and before the round's usual moderator
            // checkpoint. If the proponent's opening and the opponent's first
            // attack have quietly converged on the same conclusion instead of
            // genuinely disagreeing, attacking each other's phrasing for the
            // rest of the debate wastes rounds on a false adversarial framing.
            // The moderator is asked to judge this explicitly; if it finds no
            // real divergence, a one-off "contrarian brief" is run to attack
            // the shared premise directly, assigned to whichever voice isn't
            // already committed to defending it (the third-party interjector
            // if one exists, otherwise the opponent doubles up).
            if round == 1 && !contrarian_brief_used {
                let opening_text = transcript[0].1.clone();
                let first_attack_text = first_round_attack_text
                    .expect("first_round_attack_text is always Some when round == 1");
                let divergence_prompt = DebatePromptTemplate::divergence_check_prompt(
                    question,
                    &opening_text,
                    &first_attack_text,
                );
                match Self::query(
                    &gateway,
                    &moderator,
                    DebatePromptTemplate::moderator_divergence_system(),
                    &divergence_prompt,
                )
                .await
                {
                    Ok(divergence_text) => {
                        progress.on_task_complete(&Phase::Review, &moderator, true);
                        let (divergent, note) = parse_divergence_check(&divergence_text);
                        if !divergent {
                            contrarian_brief_used = true;
                            let contrarian = interjectors.first().unwrap_or(&opponent).clone();
                            let brief_prompt = DebatePromptTemplate::contrarian_brief_prompt(
                                question,
                                &note,
                                &Self::render_transcript(&transcript),
                            );
                            match Self::query(
                                &gateway,
                                &contrarian,
                                DebatePromptTemplate::contrarian_system(),
                                &brief_prompt,
                            )
                            .await
                            {
                                Ok(brief_text) => {
                                    progress.on_task_complete(&Phase::Review, &contrarian, true);
                                    for (claim, evidence, severity) in
                                        parse_opponent_rebuttals(&brief_text)
                                    {
                                        ledger.add(round, claim, evidence, severity);
                                    }
                                    peer_reviews.push(PeerReview::new(
                                        contrarian.to_string(),
                                        "Contrarian brief (shared-premise attack)".to_string(),
                                        brief_text.clone(),
                                    ));
                                    transcript.push((
                                        format!("Contrarian brief ({})", contrarian),
                                        brief_text,
                                    ));
                                }
                                Err(e) => {
                                    // The contrarian brief is a supplementary
                                    // safeguard, not load-bearing — log and
                                    // continue the debate rather than aborting
                                    // over it.
                                    warn!("Contrarian brief failed for {}: {}", contrarian, e);
                                    progress.on_task_complete(&Phase::Review, &contrarian, false);
                                }
                            }
                        } else {
                            info!(
                                "Debate divergence check: genuine disagreement detected, note: {}",
                                note
                            );
                        }
                    }
                    Err(e) => {
                        // Likewise supplementary — a failed divergence check
                        // shouldn't abort a debate that's otherwise proceeding
                        // normally.
                        warn!("Moderator divergence check failed: {}", e);
                        progress.on_task_complete(&Phase::Review, &moderator, false);
                    }
                }
            }

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
            let open_objections: Vec<(&str, &str)> = ledger
                .open_objections()
                .iter()
                .map(|o| (o.id.as_str(), o.claim.as_str()))
                .collect();
            let checkpoint_prompt =
                DebatePromptTemplate::moderator_checkpoint_prompt_with_objections(
                    question,
                    &Self::render_transcript(&transcript),
                    round,
                    max_rounds,
                    is_final_round,
                    &open_objections,
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

            for (rebuttal_id, accepted, reason) in parse_moderator_rulings(&checkpoint_text) {
                if ledger.all().iter().any(|o| o.id == rebuttal_id) {
                    ledger.apply_ruling(&rebuttal_id, accepted, reason);
                } else {
                    warn!(
                        "Moderator ruling references unknown rebuttal ID {} in round {}",
                        rebuttal_id, round
                    );
                }
            }

            let (verdict_settled, body) = parse_debate_verdict(&checkpoint_text);
            if verdict_settled || is_final_round {
                // Gate every settle point — not just the final round — so a
                // moderator can't rubber-stamp a premature "SETTLED" verdict
                // while critical/major objections are still open.
                let unresolved: Vec<Objection> = ledger
                    .unresolved_critical_or_major()
                    .into_iter()
                    .cloned()
                    .collect();

                if unresolved.is_empty() {
                    settled_synthesis = Some(SynthesisResult::new(moderator.to_string(), body));
                    settled = true;
                    break;
                }

                let decision = Self::handle_debate_escalation(
                    input.hil_mode,
                    human_intervention.as_ref(),
                    question,
                    &unresolved,
                    &truncate_head_tail(
                        &Self::render_transcript(&transcript),
                        TRANSCRIPT_SUMMARY_MAX_BYTES,
                    ),
                    !is_final_round,
                )
                .await?;

                match decision {
                    HumanDecision::Approve => {
                        warn!(
                            "Forcing debate settlement at round {} despite {} unresolved critical/major objection(s)",
                            round,
                            unresolved.len()
                        );
                        let mut synthesis = SynthesisResult::new(moderator.to_string(), body);
                        synthesis.disagreements.push(format!(
                            "⚠️ Settled with {} unresolved critical/major objection(s) still open: {}",
                            unresolved.len(),
                            unresolved
                                .iter()
                                .map(|o| format!("{} ({})", o.claim, o.id))
                                .collect::<Vec<_>>()
                                .join("; ")
                        ));
                        settled_synthesis = Some(synthesis);
                        settled = true;
                        break;
                    }
                    HumanDecision::Reject | HumanDecision::Edit(_) => {
                        if is_final_round {
                            return Err(RunQuorumError::DebateEscalationRejected(format!(
                                "{} unresolved critical/major objection(s) at round {}",
                                unresolved.len(),
                                round
                            )));
                        }
                        // Non-final round: the natural response to a
                        // premature settle attempt is to decline it and keep
                        // debating, not to kill the whole run — fall through
                        // to the same transcript note a normal
                        // VERDICT: CONTINUE round would get, and proceed to
                        // round + 1.
                        info!(
                            "Debate escalation declined at round {} — continuing rather than settling ({} unresolved critical/major objection(s))",
                            round,
                            unresolved.len()
                        );
                    }
                }
            }
            transcript.push((format!("Moderator (round {} note)", round), body));
        }

        progress.on_phase_complete(&Phase::Review);

        // The final round always forces a settle, so the loop never exits without one.
        let synthesis = settled_synthesis.expect("debate loop always produces a synthesis");

        progress.on_phase_start(&Phase::Synthesis, 1);
        progress.on_task_complete(&Phase::Synthesis, &moderator, true);
        progress.on_phase_complete(&Phase::Synthesis);

        // Fold every objection ruling into the shared Vote schema: each
        // ledger entry becomes one vote from the moderator, so the
        // `quorum_result` contract (JSONL / RPC / Lua) sees the same
        // per-claim verdicts a human reading the transcript would.
        // Refuted (critic's objection rejected) reads as the moderator
        // "approving" the proponent's claim; Conceded reads as a rejection
        // of it; anything never ruled on abstains rather than silently
        // counting either way. The ruling itself (refuted/conceded/
        // unresolved) is spelled out in `reasoning` alongside the verdict —
        // without it, e.g. `verdict=Approve` next to `reasoning` quoting the
        // *attacking* claim reads backwards (as if the attack were being
        // approved, not rejected).
        let mut votes: Vec<Vote> = ledger
            .all()
            .iter()
            .map(|objection| {
                let id = &objection.id;
                let claim = &objection.claim;
                let reason = objection
                    .ruling_reason
                    .as_deref()
                    .unwrap_or("no ruling recorded");
                let (verdict, ruling_label) = match objection.status {
                    ObjectionStatus::Refuted => (VoteVerdict::Approve, "refuted"),
                    ObjectionStatus::Conceded => (VoteVerdict::Reject, "conceded"),
                    ObjectionStatus::Unresolved => (VoteVerdict::Abstain, "unresolved"),
                };
                Vote::new(
                    moderator.to_string(),
                    verdict,
                    format!("[{id}][{ruling_label}] {claim}: {reason}"),
                )
            })
            .collect();

        // Explicit closing vote for the debate as a whole — the moderator's
        // final ruling, distinct from (and appended after) the per-objection
        // votes above.
        votes.push(Vote::new(
            moderator.to_string(),
            if settled {
                VoteVerdict::Approve
            } else {
                VoteVerdict::Reject
            },
            synthesis.conclusion.clone(),
        ));

        // Built directly (not `VoteResult::from_votes`): `passed` must
        // reflect the domain-computed `settled` outcome, not a majority
        // tally of the votes above — a debate can settle with more Reject
        // (conceded) objection-votes than Approve ones and still be a valid,
        // decided outcome.
        let approve_count = votes.iter().filter(|v| v.is_approve()).count();
        let reject_count = votes.iter().filter(|v| v.is_reject()).count();
        let total_votes = votes.len();
        let vote_result = VoteResult {
            passed: settled,
            approve_count,
            reject_count,
            total_votes,
            votes,
            aggregated_feedback: None,
        };

        let payload = QuorumResultPayload::new(QuorumTopic::Debate, None, &vote_result)
            .with_synthesis(synthesis.clone());
        event_publisher.publish(AppEvent::QuorumResult(Box::new(payload)));

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
    use crate::ports::event_publisher::{AppEvent, NoEventPublisher, RecordingEventPublisher};
    use crate::ports::progress::NoProgress;
    use crate::use_cases::run_quorum::test_support::ScriptedGateway;
    use quorum_domain::{DebateIntensity, ModelConfig, Plan, ReviewRound};
    use std::sync::Mutex;

    /// Mock `HumanInterventionPort` that returns a pre-configured
    /// (`request_debate_escalation`-only) outcome and records each
    /// invocation's `can_continue` flag and `transcript_summary` length (in
    /// call order) alongside the count.
    struct MockEscalationHandler {
        outcome: Result<HumanDecision, HumanInterventionError>,
        calls: Mutex<usize>,
        can_continue_calls: Mutex<Vec<bool>>,
        transcript_summary_lens: Mutex<Vec<usize>>,
    }

    impl MockEscalationHandler {
        fn new(outcome: Result<HumanDecision, HumanInterventionError>) -> Self {
            Self {
                outcome,
                calls: Mutex::new(0),
                can_continue_calls: Mutex::new(Vec::new()),
                transcript_summary_lens: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl HumanInterventionPort for MockEscalationHandler {
        async fn request_intervention(
            &self,
            _request: &str,
            _plan: &Plan,
            _review_history: &[ReviewRound],
        ) -> Result<HumanDecision, HumanInterventionError> {
            unreachable!("not exercised by debate escalation tests")
        }

        async fn request_debate_escalation(
            &self,
            _question: &str,
            _unresolved: &[Objection],
            transcript_summary: &str,
            can_continue: bool,
        ) -> Result<HumanDecision, HumanInterventionError> {
            *self.calls.lock().unwrap() += 1;
            self.can_continue_calls.lock().unwrap().push(can_continue);
            self.transcript_summary_lens
                .lock()
                .unwrap()
                .push(transcript_summary.len());
            self.outcome.clone()
        }
    }

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

    /// Builds a scripted moderator divergence-check response (see
    /// `DebatePromptTemplate::moderator_divergence_system`'s `DIVERGENT:
    /// YES|NO` format). Every round-1 attack in this file is followed by one
    /// of these before the usual checkpoint — spelling it out as a literal
    /// each time obscures which line is the actual signal under test.
    fn divergence_response(divergent: bool, note: &str) -> String {
        format!(
            "DIVERGENT: {}\n\n{}",
            if divergent { "YES" } else { "NO" },
            note
        )
    }

    /// Builds a scripted moderator checkpoint response (see
    /// `DebatePromptTemplate::moderator_system`'s `VERDICT: SETTLED|CONTINUE`
    /// format). `rulings` are optional `REBUTTAL_ID`/`RULING`/`REASON`
    /// blocks (see [`ruling_block`]) prefixed before the verdict body —
    /// pass an empty slice for scenarios with no open objections to rule on.
    fn verdict_response(settled: bool, rulings: &[String], body: &str) -> String {
        let verdict_line = format!("VERDICT: {}", if settled { "SETTLED" } else { "CONTINUE" });
        if rulings.is_empty() {
            format!("{}\n\n{}", verdict_line, body)
        } else {
            format!("{}\n\n{}\n\n{}", verdict_line, rulings.join("\n\n"), body)
        }
    }

    /// Builds a single `REBUTTAL_ID`/`RULING`/`REASON` block for use inside
    /// [`verdict_response`]'s `rulings` slice.
    fn ruling_block(id: &str, accepted: bool, reason: &str) -> String {
        format!(
            "REBUTTAL_ID: {}\nRULING: {}\nREASON: {}",
            id,
            if accepted { "ACCEPTED" } else { "REJECTED" },
            reason
        )
    }

    #[tokio::test]
    async fn debate_settles_early_when_moderator_agrees() {
        let config = debate_config(vec![Model::Gpt53Codex, Model::Gemini31Pro], 5);
        let input = base_input(OrchestrationStrategy::Debate(config));

        let gateway = ScriptedGateway::new();
        gateway.respond(Model::Gpt53Codex, "Write-through keeps consistency simple.");
        gateway.respond(Model::Gemini31Pro, "Write-behind has better write latency.");
        // Round 1's divergence check (asked before the usual checkpoint) —
        // genuine disagreement, so no contrarian brief fires.
        gateway.respond(
            Model::ClaudeSonnet45,
            divergence_response(true, "They disagree on consistency vs. latency tradeoffs."),
        );
        // No structured opponent rebuttal was raised above, so there's
        // nothing in the ledger to rule on — the checkpoint response omits
        // any REBUTTAL_ID block.
        gateway.respond(
            Model::ClaudeSonnet45,
            verdict_response(
                true,
                &[],
                "Write-through wins for correctness-critical systems.",
            ),
        );

        let event_publisher = Arc::new(RecordingEventPublisher::new());
        let result = DebateStrategyExecutor::new()
            .execute(
                &input,
                Arc::new(gateway),
                &NoProgress,
                event_publisher.clone(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.responses.len(), 1); // opening only, settled before round 2's defense
        assert_eq!(result.reviews.len(), 1); // opponent's round-1 attack
        assert_eq!(
            result.synthesis.conclusion,
            "Write-through wins for correctness-critical systems."
        );

        // No structured rebuttal was raised, so the only vote is the closing
        // one, and — since the debate settled cleanly — it approves.
        let events = event_publisher.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        let AppEvent::QuorumResult(payload) = &events[0] else {
            panic!("expected QuorumResult event");
        };
        assert_eq!(payload.topic, QuorumTopic::Debate);
        assert!(payload.target.is_none());
        assert!(payload.approved);
        assert_eq!(payload.votes.len(), 1);
        assert_eq!(payload.votes[0].verdict, VoteVerdict::Approve);
        assert_eq!(
            payload.votes[0].reasoning,
            "Write-through wins for correctness-critical systems."
        );
        assert_eq!(
            payload.synthesis.as_ref().unwrap().conclusion,
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
        // Round 1's divergence check — genuine disagreement, no contrarian brief.
        gateway.respond(
            Model::ClaudeSonnet45,
            divergence_response(true, "They disagree on consistency vs. latency tradeoffs."),
        );
        // Neither round's opponent attack is a structured rebuttal, so the
        // ledger stays empty and neither checkpoint response needs a
        // REBUTTAL_ID ruling.
        gateway.respond(
            Model::ClaudeSonnet45,
            verdict_response(false, &[], "Still contested: latency vs consistency."),
        );
        gateway.respond(
            Model::Gpt53Codex,
            "Defense round 2: consistency still wins.",
        );
        gateway.respond(Model::Gemini31Pro, "Attack round 2: but writes pile up.");
        gateway.respond(
            Model::ClaudeSonnet45,
            verdict_response(false, &[], "No real resolution, but rounds are up."),
        );

        let result = DebateStrategyExecutor::new()
            .execute(
                &input,
                Arc::new(gateway),
                &NoProgress,
                Arc::new(NoEventPublisher),
                None,
            )
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
    async fn debate_contrarian_brief_fires_when_divergence_check_finds_no_divergence() {
        // No interjectors — the opponent doubles up as the contrarian.
        let config = debate_config(vec![Model::Gpt53Codex, Model::Gemini31Pro], 1);
        let input = base_input(OrchestrationStrategy::Debate(config));

        let gateway = ScriptedGateway::new();
        gateway.respond(
            Model::Gpt53Codex,
            "Opening: AI will match human performance.",
        );
        gateway.respond(
            Model::Gemini31Pro,
            "Attack round 1: AI will match human performance, just later than claimed.",
        );
        // The moderator's divergence check finds no genuine disagreement —
        // both sides share the premise that AI will eventually match human
        // performance, they just quibble over timing.
        gateway.respond(
            Model::ClaudeSonnet45,
            "DIVERGENT: NO\n\nBoth sides agree AI will eventually match human performance.",
        );
        // The contrarian brief (opponent, since no interjector exists) attacks
        // that shared premise directly.
        gateway.respond(
            Model::Gemini31Pro,
            "CLAIM: AI will not match human performance\nEVIDENCE: benchmark X has plateaued for 3 years\nSEVERITY: MINOR",
        );
        gateway.respond(
            Model::ClaudeSonnet45,
            "VERDICT: SETTLED\n\nAI trajectory remains contested.",
        );

        let result = DebateStrategyExecutor::new()
            .execute(
                &input,
                Arc::new(gateway),
                &NoProgress,
                Arc::new(NoEventPublisher),
                None,
            )
            .await
            .unwrap();

        // The contrarian brief is recorded as an extra review beyond the
        // opponent's normal round-1 attack.
        assert_eq!(result.reviews.len(), 2);
        assert!(
            result
                .reviews
                .iter()
                .any(|r| r.reviewed_id.contains("Contrarian brief")),
            "expected a contrarian-brief review, got: {:?}",
            result
                .reviews
                .iter()
                .map(|r| &r.reviewed_id)
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn debate_contrarian_brief_uses_interjector_when_present() {
        let config = DebateConfig {
            allow_interjection: true,
            ..debate_config(
                vec![Model::Gpt53Codex, Model::Gemini31Pro, Model::ClaudeHaiku45],
                1,
            )
        };
        let input = base_input(OrchestrationStrategy::Debate(config));

        let gateway = ScriptedGateway::new();
        gateway.respond(
            Model::Gpt53Codex,
            "Opening: AI will match human performance.",
        );
        gateway.respond(
            Model::Gemini31Pro,
            "Attack round 1: AI will match human performance, just later than claimed.",
        );
        gateway.respond(
            Model::ClaudeSonnet45,
            "DIVERGENT: NO\n\nBoth sides agree AI will eventually match human performance.",
        );
        // With a third model available, the interjector — not the opponent —
        // is assigned the contrarian brief.
        gateway.respond(
            Model::ClaudeHaiku45,
            "CLAIM: AI will not match human performance\nEVIDENCE: benchmark X has plateaued for 3 years\nSEVERITY: MINOR",
        );
        gateway.respond(
            Model::ClaudeHaiku45,
            "Interjection: both ignore compute cost trends.",
        );
        gateway.respond(
            Model::ClaudeSonnet45,
            "VERDICT: SETTLED\n\nAI trajectory remains contested.",
        );

        let result = DebateStrategyExecutor::new()
            .execute(
                &input,
                Arc::new(gateway),
                &NoProgress,
                Arc::new(NoEventPublisher),
                None,
            )
            .await
            .unwrap();

        assert!(
            result
                .reviews
                .iter()
                .any(|r| r.reviewed_id.contains("Contrarian brief")
                    && r.reviewer == Model::ClaudeHaiku45.to_string()),
            "expected the interjector to deliver the contrarian brief, got: {:?}",
            result
                .reviews
                .iter()
                .map(|r| (&r.reviewer, &r.reviewed_id))
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn debate_decomposition_request_triggers_proponent_split_then_requery() {
        // No interjectors, single round: opponent's first attack asks for
        // decomposition; the proponent splits the claim; the opponent then
        // re-attacks the split claim with a normal structured rebuttal, all
        // still counted as round 1 (max_rounds stays 1).
        let config = debate_config(vec![Model::Gpt53Codex, Model::Gemini31Pro], 1);
        let input = base_input(OrchestrationStrategy::Debate(config));

        let gateway = ScriptedGateway::new();
        gateway.respond(
            Model::Gpt53Codex,
            "Opening: caching and retry policy should share one config knob.",
        );
        gateway.respond(
            Model::Gemini31Pro,
            "DECOMPOSE_REQUEST: caching and retry policy should share one config knob",
        );
        gateway.respond(
            Model::Gpt53Codex,
            "Sub-claim 1: caching should be a single knob.\nSub-claim 2: retry policy should be a single knob.",
        );
        gateway.respond(
            Model::Gemini31Pro,
            "CLAIM: retry policy as a single knob\nEVIDENCE: it hides per-endpoint backoff needs\nSEVERITY: MINOR",
        );
        // Round 1's divergence check — genuine disagreement, no contrarian brief.
        gateway.respond(
            Model::ClaudeSonnet45,
            "DIVERGENT: YES\n\nThey disagree on whether one knob suffices.",
        );
        gateway.respond(
            Model::ClaudeSonnet45,
            "VERDICT: SETTLED\n\nSplit the knobs.",
        );

        let result = DebateStrategyExecutor::new()
            .execute(
                &input,
                Arc::new(gateway),
                &NoProgress,
                Arc::new(NoEventPublisher),
                None,
            )
            .await
            .unwrap();

        // Only the opening is a "response"; the decomposition detour doesn't
        // add one (it's transcript-only, not a model_responses entry).
        assert_eq!(result.responses.len(), 1);
        // The single opponent review slot holds the *post-decomposition*
        // rebuttal, not the original DECOMPOSE_REQUEST text.
        assert_eq!(result.reviews.len(), 1);
        assert!(result.reviews[0].content.starts_with("CLAIM:"));
        assert_eq!(result.synthesis.conclusion, "Split the knobs.");
    }

    #[tokio::test]
    async fn debate_divergence_check_compares_opening_against_opponent_attack_after_decomposition()
    {
        // Regression: when round 1's opponent attack triggers a decomposition
        // detour, `query_opponent_attack_with_decomposition` pushes an
        // intermediate "Proponent (round 1 decomposition)" entry into the
        // transcript before the opponent's actual (post-decomposition) attack
        // is pushed. The divergence check must compare the proponent's
        // opening against that actual attack — not against the proponent's
        // own decomposition response that now sits at transcript[1].
        let config = debate_config(vec![Model::Gpt53Codex, Model::Gemini31Pro], 1);
        let input = base_input(OrchestrationStrategy::Debate(config));

        let gateway = ScriptedGateway::new();
        gateway.respond(
            Model::Gpt53Codex,
            "Opening: caching and retry policy should share one config knob.",
        );
        gateway.respond(
            Model::Gemini31Pro,
            "DECOMPOSE_REQUEST: caching and retry policy should share one config knob",
        );
        gateway.respond(
            Model::Gpt53Codex,
            "Sub-claim 1: caching should be a single knob.\nSub-claim 2: retry policy should be a single knob.",
        );
        gateway.respond(
            Model::Gemini31Pro,
            "CLAIM: retry policy as a single knob\nEVIDENCE: it hides per-endpoint backoff needs\nSEVERITY: MINOR",
        );
        gateway.respond(
            Model::ClaudeSonnet45,
            "DIVERGENT: YES\n\nThey disagree on whether one knob suffices.",
        );
        gateway.respond(
            Model::ClaudeSonnet45,
            "VERDICT: SETTLED\n\nSplit the knobs.",
        );

        let gateway = Arc::new(gateway);
        DebateStrategyExecutor::new()
            .execute(
                &input,
                Arc::clone(&gateway) as Arc<dyn LlmGateway>,
                &NoProgress,
                Arc::new(NoEventPublisher),
                None,
            )
            .await
            .unwrap();

        let moderator_prompts = gateway.sent_prompts(Model::ClaudeSonnet45);
        let divergence_prompt = moderator_prompts
            .first()
            .expect("moderator should have been queried for the divergence check first");
        assert!(
            divergence_prompt.contains("CLAIM: retry policy as a single knob"),
            "divergence check must see the opponent's actual (post-decomposition) attack: {}",
            divergence_prompt
        );
        assert!(
            !divergence_prompt.contains("Sub-claim 1: caching should be a single knob"),
            "divergence check must not see the proponent's own decomposition response: {}",
            divergence_prompt
        );
    }

    #[tokio::test]
    async fn debate_second_decomposition_request_in_same_round_is_ignored() {
        // The opponent tries to request decomposition twice in the same
        // round (after already getting one). The second request must be
        // ignored — no second proponent detour — and the raw (still
        // DECOMPOSE_REQUEST-prefixed) text is parsed as a normal rebuttal,
        // which yields no CLAIM/EVIDENCE/SEVERITY (i.e. no ledger entry).
        let config = debate_config(vec![Model::Gpt53Codex, Model::Gemini31Pro], 1);
        let input = base_input(OrchestrationStrategy::Debate(config));

        let gateway = ScriptedGateway::new();
        gateway.respond(Model::Gpt53Codex, "Opening: one knob for everything.");
        gateway.respond(
            Model::Gemini31Pro,
            "DECOMPOSE_REQUEST: one knob for everything",
        );
        gateway.respond(
            Model::Gpt53Codex,
            "Sub-claim 1: caching knob.\nSub-claim 2: retry knob.",
        );
        // Second decomposition request in the same round — must be ignored.
        gateway.respond(
            Model::Gemini31Pro,
            "DECOMPOSE_REQUEST: caching knob still too broad",
        );
        gateway.respond(
            Model::ClaudeSonnet45,
            "DIVERGENT: YES\n\nGenuine disagreement over knob granularity.",
        );
        gateway.respond(Model::ClaudeSonnet45, "VERDICT: SETTLED\n\nUse two knobs.");

        let result = DebateStrategyExecutor::new()
            .execute(
                &input,
                Arc::new(gateway),
                &NoProgress,
                Arc::new(NoEventPublisher),
                None,
            )
            .await
            .unwrap();

        // The ignored second request is recorded as the raw review text —
        // no third gateway call was made for it (the scripted queues above
        // would have errored with "no scripted response left" otherwise).
        assert_eq!(result.reviews.len(), 1);
        assert_eq!(
            result.reviews[0].content,
            "DECOMPOSE_REQUEST: caching knob still too broad"
        );
        assert_eq!(result.synthesis.conclusion, "Use two knobs.");
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
        // Round 1's divergence check — genuine disagreement, no contrarian
        // brief, so the interjector's queue below is used only for its
        // normal interjection turn.
        gateway.respond(
            Model::ClaudeSonnet45,
            divergence_response(true, "They disagree on latency vs consistency."),
        );
        gateway.respond(
            Model::ClaudeHaiku45,
            "Interjection: both ignore cold-start cost.",
        );
        // No structured opponent rebuttal, so the checkpoint response omits
        // any REBUTTAL_ID ruling.
        gateway.respond(
            Model::ClaudeSonnet45,
            verdict_response(true, &[], "Write-through, with a cache warmer."),
        );

        let result = DebateStrategyExecutor::new()
            .execute(
                &input,
                Arc::new(gateway),
                &NoProgress,
                Arc::new(NoEventPublisher),
                None,
            )
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
            .execute(
                &input,
                Arc::new(gateway),
                &NoProgress,
                Arc::new(NoEventPublisher),
                None,
            )
            .await
            .unwrap_err();

        assert!(matches!(err, RunQuorumError::NotEnoughModelsForDebate(1)));
    }

    /// Builds a common fixture: opponent raises one CRITICAL rebuttal and the
    /// moderator declares `SETTLED` in round 1 without ruling on it — i.e. the
    /// moderator tries to settle early while a critical objection is still open.
    ///
    /// Paired with `debate_config(_, 1)` this makes round 1 the *final*
    /// round, so the escalation this triggers has `can_continue = false`.
    fn escalation_gateway() -> ScriptedGateway {
        let gateway = ScriptedGateway::new();
        gateway.respond(Model::Gpt53Codex, "Opening: write-through.");
        gateway.respond(
            Model::Gemini31Pro,
            "CLAIM: write-through never expires\nEVIDENCE: TTL config defaults to infinite\nSEVERITY: CRITICAL",
        );
        // Round 1's divergence check — genuine disagreement, no contrarian brief.
        gateway.respond(
            Model::ClaudeSonnet45,
            "DIVERGENT: YES\n\nThey disagree on TTL handling.",
        );
        gateway.respond(
            Model::ClaudeSonnet45,
            "VERDICT: SETTLED\n\nWrite-through wins.",
        );
        gateway
    }

    /// Same round-1 premature-settle setup as [`escalation_gateway`], but
    /// paired with `debate_config(_, 2)` so round 1 is *not* final —
    /// `can_continue = true`. Scripts a round 2 that resolves the R1-1
    /// objection and settles cleanly, so tests can assert the debate
    /// actually continues (rather than aborting) after the escalation is
    /// declined in round 1.
    fn escalation_continues_gateway() -> ScriptedGateway {
        let gateway = escalation_gateway();
        gateway.respond(
            Model::Gpt53Codex,
            "Defense round 2: adds a cache warmer to mitigate the TTL gap.",
        );
        gateway.respond(Model::Gemini31Pro, "No further rebuttal.");
        gateway.respond(
            Model::ClaudeSonnet45,
            verdict_response(
                true,
                &[ruling_block(
                    "R1-1",
                    false,
                    "cache warmer mitigates the TTL gap",
                )],
                "Write-through wins with a cache warmer.",
            ),
        );
        gateway
    }

    #[tokio::test]
    async fn debate_early_settle_reject_at_final_round_aborts() {
        let config = debate_config(vec![Model::Gpt53Codex, Model::Gemini31Pro], 1);
        // RunQuorumInput::new() defaults to HilMode::AutoReject.
        let input = base_input(OrchestrationStrategy::Debate(config));

        let err = DebateStrategyExecutor::new()
            .execute(
                &input,
                Arc::new(escalation_gateway()),
                &NoProgress,
                Arc::new(NoEventPublisher),
                None,
            )
            .await
            .unwrap_err();

        assert!(matches!(err, RunQuorumError::DebateEscalationRejected(_)));
    }

    #[tokio::test]
    async fn debate_early_settle_reject_at_non_final_round_continues_debate() {
        let config = debate_config(vec![Model::Gpt53Codex, Model::Gemini31Pro], 2);
        // RunQuorumInput::new() defaults to HilMode::AutoReject — but at a
        // non-final round that now means "decline the early settle and keep
        // debating", not "abort".
        let input = base_input(OrchestrationStrategy::Debate(config));

        let result = DebateStrategyExecutor::new()
            .execute(
                &input,
                Arc::new(escalation_continues_gateway()),
                &NoProgress,
                Arc::new(NoEventPublisher),
                None,
            )
            .await
            .unwrap();

        assert!(
            result
                .synthesis
                .conclusion
                .contains("Write-through wins with a cache warmer."),
            "unexpected conclusion: {}",
            result.synthesis.conclusion
        );
    }

    #[tokio::test]
    async fn debate_escalation_transcript_summary_is_truncated_to_budget() {
        // Regression: debate_strategy.rs used to pass the full, unbounded
        // render_transcript() output as `transcript_summary` — a single TUI
        // modal line, or CLI dump, of the entire debate. It must be
        // head+tail truncated to TRANSCRIPT_SUMMARY_MAX_BYTES instead.
        let config = debate_config(vec![Model::Gpt53Codex, Model::Gemini31Pro], 1);
        let input =
            base_input(OrchestrationStrategy::Debate(config)).with_hil_mode(HilMode::Interactive);
        let handler = Arc::new(MockEscalationHandler::new(Ok(HumanDecision::Approve)));

        let gateway = ScriptedGateway::new();
        gateway.respond(Model::Gpt53Codex, "Opening: write-through.");
        // A very long EVIDENCE line pushes the rendered transcript well past
        // the budget by the time escalation fires.
        let long_evidence = "x".repeat(TRANSCRIPT_SUMMARY_MAX_BYTES * 2);
        gateway.respond(
            Model::Gemini31Pro,
            format!(
                "CLAIM: write-through never expires\nEVIDENCE: {}\nSEVERITY: CRITICAL",
                long_evidence
            ),
        );
        gateway.respond(
            Model::ClaudeSonnet45,
            "DIVERGENT: YES\n\nThey disagree on TTL handling.",
        );
        gateway.respond(
            Model::ClaudeSonnet45,
            "VERDICT: SETTLED\n\nWrite-through wins.",
        );

        DebateStrategyExecutor::new()
            .execute(
                &input,
                Arc::new(gateway),
                &NoProgress,
                Arc::new(NoEventPublisher),
                Some(handler.clone() as Arc<dyn HumanInterventionPort>),
            )
            .await
            .unwrap();

        let lens = handler.transcript_summary_lens.lock().unwrap();
        assert_eq!(*lens, vec![TRANSCRIPT_SUMMARY_MAX_BYTES]);
    }

    #[tokio::test]
    async fn debate_early_settle_with_unresolved_objection_forced_on_auto_approve() {
        let config = debate_config(vec![Model::Gpt53Codex, Model::Gemini31Pro], 3);
        let input =
            base_input(OrchestrationStrategy::Debate(config)).with_hil_mode(HilMode::AutoApprove);

        let event_publisher = Arc::new(RecordingEventPublisher::new());
        let result = DebateStrategyExecutor::new()
            .execute(
                &input,
                Arc::new(escalation_gateway()),
                &NoProgress,
                event_publisher.clone(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.synthesis.conclusion, "Write-through wins.");
        // Forced settlement leaves a note about the unresolved objection.
        assert_eq!(result.synthesis.disagreements.len(), 1);
        assert!(
            result.synthesis.disagreements[0].contains("unresolved"),
            "expected disagreement note about the unresolved objection, got: {:?}",
            result.synthesis.disagreements
        );

        // The domain-computed `settled` outcome (forced via escalation
        // Approve) drives `passed` — not a majority tally of the votes
        // below, which include an Abstain for the still-unresolved
        // objection alongside the closing Approve vote.
        let events = event_publisher.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        let AppEvent::QuorumResult(payload) = &events[0] else {
            panic!("expected QuorumResult event");
        };
        assert!(payload.approved);
        assert_eq!(payload.votes.len(), 2);
        assert_eq!(payload.votes[0].verdict, VoteVerdict::Abstain);
        assert!(payload.votes[0].reasoning.starts_with("[R1-1][unresolved]"));
        assert!(
            payload.votes[0]
                .reasoning
                .contains("write-through never expires")
        );
        assert_eq!(payload.votes[1].verdict, VoteVerdict::Approve);
        assert_eq!(payload.votes[1].reasoning, "Write-through wins.");
    }

    #[tokio::test]
    async fn debate_interactive_escalation_approve_forces_settlement_with_note() {
        let config = debate_config(vec![Model::Gpt53Codex, Model::Gemini31Pro], 3);
        let input =
            base_input(OrchestrationStrategy::Debate(config)).with_hil_mode(HilMode::Interactive);
        let handler = Arc::new(MockEscalationHandler::new(Ok(HumanDecision::Approve)));

        let result = DebateStrategyExecutor::new()
            .execute(
                &input,
                Arc::new(escalation_gateway()),
                &NoProgress,
                Arc::new(NoEventPublisher),
                Some(handler.clone() as Arc<dyn HumanInterventionPort>),
            )
            .await
            .unwrap();

        assert_eq!(*handler.calls.lock().unwrap(), 1);
        assert_eq!(result.synthesis.disagreements.len(), 1);
    }

    #[tokio::test]
    async fn debate_interactive_escalation_reject_at_final_round_aborts_execute() {
        let config = debate_config(vec![Model::Gpt53Codex, Model::Gemini31Pro], 1);
        let input =
            base_input(OrchestrationStrategy::Debate(config)).with_hil_mode(HilMode::Interactive);
        let handler = Arc::new(MockEscalationHandler::new(Ok(HumanDecision::Reject)));

        let err = DebateStrategyExecutor::new()
            .execute(
                &input,
                Arc::new(escalation_gateway()),
                &NoProgress,
                Arc::new(NoEventPublisher),
                Some(handler.clone() as Arc<dyn HumanInterventionPort>),
            )
            .await
            .unwrap_err();

        assert_eq!(*handler.calls.lock().unwrap(), 1);
        assert_eq!(
            handler.can_continue_calls.lock().unwrap().as_slice(),
            &[false]
        );
        assert!(matches!(err, RunQuorumError::DebateEscalationRejected(_)));
    }

    #[tokio::test]
    async fn debate_interactive_escalation_reject_at_non_final_round_continues_debate() {
        let config = debate_config(vec![Model::Gpt53Codex, Model::Gemini31Pro], 2);
        let input =
            base_input(OrchestrationStrategy::Debate(config)).with_hil_mode(HilMode::Interactive);
        let handler = Arc::new(MockEscalationHandler::new(Ok(HumanDecision::Reject)));

        let result = DebateStrategyExecutor::new()
            .execute(
                &input,
                Arc::new(escalation_continues_gateway()),
                &NoProgress,
                Arc::new(NoEventPublisher),
                Some(handler.clone() as Arc<dyn HumanInterventionPort>),
            )
            .await
            .unwrap();

        assert_eq!(*handler.calls.lock().unwrap(), 1);
        assert_eq!(
            handler.can_continue_calls.lock().unwrap().as_slice(),
            &[true]
        );
        assert!(
            result
                .synthesis
                .conclusion
                .contains("Write-through wins with a cache warmer."),
            "unexpected conclusion: {}",
            result.synthesis.conclusion
        );
    }

    #[tokio::test]
    async fn debate_interactive_without_handler_falls_back_to_reject_at_final_round() {
        let config = debate_config(vec![Model::Gpt53Codex, Model::Gemini31Pro], 1);
        let input =
            base_input(OrchestrationStrategy::Debate(config)).with_hil_mode(HilMode::Interactive);

        let err = DebateStrategyExecutor::new()
            .execute(
                &input,
                Arc::new(escalation_gateway()),
                &NoProgress,
                Arc::new(NoEventPublisher),
                None,
            )
            .await
            .unwrap_err();

        assert!(matches!(err, RunQuorumError::DebateEscalationRejected(_)));
    }

    #[tokio::test]
    async fn debate_interactive_without_handler_falls_back_to_reject_at_non_final_round_continues()
    {
        let config = debate_config(vec![Model::Gpt53Codex, Model::Gemini31Pro], 2);
        let input =
            base_input(OrchestrationStrategy::Debate(config)).with_hil_mode(HilMode::Interactive);

        let result = DebateStrategyExecutor::new()
            .execute(
                &input,
                Arc::new(escalation_continues_gateway()),
                &NoProgress,
                Arc::new(NoEventPublisher),
                None,
            )
            .await
            .unwrap();

        assert!(
            result
                .synthesis
                .conclusion
                .contains("Write-through wins with a cache warmer."),
            "unexpected conclusion: {}",
            result.synthesis.conclusion
        );
    }

    #[tokio::test]
    async fn debate_interactive_escalation_cancelled_maps_to_cancelled_error() {
        let config = debate_config(vec![Model::Gpt53Codex, Model::Gemini31Pro], 3);
        let input =
            base_input(OrchestrationStrategy::Debate(config)).with_hil_mode(HilMode::Interactive);
        let handler = Arc::new(MockEscalationHandler::new(Err(
            HumanInterventionError::Cancelled,
        )));

        let err = DebateStrategyExecutor::new()
            .execute(
                &input,
                Arc::new(escalation_gateway()),
                &NoProgress,
                Arc::new(NoEventPublisher),
                Some(handler.clone() as Arc<dyn HumanInterventionPort>),
            )
            .await
            .unwrap_err();

        assert!(matches!(err, RunQuorumError::Cancelled));
    }
}
