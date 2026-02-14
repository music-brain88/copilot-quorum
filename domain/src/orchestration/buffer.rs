//! Buffer type and context mode for buffer-level orchestration.
//!
//! [`BufferType`] classifies execution contexts (Agent, Ask, Discuss) as
//! recursively nestable units. [`BufferContextMode`] controls whether a
//! child buffer inherits the parent's conversation history.
//!
//! # Design (Issue #127)
//!
//! Agent, Ask, and Discuss are not independent session types but
//! "recursively nestable execution contexts":
//!
//! > Ask("What's the bug?") → Agent(investigation) → Discuss(design) → Agent(PoC)
//!
//! # Naming
//!
//! [`BufferContextMode`] is deliberately distinct from
//! [`ContextMode`](crate::agent::context_mode::ContextMode) (task-level
//! context projection: Full/Projected/None). This type controls
//! **buffer-level conversation history inheritance**, not per-task
//! project context.

use serde::{Deserialize, Serialize};

/// Classifies an execution context (buffer).
///
/// Each buffer type implies different config requirements and capabilities:
///
/// | Type | SessionMode | ModelConfig | AgentPolicy | ExecutionParams |
/// |------|-------------|-------------|-------------|-----------------|
/// | Agent | Yes | Yes | Yes | Yes |
/// | Ask | No (Solo fixed) | Yes | No | Yes |
/// | Discuss | Yes | Yes | No | No |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BufferType {
    /// Full lifecycle work: planning → execution → review.
    /// Uses all four config types.
    Agent,
    /// Lightweight Q&A with optional read-only tools.
    /// Solo-fixed, no plan/review cycle.
    Ask,
    /// Multi-model consensus discussion.
    /// Uses SessionMode for consensus_level control.
    Discuss,
}

impl BufferType {
    /// Default [`BufferContextMode`] for this buffer type.
    ///
    /// | BufferType | Default | Rationale |
    /// |------------|---------|-----------|
    /// | Agent | Shared | Work context continuity is assumed |
    /// | Ask | Fresh | Avoid polluting main context |
    /// | Discuss | Fresh | Topic-specific, independent discussion space |
    pub fn default_context_mode(&self) -> BufferContextMode {
        match self {
            BufferType::Agent => BufferContextMode::Shared,
            BufferType::Ask => BufferContextMode::Fresh,
            BufferType::Discuss => BufferContextMode::Fresh,
        }
    }

    /// Returns the string representation.
    pub fn as_str(&self) -> &str {
        match self {
            BufferType::Agent => "agent",
            BufferType::Ask => "ask",
            BufferType::Discuss => "discuss",
        }
    }
}

impl std::str::FromStr for BufferType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "agent" => Ok(BufferType::Agent),
            "ask" => Ok(BufferType::Ask),
            "discuss" => Ok(BufferType::Discuss),
            _ => Err(format!("Invalid BufferType: {}", s)),
        }
    }
}

impl std::fmt::Display for BufferType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Controls conversation history inheritance between parent and child buffers.
///
/// This is a **buffer-level** concept, distinct from the task-level
/// [`ContextMode`](crate::agent::context_mode::ContextMode) which controls
/// project context projection per task.
///
/// | Mode | Behavior |
/// |------|----------|
/// | Shared | Child inherits parent conversation history |
/// | Fresh | Child starts with a clean slate |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BufferContextMode {
    /// Inherit parent buffer's conversation history.
    /// Default for Agent buffers (work context continuity).
    Shared,
    /// Start with no parent context.
    /// Default for Ask and Discuss buffers (independent context).
    Fresh,
}

impl BufferContextMode {
    /// Returns the string representation.
    pub fn as_str(&self) -> &str {
        match self {
            BufferContextMode::Shared => "shared",
            BufferContextMode::Fresh => "fresh",
        }
    }
}

impl Default for BufferContextMode {
    fn default() -> Self {
        BufferContextMode::Fresh
    }
}

impl std::str::FromStr for BufferContextMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "shared" => Ok(BufferContextMode::Shared),
            "fresh" => Ok(BufferContextMode::Fresh),
            _ => Err(format!("Invalid BufferContextMode: {}", s)),
        }
    }
}

