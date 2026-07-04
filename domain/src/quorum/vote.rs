//! Vote types for Quorum consensus
//!
//! This module defines the core voting primitives used in Quorum decision making.

use serde::{Deserialize, Serialize};

/// The verdict a model returned for a Quorum decision
///
/// `Abstain` and `ModelError` are recorded for visibility but are
/// **not counted in the voting denominator** — only cast votes
/// (`Approve` / `Reject`) decide the outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoteVerdict {
    /// The model approved
    Approve,
    /// The model rejected
    Reject,
    /// The model explicitly declined to judge (reserved; no producer yet)
    Abstain,
    /// The model could not be queried (gateway failure, timeout, etc.)
    ModelError,
}

/// A single vote from a model in a Quorum decision
///
/// # Example
///
/// ```
/// use quorum_domain::quorum::Vote;
///
/// let approval = Vote::approve("claude-sonnet-4.5", "The plan is sound and follows best practices.");
/// assert!(approval.is_approve());
///
/// let rejection = Vote::reject("gpt-5.2-codex", "Security concern: SQL injection risk in query.");
/// assert!(rejection.is_reject());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Vote {
    /// Model identifier (e.g., "claude-sonnet-4.5", "gpt-5.2-codex")
    pub model: String,
    /// The verdict this model returned
    pub verdict: VoteVerdict,
    /// Reasoning or feedback from this model
    pub reasoning: String,
    /// Confidence level (0.0 to 1.0, if available)
    pub confidence: Option<f64>,
}

impl Vote {
    /// Create a new vote
    pub fn new(
        model: impl Into<String>,
        verdict: VoteVerdict,
        reasoning: impl Into<String>,
    ) -> Self {
        Self {
            model: model.into(),
            verdict,
            reasoning: reasoning.into(),
            confidence: None,
        }
    }

    /// Create an approval vote
    pub fn approve(model: impl Into<String>, reasoning: impl Into<String>) -> Self {
        Self::new(model, VoteVerdict::Approve, reasoning)
    }

    /// Create a rejection vote
    pub fn reject(model: impl Into<String>, reasoning: impl Into<String>) -> Self {
        Self::new(model, VoteVerdict::Reject, reasoning)
    }

    /// Create an abstention vote
    pub fn abstain(model: impl Into<String>, reasoning: impl Into<String>) -> Self {
        Self::new(model, VoteVerdict::Abstain, reasoning)
    }

    /// Create a vote recording a model failure (not counted in the tally)
    pub fn model_error(model: impl Into<String>, reasoning: impl Into<String>) -> Self {
        Self::new(model, VoteVerdict::ModelError, reasoning)
    }

    /// Whether this vote is an approval
    pub fn is_approve(&self) -> bool {
        self.verdict == VoteVerdict::Approve
    }

    /// Whether this vote is a rejection
    pub fn is_reject(&self) -> bool {
        self.verdict == VoteVerdict::Reject
    }

    /// Whether this vote was actually cast (counts in the denominator)
    pub fn is_cast(&self) -> bool {
        matches!(self.verdict, VoteVerdict::Approve | VoteVerdict::Reject)
    }

    /// Add confidence level to the vote
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = Some(confidence.clamp(0.0, 1.0));
        self
    }

    /// Get a short display name for the model
    ///
    /// E.g., "claude-sonnet-4.5" -> "claude"
    pub fn short_model_name(&self) -> &str {
        self.model.split(['-', '_']).next().unwrap_or(&self.model)
    }
}

/// Result of a Quorum vote aggregation
///
/// Contains the aggregated result of multiple votes along with
/// statistics and the original votes for detailed analysis.
/// Abstentions and model errors are kept in `votes` for visibility
/// but excluded from the counting denominator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteResult {
    /// Whether the vote passed (per the applied rule, over cast votes)
    pub passed: bool,
    /// Number of approving votes
    pub approve_count: usize,
    /// Number of rejecting votes
    pub reject_count: usize,
    /// Total number of recorded votes (including abstain / model_error)
    pub total_votes: usize,
    /// All individual votes
    pub votes: Vec<Vote>,
    /// Aggregated feedback from all votes
    pub aggregated_feedback: Option<String>,
}

