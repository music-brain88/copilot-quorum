//! Context budget for controlling task result memory usage.
//!
//! [`ContextBudget`] limits how much of the `previous_results` buffer is
//! passed to subsequent task executors, preventing unbounded context growth
//! that can cause LLMs to lose track of the original task.
//!
//! # Problem
//!
//! Without budgeting, 6+ tasks can accumulate >100KB of tool output
//! (e.g. full `cargo test` output), pushing the original issue context
//! out of the LLM's effective attention window.
//!
//! # Design
//!
//! Inspired by competing tools:
//! - **Codex CLI**: 10KiB/256-line hard limit (head+tail)
//! - **OpenCode**: 2-stage (rule-based pruning → LLM compaction)
//! - **Claude Code**: `clear_tool_uses` for 84% reduction

use crate::context::ContextMode;
use serde::{Deserialize, Serialize};

/// Budget controlling how much task result context is retained.
///
/// Three knobs:
/// - `max_entry_bytes`: Maximum bytes for a single task result (head+tail truncated)
/// - `max_total_bytes`: Maximum bytes for the entire `previous_results` buffer
/// - `recent_full_count`: How many recent task results to keep in full
///   (older entries are replaced with a one-line summary)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextBudget {
    max_entry_bytes: usize,
    max_total_bytes: usize,
    recent_full_count: usize,
}

impl ContextBudget {
    /// Create a new budget with explicit values.
    pub fn new(max_entry_bytes: usize, max_total_bytes: usize, recent_full_count: usize) -> Self {
        Self {
            max_entry_bytes,
            max_total_bytes,
            recent_full_count,
        }
    }

    /// Budget preset tuned for a given [`ContextMode`].
    ///
    /// - **Full**: Standard budget (project context takes space)
    /// - **Projected**: Slightly tighter (projected context is smaller)
    /// - **Fresh**: Generous (no project context, so more room for results)
    pub fn for_context_mode(mode: ContextMode) -> Self {
        match mode {
            ContextMode::Full => Self {
                max_entry_bytes: 20_000,
                max_total_bytes: 60_000,
                recent_full_count: 3,
            },
            ContextMode::Projected => Self {
                max_entry_bytes: 15_000,
                max_total_bytes: 50_000,
                recent_full_count: 3,
            },
            ContextMode::Fresh => Self {
                max_entry_bytes: 25_000,
                max_total_bytes: 80_000,
                recent_full_count: 4,
            },
        }
    }

    /// Generous preset: larger limits for long-running sessions.
    pub fn generous() -> Self {
        Self {
            max_entry_bytes: 40_000,
            max_total_bytes: 120_000,
            recent_full_count: 5,
        }
    }

    /// Strict preset: tight limits for cost-sensitive usage.
    pub fn strict() -> Self {
        Self {
            max_entry_bytes: 10_000,
            max_total_bytes: 30_000,
            recent_full_count: 2,
        }
    }

    /// Unlimited preset: no truncation (backward compatible behavior).
    pub fn unlimited() -> Self {
        Self {
            max_entry_bytes: usize::MAX,
            max_total_bytes: usize::MAX,
            recent_full_count: usize::MAX,
        }
    }

    // ==================== Accessors ====================

    pub fn max_entry_bytes(&self) -> usize {
        self.max_entry_bytes
    }

    pub fn max_total_bytes(&self) -> usize {
        self.max_total_bytes
    }

    pub fn recent_full_count(&self) -> usize {
        self.recent_full_count
    }

    // ==================== Builder Methods ====================

    pub fn with_max_entry_bytes(mut self, bytes: usize) -> Self {
        self.max_entry_bytes = bytes;
        self
    }

    pub fn with_max_total_bytes(mut self, bytes: usize) -> Self {
        self.max_total_bytes = bytes;
        self
    }

    pub fn with_recent_full_count(mut self, count: usize) -> Self {
        self.recent_full_count = count;
        self
    }

