//! Headless `review` subcommand (#300, RFC Discussion #304 D2).
//!
//! A thin wrapper over the headless `TuiApp` core: build it, spawn a Review
//! interaction (`InteractionForm::Review`), await its completion via
//! `run_headless_until`, then format `InteractionResult::ReviewResult` to
//! stdout. This is not a fork of the one-shot agent path below it in
//! `main.rs` — it reuses the same actor/event plumbing every other
//! interaction form does, so JSONL logging and Lua `QuorumResult` events
//! come along for free.

use anyhow::{Context, Result};
use quorum_application::QuorumConfig;
use quorum_application::{
    ContextLoaderPort, ConversationLogger, LlmGateway, ScriptingEnginePort, ToolExecutorPort,
    ToolSchemaPort,
};
use quorum_domain::{
    InteractionForm, InteractionResult, QuorumResultPayload, QuorumTarget, QuorumTopic,
    ReviewPromptTemplate, SynthesisResult, Vote, VoteResult,
};
use quorum_presentation::{InteractionOutcome, ReviewArgs, ReviewOutputFormat, TuiApp};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;

/// Exit codes per Issue #300: 0 = approved, 1 = rejected, 2 = execution error.
const EXIT_APPROVED: i32 = 0;
const EXIT_REJECTED: i32 = 1;
const EXIT_ERROR: i32 = 2;

#[allow(clippy::too_many_arguments)]
pub async fn run(
    args: ReviewArgs,
    gateway: Arc<dyn LlmGateway>,
    tool_executor: Arc<dyn ToolExecutorPort>,
    tool_schema: Arc<dyn ToolSchemaPort>,
    context_loader: Arc<dyn ContextLoaderPort>,
    shared_config: Arc<Mutex<QuorumConfig>>,
    conversation_logger: Arc<dyn ConversationLogger>,
    scripting_engine: Arc<dyn ScriptingEnginePort>,
) -> Result<i32> {
    let (diff, pr, title) = match (args.pr, &args.diff) {
        (Some(pr), _) => {
            let (diff, title) = fetch_pr_diff(pr).await?;
            (diff, Some(pr), title)
        }
        (None, Some(path)) => {
            let diff = tokio::fs::read_to_string(path)
                .await
                .with_context(|| format!("failed to read diff file: {}", path.display()))?;
            (diff, None, None)
        }
        (None, None) => {
            let mut diff = String::new();
            tokio::io::stdin()
                .read_to_string(&mut diff)
                .await
                .context("failed to read diff from stdin")?;
            if diff.trim().is_empty() {
                anyhow::bail!(
                    "no diff provided — use --pr, --diff, or pipe a diff to stdin \
                     (e.g. `git diff main...feature | copilot-quorum review`)"
                );
            }
            (diff, None, None)
        }
    };

    let pr_context = pr.map(|pr| match &title {
        Some(t) => format!("PR #{}: {}", pr, t),
        None => format!("PR #{}", pr),
    });
    let material =
        ReviewPromptTemplate::build_material(&diff, args.focus.as_deref(), pr_context.as_deref());
    let label = match pr {
        Some(pr) => format!("Review PR #{}", pr),
        None => "Review diff".to_string(),
    };

    // No clipboard, no --listen: review is a one-shot, non-interactive
    // process (`TuiApp` already defaults clipboard to `NoClipboard`).
    let mut tui_app = TuiApp::new_with_logger(
        gateway,
        tool_executor,
        tool_schema,
        context_loader,
        shared_config,
        conversation_logger,
    )
    .with_scripting_engine(scripting_engine);

    let interaction_id = tui_app
        .spawn_root_interaction(InteractionForm::Review, label, material)
        .await?;

    match tui_app.run_headless_until(interaction_id).await? {
        InteractionOutcome::Completed(InteractionResult::ReviewResult {
            approved,
            votes,
            synthesis,
        }) => {
            print_output(args.output, pr, title, votes, synthesis)?;
            Ok(if approved {
                EXIT_APPROVED
            } else {
                EXIT_REJECTED
            })
        }
        InteractionOutcome::Completed(other) => {
            anyhow::bail!("unexpected interaction result for review: {:?}", other);
        }
        InteractionOutcome::Failed(error) => {
            eprintln!("Review failed: {}", error);
            Ok(EXIT_ERROR)
        }
        InteractionOutcome::Interrupted => {
            eprintln!("Review interrupted before completion");
            Ok(EXIT_ERROR)
        }
    }
}

fn print_output(
    output: ReviewOutputFormat,
    pr: Option<u64>,
    title: Option<String>,
    votes: Vec<Vote>,
    synthesis: SynthesisResult,
) -> Result<()> {
    match output {
        ReviewOutputFormat::Synthesis => {
            println!("{}", synthesis.conclusion);
        }
        ReviewOutputFormat::Json => {
            let vote_result = VoteResult::from_votes(votes);
            let target = QuorumTarget::pr_review(pr, title);
            let payload =
                QuorumResultPayload::new(QuorumTopic::PrReview, Some(target), &vote_result)
                    .with_synthesis(synthesis);
            let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            let record = payload.to_record(timestamp);
            println!("{}", serde_json::to_string_pretty(&record)?);
        }
    }
    Ok(())
}

/// Fetch a PR's diff and title via the `gh` CLI.
///
/// Title lookup failure is non-fatal (falls back to `None`) — the diff is
/// the only thing the review actually needs; the title is just nicer output.
async fn fetch_pr_diff(pr: u64) -> Result<(String, Option<String>)> {
    let diff_output = tokio::process::Command::new("gh")
        .args(["pr", "diff", &pr.to_string()])
        .output()
        .await
        .context("failed to run `gh pr diff` (is the `gh` CLI installed and authenticated?)")?;
    if !diff_output.status.success() {
        anyhow::bail!(
            "`gh pr diff {}` failed: {}",
            pr,
            String::from_utf8_lossy(&diff_output.stderr).trim()
        );
    }
    let diff = String::from_utf8_lossy(&diff_output.stdout).into_owned();

    let title = tokio::process::Command::new("gh")
        .args([
            "pr",
            "view",
            &pr.to_string(),
            "--json",
            "title",
            "-q",
            ".title",
        ])
        .output()
        .await
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|title| !title.is_empty());

    Ok((diff, title))
}
