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
round on it.

## Requesting decomposition

Sometimes the proponent's position is compound or ambiguous — it bundles several
independent claims together, or is phrased so vaguely that no single CLAIM/EVIDENCE/
SEVERITY rebuttal can attack it directly. In that situation only, instead of a normal
rebuttal you may respond with exactly one line:
DECOMPOSE_REQUEST: <the specific claim you want broken into verifiable sub-claims>
This asks the proponent to split that claim into sub-claims you can attack one at a
time. Do not include CLAIM/EVIDENCE/SEVERITY when using this — it replaces them for
that turn. You may use DECOMPOSE_REQUEST at most once per round; if the position is
still unattackable after decomposition, fall back to your best CLAIM/EVIDENCE/SEVERITY
rebuttal rather than requesting decomposition again."#,
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

    /// Prompt for the proponent responding to an opponent's `DECOMPOSE_REQUEST:`.
    ///
    /// The opponent has judged `target` too compound or ambiguous to attack
    /// directly and asked for it to be split into independently verifiable
    /// sub-claims. This asks the proponent to do that decomposition rather
    /// than to defend the claim as-is.
    pub fn proponent_decomposition_prompt(target: &str, transcript: &str) -> String {
        format!(
            r#"Debate so far:

{}

The opponent has judged the following claim of yours too compound or ambiguous to
attack directly, and requested that you decompose it:

{}

Break this claim into a small number of independently verifiable sub-claims — each
one specific and narrow enough that the opponent could attack it with a concrete
counterexample. Do not weaken or abandon the claim; restate it more precisely as a
list of sub-claims that, together, are equivalent to what you originally meant."#,
            transcript, target
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

Then, for EACH open objection listed in the checkpoint prompt (each given its own
REBUTTAL_ID), list:
REBUTTAL_ID: <the ID exactly as given, e.g. R1-1 — do not paraphrase or renumber it>
RULING: ACCEPTED or REJECTED
REASON: <why — cite the concrete counterexample/evidence if accepted, or explain why it
was unfalsifiable or wrong if rejected>

Finally, exactly one of:
- If SETTLED: CONCLUSION: <the position that survives, informed by the rulings above,
  written for someone who did not read the debate>
- If CONTINUE: NEXT: <what remains unresolved — an unrefuted CRITICAL or MAJOR
  rebuttal — that the next round should focus on>"#
    }

    /// Per-round checkpoint prompt for the moderator, with the still-open
    /// objections (from an [`ObjectionLedger`](crate::quorum::ObjectionLedger))
    /// explicitly enumerated by ID and claim.
    ///
    /// Instead of asking the moderator to re-derive rebuttal identity from
    /// prose, it hands over the exact `REBUTTAL_ID`s the moderator must rule
    /// on and echo back, so [`parse_moderator_rulings`](crate::quorum::parsing::parse_moderator_rulings)
    /// can match rulings to ledger entries by exact ID.
    ///
    /// `open_objections` is a list of `(id, claim)` pairs — typically built
    /// from `ObjectionLedger::open_objections()`.
    pub fn moderator_checkpoint_prompt_with_objections(
        question: &str,
        transcript: &str,
        round: usize,
        max_rounds: usize,
        is_final_round: bool,
        open_objections: &[(&str, &str)],
    ) -> String {
        let final_notice = if is_final_round {
            "\nThis is the final allowed round. You MUST respond with VERDICT: SETTLED \
             and produce your rulings and the best conclusion possible from the debate \
             so far, even if disagreement remains."
        } else {
            ""
        };
        let objections_block = if open_objections.is_empty() {
            "None — no unresolved objections remain in the ledger.".to_string()
        } else {
            open_objections
                .iter()
                .map(|(id, claim)| format!("REBUTTAL_ID: {}\nCLAIM: {}", id, claim))
                .collect::<Vec<_>>()
                .join("\n\n")
        };
        format!(
            r#"Question under debate:

{}

Debate so far (round {} of {}):

{}

Open objections requiring a ruling (rule on each by its REBUTTAL_ID, exactly as given):

{}

Has the debate settled? Rule on each open objection above, per your instructions.{}"#,
            question, round, max_rounds, transcript, objections_block, final_notice
        )
    }

    /// Additional system instructions for the moderator's divergence check.
    ///
    /// Before committing rounds to attacking each other's claims, the
    /// moderator can be asked whether the proponent's opening and the
    /// opponent's first attack are actually reaching different conclusions,
    /// or whether they've quietly converged on the same one while disputing
    /// unrelated framing. If they agree, the debate should attack the shared
    /// premise itself rather than continue as if in genuine disagreement —
    /// see [`contrarian_system`](Self::contrarian_system).
    pub fn moderator_divergence_system() -> &'static str {
        r#"You are being asked an additional question as the moderator of an adversarial
debate, separate from your usual per-round ruling.

Read the proponent's opening position and the opponent's first attack. Ask: are these
substantively reaching the same conclusion, just phrased differently or arguing past
each other on surface details? Or do they genuinely disagree on the substance?

If they are substantively the same conclusion, the debate as framed is not adversarial —
both sides share an unexamined premise, and attacking each other's phrasing wastes
rounds. In that case the shared premise itself should be attacked instead.

Your response MUST start with exactly one of these two lines:
DIVERGENT: YES
DIVERGENT: NO

Then, on the following lines:
- If DIVERGENT: NO — state the shared premise both sides rest on, precisely enough that
  it could be attacked directly.
- If DIVERGENT: YES — briefly note what genuinely differs between the two positions."#
    }

    /// Prompt for the moderator's divergence check, given the opening and
    /// first attack.
    pub fn divergence_check_prompt(question: &str, opening: &str, first_attack: &str) -> String {
        format!(
            r#"Question under debate:

{}

Proponent's opening position:

{}

Opponent's first attack:

{}

Are these two positions substantively the same conclusion, or do they genuinely
diverge? Follow the DIVERGENT: YES/NO format from your instructions."#,
            question, opening, first_attack
        )
    }

    /// System prompt for the contrarian role.
    ///
    /// Used when the moderator's divergence check finds the proponent and
    /// opponent have quietly converged on a shared premise instead of
    /// genuinely disagreeing. The contrarian is deliberately assigned the
    /// opposing stance to that shared premise and told to attack it directly,
    /// rather than restate either side's existing arguments.
    pub fn contrarian_system() -> &'static str {
        r#"You are the contrarian in an adversarial debate between AI models.
