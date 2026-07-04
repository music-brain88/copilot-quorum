//! Prompt templates for headless PR/diff review (#300, RFC Discussion #304 D2).
//!
//! Unlike [`AgentPromptTemplate`](super::AgentPromptTemplate)'s `plan_review`/
//! `action_review` (which review agent-internal artifacts), these templates
//! review an external diff — with optional PR metadata and a reviewer-supplied
//! focus — so the "subject under review" is built once via [`build_material`]
//! and then reused for both the per-model vote prompt and the moderator's
//! synthesis prompt.

use crate::quorum::{Vote, VoteVerdict};

/// Templates for generating PR/diff review prompts.
pub struct ReviewPromptTemplate;

impl ReviewPromptTemplate {
    /// Combine optional PR context, optional focus instructions, and the diff
    /// itself into the single "review material" string reused by both the
    /// vote and synthesis prompts.
    pub fn build_material(diff: &str, focus: Option<&str>, pr_context: Option<&str>) -> String {
        let mut material = String::new();
        if let Some(pr_context) = pr_context {
            material.push_str("## Pull Request\n\n");
            material.push_str(pr_context);
            material.push_str("\n\n");
        }
        if let Some(focus) = focus {
            material.push_str("## Review Focus\n\n");
            material.push_str(focus);
            material.push_str("\n\n");
        }
        material.push_str("## Diff\n\n```diff\n");
        material.push_str(diff.trim_end());
        material.push_str("\n```");
        material
    }

    /// System prompt for the per-model vote phase.
    pub fn review_system() -> &'static str {
        r#"You are a senior software engineer performing a code review as one voice
in a multi-model quorum. Other models are reviewing the same diff independently
and will not see your response. Focus on correctness bugs, security issues, and
significant design problems — not style nits.
Provide your assessment with a clear APPROVE or REJECT recommendation."#
    }

    /// Generates the user prompt for the per-model vote phase.
    pub fn review_prompt(material: &str) -> String {
        format!(
            r#"## Task

Review the following pull request diff and vote APPROVE or REJECT.

{material}

## Review Instructions

Evaluate the diff for:
1. **Correctness**: Does the code do what it appears to intend? Any logic bugs?
2. **Safety**: Security issues, data loss risks, or unhandled edge cases?
3. **Completeness**: Is anything obviously missing (e.g. tests for new behavior)?
4. **Design**: Are there significant design or maintainability concerns?

Provide your assessment with:
- Overall recommendation: APPROVE or REJECT
- Specific findings (if any), each with a file/line reference when possible"#,
            material = material
        )
    }

    /// System prompt for the moderator's synthesis phase.
    pub fn synthesis_system() -> &'static str {
        r#"You are the moderator synthesizing independent code review votes from
multiple models into a single, unified review.
Your task is to:
1. Summarize the overall recommendation and why
2. Consolidate findings that multiple reviewers agree on
3. Note any disagreements between reviewers and assess which position is better supported
4. Produce a review a human (or CI) can act on directly

Be balanced and specific. Give weight to well-reasoned, concrete findings over vague concerns."#
    }

    /// Generates the user prompt for the moderator's synthesis phase.
    pub fn synthesis_prompt(material: &str, votes: &[Vote]) -> String {
        let mut prompt = format!(
            r#"{material}

## Reviewer Votes
"#,
            material = material
        );

        for vote in votes {
            let verdict = match vote.verdict {
                VoteVerdict::Approve => "APPROVE",
                VoteVerdict::Reject => "REJECT",
                VoteVerdict::Abstain => "ABSTAIN",
                VoteVerdict::ModelError => "ERROR (no vote)",
            };
            prompt.push_str(&format!(
                "\n--- {} ({}) ---\n{}\n",
                vote.model, verdict, vote.reasoning
            ));
        }

        prompt.push_str(
            r#"
## Instructions

Based on all votes above, write a unified review in markdown with:

1. **Recommendation**: APPROVE or REJECT, and a one-paragraph summary of why
2. **Key Findings**: Consolidated, deduplicated findings (bullet list)
3. **Disagreements**: Where reviewers disagreed and your assessment of which is better supported (bullet list, omit if none)"#,
        );

        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_material_diff_only() {
        let material = ReviewPromptTemplate::build_material("diff --git a b", None, None);
        assert!(material.contains("## Diff"));
        assert!(material.contains("diff --git a b"));
        assert!(!material.contains("## Pull Request"));
        assert!(!material.contains("## Review Focus"));
    }

    #[test]
    fn test_build_material_with_pr_and_focus() {
        let material = ReviewPromptTemplate::build_material(
            "diff --git a b",
            Some("Concurrency safety"),
            Some("PR #123: Fix the bug"),
        );
        assert!(material.contains("## Pull Request"));
        assert!(material.contains("PR #123: Fix the bug"));
        assert!(material.contains("## Review Focus"));
        assert!(material.contains("Concurrency safety"));
        // PR context must appear before focus, and focus before the diff
        let pr_pos = material.find("## Pull Request").unwrap();
        let focus_pos = material.find("## Review Focus").unwrap();
        let diff_pos = material.find("## Diff").unwrap();
        assert!(pr_pos < focus_pos);
        assert!(focus_pos < diff_pos);
    }

    #[test]
    fn test_review_prompt_contains_material_and_instructions() {
        let material = ReviewPromptTemplate::build_material("diff --git a b", None, None);
        let prompt = ReviewPromptTemplate::review_prompt(&material);
        assert!(prompt.contains("diff --git a b"));
        assert!(prompt.contains("APPROVE or REJECT"));
    }

    #[test]
    fn test_synthesis_prompt_includes_votes() {
        let material = ReviewPromptTemplate::build_material("diff --git a b", None, None);
        let votes = vec![
            Vote::approve("claude-opus-4.5", "Looks safe"),
            Vote::reject("gpt-5.3-codex", "Missing test coverage"),
        ];
        let prompt = ReviewPromptTemplate::synthesis_prompt(&material, &votes);

        assert!(prompt.contains("claude-opus-4.5"));
        assert!(prompt.contains("APPROVE"));
        assert!(prompt.contains("Looks safe"));
        assert!(prompt.contains("gpt-5.3-codex"));
        assert!(prompt.contains("REJECT"));
        assert!(prompt.contains("Missing test coverage"));
        assert!(prompt.contains("Recommendation"));
    }
}
