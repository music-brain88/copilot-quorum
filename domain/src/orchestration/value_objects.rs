//! Orchestration value objects - immutable result types

use serde::{Deserialize, Serialize};

/// Response from a single model in the initial query phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelResponse {
    /// The model that generated this response
    pub model: String,
    /// The response content
    pub content: String,
    /// Whether this response was successful
    pub success: bool,
    /// Error message if failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ModelResponse {
    pub fn success(model: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            content: content.into(),
            success: true,
            error: None,
        }
    }

    pub fn failure(model: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            content: String::new(),
            success: false,
            error: Some(error.into()),
        }
    }

    pub fn is_success(&self) -> bool {
        self.success
    }
}

/// Peer review of one model's response by another model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerReview {
    /// The model that performed the review
    pub reviewer: String,
    /// Anonymous identifier of the reviewed response (e.g., "Response A")
    pub reviewed_id: String,
    /// The review content
    pub content: String,
    /// Numerical score (1-10)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<u8>,
}

impl PeerReview {
    pub fn new(
        reviewer: impl Into<String>,
        reviewed_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            reviewer: reviewer.into(),
            reviewed_id: reviewed_id.into(),
            content: content.into(),
            score: None,
        }
    }

    pub fn with_score(mut self, score: u8) -> Self {
        self.score = Some(score.min(10));
        self
    }
}

/// Final synthesis result from the moderator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesisResult {
    /// The model that performed the synthesis
    pub moderator: String,
    /// The synthesized conclusion
    pub conclusion: String,
    /// Key points from all responses
    #[serde(default)]
    pub key_points: Vec<String>,
    /// Areas of consensus
    #[serde(default)]
    pub consensus: Vec<String>,
    /// Areas of disagreement
    #[serde(default)]
    pub disagreements: Vec<String>,
}

impl SynthesisResult {
    pub fn new(moderator: impl Into<String>, conclusion: impl Into<String>) -> Self {
        Self {
            moderator: moderator.into(),
            conclusion: conclusion.into(),
            key_points: Vec::new(),
            consensus: Vec::new(),
            disagreements: Vec::new(),
        }
    }

    pub fn with_key_points(mut self, points: Vec<String>) -> Self {
        self.key_points = points;
        self
    }

    pub fn with_consensus(mut self, consensus: Vec<String>) -> Self {
        self.consensus = consensus;
        self
    }

    pub fn with_disagreements(mut self, disagreements: Vec<String>) -> Self {
        self.disagreements = disagreements;
        self
    }
}

/// Complete result of a Quorum session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumResult {
    /// The original question
    pub question: String,
    /// Models that participated
    pub models: Vec<String>,
    /// Phase 1: Initial responses from each model
    pub responses: Vec<ModelResponse>,
    /// Phase 2: Peer reviews
    pub reviews: Vec<PeerReview>,
    /// Phase 3: Final synthesis
    pub synthesis: SynthesisResult,
}

impl QuorumResult {
    pub fn new(
        question: impl Into<String>,
        models: Vec<String>,
        responses: Vec<ModelResponse>,
        reviews: Vec<PeerReview>,
        synthesis: SynthesisResult,
    ) -> Self {
        Self {
            question: question.into(),
            models,
            responses,
            reviews,
            synthesis,
        }
    }

    /// Get successful responses only
    pub fn successful_responses(&self) -> impl Iterator<Item = &ModelResponse> {
        self.responses.iter().filter(|r| r.success)
    }

    /// Get failed responses only
    pub fn failed_responses(&self) -> impl Iterator<Item = &ModelResponse> {
        self.responses.iter().filter(|r| !r.success)
    }
}
