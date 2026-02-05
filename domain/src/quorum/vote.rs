//! Vote types for Quorum consensus
//!
//! This module defines the core voting primitives used in Quorum decision making.

use serde::{Deserialize, Serialize};

/// A single vote from a model in a Quorum decision
///
/// # Example
///
/// ```
/// use quorum_domain::quorum::Vote;
///
/// let approval = Vote::approve("claude-sonnet-4.5", "The plan is sound and follows best practices.");
/// assert!(approval.approved);
///
/// let rejection = Vote::reject("gpt-5.2-codex", "Security concern: SQL injection risk in query.");
/// assert!(!rejection.approved);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Vote {
    /// Model identifier (e.g., "claude-sonnet-4.5", "gpt-5.2-codex")
    pub model: String,
    /// Whether this model approved
    pub approved: bool,
    /// Reasoning or feedback from this model
    pub reasoning: String,
    /// Confidence level (0.0 to 1.0, if available)
    pub confidence: Option<f64>,
}

impl Vote {
    /// Create a new vote
    pub fn new(model: impl Into<String>, approved: bool, reasoning: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            approved,
            reasoning: reasoning.into(),
            confidence: None,
        }
    }

    /// Create an approval vote
    pub fn approve(model: impl Into<String>, reasoning: impl Into<String>) -> Self {
        Self::new(model, true, reasoning)
    }

    /// Create a rejection vote
    pub fn reject(model: impl Into<String>, reasoning: impl Into<String>) -> Self {
        Self::new(model, false, reasoning)
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteResult {
    /// Whether the vote passed (majority approved)
    pub passed: bool,
    /// Number of approving votes
    pub approve_count: usize,
    /// Number of rejecting votes
    pub reject_count: usize,
    /// Total number of votes
    pub total_votes: usize,
    /// All individual votes
    pub votes: Vec<Vote>,
    /// Aggregated feedback from all votes
    pub aggregated_feedback: Option<String>,
}

impl VoteResult {
    /// Create a new VoteResult from a list of votes using majority rule
    pub fn from_votes(votes: Vec<Vote>) -> Self {
        let approve_count = votes.iter().filter(|v| v.approved).count();
        let reject_count = votes.len() - approve_count;
        let total_votes = votes.len();

        // Majority rule: more than half must approve
        let passed = approve_count > total_votes / 2;

        Self {
            passed,
            approve_count,
            reject_count,
            total_votes,
            votes,
            aggregated_feedback: None,
        }
    }

    /// Create a VoteResult with a specific rule
    pub fn from_votes_with_rule(votes: Vec<Vote>, rule: &super::rule::QuorumRule) -> Self {
        let approve_count = votes.iter().filter(|v| v.approved).count();
        let reject_count = votes.len() - approve_count;
        let total_votes = votes.len();

        let passed = rule.is_satisfied(approve_count, total_votes);

        Self {
            passed,
            approve_count,
            reject_count,
            total_votes,
            votes,
            aggregated_feedback: None,
        }
    }

    /// Add aggregated feedback
    pub fn with_feedback(mut self, feedback: impl Into<String>) -> Self {
        self.aggregated_feedback = Some(feedback.into());
        self
    }

    /// Check if the vote was unanimous
    pub fn is_unanimous(&self) -> bool {
        self.approve_count == self.total_votes || self.reject_count == self.total_votes
    }

    /// Get the approval ratio (0.0 to 1.0)
    pub fn approval_ratio(&self) -> f64 {
        if self.total_votes == 0 {
            0.0
        } else {
            self.approve_count as f64 / self.total_votes as f64
        }
    }

    /// Generate a visual vote summary (e.g., "[●●○]")
    pub fn vote_summary(&self) -> String {
        let mut summary = String::from("[");
        for vote in &self.votes {
            summary.push(if vote.approved { '●' } else { '○' });
        }
        summary.push(']');
        summary
    }

    /// Get rejecting votes only
    pub fn rejections(&self) -> impl Iterator<Item = &Vote> {
        self.votes.iter().filter(|v| !v.approved)
    }

    /// Get approving votes only
    pub fn approvals(&self) -> impl Iterator<Item = &Vote> {
        self.votes.iter().filter(|v| v.approved)
    }

    /// Aggregate rejection feedback into a single string
    pub fn aggregate_rejection_feedback(&self) -> String {
        self.rejections()
            .map(|v| format!("{}: {}", v.short_model_name(), v.reasoning))
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
        assert!(vote.approved);
        assert_eq!(vote.model, "claude-sonnet-4.5");
        assert_eq!(vote.reasoning, "Looks good!");
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
    fn test_vote_summary() {
        let votes = vec![
            Vote::approve("a", ""),
            Vote::approve("b", ""),
            Vote::reject("c", ""),
        ];
        let result = VoteResult::from_votes(votes);
        assert_eq!(result.vote_summary(), "[●●○]");
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

        assert!(feedback.contains("model: Security issue found"));
        assert!(feedback.contains("model: Missing error handling"));
    }
}
