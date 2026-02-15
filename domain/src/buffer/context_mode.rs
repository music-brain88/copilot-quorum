//! Buffer-level context mode.
//!
//! Controls conversation history inheritance between parent and child buffers.
//!
//! This is a **buffer-level** concept — distinct from the task-level
//! [`ContextMode`](crate::agent::context_mode::ContextMode) which controls
//! how much project context a sub-agent receives (Full/Projected/None).
//!
//! ```
//! use quorum_domain::buffer::ContextMode;
//!
//! let mode: ContextMode = "shared".parse().unwrap();
//! assert_eq!(mode, ContextMode::Shared);
//! ```

use serde::{Deserialize, Serialize};

/// Controls conversation history inheritance between parent and child buffers.
///
/// | Mode | Behavior |
/// |------|----------|
/// | Shared | Child inherits parent conversation history |
/// | Fresh | Child starts with a clean slate |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextMode {
    /// Inherit parent buffer's conversation history.
    /// Default for Agent buffers (work context continuity).
    Shared,
    /// Start with no parent context.
    /// Default for Ask and Discuss buffers (independent context).
    Fresh,
}

impl ContextMode {
    pub fn as_str(&self) -> &str {
        match self {
            ContextMode::Shared => "shared",
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
            "shared" => Ok(ContextMode::Shared),
            "fresh" => Ok(ContextMode::Fresh),
            _ => Err(format!("Invalid buffer ContextMode: {}", s)),
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
        assert_eq!(ContextMode::Shared.as_str(), "shared");
        assert_eq!(ContextMode::Fresh.as_str(), "fresh");
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", ContextMode::Shared), "shared");
        assert_eq!(format!("{}", ContextMode::Fresh), "fresh");
    }

    #[test]
    fn test_from_str() {
        assert_eq!(
            "shared".parse::<ContextMode>().unwrap(),
            ContextMode::Shared
        );
        assert_eq!(
            "fresh".parse::<ContextMode>().unwrap(),
            ContextMode::Fresh
        );
        assert_eq!(
            "SHARED".parse::<ContextMode>().unwrap(),
            ContextMode::Shared
        );
        assert!("invalid".parse::<ContextMode>().is_err());
    }

    #[test]
    fn test_serde_roundtrip() {
        for mode in [ContextMode::Shared, ContextMode::Fresh] {
            let json = serde_json::to_string(&mode).unwrap();
            let deserialized: ContextMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, deserialized);
        }
    }

    #[test]
    fn test_default() {
        assert_eq!(ContextMode::default(), ContextMode::Fresh);
    }
}
