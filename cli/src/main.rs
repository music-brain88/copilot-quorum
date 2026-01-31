//! CLI entrypoint for Copilot Quorum
//!
//! This is the main binary that wires together all layers using
//! dependency injection.

use anyhow::{bail, Result};
use clap::Parser;
use quorum_application::{RunQuorumInput, RunQuorumUseCase};
use quorum_domain::Model;
use quorum_infrastructure::CopilotLlmGateway;
use quorum_presentation::{ChatRepl, Cli, ConsoleFormatter, OutputFormat, ProgressReporter};
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging based on verbosity level
    let filter = match cli.verbose {
        0 => EnvFilter::new("warn"),
        1 => EnvFilter::new("info"),
        2 => EnvFilter::new("debug"),
        _ => EnvFilter::new("trace"), // -vvv or more
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    info!("Starting Copilot Quorum");

    // Parse models
    let models: Vec<Model> = if cli.model.is_empty() {
        Model::default_models()
    } else {
        cli.model.iter().map(|s| s.parse().unwrap()).collect()
    };

    // === Dependency Injection ===
    // Create infrastructure adapter (Copilot Gateway)
    let gateway = Arc::new(CopilotLlmGateway::new().await?);

    // Chat mode
    if cli.chat {
        let repl = ChatRepl::new(gateway, models)
            .with_progress(!cli.quiet)
            .with_skip_review(cli.no_review);

        repl.run().await?;
        return Ok(());
    }

    // Single question mode - question is required
    let question = match cli.question {
        Some(q) => q,
        None => bail!("Question is required. Use --chat for interactive mode."),
    };

    // Build input
    let mut input = RunQuorumInput::new(question.clone(), models.clone());

    if let Some(mod_str) = &cli.moderator {
        input = input.with_moderator(mod_str.parse().unwrap());
    }

    if cli.no_review {
        input = input.without_review();
    }

    // Print header
    if !cli.quiet {
        println!();
        println!("+============================================================+");
        println!("|           Copilot Quorum - LLM Council                     |");
        println!("+============================================================+");
        println!();
        println!("Question: {}", question);
        println!(
            "Models: {}",
            models
                .iter()
                .map(|m| m.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!();
    }

    // Create use case with injected gateway
    let use_case = RunQuorumUseCase::new(gateway);

    // Execute with or without progress reporting
    let result = if cli.quiet {
        use_case.execute(input).await?
    } else {
        let progress = ProgressReporter::new();
        use_case.execute_with_progress(input, &progress).await?
    };

    // Output results
    let output = match cli.output {
        OutputFormat::Full => ConsoleFormatter::format(&result),
        OutputFormat::Synthesis => ConsoleFormatter::format_synthesis_only(&result),
        OutputFormat::Json => ConsoleFormatter::format_json(&result),
    };

    println!("{}", output);

    Ok(())
}
