//! Context projection mode for task-level context control.
//!
//! Controls how much project context is passed to task executors (sub-agents)
//! and interaction instances. This is a **cross-cutting concept** used by both
//! the agent system (task-level context) and the interaction system (session-level
//! context isolation).
//!
//! The planning LLM acts as a "context compiler", projecting only the relevant
//! context for each task or interaction.
//!
//! # Vim Analogy
//!
//! Think of context modes like Vim buffer commands:
//! - **Full** = `:split` — share the same buffer (full project context)
//! - **Projected** = `:edit` — open a specific file (focused context brief)
//! - **Fresh** = `:enew` — start with an empty buffer (no inherited context)
//!
//! # Examples
//!
//! ```
//! use quorum_domain::context::ContextMode;
//!
//! let mode: ContextMode = "projected".parse().unwrap();
//! assert_eq!(mode, ContextMode::Projected);
//! assert_eq!(mode.as_str(), "projected");
//!
//! // Backward compatibility: "none" parses to Fresh
//! let fresh: ContextMode = "none".parse().unwrap();
//! assert_eq!(fresh, ContextMode::Fresh);
//! ```

use serde::{Deserialize, Serialize};

/// Controls how much project context a task executor or interaction receives.
///
/// Set per-task in the plan's `context_mode` field, or per-interaction to
/// control context isolation.
///
/// # Variants
///
/// | Mode | Context Passed | Use Case |
/// |------|---------------|----------|
/// | `Full` | All `AgentContext` | General tasks needing full project awareness |
/// | `Projected` | Only `context_brief` | Code reviews, design analysis, convention checks |
/// | `Fresh` | Nothing | Simple tool execution, isolated interactions |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextMode {
    /// Pass all gathered project context (`:split` — shared buffer).
    #[serde(alias = "shared")]
    Full,
    /// Pass only the task's `context_brief` — a focused summary written
    /// by the planner for this specific task (`:edit` — specific file).
    Projected,
    /// Pass no project context. Suitable for simple, self-contained tool
    /// calls or isolated interactions (`:enew` — empty buffer).
    ///
    /// Previously named `None` — accepts `"none"` for backward compatibility.
    #[serde(alias = "none")]
    Fresh,
}

impl ContextMode {
    /// Returns the string representation of this mode.
    pub fn as_str(&self) -> &str {
        match self {
            ContextMode::Full => "full",
            ContextMode::Projected => "projected",
            ContextMode::Fresh => "fresh",
        }
    }
}

impl std::str::FromStr for ContextMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "full" | "shared" => Ok(ContextMode::Full),
            "projected" => Ok(ContextMode::Projected),
            "fresh" | "none" => Ok(ContextMode::Fresh),
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
        assert_eq!(ContextMode::Fresh.as_str(), "fresh");
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", ContextMode::Full), "full");
        assert_eq!(format!("{}", ContextMode::Projected), "projected");
        assert_eq!(format!("{}", ContextMode::Fresh), "fresh");
    }

    #[test]
    fn test_from_str() {
        assert_eq!("full".parse::<ContextMode>().unwrap(), ContextMode::Full);
        assert_eq!(
            "projected".parse::<ContextMode>().unwrap(),
            ContextMode::Projected
        );
        assert_eq!("fresh".parse::<ContextMode>().unwrap(), ContextMode::Fresh);
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
    fn test_from_str_backward_compat() {
        // "none" → Fresh (backward compatibility)
        assert_eq!("none".parse::<ContextMode>().unwrap(), ContextMode::Fresh);
        assert_eq!("None".parse::<ContextMode>().unwrap(), ContextMode::Fresh);
        // "shared" → Full (alias)
        assert_eq!("shared".parse::<ContextMode>().unwrap(), ContextMode::Full);
    }

    #[test]
    fn test_serde_roundtrip() {
        for mode in [
            ContextMode::Full,
            ContextMode::Projected,
            ContextMode::Fresh,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let deserialized: ContextMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, deserialized);
        }
    }

    #[test]
    fn test_serde_snake_case() {
        assert_eq!(
            serde_json::to_string(&ContextMode::Full).unwrap(),
            "\"full\""
        );
        assert_eq!(
            serde_json::to_string(&ContextMode::Projected).unwrap(),
            "\"projected\""
        );
        assert_eq!(
            serde_json::to_string(&ContextMode::Fresh).unwrap(),
            "\"fresh\""
        );
    }

    #[test]
    fn test_serde_backward_compat_deserialization() {
        // "none" should deserialize to Fresh
        let fresh: ContextMode = serde_json::from_str("\"none\"").unwrap();
        assert_eq!(fresh, ContextMode::Fresh);
        // "shared" should deserialize to Full
        let full: ContextMode = serde_json::from_str("\"shared\"").unwrap();
        assert_eq!(full, ContextMode::Full);
    }

    #[test]
    fn test_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ContextMode::Full);
        set.insert(ContextMode::Projected);
        set.insert(ContextMode::Fresh);
        assert_eq!(set.len(), 3);
        // Duplicate should not increase size
        set.insert(ContextMode::Full);
        assert_eq!(set.len(), 3);
    }
}
