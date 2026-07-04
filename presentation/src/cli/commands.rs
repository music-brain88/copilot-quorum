//! CLI command definitions

use clap::{Parser, ValueEnum};
use quorum_domain::OutputFormat;
use std::path::PathBuf;

/// CLI-specific output format (newtype for clap ValueEnum)
///
/// This wrapper exists because Rust's orphan rules prevent implementing
/// an external trait (ValueEnum) for an external type (domain::OutputFormat).
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliOutputFormat {
    /// Full formatted output with all phases
    Full,
    /// Only the final synthesis
    Synthesis,
    /// JSON output
    Json,
}

impl From<CliOutputFormat> for OutputFormat {
    fn from(format: CliOutputFormat) -> Self {
        match format {
            CliOutputFormat::Full => OutputFormat::Full,
            CliOutputFormat::Synthesis => OutputFormat::Synthesis,
            CliOutputFormat::Json => OutputFormat::Json,
        }
    }
}

/// CLI arguments for copilot-quorum
#[derive(Parser, Debug)]
#[command(name = "copilot-quorum")]
#[command(author, version, about = "AI Agent with Quorum-based safety reviews")]
#[command(long_about = r#"
Copilot Quorum is an AI Agent that executes tasks with quorum-based safety reviews.

By default, it starts in Solo mode (single model, quick execution).
Use --ensemble for multi-model Ensemble mode.

MODES:
  Solo (default)     Single model driven, quick execution
                     Use /discuss for ad-hoc multi-model consultation
  Ensemble           Multi-model driven, for complex decisions
                     All queries go through Quorum Discussion

The agent will:
1. Gather context about your project
2. Create a plan (reviewed by quorum)
3. Execute tasks (high-risk operations reviewed by quorum)

Use /discuss in the REPL to consult multiple models on a question.

Configuration is done via Lua:
  ~/.config/copilot-quorum/init.lua        Main config file
  ~/.config/copilot-quorum/plugins/*.lua   Plugin scripts (alphabetical)

Example:
  copilot-quorum                           # Start Solo mode REPL (default)
  copilot-quorum --ensemble                # Start Ensemble mode REPL
  copilot-quorum "Fix the bug in login.rs" # Run single task (Solo)
  copilot-quorum --ensemble "Design the auth system"  # Multi-model discussion
  copilot-quorum --no-quorum "Show README" # Skip quorum review (faster)
  copilot-quorum -m claude-haiku-4.5 "Add tests"  # Use specific model
"#)]
pub struct Cli {
    /// The task/question to process (if not provided, starts Agent REPL)
    pub question: Option<String>,

    // ==================== Mode Selection ====================
    /// Start in Solo mode (default, single model driven)
    ///
    /// Solo mode uses a single model for quick execution.
    /// Use /discuss for ad-hoc multi-model consultation.
    #[arg(long, conflicts_with = "ensemble")]
    pub solo: bool,

    /// Start in Ensemble mode (multi-model driven)
    ///
    /// Ensemble mode uses multiple models for all decisions.
    /// Inspired by ML ensemble learning - combines perspectives
    /// for improved accuracy and reliability.
    #[arg(long, conflicts_with = "solo")]
    pub ensemble: bool,

    // ==================== Quorum Settings ====================
    /// Skip quorum review (plan review will be auto-approved)
    #[arg(long)]
    pub no_quorum: bool,

    /// Models to use (first = primary, rest = quorum reviewers)
    #[arg(short, long, value_name = "MODEL")]
    pub model: Vec<String>,

    /// Enable final review in agent mode
    #[arg(long)]
    pub final_review: bool,

    /// Working directory for agent mode
    #[arg(short, long, value_name = "PATH")]
    pub working_dir: Option<PathBuf>,

    /// Output format (default: synthesis, or from config file)
    #[arg(short, long, value_enum)]
    pub output: Option<CliOutputFormat>,

    /// Verbosity level (-v = info, -vv = debug, -vvv = trace)
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Show detailed quorum vote information
    #[arg(long)]
    pub show_votes: bool,

    /// Suppress progress indicators
    #[arg(short, long)]
    pub quiet: bool,

    /// Override log file directory (default: ~/.local/share/copilot-quorum/logs/)
    #[arg(long, value_name = "PATH")]
    pub log_dir: Option<PathBuf>,

    /// Disable file logging entirely
    #[arg(long)]
    pub no_log_file: bool,

    /// Show init.lua and plugin paths and exit
    #[arg(long)]
    pub show_config: bool,

    /// Expose a JSON-RPC remote control socket at PATH (TUI mode only)
    ///
    /// External processes (e.g. coding agents) can inspect panes and
    /// inject input over this Unix socket. See docs/reference/tui-remote-control.md.
    #[arg(long, value_name = "PATH")]
    pub listen: Option<PathBuf>,

    /// Run the TUI event loop without a terminal (no raw mode / alternate
    /// screen / keyboard input). State and rendering are still available
    /// through the `--listen` socket — same state, same `screen.capture`,
    /// just no TTY. Requires `--listen`, since without it the process would
    /// be unoperable. See docs/reference/tui-remote-control.md.
    #[arg(long, requires = "listen")]
    pub headless: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_requires_listen() {
        let err = Cli::try_parse_from(["copilot-quorum", "--headless"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn headless_with_listen_parses() {
        let cli = Cli::try_parse_from(["copilot-quorum", "--headless", "--listen", "/tmp/q.sock"])
            .unwrap();
        assert!(cli.headless);
        assert_eq!(cli.listen, Some(PathBuf::from("/tmp/q.sock")));
    }
}
