//! Prompt templates for the Debate strategy — adversarial discussion (#314, RFC #313).
//!
//! Unlike [`PromptTemplate`](super::PromptTemplate)'s Quorum flow (equal peers,
//! reviewing each other), Debate assigns fixed opposing roles: a **proponent**
//! defends a position, an **opponent** attacks it, optional **interjectors**
//! weigh in from the sidelines, and a **moderator** checks each round for
//! settlement and produces the final verdict.
//!
//! # Design: adversarial generation ported from training-time GAN to inference-time
//!
//! RFC Discussion #313 formalizes this strategy as porting GAN's adversarial structure
//! to inference time, and works out where the analogy breaks (an LLM critic is only
//! *asked* to be adversarial via a prompt, unlike a discriminator constrained by loss).
//! Three prompt-level countermeasures come directly out of that analysis:
//!
//! 1. **Falsifiability gate** — a GAN discriminator can't fake gradient, but an LLM
//!    critic *can* fake criticism (RLHF-induced sycophancy makes "attacks" collapse into
//!    performative pushback). Rebuttals are forced into a {claim, evidence, severity}
//!    structure, and the moderator is told to discard rebuttals with no concrete
//!    counterexample as unfalsifiable — regardless of how confident they sound.
//!    `intensity` accordingly controls how deep the opponent searches for
//!    counterexamples, not how aggressive its tone is.
//! 2. **Reasoned concession** — the mirror failure: a critic's "this is wrong" can
//!    itself be a hallucination, and a proponent conceding without checking is
//!    sycophantic capitulation (GAN's mode collapse analog — a degenerate consensus
//!    that *looks* like success). Concessions must state a reason, and the moderator
//!    is told to distrust concessions that aren't backed by the rebuttal's own merits.
//! 3. **Structured verdict** — free-form synthesis re-admits single-model bias at the
//!    top (GAN's minimax step is the analogy's weakest link). The moderator must rule
//!    each rebuttal accepted/rejected with a reason before stating its conclusion,
//!    rather than writing prose that can quietly ignore an unresolved attack.

use crate::orchestration::strategy::DebateIntensity;

/// Templates for generating Debate strategy prompts.
pub struct DebatePromptTemplate;

impl DebatePromptTemplate {
    /// System prompt for the proponent (position-defending) role.
    pub fn proponent_system(intensity: DebateIntensity) -> String {
        let stance = match intensity {
            DebateIntensity::Mild => {
                "Defend your position against the opponent's most likely objections."
            }
            DebateIntensity::Strong => {
                "Defend your position against the most demanding scrutiny — assume the \
                 opponent is searching hard for edge cases and adversarial scenarios."
            }
        };
        format!(
            r#"You are the proponent in an adversarial debate between AI models.
Your task is to propose and defend a well-reasoned position on the question at hand.
{}
Be specific and concrete — vague positions are easy targets.

When the opponent raises a rebuttal, evaluate it on its merits:
- If it gives a concrete counterexample or evidence you cannot refute, concede the
  point — but you MUST state your reason: what the rebuttal proved and how it changes
  your position.
- If it has no concrete counterexample (a vague impression, a stylistic preference),
  do not concede — say so explicitly and hold your position.
Never concede just to be agreeable. An ungrounded concession (sycophantic capitulation)
is worse than a firm position the moderator can actually evaluate."#,
            stance
        )
    }

    /// Opening statement prompt for the proponent (round 1).
    pub fn proponent_opening_prompt(question: &str) -> String {
        format!(
            r#"Question under debate:

{}

State and defend your initial position. Be concrete about the design/answer you propose and why."#,
            question
        )
    }

    /// Defense prompt for the proponent (round 2+), responding to the transcript so far.
    pub fn proponent_defense_prompt(question: &str, transcript: &str) -> String {
        format!(
            r#"Question under debate:

{}

Debate so far:

{}

The opponent has attacked your position above. Evaluate the rebuttal:
- If it has a real counterexample or evidence you cannot refute, concede — and state
  your reason.
- If it's unfalsifiable (no concrete counterexample or evidence), hold your position
  and say plainly why the rebuttal doesn't land.
Refine or narrow your position only where a real gap was shown — do not abandon it
without cause."#,
            question, transcript
        )
    }

