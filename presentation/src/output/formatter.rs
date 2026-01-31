//! Output formatter trait

use quorum_domain::QuorumResult;

/// Trait for formatting Quorum results
pub trait OutputFormatter {
    /// Format the complete Quorum result
    fn format(&self, result: &QuorumResult) -> String;

    /// Format as JSON
    fn format_json(&self, result: &QuorumResult) -> String;

    /// Format synthesis only (concise output)
    fn format_synthesis_only(&self, result: &QuorumResult) -> String;
}
