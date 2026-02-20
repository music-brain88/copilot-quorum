//! Bounded buffer for accumulating task execution results.
//!
//! [`TaskResultBuffer`] replaces the unbounded `previous_results: String`
//! in `execute_task.rs`. It applies per-entry head+tail truncation on push
//! and a sliding-window strategy on render, keeping recent results in full
//! and summarizing older ones.

use crate::context::context_budget::ContextBudget;
use crate::util::truncate_head_tail;

/// A single buffered task result.
#[derive(Debug, Clone)]
struct TaskResultEntry {
    task_id: String,
    content: String,
    original_bytes: usize,
    is_truncated: bool,
}

/// Bounded buffer that accumulates task results and renders them
/// within a configured [`ContextBudget`].
#[derive(Debug, Clone)]
pub struct TaskResultBuffer {
    budget: ContextBudget,
    entries: Vec<TaskResultEntry>,
}

impl TaskResultBuffer {
    /// Create a new buffer with the given budget.
    pub fn new(budget: ContextBudget) -> Self {
        Self {
            budget,
            entries: Vec::new(),
        }
    }

    /// Push a task result into the buffer.
    ///
    /// The content is immediately truncated to `max_entry_bytes` using
    /// head+tail strategy if it exceeds the budget.
    pub fn push(&mut self, task_id: &str, output: &str) {
        let original_bytes = output.len();
        let max_entry = self.budget.max_entry_bytes();

        let (content, is_truncated) = if original_bytes > max_entry {
            (truncate_head_tail(output, max_entry), true)
        } else {
            (output.to_string(), false)
        };

        self.entries.push(TaskResultEntry {
            task_id: task_id.to_string(),
            content,
            original_bytes,
            is_truncated,
        });
    }

    /// Render the buffer contents as a single string, applying the
    /// sliding-window strategy.
    ///
    /// - Recent `recent_full_count` entries: rendered in full
    /// - Older entries: replaced with a one-line summary
    /// - If total exceeds `max_total_bytes`: oldest summaries are dropped
    pub fn render(&self) -> String {
        self.render_internal(&self.budget)
    }

    /// Render with an overridden budget (e.g. for task-specific ContextMode).
    pub fn render_with_budget(&self, override_budget: Option<&ContextBudget>) -> String {
        let budget = override_budget.unwrap_or(&self.budget);
        self.render_internal(budget)
    }

    /// Render the buffer and append action-retry feedback.
    ///
    /// Accepts an optional budget override so that per-task `ContextMode`
    /// budgets are respected even during action retries.
    pub fn render_with_feedback(
        &self,
        feedback: &str,
        override_budget: Option<&ContextBudget>,
    ) -> String {
        let base = self.render_with_budget(override_budget);
        if base.is_empty() {
            format!(
                "\n---\n[Previous action was rejected]\nFeedback: {}\nPlease try a different approach.",
                feedback
            )
        } else {
            format!(
                "{}\n\n---\n[Previous action was rejected]\nFeedback: {}\nPlease try a different approach.",
                base, feedback
            )
        }
    }

    /// Whether the buffer has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of entries in the buffer.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Total stored bytes (after per-entry truncation, before render-time truncation).
    pub fn stored_bytes(&self) -> usize {
        self.entries.iter().map(|e| e.content.len()).sum()
    }

    // ==================== Internal ====================

    fn render_internal(&self, budget: &ContextBudget) -> String {
        if self.entries.is_empty() {
            return String::new();
        }

        let recent_count = budget.recent_full_count();
        let max_total = budget.max_total_bytes();
        let entry_count = self.entries.len();

        // Split entries: older ones get summarized, recent ones stay full
        let full_start = entry_count.saturating_sub(recent_count);

        let mut parts: Vec<String> = Vec::new();

        // First pass: render recent (full) entries from the back to know their size
        let mut recent_parts: Vec<String> = Vec::new();
        let mut recent_bytes: usize = 0;
        for entry in &self.entries[full_start..] {
            let part = self.format_entry(entry);
            recent_bytes += part.len();
            recent_parts.push(part);
        }

        // Budget remaining for older summaries
        let summary_budget = max_total.saturating_sub(recent_bytes);

        // Second pass: render older entries as summaries, from newest to oldest,
        // dropping if they don't fit
        let mut summary_parts: Vec<String> = Vec::new();
        let mut summary_bytes: usize = 0;
        for entry in self.entries[..full_start].iter().rev() {
            let summary = self.format_summary(entry);
            if summary_bytes + summary.len() > summary_budget {
                break; // Drop older summaries that don't fit
            }
            summary_bytes += summary.len();
            summary_parts.push(summary);
        }
        summary_parts.reverse(); // Restore chronological order

        parts.extend(summary_parts);
        parts.extend(recent_parts);

        // Final safety check: if still over budget, trim from the front
        let mut result = parts.join("");
        if result.len() > max_total {
            result = truncate_head_tail(&result, max_total);
        }

        result
    }

    fn format_entry(&self, entry: &TaskResultEntry) -> String {
        let truncation_note = if entry.is_truncated {
            format!(" [truncated from {} bytes]", entry.original_bytes)
        } else {
            String::new()
        };
        format!(
            "\n---\nTask {}{}:\n{}\n",
            entry.task_id, truncation_note, entry.content
        )
    }