    /// System prompt for the opponent (attacking) role.
    pub fn opponent_system(intensity: DebateIntensity) -> String {
        let depth = match intensity {
            DebateIntensity::Mild => {
                "Focus on the most likely failure modes — common edge cases and the \
                 most obvious gaps. You don't need to exhaustively search for rare \
                 scenarios."
            }
            DebateIntensity::Strong => {
                "Search aggressively and exhaustively for failure modes — rare edge \
                 cases, adversarial inputs, and structural gaps that only surface \
                 under scrutiny. Do not stop at the first plausible counterexample; \
                 keep looking for the strongest one."
            }
        };
        format!(
            r#"You are the opponent in an adversarial debate between AI models.
Your task is to find falsifiable flaws in the proponent's position — the goal is to be
right, not to sound aggressive. {}
`intensity` controls how deep you search for counterexamples, not how harsh your tone is.

Every rebuttal you raise MUST have this structure:
CLAIM: the specific part of the proponent's position you are attacking (quote or paraphrase it precisely)
EVIDENCE: a concrete counterexample, scenario, or piece of reasoning that breaks the claim — not a vague impression
SEVERITY: CRITICAL (breaks the position entirely) / MAJOR (a real gap that needs addressing) / MINOR (a nitpick worth noting)

A rebuttal without a concrete counterexample or evidence — e.g. "this could probably be
made more robust" — is not falsifiable. The moderator will discard it, so don't waste a
round on it."#,
            depth
        )
    }

    /// Attack prompt for the opponent, responding to the transcript so far.
    pub fn opponent_attack_prompt(question: &str, transcript: &str) -> String {
        format!(
            r#"Question under debate:

{}

Debate so far:

{}

Challenge the proponent's latest position above. Find its weakest point and attack it
directly, using the CLAIM / EVIDENCE / SEVERITY structure from your instructions."#,
            question, transcript
        )
    }

    /// System prompt for a third-model interjector.
    pub fn interjector_system() -> &'static str {
        r#"You are a third-party expert interjecting in an ongoing debate between
two other AI models. You are not aligned with either side.
Your task is to raise one consideration neither side has raised, or flag a blind spot
both sides share. Like the opponent's rebuttals, your interjection must be falsifiable:
give a concrete counterexample, scenario, or piece of evidence — not a vague impression.
Be brief: one sharp point with its evidence, not a full argument."#
    }

    /// Interjection prompt, given the transcript so far.
    pub fn interjector_prompt(question: &str, transcript: &str) -> String {
        format!(
            r#"Question under debate:

{}

Debate so far:

{}

Add one brief interjection: a missing consideration or a blind spot shared by both sides.
Back it with a concrete counterexample or evidence, not an impression."#,
            question, transcript
        )
    }

    /// System prompt for the moderator's per-round checkpoint.
    pub fn moderator_system() -> &'static str {
        r#"You are the moderator of an adversarial debate between AI models.
After each round, you decide whether the debate has settled — one position has
clearly prevailed, or the remaining disagreement is not worth further rounds —
or whether it should continue.

## Judging rebuttals

For every rebuttal raised in the debate so far, apply this test:
- If it has a concrete counterexample or evidence: judge it on the merits — is the
  counterexample real, and does it actually break the claim?
- If it has no concrete counterexample or evidence (a vague impression, a stylistic
  preference, "this could be more robust" without saying how) — rule it REJECTED as
  unfalsifiable, no matter how confident it sounds.

Also scrutinize concessions. If the proponent conceded a point, don't take that at face
value — check whether the rebuttal that triggered it was actually grounded. A model
conceding just to sound cooperative (sycophantic capitulation) is a failure mode: if the
underlying rebuttal was unfalsifiable or wrong, rule it REJECTED even though the
proponent agreed to it.

## Response format

Your response MUST start with exactly one of these two lines:
VERDICT: SETTLED
VERDICT: CONTINUE

