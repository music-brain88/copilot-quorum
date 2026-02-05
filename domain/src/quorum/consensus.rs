//! Quorum Consensus types
//!
//! This module defines the consensus round for tracking voting history.

use super::rule::QuorumRule;
use super::vote::{Vote, VoteResult};
use serde::{Deserialize, Serialize};

/// Outcome of a consensus round
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsensusOutcome {
    /// Consensus reached: approved
    Approved,
    /// Consensus reached: rejected
    Rejected,
    /// No consensus yet (e.g., tie or insufficient votes)
    Pending,
}

impl ConsensusOutcome {
    /// Check if the outcome is approved
    pub fn is_approved(&self) -> bool {
        matches!(self, ConsensusOutcome::Approved)
    }

    /// Check if the outcome is rejected
    pub fn is_rejected(&self) -> bool {
        matches!(self, ConsensusOutcome::Rejected)
    }

    /// Check if the outcome is still pending
    pub fn is_pending(&self) -> bool {
        matches!(self, ConsensusOutcome::Pending)
    }
}

impl std::fmt::Display for ConsensusOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsensusOutcome::Approved => write!(f, "Approved"),
            ConsensusOutcome::Rejected => write!(f, "Rejected"),
            ConsensusOutcome::Pending => write!(f, "Pending"),
        }
    }
}

/// A single round of Quorum consensus voting
///
/// This tracks the complete state of a voting round, including:
/// - The round number
/// - The outcome (approved/rejected)
/// - Individual votes from each model
/// - Timestamp for auditing
///
/// # Example
///
/// ```
/// use quorum_domain::quorum::{ConsensusRound, Vote, QuorumRule};
///
/// let votes = vec![
///     Vote::approve("claude-sonnet-4.5", "Plan looks good"),
///     Vote::approve("gpt-5.2-codex", "No issues found"),
///     Vote::reject("gemini-3-pro", "Missing error handling"),
/// ];
///
/// let round = ConsensusRound::new(1, votes, QuorumRule::Majority);
/// assert!(round.is_approved()); // 2/3 majority
/// assert_eq!(round.approve_count(), 2);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusRound {
    /// Round number (1-indexed)
    pub round: usize,
    /// Outcome of this round
    pub outcome: ConsensusOutcome,
    /// The rule used for this round
    pub rule: QuorumRule,
    /// Individual votes from each model
    pub votes: Vec<Vote>,
    /// Aggregated vote result
    pub result: VoteResult,
    /// Timestamp of this round (milliseconds since epoch)
    pub timestamp: u64,
    /// Optional context about what was being voted on
    pub context: Option<String>,
}

impl ConsensusRound {
    /// Create a new consensus round with default majority rule
    pub fn new(round: usize, votes: Vec<Vote>, rule: QuorumRule) -> Self {
        let result = VoteResult::from_votes_with_rule(votes.clone(), &rule);
        let outcome = if result.passed {
            ConsensusOutcome::Approved
        } else {
            ConsensusOutcome::Rejected
        };

        Self {
            round,
            outcome,
            rule,
            votes,
            result,
            timestamp: current_timestamp(),
            context: None,
        }
    }

    /// Create a consensus round from a VoteResult
    pub fn from_result(round: usize, result: VoteResult) -> Self {
        let outcome = if result.passed {
            ConsensusOutcome::Approved
        } else {
            ConsensusOutcome::Rejected
        };

        Self {
            round,
            outcome,
            rule: QuorumRule::Majority,
            votes: result.votes.clone(),
            result,
            timestamp: current_timestamp(),
            context: None,
        }
    }

    /// Add context about what was being voted on
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Check if this round was approved
    pub fn is_approved(&self) -> bool {
        self.outcome.is_approved()
    }

    /// Check if this round was rejected
    pub fn is_rejected(&self) -> bool {
        self.outcome.is_rejected()
    }

    /// Number of approving votes
    pub fn approve_count(&self) -> usize {
        self.result.approve_count
    }

    /// Number of rejecting votes
    pub fn reject_count(&self) -> usize {
        self.result.reject_count
    }

    /// Whether this was a unanimous decision
    pub fn is_unanimous(&self) -> bool {
        self.result.is_unanimous()
    }

    /// Get the visual vote summary (e.g., "[●●○]")
    pub fn vote_summary(&self) -> String {
        self.result.vote_summary()
    }

    /// Get aggregated rejection feedback
    pub fn rejection_feedback(&self) -> String {
        self.result.aggregate_rejection_feedback()
    }
}

/// Get current timestamp in milliseconds
fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ============================================================================
// Backward Compatibility
// ============================================================================

/// Alias for backward compatibility with agent::entities::ReviewRound
#[deprecated(since = "0.6.0", note = "Use ConsensusRound instead")]
pub type ReviewRound = ConsensusRound;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consensus_round_approved() {
        let votes = vec![
            Vote::approve("model-a", "Good"),
            Vote::approve("model-b", "Good"),
            Vote::reject("model-c", "Bad"),
        ];

        let round = ConsensusRound::new(1, votes, QuorumRule::Majority);

        assert!(round.is_approved());
        assert_eq!(round.approve_count(), 2);
        assert_eq!(round.reject_count(), 1);
        assert!(!round.is_unanimous());
    }

    #[test]
    fn test_consensus_round_rejected() {
        let votes = vec![
            Vote::reject("model-a", "Issue 1"),
            Vote::reject("model-b", "Issue 2"),
            Vote::approve("model-c", "Looks OK"),
        ];

        let round = ConsensusRound::new(1, votes, QuorumRule::Majority);

        assert!(round.is_rejected());
        assert_eq!(round.approve_count(), 1);
        assert_eq!(round.reject_count(), 2);
    }

    #[test]
    fn test_consensus_round_unanimous() {
        let votes = vec![
            Vote::approve("model-a", "Perfect"),
            Vote::approve("model-b", "Great"),
        ];

        let round = ConsensusRound::new(1, votes, QuorumRule::Majority);

        assert!(round.is_approved());
        assert!(round.is_unanimous());
    }

    #[test]
    fn test_consensus_round_with_context() {
        let votes = vec![Vote::approve("model-a", "OK")];
        let round = ConsensusRound::new(1, votes, QuorumRule::Majority)
            .with_context("Plan review for task: Add authentication");

        assert!(round.context.is_some());
        assert!(round.context.unwrap().contains("authentication"));
    }

    #[test]
    fn test_vote_summary_display() {
        let votes = vec![
            Vote::approve("a", ""),
            Vote::approve("b", ""),
            Vote::reject("c", ""),
        ];
        let round = ConsensusRound::new(1, votes, QuorumRule::Majority);

        assert_eq!(round.vote_summary(), "[●●○]");
    }

    #[test]
    fn test_consensus_outcome_display() {
        assert_eq!(ConsensusOutcome::Approved.to_string(), "Approved");
        assert_eq!(ConsensusOutcome::Rejected.to_string(), "Rejected");
        assert_eq!(ConsensusOutcome::Pending.to_string(), "Pending");
    }

    #[test]
    fn test_from_result() {
        let votes = vec![
            Vote::approve("model-a", "Yes"),
            Vote::approve("model-b", "Yes"),
        ];
        let result = VoteResult::from_votes(votes);
        let round = ConsensusRound::from_result(2, result);

        assert_eq!(round.round, 2);
        assert!(round.is_approved());
    }
}
