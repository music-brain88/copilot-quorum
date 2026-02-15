//! Unified context mode — controls how much context a new execution unit inherits.
//!
//! Inspired by Vim/Neovim's buffer context model:
//!
//! | Vim command | ContextMode | Behavior |
//! |-------------|-------------|----------|
//! | `:split`    | `Full`      | Same buffer — full context continuity |
//! | `:edit`     | `Projected` | Load specific content into the buffer |
//! | `:enew`     | `Fresh`     | Blank buffer — start from global defaults |
//!
//! This is a **cross-cutting concept** used by both the agent system (task-level
//! context projection) and the buffer system (conversation history inheritance).
//!
//! # Examples
//!
//! ```
//! use quorum_domain::context::ContextMode;
//!
//! let mode: ContextMode = "projected".parse().unwrap();
//! assert_eq!(mode, ContextMode::Projected);
//! assert_eq!(mode.as_str(), "projected");
//! ```

use serde::{Deserialize, Serialize};

/// Controls how much context a new execution unit inherits from its parent.
///
/// This enum unifies the previously separate agent-level and buffer-level
/// context modes into a single, independent concept.
///
/// # Variants
///
/// | Mode | Context Passed | Use Case |
/// |------|---------------|----------|
/// | `Full` | All parent context | Agent tasks, work continuity |
/// | `Projected` | Only a focused `context_brief` | Code reviews, targeted analysis |
/// | `Fresh` | Nothing — clean slate | Ask, Discuss, simple tool execution |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextMode {
    /// Inherit all parent context (conversation history, project context).
    ///
    /// Vim equivalent: `:split` — same buffer, full continuity.
    /// Default for Agent buffers and general tasks.
    #[serde(alias = "shared")]
    Full,
    /// Inherit only a focused `context_brief` written by the planner.
    ///
    /// Vim equivalent: `:edit file` — load specific content.
    /// For tasks that need targeted context (code reviews, design analysis).
    Projected,
    /// Start with no parent context — a clean slate.
    ///
    /// Vim equivalent: `:enew` — blank buffer from global defaults.
    /// Default for Ask and Discuss buffers, simple tool execution.
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

impl Default for ContextMode {
    fn default() -> Self {
        ContextMode::Fresh
    }
}

impl std::str::FromStr for ContextMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "full" => Ok(ContextMode::Full),
            "projected" => Ok(ContextMode::Projected),
            "fresh" => Ok(ContextMode::Fresh),
            // Backward compatibility
            "none" => Ok(ContextMode::Fresh),
            "shared" => Ok(ContextMode::Full),
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
        assert_eq!(
            "fresh".parse::<ContextMode>().unwrap(),
            ContextMode::Fresh
        );
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
        // "none" → Fresh (was agent::ContextMode::None)
        assert_eq!("none".parse::<ContextMode>().unwrap(), ContextMode::Fresh);
        // "shared" → Full (was buffer::ContextMode::Shared)
        assert_eq!("shared".parse::<ContextMode>().unwrap(), ContextMode::Full);
    }

    #[test]
    fn test_default() {
        assert_eq!(ContextMode::default(), ContextMode::Fresh);
    }

    #[test]
    fn test_serde_roundtrip() {
        for mode in [ContextMode::Full, ContextMode::Projected, ContextMode::Fresh] {
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
    fn test_serde_backward_compat_deserialize() {
        // "none" should deserialize to Fresh
        let mode: ContextMode = serde_json::from_str("\"none\"").unwrap();
        assert_eq!(mode, ContextMode::Fresh);
    }
}
