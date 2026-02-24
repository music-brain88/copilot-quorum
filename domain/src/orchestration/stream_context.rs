//! Stream context — identifies which orchestration phase is streaming.
//!
//! Used by [`StreamObserver`](quorum_application::ports::llm_gateway::StreamObserver)
//! and progress callbacks to distinguish streaming contexts, enabling the
//! presentation layer to route per-model chunks to the correct UI surface.

/// Identifies the orchestration phase producing a model stream.
///
/// The TUI uses this to decide which pane/renderer receives stream chunks.
/// Future Lua plugins can filter/transform streams based on this context.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StreamContext {
    /// Ensemble mode: independent plan generation by multiple models.
    EnsemblePlanning,
    /// Quorum Discussion Phase 1: initial parallel queries.
    QuorumInitial,
    /// Quorum Discussion Phase 2: peer review.
    QuorumReview,
    /// Quorum Discussion Phase 3: moderator synthesis.
    QuorumSynthesis,
}

impl StreamContext {
    /// Human-readable label for display purposes.
    pub fn label(&self) -> &'static str {
        match self {
            Self::EnsemblePlanning => "Ensemble Planning",
            Self::QuorumInitial => "Initial Query",
            Self::QuorumReview => "Peer Review",
            Self::QuorumSynthesis => "Synthesis",
        }
    }
}

impl std::fmt::Display for StreamContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_context_display() {
        assert_eq!(
            StreamContext::EnsemblePlanning.to_string(),
            "Ensemble Planning"
        );
        assert_eq!(StreamContext::QuorumInitial.to_string(), "Initial Query");
        assert_eq!(StreamContext::QuorumReview.to_string(), "Peer Review");
        assert_eq!(StreamContext::QuorumSynthesis.to_string(), "Synthesis");
    }

    #[test]
    fn test_stream_context_equality() {
        assert_eq!(
            StreamContext::EnsemblePlanning,
            StreamContext::EnsemblePlanning
        );
        assert_ne!(StreamContext::QuorumInitial, StreamContext::QuorumReview);
    }
}
