//! Console output formatter for Quorum results

use crate::output::formatter::OutputFormatter;
use colored::Colorize;
use quorum_domain::QuorumResult;

/// Formats Quorum results for console display
pub struct ConsoleFormatter;

impl ConsoleFormatter {
    /// Format the complete Quorum result
    pub fn format(result: &QuorumResult) -> String {
        let mut output = String::new();

        // Header
        output.push_str(&Self::header("LLM Council Results"));
        output.push('\n');

        // Question
        output.push_str(&format!(
            "{} {}\n\n",
            "Question:".cyan().bold(),
            result.question
        ));

        // Models
        output.push_str(&format!(
            "{} {}\n\n",
            "Models:".cyan().bold(),
            result.models.join(", ")
        ));

        // Phase 1: Initial Responses
        output.push_str(&Self::section_header("Phase 1: Initial Responses"));
        for response in &result.responses {
            if response.success {
                output.push_str(&format!(
                    "\n{}\n{}\n",
                    format!("── {} ──", response.model).yellow().bold(),
                    response.content
                ));
            } else {
                output.push_str(&format!(
                    "\n{}\nError: {}\n",
                    format!("── {} ──", response.model).red().bold(),
                    response.error.as_deref().unwrap_or("Unknown")
                ));
            }
        }

        // Phase 2: Peer Reviews (if any)
        if !result.reviews.is_empty() {
            output.push_str(&Self::section_header("Phase 2: Peer Reviews"));
            for review in &result.reviews {
                output.push_str(&format!(
                    "\n{}\n{}\n",
                    format!("── {} reviewed {} ──", review.reviewer, review.reviewed_id)
                        .yellow()
                        .bold(),
                    review.content
                ));
            }
        }

        // Phase 3: Synthesis
        output.push_str(&Self::section_header("Phase 3: Final Synthesis"));
        output.push_str(&format!(
            "\n{}\n\n{}\n",
            format!("Moderator: {}", result.synthesis.moderator)
                .yellow()
                .bold(),
            result.synthesis.conclusion
        ));

        // Key points (if extracted)
        if !result.synthesis.key_points.is_empty() {
            output.push_str(&format!("\n{}\n", "Key Points:".cyan().bold()));
            for point in &result.synthesis.key_points {
                output.push_str(&format!("  * {}\n", point));
            }
        }

        // Consensus (if extracted)
        if !result.synthesis.consensus.is_empty() {
            output.push_str(&format!("\n{}\n", "Areas of Consensus:".green().bold()));
            for point in &result.synthesis.consensus {
                output.push_str(&format!("  * {}\n", point));
            }
        }

        // Disagreements (if extracted)
        if !result.synthesis.disagreements.is_empty() {
            output.push_str(&format!("\n{}\n", "Disagreements:".yellow().bold()));
            for point in &result.synthesis.disagreements {
                output.push_str(&format!("  * {}\n", point));
            }
        }

        output.push_str(&Self::footer());

        output
    }

    /// Format as JSON
    pub fn format_json(result: &QuorumResult) -> String {
        serde_json::to_string_pretty(result).unwrap_or_else(|_| "{}".to_string())
    }

    /// Format synthesis only (concise output)
    pub fn format_synthesis_only(result: &QuorumResult) -> String {
        let mut output = String::new();

        output.push_str(&format!(
            "{}\n\n",
            "=== LLM Council Conclusion ===".cyan().bold()
        ));

        output.push_str(&format!("{} {}\n\n", "Q:".bold(), result.question));

        output.push_str(&format!(
            "{} {}\n\n",
            "Models consulted:".dimmed(),
            result.models.join(", ")
        ));

        output.push_str(&result.synthesis.conclusion);
        output.push('\n');

        output
    }

    fn header(title: &str) -> String {
        let line = "=".repeat(60);
        format!("{}\n{:^60}\n{}", line.cyan(), title.bold(), line.cyan())
    }

    fn section_header(title: &str) -> String {
        format!("\n{}\n{}\n", title.cyan().bold(), "-".repeat(40))
    }

    fn footer() -> String {
        format!("\n{}\n", "=".repeat(60).cyan())
    }

    /// Indent a multi-line string
    pub fn indent(text: &str, prefix: &str) -> String {
        text.lines()
            .map(|line| format!("{}{}", prefix, line))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl OutputFormatter for ConsoleFormatter {
    fn format(&self, result: &QuorumResult) -> String {
        Self::format(result)
    }

    fn format_json(&self, result: &QuorumResult) -> String {
        Self::format_json(result)
    }

    fn format_synthesis_only(&self, result: &QuorumResult) -> String {
        Self::format_synthesis_only(result)
    }
}
