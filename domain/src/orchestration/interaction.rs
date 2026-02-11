//! Interaction type and context mode definitions.
//!
//! These axes complement [`ConsensusLevel`](super::mode::ConsensusLevel) and
//! [`PhaseScope`](super::scope::PhaseScope) to express **how** the user
//! interacts with the system:
//!
//! - [`InteractionType`] — Ask (Q&A) vs Discuss (multi-model discussion)
//! - [`ContextMode`] — Shared (conversation context) vs Fresh (clean slate)
//!
//! # Orchestration applicability
//!
//! Orchestration strategy only applies when multiple models are involved:
//!
//! | ConsensusLevel | InteractionType | Orchestration applies? |
//! |----------------|----------------|----------------------|
//! | Solo           | Ask            | No                   |
//! | Solo           | Discuss        | Discussion only      |
//! | Ensemble       | Ask            | Overall flow only    |
//! | Ensemble       | Discuss        | Always               |

use serde::{Deserialize, Serialize};
use std::fmt;

/// Interaction type — how the user engages with the system.
///
/// # Ask vs Discuss
///
/// - **Ask** (default): Simple question → answer flow. Lightweight, single-turn.
/// - **Discuss**: Multi-model discussion flow. Models exchange perspectives
///   and reach consensus through orchestration.
///
/// # REPL Commands
///
/// ```text
/// /ask      Switch to Ask mode
/// /discuss  Switch to Discuss mode
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum InteractionType {
    /// Question → answer (lightweight, single model response)
    #[default]
    Ask,
    /// Discussion → consensus (multi-model dialogue)
    Discuss,
}

impl InteractionType {
    /// Whether this interaction type involves multi-model discussion.
    pub fn is_discussion(&self) -> bool {
        matches!(self, InteractionType::Discuss)
    }

    /// Get a human-readable description of this type.
    pub fn description(&self) -> &'static str {
        match self {
            InteractionType::Ask => "Ask: Question → answer (lightweight)",
            InteractionType::Discuss => "Discuss: Multi-model discussion → consensus",
        }
    }

    /// Get a short label for prompt display.
    pub fn short_label(&self) -> &'static str {
        match self {
            InteractionType::Ask => "ask",
            InteractionType::Discuss => "discuss",
        }
    }
}

impl fmt::Display for InteractionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.short_label())
    }
}

impl std::str::FromStr for InteractionType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ask" | "a" => Ok(InteractionType::Ask),
            "discuss" | "disc" | "d" => Ok(InteractionType::Discuss),
            _ => Err(format!("Invalid InteractionType: {}", s)),
        }
    }
}

/// Context mode — whether conversation context is shared with the LLM.
///
/// # Shared vs Fresh
///
/// - **Shared** (default): Current conversation context is included.
///   Good for follow-up questions and iterative development.
/// - **Fresh**: No previous context. Good for independent questions
///   or when context might confuse the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ContextMode {
    /// Share current conversation context (default)
    #[default]
    Shared,
    /// Start with a clean slate (no previous context)
    Fresh,
}

impl ContextMode {
    /// Whether context is shared with the model.
    pub fn is_shared(&self) -> bool {
        matches!(self, ContextMode::Shared)
    }

    /// Get a human-readable description of this mode.
    pub fn description(&self) -> &'static str {
        match self {
            ContextMode::Shared => "Shared: Conversation context included",
            ContextMode::Fresh => "Fresh: Clean slate, no previous context",
        }
    }
}

impl fmt::Display for ContextMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContextMode::Shared => write!(f, "shared"),
            ContextMode::Fresh => write!(f, "fresh"),
        }
    }
}

impl std::str::FromStr for ContextMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "shared" | "s" => Ok(ContextMode::Shared),
            "fresh" | "f" => Ok(ContextMode::Fresh),
            _ => Err(format!("Invalid ContextMode: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== InteractionType ====================

    #[test]
    fn test_interaction_type_default() {
        assert_eq!(InteractionType::default(), InteractionType::Ask);
    }

    #[test]
    fn test_interaction_type_display() {
        assert_eq!(format!("{}", InteractionType::Ask), "ask");
        assert_eq!(format!("{}", InteractionType::Discuss), "discuss");
    }

    #[test]
    fn test_interaction_type_from_str() {
        assert_eq!(
            "ask".parse::<InteractionType>().ok(),
            Some(InteractionType::Ask)
        );
        assert_eq!(
            "a".parse::<InteractionType>().ok(),
            Some(InteractionType::Ask)
        );
        assert_eq!(
            "discuss".parse::<InteractionType>().ok(),
            Some(InteractionType::Discuss)
        );
        assert_eq!(
            "disc".parse::<InteractionType>().ok(),
            Some(InteractionType::Discuss)
        );
        assert_eq!(
            "d".parse::<InteractionType>().ok(),
            Some(InteractionType::Discuss)
        );
        assert!("unknown".parse::<InteractionType>().is_err());
    }

    #[test]
    fn test_interaction_type_case_insensitive() {
        assert_eq!(
            "ASK".parse::<InteractionType>().ok(),
            Some(InteractionType::Ask)
        );
        assert_eq!(
            "Discuss".parse::<InteractionType>().ok(),
            Some(InteractionType::Discuss)
        );
    }

    #[test]
    fn test_interaction_type_is_discussion() {
        assert!(!InteractionType::Ask.is_discussion());
        assert!(InteractionType::Discuss.is_discussion());
    }

    // ==================== ContextMode ====================

    #[test]
    fn test_context_mode_default() {
        assert_eq!(ContextMode::default(), ContextMode::Shared);
    }

    #[test]
    fn test_context_mode_display() {
        assert_eq!(format!("{}", ContextMode::Shared), "shared");
        assert_eq!(format!("{}", ContextMode::Fresh), "fresh");
    }

    #[test]
    fn test_context_mode_from_str() {
        assert_eq!(
            "shared".parse::<ContextMode>().ok(),
            Some(ContextMode::Shared)
        );
        assert_eq!(
            "s".parse::<ContextMode>().ok(),
            Some(ContextMode::Shared)
        );
        assert_eq!(
            "fresh".parse::<ContextMode>().ok(),
            Some(ContextMode::Fresh)
        );
        assert_eq!("f".parse::<ContextMode>().ok(), Some(ContextMode::Fresh));
        assert!("invalid".parse::<ContextMode>().is_err());
    }

    #[test]
    fn test_context_mode_is_shared() {
        assert!(ContextMode::Shared.is_shared());
        assert!(!ContextMode::Fresh.is_shared());
    }
}
