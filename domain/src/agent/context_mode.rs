//! Context projection mode for task-level context control.
//!
//! Controls how much project context is passed to task executors (sub-agents).
//! This allows the planning LLM to act as a "context compiler", projecting
//! only the relevant context for each task.
//!
//! # Motivation
//!
//! Sub-agents inherit the full `AgentContext` by default, but often need only
//! a focused subset. For example, a code-review task benefits from knowing
//! specific conventions rather than the entire project structure.
//!
//! # Examples
//!
//! ```
//! use quorum_domain::agent::context_mode::ContextMode;
//!
//! let mode: ContextMode = "projected".parse().unwrap();
//! assert_eq!(mode, ContextMode::Projected);
//! assert_eq!(mode.as_str(), "projected");
//! ```

use serde::{Deserialize, Serialize};

/// Controls how much project context a task executor receives.
///
/// Set per-task in the plan's `context_mode` field. When omitted, defaults
/// to [`ContextMode::Full`] behavior (backward compatible).
///
/// # Variants
///
/// | Mode | Context Passed | Use Case |
/// |------|---------------|----------|
/// | `Full` | All `AgentContext` | General tasks needing full project awareness |
/// | `Projected` | Only `context_brief` | Code reviews, design analysis, convention checks |
/// | `None` | Nothing | Simple tool execution (file reads, searches) |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextMode {
    /// Pass all gathered project context (default behavior).
    Full,
    /// Pass only the task's `context_brief` — a focused summary written
    /// by the planner for this specific task.
    Projected,
    /// Pass no project context. Suitable for simple, self-contained tool calls.
    None,
}

impl ContextMode {
    /// Returns the string representation of this mode.
    pub fn as_str(&self) -> &str {
        match self {
            ContextMode::Full => "full",
            ContextMode::Projected => "projected",
            ContextMode::None => "none",
        }
    }
}

impl std::str::FromStr for ContextMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "full" => Ok(ContextMode::Full),
            "projected" => Ok(ContextMode::Projected),
            "none" => Ok(ContextMode::None),
            _ => Err(format!("Invalid ContextMode: {}", s)),
        }
    }
}

impl std::fmt::Display for ContextMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_as_str() {
        assert_eq!(ContextMode::Full.as_str(), "full");
        assert_eq!(ContextMode::Projected.as_str(), "projected");
        assert_eq!(ContextMode::None.as_str(), "none");
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", ContextMode::Full), "full");
        assert_eq!(format!("{}", ContextMode::Projected), "projected");
        assert_eq!(format!("{}", ContextMode::None), "none");
    }

    #[test]
    fn test_from_str() {
        assert_eq!("full".parse::<ContextMode>().unwrap(), ContextMode::Full);
        assert_eq!(
            "projected".parse::<ContextMode>().unwrap(),
            ContextMode::Projected
        );
        assert_eq!("none".parse::<ContextMode>().unwrap(), ContextMode::None);
        // Case insensitive
        assert_eq!("FULL".parse::<ContextMode>().unwrap(), ContextMode::Full);
        assert_eq!(
            "Projected".parse::<ContextMode>().unwrap(),
            ContextMode::Projected
        );
        // Invalid
        assert!("invalid".parse::<ContextMode>().is_err());
    }

    #[test]
    fn test_serde_roundtrip() {
        for mode in [ContextMode::Full, ContextMode::Projected, ContextMode::None] {
            let json = serde_json::to_string(&mode).unwrap();
            let deserialized: ContextMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, deserialized);
        }
    }

    #[test]
    fn test_serde_snake_case() {
        assert_eq!(serde_json::to_string(&ContextMode::Full).unwrap(), "\"full\"");
        assert_eq!(
            serde_json::to_string(&ContextMode::Projected).unwrap(),
            "\"projected\""
        );
        assert_eq!(serde_json::to_string(&ContextMode::None).unwrap(), "\"none\"");
    }
}