    // ==================== Validation ====================

    /// Validate this budget, returning a list of issues.
    ///
    /// Rules:
    /// - `max_total_bytes >= max_entry_bytes`
    /// - `recent_full_count >= 1`
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.max_total_bytes < self.max_entry_bytes {
            issues.push(format!(
                "context_budget: max_total_bytes ({}) must be >= max_entry_bytes ({})",
                self.max_total_bytes, self.max_entry_bytes
            ));
        }
        if self.recent_full_count < 1 {
            issues.push("context_budget: recent_full_count must be >= 1".to_string());
        }
        issues
    }
}

impl Default for ContextBudget {
    /// Default: 20KB/entry, 60KB total, 3 recent entries kept in full.
    fn default() -> Self {
        Self::for_context_mode(ContextMode::Full)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_values() {
        let budget = ContextBudget::default();
        assert_eq!(budget.max_entry_bytes(), 20_000);
        assert_eq!(budget.max_total_bytes(), 60_000);
        assert_eq!(budget.recent_full_count(), 3);
    }

    #[test]
    fn test_for_context_mode_full() {
        let budget = ContextBudget::for_context_mode(ContextMode::Full);
        assert_eq!(budget.max_entry_bytes(), 20_000);
        assert_eq!(budget.max_total_bytes(), 60_000);
        assert_eq!(budget.recent_full_count(), 3);
    }

    #[test]
    fn test_for_context_mode_projected() {
        let budget = ContextBudget::for_context_mode(ContextMode::Projected);
        assert_eq!(budget.max_entry_bytes(), 15_000);
        assert_eq!(budget.max_total_bytes(), 50_000);
    }

    #[test]
    fn test_for_context_mode_fresh() {
        let budget = ContextBudget::for_context_mode(ContextMode::Fresh);
        assert_eq!(budget.max_entry_bytes(), 25_000);
        assert_eq!(budget.max_total_bytes(), 80_000);
        assert_eq!(budget.recent_full_count(), 4);
    }

    #[test]
    fn test_presets() {
        let generous = ContextBudget::generous();
        assert_eq!(generous.max_entry_bytes(), 40_000);

        let strict = ContextBudget::strict();
        assert_eq!(strict.max_entry_bytes(), 10_000);

        let unlimited = ContextBudget::unlimited();
        assert_eq!(unlimited.max_entry_bytes(), usize::MAX);
    }

    #[test]
    fn test_builder() {
        let budget = ContextBudget::default()
            .with_max_entry_bytes(5_000)
            .with_max_total_bytes(15_000)
            .with_recent_full_count(2);
        assert_eq!(budget.max_entry_bytes(), 5_000);
        assert_eq!(budget.max_total_bytes(), 15_000);
        assert_eq!(budget.recent_full_count(), 2);
    }

    #[test]
    fn test_validate_ok() {
        let budget = ContextBudget::default();
        assert!(budget.validate().is_empty());
    }

    #[test]
    fn test_validate_total_less_than_entry() {
        let budget = ContextBudget::new(50_000, 10_000, 3);
        let issues = budget.validate();
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("max_total_bytes"));
    }

    #[test]
    fn test_validate_zero_recent() {
        let budget = ContextBudget::new(20_000, 60_000, 0);
        let issues = budget.validate();
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("recent_full_count"));
    }

    #[test]
    fn test_serde_roundtrip() {
        let budget = ContextBudget::new(10_000, 30_000, 2);
        let json = serde_json::to_string(&budget).unwrap();
        let deserialized: ContextBudget = serde_json::from_str(&json).unwrap();
        assert_eq!(budget, deserialized);
    }

    #[test]
    fn test_equality() {
        let a = ContextBudget::default();
        let b = ContextBudget::for_context_mode(ContextMode::Full);
        assert_eq!(a, b);

        let c = ContextBudget::strict();
        assert_ne!(a, c);
    }
}