impl std::fmt::Display for BufferContextMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Maximum nesting depth for buffer spawning (configurable).
pub const DEFAULT_MAX_BUFFER_DEPTH: usize = 5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_type_default_context_mode() {
        assert_eq!(
            BufferType::Agent.default_context_mode(),
            BufferContextMode::Shared
        );
        assert_eq!(
            BufferType::Ask.default_context_mode(),
            BufferContextMode::Fresh
        );
        assert_eq!(
            BufferType::Discuss.default_context_mode(),
            BufferContextMode::Fresh
        );
    }

    #[test]
    fn test_buffer_type_as_str() {
        assert_eq!(BufferType::Agent.as_str(), "agent");
        assert_eq!(BufferType::Ask.as_str(), "ask");
        assert_eq!(BufferType::Discuss.as_str(), "discuss");
    }

    #[test]
    fn test_buffer_type_display() {
        assert_eq!(format!("{}", BufferType::Agent), "agent");
        assert_eq!(format!("{}", BufferType::Ask), "ask");
        assert_eq!(format!("{}", BufferType::Discuss), "discuss");
    }

    #[test]
    fn test_buffer_type_from_str() {
        assert_eq!("agent".parse::<BufferType>().unwrap(), BufferType::Agent);
        assert_eq!("ask".parse::<BufferType>().unwrap(), BufferType::Ask);
        assert_eq!(
            "discuss".parse::<BufferType>().unwrap(),
            BufferType::Discuss
        );
        // Case insensitive
        assert_eq!("AGENT".parse::<BufferType>().unwrap(), BufferType::Agent);
        assert_eq!("Ask".parse::<BufferType>().unwrap(), BufferType::Ask);
        // Invalid
        assert!("invalid".parse::<BufferType>().is_err());
    }

    #[test]
    fn test_buffer_type_serde_roundtrip() {
        for bt in [BufferType::Agent, BufferType::Ask, BufferType::Discuss] {
            let json = serde_json::to_string(&bt).unwrap();
            let deserialized: BufferType = serde_json::from_str(&json).unwrap();
            assert_eq!(bt, deserialized);
        }
    }

    #[test]
    fn test_buffer_type_serde_snake_case() {
        assert_eq!(
            serde_json::to_string(&BufferType::Agent).unwrap(),
            "\"agent\""
        );
        assert_eq!(
            serde_json::to_string(&BufferType::Ask).unwrap(),
            "\"ask\""
        );
        assert_eq!(
            serde_json::to_string(&BufferType::Discuss).unwrap(),
            "\"discuss\""
        );
    }

    #[test]
    fn test_buffer_context_mode_as_str() {
        assert_eq!(BufferContextMode::Shared.as_str(), "shared");
        assert_eq!(BufferContextMode::Fresh.as_str(), "fresh");
    }

    #[test]
    fn test_buffer_context_mode_display() {
        assert_eq!(format!("{}", BufferContextMode::Shared), "shared");
        assert_eq!(format!("{}", BufferContextMode::Fresh), "fresh");
    }

    #[test]
    fn test_buffer_context_mode_from_str() {
        assert_eq!(
            "shared".parse::<BufferContextMode>().unwrap(),
            BufferContextMode::Shared
        );
        assert_eq!(
            "fresh".parse::<BufferContextMode>().unwrap(),
            BufferContextMode::Fresh
        );
        // Case insensitive
        assert_eq!(
            "SHARED".parse::<BufferContextMode>().unwrap(),
            BufferContextMode::Shared
        );
        // Invalid
        assert!("invalid".parse::<BufferContextMode>().is_err());
    }

    #[test]
    fn test_buffer_context_mode_serde_roundtrip() {
        for mode in [BufferContextMode::Shared, BufferContextMode::Fresh] {
            let json = serde_json::to_string(&mode).unwrap();
            let deserialized: BufferContextMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, deserialized);
        }
    }

    #[test]
    fn test_buffer_context_mode_default() {
        assert_eq!(BufferContextMode::default(), BufferContextMode::Fresh);
    }
}