Then, for EACH rebuttal raised so far in the debate, list:
REBUTTAL: <one-line summary of what was attacked>
RULING: ACCEPTED or REJECTED
REASON: <why — cite the concrete counterexample/evidence if accepted, or explain why it
was unfalsifiable or wrong if rejected>

Finally, exactly one of:
- If SETTLED: CONCLUSION: <the position that survives, informed by the rulings above,
  written for someone who did not read the debate>
- If CONTINUE: NEXT: <what remains unresolved — an unrefuted CRITICAL or MAJOR
  rebuttal — that the next round should focus on>"#
    }

    /// Per-round checkpoint prompt for the moderator.
    ///
    /// When `is_final_round` is `true`, the moderator is instructed to settle
    /// regardless of remaining disagreement — `max_rounds` is a hard cap.
    pub fn moderator_checkpoint_prompt(
        question: &str,
        transcript: &str,
        round: usize,
        max_rounds: usize,
        is_final_round: bool,
    ) -> String {
        let final_notice = if is_final_round {
            "\nThis is the final allowed round. You MUST respond with VERDICT: SETTLED \
             and produce your rulings and the best conclusion possible from the debate \
             so far, even if disagreement remains."
        } else {
            ""
        };
        format!(
            r#"Question under debate:

{}

Debate so far (round {} of {}):

{}

Has the debate settled? Rule on each rebuttal raised so far, per your instructions.{}"#,
            question, round, max_rounds, transcript, final_notice
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proponent_opening_contains_question() {
        let prompt = DebatePromptTemplate::proponent_opening_prompt("Should we use REST or gRPC?");
        assert!(prompt.contains("Should we use REST or gRPC?"));
    }

    #[test]
    fn test_opponent_attack_contains_transcript() {
        let prompt = DebatePromptTemplate::opponent_attack_prompt(
            "Q",
            "--- Proponent ---\nUse REST for simplicity.",
        );
        assert!(prompt.contains("Use REST for simplicity."));
    }

    #[test]
    fn test_moderator_checkpoint_final_round_forces_settle() {
        let prompt =
            DebatePromptTemplate::moderator_checkpoint_prompt("Q", "transcript", 3, 3, true);
        assert!(prompt.contains("MUST respond with VERDICT: SETTLED"));
    }

    #[test]
    fn test_moderator_checkpoint_non_final_round_allows_continue() {
        let prompt =
            DebatePromptTemplate::moderator_checkpoint_prompt("Q", "transcript", 1, 3, false);
        assert!(!prompt.contains("MUST respond with VERDICT: SETTLED"));
    }

    #[test]
    fn test_intensity_changes_search_depth_not_tone() {
        let mild = DebatePromptTemplate::opponent_system(DebateIntensity::Mild);
        let strong = DebatePromptTemplate::opponent_system(DebateIntensity::Strong);
        assert_ne!(mild, strong);
        // Both variants must frame `intensity` as search depth, not rhetorical aggression.
        assert!(
            mild.contains("how deep you search for counterexamples, not how harsh your tone is")
        );
        assert!(
            strong.contains("how deep you search for counterexamples, not how harsh your tone is")
        );
        assert!(strong.contains("exhaustively"));
    }

    #[test]
    fn test_opponent_system_requires_falsifiable_structure() {
        let prompt = DebatePromptTemplate::opponent_system(DebateIntensity::Strong);
        assert!(prompt.contains("CLAIM:"));
        assert!(prompt.contains("EVIDENCE:"));
        assert!(prompt.contains("SEVERITY:"));
    }

    #[test]
    fn test_proponent_system_requires_reasoned_concession() {
        let prompt = DebatePromptTemplate::proponent_system(DebateIntensity::Mild);
        assert!(prompt.contains("MUST state your reason"));
        assert!(prompt.contains("sycophantic capitulation"));
    }

    #[test]
    fn test_moderator_system_requires_structured_ruling() {
        let system = DebatePromptTemplate::moderator_system();
        assert!(system.contains("REBUTTAL:"));
        assert!(system.contains("RULING:"));
        assert!(system.contains("REASON:"));
        assert!(system.contains("unfalsifiable"));
        assert!(system.contains("sycophantic capitulation"));
    }
}
