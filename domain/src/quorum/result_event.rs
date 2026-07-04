//! `quorum_result` event — the shared serialization contract
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
//! - A full `quorum_result` **record** = [`QuorumResultPayload`] + `type` +
//!   `timestamp`. The JSONL sink injects those two fields when logging;
//!   every other sink (stdout, RPC) must produce the identical shape via
//!   [`QuorumResultPayload::to_record`] so the three surfaces never drift.

use super::rule::QuorumRule;
use super::vote::{Vote, VoteResult};
use crate::orchestration::value_objects::SynthesisResult;
use serde::{Deserialize, Serialize};

/// Schema version of the `quorum_result` contract
pub const QUORUM_RESULT_API_VERSION: u32 = 1;

/// The `type` field value of a `quorum_result` record
pub const QUORUM_RESULT_EVENT_TYPE: &str = "quorum_result";

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
    /// Headless review of a PR/diff (#300)
    PrReview,
}

impl QuorumTopic {
    /// The snake_case name used in serialized payloads
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PlanReview => "plan_review",
            Self::ActionReview => "action_review",
            Self::FinalReview => "final_review",
            Self::PrReview => "pr_review",
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
    /// PR number being reviewed (pr_review)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<u64>,
    /// PR title (pr_review)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl QuorumTarget {
    /// Target for an action review
    pub fn action(task_id: impl Into<String>, tool: Option<String>) -> Self {
        Self {
            task_id: Some(task_id.into()),
            tool,
            ..Default::default()
        }
    }

    /// Target for a PR/diff review (#300). Returns `None` when neither `pr`
    /// nor `title` is known (e.g. a bare `git diff | copilot-quorum review`
    /// via stdin) — the `quorum_result` contract omits `target` entirely in
    /// that case rather than serializing an empty object, so this returns
    /// `Option<Self>` instead of always producing a `QuorumTarget`.
    pub fn pr_review(pr: Option<u64>, title: Option<String>) -> Option<Self> {
        if pr.is_none() && title.is_none() {
            return None;
        }
        Some(Self {
            pr,
            title,
            ..Default::default()
        })
    }
}

/// The `quorum_result` payload (v1) — everything except `type` / `timestamp`
///
/// Sinks that don't inject those fields themselves (unlike the JSONL logger)
/// should emit [`Self::to_record`] instead of serializing this directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumResultPayload {
    /// Schema version of this payload
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
    /// Moderator's synthesized review (pr_review; additive to v1, #300)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synthesis: Option<SynthesisResult>,
}

impl QuorumResultPayload {
    /// Build a v1 payload from an aggregated vote result
    pub fn new(topic: QuorumTopic, target: Option<QuorumTarget>, result: &VoteResult) -> Self {
        Self {
            api_version: QUORUM_RESULT_API_VERSION,
            topic,
            target,
            approved: result.passed,
            rule: QuorumRule::Majority,
            votes: result.votes.clone(),
            feedback: result.aggregated_feedback.clone(),
            synthesis: None,
        }
    }

    /// Attach the moderator's synthesized review (pr_review, #300).
    pub fn with_synthesis(mut self, synthesis: SynthesisResult) -> Self {
        self.synthesis = Some(synthesis);
        self
    }