The proponent and opponent have converged on a shared premise instead of genuinely
disagreeing — the debate has not been adversarial in substance, only in surface framing.

Your task is to explicitly take the opposite stance to that shared premise and attack
it directly. Do not restate or referee the existing arguments between the proponent and
opponent — assume that premise is wrong and make the strongest case you can for why.

Like the opponent's rebuttals, your case must be falsifiable: give a concrete
counterexample, scenario, or piece of evidence for why the shared premise doesn't hold —
not a vague impression or stylistic objection."#
    }

    /// Brief prompt for the contrarian, given the shared premise and transcript.
    pub fn contrarian_brief_prompt(
        question: &str,
        shared_premise: &str,
        transcript: &str,
    ) -> String {
        format!(
            r#"Question under debate:

{}

Debate so far:

{}

The moderator has identified this as a premise both sides share instead of genuinely
contesting:

{}

Take the opposite stance to this premise and attack it directly, per your instructions."#,
            question, transcript, shared_premise
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
        assert!(system.contains("REBUTTAL_ID:"));
        assert!(system.contains("RULING:"));
        assert!(system.contains("REASON:"));
        assert!(system.contains("unfalsifiable"));
        assert!(system.contains("sycophantic capitulation"));
        // The old free-text "REBUTTAL:" summary format must be gone in favor
        // of ID references the moderator echoes back exactly.
        assert!(!system.contains("REBUTTAL: <one-line summary"));
    }

    #[test]
    fn test_opponent_system_describes_decompose_request() {
        let prompt = DebatePromptTemplate::opponent_system(DebateIntensity::Mild);
        assert!(prompt.contains("DECOMPOSE_REQUEST:"));
        assert!(prompt.contains("at most once per round"));
    }

    #[test]
    fn test_proponent_decomposition_prompt_contains_target_and_transcript() {
        let prompt = DebatePromptTemplate::proponent_decomposition_prompt(
            "the system is secure",
            "--- Opponent ---\nDECOMPOSE_REQUEST: the system is secure",
        );
        assert!(prompt.contains("the system is secure"));
        assert!(prompt.contains("DECOMPOSE_REQUEST: the system is secure"));
        assert!(prompt.contains("sub-claims"));
    }

    #[test]
    fn test_moderator_checkpoint_with_objections_lists_ids_and_claims() {
        let prompt = DebatePromptTemplate::moderator_checkpoint_prompt_with_objections(
            "Q",
            "transcript",
            2,
            3,
            false,
            &[
                ("R1-1", "the cache never expires"),
                ("R2-1", "concurrent writes are unguarded"),
            ],
        );
        assert!(prompt.contains("REBUTTAL_ID: R1-1"));
        assert!(prompt.contains("CLAIM: the cache never expires"));
        assert!(prompt.contains("REBUTTAL_ID: R2-1"));
        assert!(prompt.contains("CLAIM: concurrent writes are unguarded"));
    }

    #[test]
    fn test_moderator_checkpoint_with_objections_handles_empty_ledger() {
        let prompt = DebatePromptTemplate::moderator_checkpoint_prompt_with_objections(
            "Q",
            "transcript",
            1,
            3,
            false,
            &[],
        );
        assert!(prompt.contains("None"));
    }

    #[test]
    fn test_moderator_checkpoint_with_objections_final_round_forces_settle() {
        let prompt = DebatePromptTemplate::moderator_checkpoint_prompt_with_objections(
            "Q",
            "transcript",
            3,
            3,
            true,
            &[("R3-1", "claim")],
        );
        assert!(prompt.contains("MUST respond with VERDICT: SETTLED"));
    }

    #[test]
    fn test_divergence_check_prompt_contains_opening_and_first_attack() {
        let prompt = DebatePromptTemplate::divergence_check_prompt(
            "Should we use REST or gRPC?",
            "We should use REST for simplicity.",
            "REST is fine but the real issue is versioning strategy.",
        );
        assert!(prompt.contains("Should we use REST or gRPC?"));
        assert!(prompt.contains("We should use REST for simplicity."));
        assert!(prompt.contains("REST is fine but the real issue is versioning strategy."));
        assert!(prompt.contains("DIVERGENT: YES/NO"));
    }

    #[test]
    fn test_moderator_divergence_system_requires_divergent_format() {
        let system = DebatePromptTemplate::moderator_divergence_system();
        assert!(system.contains("DIVERGENT: YES"));
        assert!(system.contains("DIVERGENT: NO"));
        assert!(system.contains("shared premise"));
    }

    #[test]
    fn test_contrarian_system_requires_shared_premise_attack() {
        let system = DebatePromptTemplate::contrarian_system();
        assert!(system.contains("shared premise"));
        assert!(system.contains("falsifiable"));
        assert!(system.contains("opposite stance"));
    }

    #[test]
    fn test_contrarian_brief_prompt_contains_shared_premise_and_transcript() {
        let prompt = DebatePromptTemplate::contrarian_brief_prompt(
            "Q",
            "Both sides assume the API must be synchronous.",
            "--- Proponent ---\nUse a synchronous API.",
        );
        assert!(prompt.contains("Both sides assume the API must be synchronous."));
        assert!(prompt.contains("Use a synchronous API."));
    }
}
