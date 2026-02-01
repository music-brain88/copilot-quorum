//! Thought streaming for visualizing agent thinking process

use colored::Colorize;
use quorum_domain::{Thought, ThoughtType};
use std::io::{self, Write};

/// Streams agent thoughts to the console in real-time
pub struct ThoughtStream {
    /// Show detailed thoughts
    verbose: bool,
    /// Use colors
    colored: bool,
    /// Prefix for indentation
    indent: String,
}

impl ThoughtStream {
    /// Create a new thought stream
    pub fn new() -> Self {
        Self {
            verbose: false,
            colored: true,
            indent: "  ".to_string(),
        }
    }

    /// Create with verbose output
    pub fn verbose() -> Self {
        Self {
            verbose: true,
            colored: true,
            indent: "  ".to_string(),
        }
    }

    /// Set indentation
    pub fn with_indent(mut self, indent: impl Into<String>) -> Self {
        self.indent = indent.into();
        self
    }

    /// Disable colors
    pub fn without_colors(mut self) -> Self {
        self.colored = false;
        self
    }

    /// Stream a thought to output
    pub fn stream(&self, thought: &Thought) {
        self.stream_to(&mut io::stdout(), thought);
    }

    /// Stream a thought to a specific writer
    pub fn stream_to<W: Write>(&self, writer: &mut W, thought: &Thought) {
        let type_info = self.format_thought_type(&thought.thought_type);
        let content = self.format_content(&thought.content, &thought.thought_type);

        // Simple thoughts get one line
        if !self.verbose && thought.content.len() < 80 {
            let _ = writeln!(writer, "{}{} {}", self.indent, type_info, content);
            return;
        }

        // Verbose or long thoughts get formatted display
        let _ = writeln!(writer, "{}{}", self.indent, type_info);
        for line in thought.content.lines() {
            let _ = writeln!(writer, "{}  {}", self.indent, self.style_line(line, &thought.thought_type));
        }
        let _ = writeln!(writer);
    }

    /// Format the thought type indicator
    fn format_thought_type(&self, thought_type: &ThoughtType) -> String {
        let (emoji, label) = match thought_type {
            ThoughtType::Observation => ("👁️ ", "Observation"),
            ThoughtType::Analysis => ("🔬", "Analysis"),
            ThoughtType::Planning => ("📋", "Planning"),
            ThoughtType::Reasoning => ("🧠", "Reasoning"),
            ThoughtType::Reflection => ("💭", "Reflection"),
            ThoughtType::Conclusion => ("✅", "Conclusion"),
        };

        if self.colored {
            let colored_label = match thought_type {
                ThoughtType::Observation => label.blue(),
                ThoughtType::Analysis => label.cyan(),
                ThoughtType::Planning => label.magenta(),
                ThoughtType::Reasoning => label.white(),
                ThoughtType::Reflection => label.yellow(),
                ThoughtType::Conclusion => label.green(),
            };
            format!("{} {}", emoji, colored_label.bold())
        } else {
            format!("{} {}", emoji, label)
        }
    }

    /// Format thought content
    fn format_content(&self, content: &str, thought_type: &ThoughtType) -> String {
        if self.colored {
            match thought_type {
                ThoughtType::Conclusion => content.green().to_string(),
                ThoughtType::Reflection => content.yellow().to_string(),
                _ => content.to_string(),
            }
        } else {
            content.to_string()
        }
    }

    /// Style a line based on thought type
    fn style_line(&self, line: &str, thought_type: &ThoughtType) -> String {
        if !self.colored {
            return line.to_string();
        }

        match thought_type {
            ThoughtType::Planning if line.trim().starts_with('-') => {
                line.bright_white().to_string()
            }
            ThoughtType::Planning if line.trim().starts_with(|c: char| c.is_ascii_digit()) => {
                line.cyan().to_string()
            }
            ThoughtType::Conclusion => line.green().to_string(),
            ThoughtType::Reflection => line.yellow().to_string(),
            _ => line.dimmed().to_string(),
        }
    }
}

impl Default for ThoughtStream {
    fn default() -> Self {
        Self::new()
    }
}

/// Formats a list of thoughts for display
pub fn format_thoughts(thoughts: &[Thought]) -> String {
    let stream = ThoughtStream::new();
    let mut output = Vec::new();

    for thought in thoughts {
        stream.stream_to(&mut output, thought);
    }

    String::from_utf8_lossy(&output).to_string()
}

/// Formats thoughts in a compact summary form
pub fn summarize_thoughts(thoughts: &[Thought]) -> String {
    if thoughts.is_empty() {
        return "No recorded thoughts.".to_string();
    }

    let mut lines = Vec::new();

    // Group by type
    let observations: Vec<_> = thoughts.iter().filter(|t| matches!(t.thought_type, ThoughtType::Observation)).collect();
    let reasoning: Vec<_> = thoughts.iter().filter(|t| matches!(t.thought_type, ThoughtType::Reasoning)).collect();
    let conclusions: Vec<_> = thoughts.iter().filter(|t| matches!(t.thought_type, ThoughtType::Conclusion)).collect();

    if !observations.is_empty() {
        lines.push(format!("Observations: {}", observations.len()));
    }
    if !reasoning.is_empty() {
        lines.push(format!("Reasoning steps: {}", reasoning.len()));
    }
    if !conclusions.is_empty() {
        lines.push("Conclusions:".to_string());
        for c in conclusions {
            lines.push(format!("  - {}", c.content));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thought_stream() {
        let stream = ThoughtStream::new().without_colors();
        let thought = Thought::observation("Found 5 files");

        let mut output = Vec::new();
        stream.stream_to(&mut output, &thought);

        let output_str = String::from_utf8_lossy(&output);
        assert!(output_str.contains("Observation"));
        assert!(output_str.contains("Found 5 files"));
    }

    #[test]
    fn test_summarize_thoughts() {
        let thoughts = vec![
            Thought::observation("File exists"),
            Thought::observation("Has 100 lines"),
            Thought::conclusion("Task complete"),
        ];

        let summary = summarize_thoughts(&thoughts);
        assert!(summary.contains("Observations: 2"));
        assert!(summary.contains("Task complete"));
    }
}
