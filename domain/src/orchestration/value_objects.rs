//! Orchestration value objects - immutable result types for Quorum sessions.
//!
//! These types represent the outputs of each Quorum phase:
//! - [`ModelResponse`] - Individual model's answer from the Initial Query phase
//! - [`PeerReview`] - Review of one model's response by another
//! - [`SynthesisResult`] - Final combined answer from the moderator
//! - [`QuorumResult`] - Complete result containing all phases

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
    /// Creates a successful response from a model.
    ///
    /// # Arguments
    /// * `model` - Name or identifier of the model that generated this response
    /// * `content` - The model's answer to the question
    pub fn success(model: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            content: content.into(),
            success: true,
            error: None,
        }
    }

    /// Creates a failed response indicating the model could not answer.
    ///
    /// # Arguments
    /// * `model` - Name or identifier of the model
    /// * `error` - Description of why the model failed
    pub fn failure(model: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            content: String::new(),
            success: false,
            error: Some(error.into()),
        }
    }

    /// Returns `true` if this response was generated successfully.
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
    /// Creates a new peer review.
    ///
    /// # Arguments
    /// * `reviewer` - The model that performed the review
    /// * `reviewed_id` - Anonymous identifier of the response being reviewed (e.g., "Response A")
    /// * `content` - The review text with feedback and analysis
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

    /// Adds a numerical score to the review (capped at 10).
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
    /// Creates a new synthesis result with the final conclusion.
    ///
    /// # Arguments
    /// * `moderator` - The model that performed the synthesis
    /// * `conclusion` - The combined final answer synthesizing all responses
    pub fn new(moderator: impl Into<String>, conclusion: impl Into<String>) -> Self {
        Self {
            moderator: moderator.into(),
            conclusion: conclusion.into(),
            key_points: Vec::new(),
            consensus: Vec::new(),
            disagreements: Vec::new(),
        }
    }

    /// Adds key points extracted from all model responses.
    pub fn with_key_points(mut self, points: Vec<String>) -> Self {
        self.key_points = points;
        self
    }

    /// Adds areas where all models agreed.
    pub fn with_consensus(mut self, consensus: Vec<String>) -> Self {
        self.consensus = consensus;
        self
    }

    /// Adds areas where models had different opinions.
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
    /// Creates a complete QuorumResult from all phases.
    ///
    /// This represents the final output of a Quorum session, containing
    /// results from Initial Query, Peer Review, and Synthesis phases.
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

    /// Returns an iterator over only the successful model responses.
    pub fn successful_responses(&self) -> impl Iterator<Item = &ModelResponse> {
        self.responses.iter().filter(|r| r.success)
    }

    /// Returns an iterator over only the failed model responses.
    pub fn failed_responses(&self) -> impl Iterator<Item = &ModelResponse> {
        self.responses.iter().filter(|r| !r.success)
    }
}
