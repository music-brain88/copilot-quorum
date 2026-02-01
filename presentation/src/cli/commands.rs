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
#[command(
    author,
    version,
    about = "LLM Council - Multiple LLMs discuss and reach consensus"
)]
#[command(long_about = r#"
Copilot Quorum runs a council of LLMs to discuss a question and reach consensus.

The process has three phases:
1. Initial Query: All models respond to your question in parallel
2. Peer Review: Each model reviews the other responses
3. Synthesis: A moderator model synthesizes everything into a final conclusion

Agent Mode (--agent):
  Autonomous task execution with quorum-based safety reviews.
  The agent will:
  1. Gather context about your project
  2. Create a plan (reviewed by quorum)
  3. Execute tasks (high-risk operations reviewed by quorum)

Configuration files are loaded from (in priority order):
1. --config <path>     Explicit config file
2. ./quorum.toml       Project-level config
3. ~/.config/copilot-quorum/config.toml   Global config

Example:
  copilot-quorum "What's the best way to handle errors in Rust?"
  copilot-quorum -m gpt-5.2-codex -m claude-sonnet-4.5 "Compare async/await patterns"
  copilot-quorum --chat -m claude-haiku-4.5
  copilot-quorum --agent "Fix the bug in login.rs"
  copilot-quorum --agent --agent-interactive
"#)]
pub struct Cli {
    /// The question to ask the council (not required in chat/agent mode)
    pub question: Option<String>,

    /// Start interactive chat mode
    #[arg(short, long)]
    pub chat: bool,

    /// Start agent mode (autonomous task execution)
    #[arg(short, long)]
    pub agent: bool,

    /// Start interactive agent REPL
    #[arg(long)]
    pub agent_interactive: bool,

    /// Models to include in the council (can be specified multiple times)
    /// In agent mode: first model is primary, rest are quorum reviewers
    #[arg(short, long, value_name = "MODEL")]
    pub model: Vec<String>,

    /// Model to use as moderator for final synthesis
    #[arg(long, value_name = "MODEL")]
    pub moderator: Option<String>,

    /// Skip the peer review phase
    #[arg(long)]
    pub no_review: bool,

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

    /// Suppress progress indicators
    #[arg(short, long)]
    pub quiet: bool,

    /// Path to configuration file
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Disable loading of configuration files
    #[arg(long)]
    pub no_config: bool,

    /// Show configuration file locations and exit
    #[arg(long)]
    pub show_config: bool,
}