    /// Build the complete `quorum_result` record: payload + `type` + `timestamp`
    ///
    /// Produces the exact shape the JSONL sink writes, for use by other
    /// sinks (headless `review` stdout, RPC). `timestamp` is RFC 3339 UTC.
    pub fn to_record(&self, timestamp: impl Into<String>) -> serde_json::Value {
        let mut record = serde_json::to_value(self).unwrap_or_default();
        if let Some(map) = record.as_object_mut() {
            map.insert(
                "type".to_string(),
                serde_json::Value::String(QUORUM_RESULT_EVENT_TYPE.to_string()),
            );
            map.insert(
                "timestamp".to_string(),
                serde_json::Value::String(timestamp.into()),
            );
        }
        record
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Golden test: pins the v1 JSON contract. If this test needs changing,
    // the api_version must be bumped and consumers notified.
    #[test]
    fn test_payload_v1_json_shape() {
        let votes = vec![
            Vote::approve("claude-opus-4.5", "Safe command"),
            Vote::reject("gpt-5.3-codex", "Risky flag"),
            Vote::model_error("gemini-3.1-pro-preview", "gateway timeout"),
        ];
        let result = VoteResult::from_votes(votes);
        let payload = QuorumResultPayload::new(
            QuorumTopic::ActionReview,
            Some(QuorumTarget::action("task-1", Some("run_command".into()))),
            &result,
        );

        let json = serde_json::to_value(&payload).unwrap();
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
    fn test_payload_omits_empty_optionals() {
        let result = VoteResult::from_votes(vec![Vote::approve("m", "ok")]);
        let payload = QuorumResultPayload::new(QuorumTopic::PlanReview, None, &result);
        let json = serde_json::to_value(&payload).unwrap();

        assert_eq!(json["topic"], "plan_review");
        assert!(json.get("target").is_none());
        assert!(json.get("feedback").is_none());
        assert!(json.get("synthesis").is_none());
    }

    #[test]
    fn test_pr_review_topic_and_target() {
        assert_eq!(QuorumTopic::PrReview.as_str(), "pr_review");

        let target =
            QuorumTarget::pr_review(Some(123), Some("Fix the bug".into())).expect("has pr/title");
        let json = serde_json::to_value(&target).unwrap();
        assert_eq!(json, serde_json::json!({"pr": 123, "title": "Fix the bug"}));

        // A bare stdin diff has neither — no target at all, not an empty object
        assert_eq!(QuorumTarget::pr_review(None, None), None);
    }

    // Regression test: a bare `git diff | copilot-quorum review` (no --pr,
    // no title) must omit `target` from the record entirely, per the
    // `quorum_result` contract ("target: object? — omitted when absent").
    // An earlier version of the `review` CLI handler always wrapped
    // `QuorumTarget::pr_review(..)` in `Some(..)`, producing `"target": {}`
    // instead of omitting the key.
    #[test]
    fn test_payload_omits_target_when_pr_review_has_neither_pr_nor_title() {
        let result = VoteResult::from_votes(vec![Vote::approve("m", "ok")]);
        let payload = QuorumResultPayload::new(
            QuorumTopic::PrReview,
            QuorumTarget::pr_review(None, None),
            &result,
        );
        let json = serde_json::to_value(&payload).unwrap();

        assert!(json.get("target").is_none());
    }

    #[test]
    fn test_with_synthesis_populates_and_serializes() {
        let result = VoteResult::from_votes(vec![
            Vote::approve("claude-opus-4.5", "Safe"),
            Vote::reject("gpt-5.3-codex", "Missing tests"),
        ]);
        let synthesis =
            SynthesisResult::new("claude-opus-4.5", "Overall solid, but add test coverage.");
        let payload = QuorumResultPayload::new(
            QuorumTopic::PrReview,
            QuorumTarget::pr_review(Some(123), Some("Fix login".into())),
            &result,
        )
        .with_synthesis(synthesis);

        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["topic"], "pr_review");
        assert_eq!(json["target"]["pr"], 123);
        assert_eq!(json["target"]["title"], "Fix login");
        assert_eq!(json["synthesis"]["moderator"], "claude-opus-4.5");
        assert_eq!(
            json["synthesis"]["conclusion"],
            "Overall solid, but add test coverage."
        );
    }

    // Golden test: the full record = payload + type + timestamp, matching
    // what the JSONL sink writes. Other sinks must use to_record().
    #[test]
    fn test_to_record_matches_jsonl_shape() {
        let result = VoteResult::from_votes(vec![Vote::approve("m", "ok")]);
        let payload = QuorumResultPayload::new(QuorumTopic::PlanReview, None, &result);
        let record = payload.to_record("2026-07-04T10:30:00.123Z");

        assert_eq!(record["type"], "quorum_result");
        assert_eq!(record["timestamp"], "2026-07-04T10:30:00.123Z");
        assert_eq!(record["api_version"], 1);
        assert_eq!(record["topic"], "plan_review");
        assert_eq!(record["votes"][0]["verdict"], "approve");
    }
}