impl VoteResult {
    /// Create a new VoteResult from a list of votes using majority rule
    ///
    /// The denominator is the number of *cast* votes (approve + reject).
    /// When the vote does not pass, rejection feedback is aggregated
    /// automatically.
    pub fn from_votes(votes: Vec<Vote>) -> Self {
        let approve_count = votes.iter().filter(|v| v.is_approve()).count();
        let reject_count = votes.iter().filter(|v| v.is_reject()).count();
        let cast_votes = approve_count + reject_count;
        let total_votes = votes.len();

        // Majority rule: more than half of cast votes must approve
        let passed = approve_count > cast_votes / 2;

        let mut result = Self {
            passed,
            approve_count,
            reject_count,
            total_votes,
            votes,
            aggregated_feedback: None,
        };
        if !result.passed && result.reject_count > 0 {
            result.aggregated_feedback = Some(result.aggregate_rejection_feedback());
        }
        result
    }

    /// Create a VoteResult with a specific rule
    ///
    /// The denominator is the number of *cast* votes (approve + reject).
    pub fn from_votes_with_rule(votes: Vec<Vote>, rule: &super::rule::QuorumRule) -> Self {
        let approve_count = votes.iter().filter(|v| v.is_approve()).count();
        let reject_count = votes.iter().filter(|v| v.is_reject()).count();
        let cast_votes = approve_count + reject_count;
        let total_votes = votes.len();

        let passed = rule.is_satisfied(approve_count, cast_votes);

        let mut result = Self {
            passed,
            approve_count,
            reject_count,
            total_votes,
            votes,
            aggregated_feedback: None,
        };
        if !result.passed && result.reject_count > 0 {
            result.aggregated_feedback = Some(result.aggregate_rejection_feedback());
        }
        result
    }

    /// Create a VoteResult for a skipped / auto-approved review (no votes)
    pub fn skipped() -> Self {
        Self {
            passed: true,
            approve_count: 0,
            reject_count: 0,
            total_votes: 0,
            votes: Vec::new(),
            aggregated_feedback: None,
        }
    }

    /// Number of cast votes (the counting denominator)
    pub fn cast_votes(&self) -> usize {
        self.approve_count + self.reject_count
    }

    /// Whether any vote was actually cast
    pub fn has_cast_votes(&self) -> bool {
        self.cast_votes() > 0
    }

    /// Add aggregated feedback
    pub fn with_feedback(mut self, feedback: impl Into<String>) -> Self {
        self.aggregated_feedback = Some(feedback.into());
        self
    }

    /// Check if the cast votes were unanimous
    pub fn is_unanimous(&self) -> bool {
        let cast = self.cast_votes();
        cast > 0 && (self.approve_count == cast || self.reject_count == cast)
    }

    /// Get the approval ratio over cast votes (0.0 to 1.0)
    pub fn approval_ratio(&self) -> f64 {
        let cast = self.cast_votes();
        if cast == 0 {
            0.0
        } else {
            self.approve_count as f64 / cast as f64
        }
    }

    /// Generate a visual vote summary (e.g., "[●●○]"; `!` = abstain / error)
    pub fn vote_summary(&self) -> String {
        let mut summary = String::from("[");
        for vote in &self.votes {
            summary.push(match vote.verdict {
                VoteVerdict::Approve => '●',
                VoteVerdict::Reject => '○',
                VoteVerdict::Abstain | VoteVerdict::ModelError => '!',
            });
        }
        summary.push(']');
        summary
    }

    /// Get rejecting votes only
    pub fn rejections(&self) -> impl Iterator<Item = &Vote> {
        self.votes.iter().filter(|v| v.is_reject())
    }

    /// Get approving votes only
    pub fn approvals(&self) -> impl Iterator<Item = &Vote> {
        self.votes.iter().filter(|v| v.is_approve())
    }

