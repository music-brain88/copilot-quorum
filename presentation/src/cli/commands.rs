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

/// Subcommands (#300 / RFC Discussion #304 D4).
///
/// Note for reviewers: this is the same seam #302 (rpc subcommand) needs —
/// expect a merge conflict here. Whoever merges second should rebase and add
/// their variant alongside `Review`.
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Headless multi-model Quorum review of a PR or diff (#300)
    Review(ReviewArgs),
}

/// Output format for the `review` subcommand.
#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum ReviewOutputFormat {
    /// The moderator's synthesized review, as Markdown (default)
    #[default]
    Synthesis,
    /// Structured `quorum_result` JSON (votes + synthesis + target)
    Json,
}

/// Arguments for `copilot-quorum review`.
#[derive(clap::Args, Debug)]
pub struct ReviewArgs {
    /// PR number to review (diff + title fetched via `gh pr diff`/`gh pr view`)
    #[arg(long, value_name = "NUMBER", conflicts_with = "diff")]
    pub pr: Option<u64>,

    /// Path to a diff/patch file to review (omit both --pr and --diff to read
    /// the diff from stdin, e.g. `git diff main...feature | copilot-quorum review`)
    #[arg(long, value_name = "PATH")]
    pub diff: Option<PathBuf>,

    /// Review focus / instruction for the models (e.g. "concurrency safety")
    #[arg(long, value_name = "TEXT")]
    pub focus: Option<String>,

    /// Output format
    #[arg(long, value_enum, default_value = "synthesis")]
    pub output: ReviewOutputFormat,
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
  copilot-quorum review --pr 123           # Headless multi-model PR review (#300)
"#)]
pub struct Cli {
    /// Subcommand (e.g. `review`). Global flags (`-v`, `--log-dir`, etc.) may
    /// precede the subcommand name (`copilot-quorum -v review --pr 123`);
    /// subcommand-specific flags go after it. Deliberately *not* annotated
    /// with clap's `args_conflicts_with_subcommands` — that attribute makes
    /// clap stop recognizing the subcommand name entirely once any other
    /// top-level flag/value is already present on the command line, so
    /// `copilot-quorum --log-dir X review --pr 123` would silently run
    /// `review` as the literal REPL/one-shot QUESTION text instead of
    /// dispatching to the subcommand (verified empirically — see
    /// `global_flag_before_subcommand_still_dispatches` below).
    #[command(subcommand)]
    pub command: Option<Command>,

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

    #[test]
    fn plain_question_has_no_subcommand() {
        let cli = Cli::try_parse_from(["copilot-quorum", "Fix the bug"]).unwrap();
        assert_eq!(cli.question, Some("Fix the bug".to_string()));
        assert!(cli.command.is_none());
    }

    #[test]
    fn no_args_starts_repl() {
        let cli = Cli::try_parse_from(["copilot-quorum"]).unwrap();
        assert_eq!(cli.question, None);
        assert!(cli.command.is_none());
    }

    #[test]
    fn review_pr_parses() {
        let cli = Cli::try_parse_from(["copilot-quorum", "review", "--pr", "123"]).unwrap();
        assert_eq!(cli.question, None);
        match cli.command {
            Some(Command::Review(args)) => {
                assert_eq!(args.pr, Some(123));
                assert_eq!(args.diff, None);
                assert_eq!(args.focus, None);
                assert!(matches!(args.output, ReviewOutputFormat::Synthesis));
            }
            other => panic!("Expected Command::Review, got {:?}", other),
        }
    }

    #[test]
    fn review_diff_and_focus_and_json_output_parse() {
        let cli = Cli::try_parse_from([
            "copilot-quorum",
            "review",
            "--diff",
            "changes.patch",
            "--focus",
            "concurrency safety",
            "--output",
            "json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Review(args)) => {
                assert_eq!(args.diff, Some(PathBuf::from("changes.patch")));
                assert_eq!(args.focus.as_deref(), Some("concurrency safety"));
                assert!(matches!(args.output, ReviewOutputFormat::Json));
            }
            other => panic!("Expected Command::Review, got {:?}", other),
        }
    }

    #[test]
    fn review_bare_defaults_to_stdin_and_synthesis_output() {
        let cli = Cli::try_parse_from(["copilot-quorum", "review"]).unwrap();
        match cli.command {
            Some(Command::Review(args)) => {
                assert_eq!(args.pr, None);
                assert_eq!(args.diff, None);
                assert!(matches!(args.output, ReviewOutputFormat::Synthesis));
            }
            other => panic!("Expected Command::Review, got {:?}", other),
        }
    }

    #[test]
    fn review_pr_and_diff_conflict() {
        let err = Cli::try_parse_from([
            "copilot-quorum",
            "review",
            "--pr",
            "123",
            "--diff",
            "changes.patch",
        ])
        .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    // Regression test: an earlier version of this struct had
    // `#[command(args_conflicts_with_subcommands = true)]`, which made clap
    // stop recognizing `review` as a subcommand once ANY other top-level
    // flag/value preceded it on the command line — `--log-dir X review --pr
    // 123` silently ran `review` as the literal one-shot QUESTION text
    // instead of dispatching to the subcommand (caught via manual testing,
    // not by the parse-only tests above, since the parse itself "succeeded").
    #[test]
    fn global_flag_before_subcommand_still_dispatches() {
        for prefix in [
            vec!["-v"],
            vec!["--log-dir", "/tmp/x"],
            vec!["--quiet"],
            vec!["--no-log-file"],
        ] {
            let mut argv = vec!["copilot-quorum"];
            argv.extend(prefix.iter().copied());
            argv.extend(["review", "--pr", "123"]);

            let cli = Cli::try_parse_from(&argv)
                .unwrap_or_else(|e| panic!("failed to parse {:?}: {}", argv, e));
            assert_eq!(
                cli.question, None,
                "argv {:?} should have no question",
                argv
            );
            match cli.command {
                Some(Command::Review(args)) => assert_eq!(args.pr, Some(123)),
                other => panic!("argv {:?}: expected Command::Review, got {:?}", argv, other),
            }
        }
    }
}
