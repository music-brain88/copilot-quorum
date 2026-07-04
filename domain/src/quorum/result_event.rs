//! `quorum_result` event envelope — the shared serialization contract
//!
//! This is the single vocabulary for quorum vote results consumed outside
//! the process: the JSONL conversation log, the future headless `review`
//! subcommand stdout (#300), and Remote Control API responses (#302).
//! See RFC Discussion #304.
//!
//! Contract notes (v1):
//! - Per-vote outcome is a `verdict` (`approve` / `reject` / `abstain` /
//!   `model_error`), not a boolean. `abstain` and `model_error` are recorded
//!   for visibility but excluded from the voting denominator.
//! - `type` and `timestamp` fields are injected by the log sink and are not
//!   part of this envelope.

use super::rule::QuorumRule;
use super::vote::{Vote, VoteResult};
use serde::{Deserialize, Serialize};

/// Schema version of the `quorum_result` envelope
pub const QUORUM_RESULT_API_VERSION: u32 = 1;

/// What was being decided by the quorum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuorumTopic {
    /// Review of a generated plan before execution
    PlanReview,
    /// Review of a high-risk tool call before execution
    ActionReview,
    /// Final review of the completed work
    FinalReview,
}

impl QuorumTopic {
    /// The snake_case name used in serialized payloads
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PlanReview => "plan_review",
            Self::ActionReview => "action_review",
            Self::FinalReview => "final_review",
        }
    }
}

/// Topic-dependent identification of the review subject
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuorumTarget {
    /// Task the reviewed action belongs to (action_review)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// Tool being reviewed (action_review)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
}

impl QuorumTarget {
    /// Target for an action review
    pub fn action(task_id: impl Into<String>, tool: Option<String>) -> Self {
        Self {
            task_id: Some(task_id.into()),
            tool,
        }
    }
}

/// The `quorum_result` event payload (v1)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumResultEnvelope {
    /// Schema version of this envelope
    pub api_version: u32,
    /// What was being decided
    pub topic: QuorumTopic,
    /// Topic-dependent subject identification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<QuorumTarget>,
    /// Aggregate outcome over cast votes
    pub approved: bool,
    /// The rule used to aggregate votes
    pub rule: QuorumRule,
    /// All individual votes (including abstain / model_error)
    pub votes: Vec<Vote>,
    /// Aggregated rejection feedback, if the vote did not pass
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
}

impl QuorumResultEnvelope {
    /// Build a v1 envelope from an aggregated vote result
    pub fn new(topic: QuorumTopic, target: Option<QuorumTarget>, result: &VoteResult) -> Self {
        Self {
            api_version: QUORUM_RESULT_API_VERSION,
            topic,
            target,
            approved: result.passed,
            rule: QuorumRule::Majority,
            votes: result.votes.clone(),
            feedback: result.aggregated_feedback.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Golden test: pins the v1 JSON contract. If this test needs changing,
    // the api_version must be bumped and consumers notified.
    #[test]
    fn test_envelope_v1_json_shape() {
        let votes = vec![
            Vote::approve("claude-opus-4.5", "Safe command"),
            Vote::reject("gpt-5.3-codex", "Risky flag"),
            Vote::model_error("gemini-3.1-pro-preview", "gateway timeout"),
        ];
        let result = VoteResult::from_votes(votes);
        let envelope = QuorumResultEnvelope::new(
            QuorumTopic::ActionReview,
            Some(QuorumTarget::action("task-1", Some("run_command".into()))),
            &result,
        );

        let json = serde_json::to_value(&envelope).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "api_version": 1,
                "topic": "action_review",
                "target": {"task_id": "task-1", "tool": "run_command"},
                "approved": false,
                "rule": "majority",
                "votes": [
                    {"model": "claude-opus-4.5", "verdict": "approve",
                     "reasoning": "Safe command", "confidence": null},
                    {"model": "gpt-5.3-codex", "verdict": "reject",
                     "reasoning": "Risky flag", "confidence": null},
                    {"model": "gemini-3.1-pro-preview", "verdict": "model_error",
                     "reasoning": "gateway timeout", "confidence": null}
                ],
                "feedback": "gpt-5.3-codex: Risky flag"
            })
        );
    }

    #[test]
    fn test_envelope_omits_empty_optionals() {
        let result = VoteResult::from_votes(vec![Vote::approve("m", "ok")]);
        let envelope = QuorumResultEnvelope::new(QuorumTopic::PlanReview, None, &result);
        let json = serde_json::to_value(&envelope).unwrap();

        assert_eq!(json["topic"], "plan_review");
        assert!(json.get("target").is_none());
        assert!(json.get("feedback").is_none());
    }
}
