//! Coarse-grained execution status for supervisor reporting.
//!
//! Generic across "self" (this quorum instance reporting its own state) and
//! future worker/child-agent tracking — not tied to any specific reporting
//! backend (herdr, OSC title, ...). See Issue #309 / RFC Discussion #313.

/// Working / Blocked / Idle, each optionally carrying a short human-readable
/// detail string (e.g. "HiL: プラン承認待ち") for display in a supervisor UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    /// Actively executing (planning, running tools, quorum review, ...).
    Working(Option<String>),
    /// Waiting on a human decision (HiL).
    Blocked(Option<String>),
    /// No work in flight.
    Idle,
}

impl AgentStatus {
    /// A [`Working`](Self::Working) status with a detail string.
    pub fn working(detail: impl Into<String>) -> Self {
        Self::Working(Some(detail.into()))
    }

    /// A [`Blocked`](Self::Blocked) status with a detail string.
    pub fn blocked(detail: impl Into<String>) -> Self {
        Self::Blocked(Some(detail.into()))
    }

    /// Wire-format label matching herdr's `idle|working|blocked|unknown` state enum.
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentStatus::Working(_) => "working",
            AgentStatus::Blocked(_) => "blocked",
            AgentStatus::Idle => "idle",
        }
    }

    /// The attached detail string, if any.
    pub fn detail(&self) -> Option<&str> {
        match self {
            AgentStatus::Working(d) | AgentStatus::Blocked(d) => d.as_deref(),
            AgentStatus::Idle => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_as_str() {
        assert_eq!(AgentStatus::Working(None).as_str(), "working");
        assert_eq!(AgentStatus::Blocked(None).as_str(), "blocked");
        assert_eq!(AgentStatus::Idle.as_str(), "idle");
    }

    #[test]
    fn test_detail() {
        assert_eq!(AgentStatus::working("planning").detail(), Some("planning"));
        assert_eq!(AgentStatus::blocked("HiL").detail(), Some("HiL"));
        assert_eq!(AgentStatus::Working(None).detail(), None);
        assert_eq!(AgentStatus::Idle.detail(), None);
    }

    #[test]
    fn test_equality_considers_detail() {
        assert_ne!(AgentStatus::Working(None), AgentStatus::working("x"));
        assert_eq!(AgentStatus::working("x"), AgentStatus::working("x"));
        assert_ne!(AgentStatus::working("x"), AgentStatus::working("y"));
    }
}