    /// Aggregate rejection feedback into a single string
    pub fn aggregate_rejection_feedback(&self) -> String {
        self.rejections()
            .map(|v| format!("{}: {}", v.model, v.reasoning))
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vote_creation() {
        let vote = Vote::approve("claude-sonnet-4.5", "Looks good!");
        assert!(vote.is_approve());
        assert!(vote.is_cast());
        assert_eq!(vote.model, "claude-sonnet-4.5");
        assert_eq!(vote.reasoning, "Looks good!");
    }

    #[test]
    fn test_vote_verdict_serde() {
        let vote = Vote::model_error("claude", "timeout");
        let json = serde_json::to_value(&vote).unwrap();
        assert_eq!(json["verdict"], "model_error");
        assert_eq!(
            serde_json::to_value(Vote::approve("m", "").verdict).unwrap(),
            "approve"
        );
        assert_eq!(
            serde_json::to_value(Vote::abstain("m", "").verdict).unwrap(),
            "abstain"
        );
    }

    #[test]
    fn test_vote_with_confidence() {
        let vote = Vote::approve("claude", "OK").with_confidence(0.95);
        assert_eq!(vote.confidence, Some(0.95));

        // Test clamping
        let vote2 = Vote::approve("claude", "OK").with_confidence(1.5);
        assert_eq!(vote2.confidence, Some(1.0));
    }

    #[test]
    fn test_short_model_name() {
        let vote = Vote::approve("claude-sonnet-4.5", "OK");
        assert_eq!(vote.short_model_name(), "claude");

        let vote2 = Vote::approve("gpt_5_codex", "OK");
        assert_eq!(vote2.short_model_name(), "gpt");
    }

    #[test]
    fn test_vote_result_majority() {
        let votes = vec![
            Vote::approve("model-a", "Yes"),
            Vote::approve("model-b", "Yes"),
            Vote::reject("model-c", "No"),
        ];
        let result = VoteResult::from_votes(votes);

        assert!(result.passed);
        assert_eq!(result.approve_count, 2);
        assert_eq!(result.reject_count, 1);
        assert!(!result.is_unanimous());
    }

    #[test]
    fn test_vote_result_rejected() {
        let votes = vec![
            Vote::reject("model-a", "No way"),
            Vote::reject("model-b", "Nope"),
            Vote::approve("model-c", "Maybe"),
        ];
        let result = VoteResult::from_votes(votes);

        assert!(!result.passed);
        assert_eq!(result.approve_count, 1);
        assert_eq!(result.reject_count, 2);
        // Rejection feedback is aggregated automatically
        let feedback = result.aggregated_feedback.as_deref().unwrap();
        assert!(feedback.contains("model-a: No way"));
    }

    #[test]
    fn test_vote_result_unanimous() {
        let votes = vec![
            Vote::approve("model-a", "Yes"),
            Vote::approve("model-b", "Yes"),
        ];
        let result = VoteResult::from_votes(votes);

        assert!(result.is_unanimous());
        assert_eq!(result.approval_ratio(), 1.0);
    }

    #[test]
    fn test_model_error_excluded_from_denominator() {
        // 1 approve, 1 reject, 1 model_error: cast = 2, majority needs > 1
        let votes = vec![
            Vote::approve("model-a", "Yes"),
            Vote::reject("model-b", "No"),
            Vote::model_error("model-c", "gateway timeout"),
        ];
        let result = VoteResult::from_votes(votes);

        assert_eq!(result.cast_votes(), 2);
        assert_eq!(result.total_votes, 3);
        assert!(!result.passed); // 1 > 2/2 is false

        // 2 approve + 1 model_error: passes as if the error never voted
        let votes = vec![
            Vote::approve("model-a", "Yes"),
            Vote::approve("model-b", "Yes"),
            Vote::model_error("model-c", "gateway timeout"),
        ];
        let result = VoteResult::from_votes(votes);
        assert!(result.passed);
        assert!(result.is_unanimous());
    }

    #[test]
    fn test_all_votes_error_does_not_pass() {
        let votes = vec![
            Vote::model_error("model-a", "down"),
            Vote::model_error("model-b", "down"),
        ];
        let result = VoteResult::from_votes(votes);

        assert!(!result.passed);
        assert!(!result.has_cast_votes());
        assert_eq!(result.approval_ratio(), 0.0);
        assert!(!result.is_unanimous());
    }

    #[test]
    fn test_skipped() {
        let result = VoteResult::skipped();
        assert!(result.passed);
        assert_eq!(result.total_votes, 0);
        assert!(!result.has_cast_votes());
    }

    #[test]
    fn test_vote_summary() {
        let votes = vec![
            Vote::approve("a", ""),
            Vote::approve("b", ""),
            Vote::reject("c", ""),
            Vote::model_error("d", ""),
        ];
        let result = VoteResult::from_votes(votes);
        assert_eq!(result.vote_summary(), "[●●○!]");
    }

    #[test]
    fn test_aggregate_rejection_feedback() {
        let votes = vec![
            Vote::approve("model-a", "Looks good"),
            Vote::reject("model-b", "Security issue found"),
            Vote::reject("model-c", "Missing error handling"),
        ];
        let result = VoteResult::from_votes(votes);
        let feedback = result.aggregate_rejection_feedback();

        assert!(feedback.contains("model-b: Security issue found"));
        assert!(feedback.contains("model-c: Missing error handling"));
    }
}