    fn format_summary(&self, entry: &TaskResultEntry) -> String {
        format!(
            "\n---\nTask {}: [result truncated, was {} bytes]\n",
            entry.task_id, entry.original_bytes
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextMode;

    fn small_budget() -> ContextBudget {
        ContextBudget::new(100, 500, 2)
    }

    #[test]
    fn test_empty_buffer() {
        let buf = TaskResultBuffer::new(ContextBudget::default());
        assert!(buf.is_empty());
        assert_eq!(buf.entry_count(), 0);
        assert_eq!(buf.stored_bytes(), 0);
        assert_eq!(buf.render(), "");
    }

    #[test]
    fn test_push_within_budget() {
        let mut buf = TaskResultBuffer::new(ContextBudget::default());
        buf.push("1", "Task 1 output");
        assert_eq!(buf.entry_count(), 1);
        assert!(!buf.is_empty());

        let rendered = buf.render();
        assert!(rendered.contains("Task 1"));
        assert!(rendered.contains("Task 1 output"));
    }

    #[test]
    fn test_push_truncates_large_entry() {
        let mut buf = TaskResultBuffer::new(small_budget());
        let large_output = "x".repeat(200);
        buf.push("1", &large_output);

        assert_eq!(buf.entry_count(), 1);
        // Stored content should be truncated
        assert!(buf.stored_bytes() <= 100);
        let rendered = buf.render();
        assert!(rendered.contains("truncated from 200 bytes"));
    }

    #[test]
    fn test_sliding_window_summarizes_old() {
        // recent_full_count = 2, so entry #1 should be summarized
        let mut buf = TaskResultBuffer::new(small_budget());
        buf.push("1", "first output");
        buf.push("2", "second output");
        buf.push("3", "third output");

        let rendered = buf.render();
        // Entry 1 should be summarized
        assert!(rendered.contains("Task 1: [result truncated"));
        // Entries 2 and 3 should be full
        assert!(rendered.contains("second output"));
        assert!(rendered.contains("third output"));
    }

    #[test]
    fn test_max_total_bytes_drops_old_summaries() {
        // Very tight total budget: only recent entries should survive
        let budget = ContextBudget::new(50, 150, 1);
        let mut buf = TaskResultBuffer::new(budget);
        buf.push("1", "aaaa");
        buf.push("2", "bbbb");
        buf.push("3", "cccc");
        buf.push("4", "dddd");

        let rendered = buf.render();
        // Should definitely contain the most recent entry
        assert!(rendered.contains("dddd"));
        // Total should be within budget (or close, with head+tail fallback)
        assert!(rendered.len() <= 200); // Some overhead from formatting
    }

    #[test]
    fn test_render_with_feedback() {
        let mut buf = TaskResultBuffer::new(ContextBudget::default());
        buf.push("1", "some output");

        let rendered = buf.render_with_feedback("Try using a different API", None);
        assert!(rendered.contains("some output"));
        assert!(rendered.contains("Previous action was rejected"));
        assert!(rendered.contains("Try using a different API"));
    }

    #[test]
    fn test_render_with_feedback_empty_buffer() {
        let buf = TaskResultBuffer::new(ContextBudget::default());
        let rendered = buf.render_with_feedback("feedback", None);
        assert!(rendered.contains("Previous action was rejected"));
        assert!(rendered.contains("feedback"));
    }

    #[test]
    fn test_render_with_feedback_respects_budget_override() {
        let mut buf = TaskResultBuffer::new(ContextBudget::new(1000, 5000, 3));
        buf.push("1", "first");
        buf.push("2", "second");
        buf.push("3", "third");
        buf.push("4", "fourth");

        // Override with recent_full_count=1 — entries 1-3 should be summarized
        let tight = ContextBudget::new(1000, 5000, 1);
        let rendered = buf.render_with_feedback("rejected", Some(&tight));
        assert!(rendered.contains("fourth"));
        assert!(rendered.contains("Task 3: [result truncated"));
        assert!(rendered.contains("Previous action was rejected"));
    }

    #[test]
    fn test_render_with_budget_override() {
        let mut buf = TaskResultBuffer::new(ContextBudget::new(1000, 5000, 3));
        buf.push("1", "first");
        buf.push("2", "second");
        buf.push("3", "third");
        buf.push("4", "fourth");

        // Override with recent_full_count=1 — only "fourth" should be full
        let tight = ContextBudget::new(1000, 5000, 1);
        let rendered = buf.render_with_budget(Some(&tight));
        assert!(rendered.contains("fourth"));
        assert!(rendered.contains("Task 3: [result truncated"));
    }

    #[test]
    fn test_render_with_budget_none_uses_default() {
        let mut buf = TaskResultBuffer::new(ContextBudget::new(1000, 5000, 2));
        buf.push("1", "output");

        let a = buf.render();
        let b = buf.render_with_budget(None);
        assert_eq!(a, b);
    }

    #[test]
    fn test_multibyte_safety() {
        // Ensure head+tail truncation doesn't break multi-byte chars
        let budget = ContextBudget::new(50, 200, 2);
        let mut buf = TaskResultBuffer::new(budget);
        let japanese = "テスト結果: ".repeat(20); // ~180 bytes
        buf.push("1", &japanese);

        // Should not panic and should be valid UTF-8
        let rendered = buf.render();
        assert!(!rendered.is_empty());
    }

    #[test]
    fn test_stored_bytes() {
        let mut buf = TaskResultBuffer::new(ContextBudget::default());
        buf.push("1", "hello"); // 5 bytes
        buf.push("2", "world"); // 5 bytes
        assert_eq!(buf.stored_bytes(), 10);
    }

    #[test]
    fn test_context_mode_budget_integration() {
        // Fresh mode should have more generous limits than Full
        let fresh = ContextBudget::for_context_mode(ContextMode::Fresh);
        let full = ContextBudget::for_context_mode(ContextMode::Full);
        assert!(fresh.max_total_bytes() > full.max_total_bytes());
        assert!(fresh.max_entry_bytes() > full.max_entry_bytes());
    }
}
