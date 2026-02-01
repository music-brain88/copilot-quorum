//! CLI entrypoint for Copilot Quorum
//!
//! This is the main binary that wires together all layers using
//! dependency injection.

use anyhow::{bail, Result};
use clap::Parser;
use quorum_application::{RunQuorumInput, RunQuorumUseCase};
use quorum_domain::Model;
use quorum_infrastructure::{ConfigLoader, CopilotLlmGateway, FileConfig};
use quorum_presentation::{ChatRepl, Cli, ConsoleFormatter, OutputFormat, ProgressReporter};
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle --show-config flag
    if cli.show_config {
        ConfigLoader::print_config_sources();
        return Ok(());
    }

    // Load configuration (respecting --no-config flag)
    let config: FileConfig = if cli.no_config {
        ConfigLoader::load_defaults()
    } else {
        ConfigLoader::load(cli.config.as_ref()).unwrap_or_else(|e| {
            eprintln!("Warning: Failed to load config file: {}", e);
            ConfigLoader::load_defaults()
        })
    };

    // Validate configuration
    if let Err(e) = config.validate() {
        bail!("Invalid configuration: {}", e);
    }

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

    // Parse models: CLI > config file > default
    let models: Vec<Model> = if !cli.model.is_empty() {
        // CLI takes precedence
        cli.model.iter().map(|s| s.parse().unwrap()).collect()
    } else if !config.council.models.is_empty() {
        // Config file models
        config
            .council
            .models
            .iter()
            .map(|s| s.parse().unwrap())
            .collect()
    } else {
        // Built-in defaults
        Model::default_models()
    };

    // Determine moderator: CLI > config file
    let moderator: Option<Model> = cli
        .moderator
        .as_ref()
        .or(config.council.moderator.as_ref())
        .map(|s| s.parse().unwrap());

    // Determine if review is enabled: CLI --no-review overrides config
    let enable_review = if cli.no_review {
        false
    } else {
        config.behavior.enable_review
    };

    // Determine quiet mode: CLI > config (inverted from show_progress)
    let quiet = cli.quiet || !config.repl.show_progress;

    // === Dependency Injection ===
    // Create infrastructure adapter (Copilot Gateway)
    let gateway = Arc::new(CopilotLlmGateway::new().await?);

    // Chat mode
    if cli.chat {
        let repl = ChatRepl::new(gateway, models)
            .with_progress(!quiet)
            .with_skip_review(!enable_review);

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

    if let Some(mod_model) = moderator.clone() {
        input = input.with_moderator(mod_model);
    }

    if !enable_review {
        input = input.without_review();
    }

    // Print header
    if !quiet {
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
    let result = if quiet {
        use_case.execute(input).await?
    } else {
        let progress = ProgressReporter::new();
        use_case.execute_with_progress(input, &progress).await?
    };

    // Determine output format: CLI > config file > default (synthesis)
    let output_format = cli
        .output
        .or_else(|| config.output.format.map(OutputFormat::from))
        .unwrap_or(OutputFormat::Synthesis);

    // Output results
    let output = match output_format {
        OutputFormat::Full => ConsoleFormatter::format(&result),
        OutputFormat::Synthesis => ConsoleFormatter::format_synthesis_only(&result),
        OutputFormat::Json => ConsoleFormatter::format_json(&result),
    };

    println!("{}", output);

    Ok(())
}
