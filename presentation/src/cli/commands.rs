//! CLI command definitions

use clap::{Parser, ValueEnum};
use std::path::PathBuf;

/// Output format for Quorum results
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    /// Full formatted output with all phases
    Full,
    /// Only the final synthesis
    Synthesis,
    /// JSON output
    Json,
}

/// CLI arguments for copilot-quorum
#[derive(Parser, Debug)]
#[command(name = "copilot-quorum")]
#[command(author, version, about = "LLM Council - Multiple LLMs discuss and reach consensus")]
#[command(long_about = r#"
Copilot Quorum runs a council of LLMs to discuss a question and reach consensus.

The process has three phases:
1. Initial Query: All models respond to your question in parallel
2. Peer Review: Each model reviews the other responses
3. Synthesis: A moderator model synthesizes everything into a final conclusion

Configuration files are loaded from (in priority order):
1. --config <path>     Explicit config file
2. ./quorum.toml       Project-level config
3. ~/.config/copilot-quorum/config.toml   Global config

Example:
  copilot-quorum "What's the best way to handle errors in Rust?"
  copilot-quorum -m gpt-5.2-codex -m claude-sonnet-4.5 "Compare async/await patterns"
  copilot-quorum --chat -m claude-haiku-4.5
"#)]
pub struct Cli {
    /// The question to ask the council (not required in chat mode)
    pub question: Option<String>,

    /// Start interactive chat mode
    #[arg(short, long)]
    pub chat: bool,

    /// Models to include in the council (can be specified multiple times)
    #[arg(short, long, value_name = "MODEL")]
    pub model: Vec<String>,

    /// Model to use as moderator for final synthesis
    #[arg(long, value_name = "MODEL")]
    pub moderator: Option<String>,

    /// Skip the peer review phase
    #[arg(long)]
    pub no_review: bool,

    /// Output format
    #[arg(short, long, value_enum, default_value = "synthesis")]
    pub output: OutputFormat,

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
