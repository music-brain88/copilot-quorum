//! Buffer domain — recursively nestable execution contexts.
//!
//! A **Buffer** is the fundamental unit of interaction in copilot-quorum.
//! Agent, Ask, and Discuss are not independent session types but
//! recursively nestable execution contexts:
//!
//! > Ask("What's the bug?") → Agent(investigation) → Discuss(design) → Agent(PoC)
//!
//! # Key Types
//!
//! - [`BufferType`] — classifies the buffer (Agent, Ask, Discuss)
//! - [`ContextMode`](crate::context::ContextMode) — controls context inheritance (Full, Projected, Fresh)
//!
//! # Design (Issue #127)
//!
//! Each buffer type implies different config requirements:
//!
//! | Type | SessionMode | ModelConfig | AgentPolicy | ExecutionParams |
//! |------|-------------|-------------|-------------|-----------------|
//! | Agent | Yes | Yes | Yes | Yes |
//! | Ask | No (Solo fixed) | Yes | No | Yes |
//! | Discuss | Yes | Yes | No | No |

use crate::context::ContextMode;
use serde::{Deserialize, Serialize};

/// Classifies an execution context (buffer).
///
/// Each variant carries different config requirements and execution semantics.
/// See module-level docs for the full config necessity matrix.
///
/// Acts as the "filetype" trigger (à la Vim) that determines the default
/// [`ContextMode`] and behavioral profile for the buffer.
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
    /// Default [`ContextMode`] for this buffer type.
    ///
    /// Like Vim's `ftplugin/` mechanism, the buffer type determines default
    /// context behavior:
    ///
    /// | BufferType | Default | Vim equivalent | Rationale |
    /// |------------|---------|---------------|-----------|
    /// | Agent | Full | `:split` | Work context continuity |
    /// | Ask | Fresh | `:enew` | Independent question space |
    /// | Discuss | Fresh | `:enew` | Topic-specific discussion |
    pub fn default_context_mode(&self) -> ContextMode {
        match self {
            BufferType::Agent => ContextMode::Full,
            BufferType::Ask => ContextMode::Fresh,
            BufferType::Discuss => ContextMode::Fresh,
        }
    }

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

/// Maximum nesting depth for buffer spawning (configurable).
pub const DEFAULT_MAX_BUFFER_DEPTH: usize = 5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_context_mode() {
        assert_eq!(
            BufferType::Agent.default_context_mode(),
            ContextMode::Full
        );
        assert_eq!(BufferType::Ask.default_context_mode(), ContextMode::Fresh);
        assert_eq!(
            BufferType::Discuss.default_context_mode(),
            ContextMode::Fresh
        );
    }

    #[test]
    fn test_as_str() {
        assert_eq!(BufferType::Agent.as_str(), "agent");
        assert_eq!(BufferType::Ask.as_str(), "ask");
        assert_eq!(BufferType::Discuss.as_str(), "discuss");
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", BufferType::Agent), "agent");
        assert_eq!(format!("{}", BufferType::Ask), "ask");
        assert_eq!(format!("{}", BufferType::Discuss), "discuss");
    }

    #[test]
    fn test_from_str() {
        assert_eq!("agent".parse::<BufferType>().unwrap(), BufferType::Agent);
        assert_eq!("ask".parse::<BufferType>().unwrap(), BufferType::Ask);
        assert_eq!(
            "discuss".parse::<BufferType>().unwrap(),
            BufferType::Discuss
        );
        assert_eq!("AGENT".parse::<BufferType>().unwrap(), BufferType::Agent);
        assert!("invalid".parse::<BufferType>().is_err());
    }

    #[test]
    fn test_serde_roundtrip() {
        for bt in [BufferType::Agent, BufferType::Ask, BufferType::Discuss] {
            let json = serde_json::to_string(&bt).unwrap();
            let deserialized: BufferType = serde_json::from_str(&json).unwrap();
            assert_eq!(bt, deserialized);
        }
    }
}
