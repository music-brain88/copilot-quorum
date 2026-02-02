//! CLI entrypoint for Copilot Quorum
//!
//! This is the main binary that wires together all layers using
//! dependency injection. Config conversion logic is centralized here.

use anyhow::{bail, Result};
use clap::Parser;
use quorum_application::{BehaviorConfig, RunAgentInput, RunAgentUseCase};
use quorum_domain::{AgentConfig, Model, OutputFormat};
use quorum_infrastructure::{
    ConfigLoader, CopilotLlmGateway, FileConfig, LocalContextLoader, LocalToolExecutor,
};
use quorum_presentation::{AgentProgressReporter, AgentRepl, Cli, OutputConfig, ReplConfig};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing_subscriber::EnvFilter;

/// Convert FileConfig + CLI args to layer-specific configs
/// This is the single place where FileConfig is translated to application/presentation types
fn build_configs(cli: &Cli, file: &FileConfig) -> (BehaviorConfig, OutputConfig, ReplConfig) {
    // Application layer config
    let behavior = BehaviorConfig::from_timeout_seconds(file.behavior.timeout_seconds);

    // Presentation layer configs
    // CLI uses CliOutputFormat (for clap), convert to domain OutputFormat
    let output_format = cli
        .output
        .map(OutputFormat::from)
        .or(file.output.format)
        .unwrap_or_default();

    let output = OutputConfig::new(output_format, file.output.color);

    let repl = ReplConfig::new(
        !cli.quiet && file.repl.show_progress,
        file.repl.history_file.clone(),
    );

    (behavior, output, repl)
}

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

    // Create cancellation token for graceful shutdown
    let cancellation_token = CancellationToken::new();

    // Set up Ctrl+C signal handler
    let shutdown_token = cancellation_token.clone();
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                eprintln!("\nInterrupted. Shutting down gracefully...");
                shutdown_token.cancel();
            }
            Err(e) => {
                eprintln!("Failed to listen for Ctrl+C signal: {}", e);
            }
        }
    });

    // Build layer-specific configs from FileConfig + CLI
    let (_behavior, _output_config, repl_config) = build_configs(&cli, &config);

    // Parse models: CLI > config file > default
    let models: Vec<Model> = if !cli.model.is_empty() {
        cli.model.iter().map(|s| s.parse().unwrap()).collect()
    } else if !config.council.models.is_empty() {
        config
            .council
            .models
            .iter()
            .map(|s| s.parse().unwrap())
            .collect()
    } else {
        Model::default_models()
    };

    // === Dependency Injection ===
    // Create infrastructure adapter (Copilot Gateway)
    let gateway = Arc::new(CopilotLlmGateway::new().await?);

    // Create tool executor
    let working_dir = cli
        .working_dir
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        });

    let mut tool_executor = LocalToolExecutor::new();
    if let Some(ref dir) = working_dir {
        tool_executor = tool_executor.with_working_dir(dir);
    }
    let tool_executor = Arc::new(tool_executor);

    // Create context loader
    let context_loader = Arc::new(LocalContextLoader::new());

    // Parse models for agent mode
    // First model is primary, rest are quorum reviewers
    let (primary_model, quorum_models) = if models.len() > 1 {
        (models[0].clone(), models[1..].to_vec())
    } else if !models.is_empty() {
        (models[0].clone(), Model::default_models())
    } else {
        (Model::default(), Model::default_models())
    };

    // Determine quorum models based on --no-quorum flag
    let effective_quorum_models = if cli.no_quorum {
        vec![] // Empty quorum models will auto-approve
    } else {
        quorum_models.clone()
    };

    // No question provided -> Start Agent REPL (default)
    if cli.question.is_none() {
        let mut repl = AgentRepl::new(
            gateway.clone(),
            tool_executor,
            context_loader.clone(),
            primary_model,
        )
        .with_quorum_models(effective_quorum_models)
        .with_verbose(cli.verbose > 0)
        .with_cancellation(cancellation_token.clone());

        // Set moderator if explicitly configured
        if let Some(ref moderator_name) = config.council.moderator {
            let moderator: Model = moderator_name.parse().unwrap();
            repl = repl.with_moderator(moderator);
        }

        if let Some(dir) = working_dir {
            repl = repl.with_working_dir(dir);
        }

        if cli.final_review {
            repl = repl.with_final_review(true);
        }

        repl.run().await?;
        return Ok(());
    }

    // Question provided -> Single request agent mode
    let request = cli.question.unwrap();

    // Build agent config
    let mut agent_config =
        AgentConfig::new(primary_model.clone()).with_quorum_models(effective_quorum_models.clone());

    if let Some(dir) = &working_dir {
        agent_config = agent_config.with_working_dir(dir);
    }

    if cli.final_review {
        agent_config = agent_config.with_final_review();
    }

    if cli.no_quorum {
        agent_config = agent_config.with_skip_plan_review();
    }

    // Print header
    if repl_config.show_progress {
        println!();
        println!("+============================================================+");
        println!("|           Copilot Quorum - Agent Mode                      |");
        println!("+============================================================+");
        println!();
        println!("Request: {}", request);
        println!("Primary Model: {}", primary_model);
        if cli.no_quorum {
            println!("Quorum: Disabled (--no-quorum)");
        } else {
            println!(
                "Quorum Models: {}",
                quorum_models
                    .iter()
                    .map(|m| m.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        println!();
    }

    // Create and run agent with cancellation support
    let use_case = RunAgentUseCase::with_context_loader(gateway, tool_executor, context_loader)
        .with_cancellation(cancellation_token.clone());
    let input = RunAgentInput::new(request, agent_config);

    let result = if repl_config.show_progress {
        let progress = AgentProgressReporter::with_options(cli.verbose > 0, cli.show_votes);
        use_case.execute_with_progress(input, &progress).await
    } else {
        use_case.execute(input).await
    };

    // Handle result, including cancellation
    match result {
        Ok(output) => {
            println!();
            if output.success {
                println!("Agent completed successfully!");
            } else {
                println!("Agent completed with issues.");
            }
            println!();
            println!("Summary:\n{}", output.summary);
        }
        Err(e) if e.is_cancelled() => {
            println!("\nOperation cancelled.");
        }
        Err(e) => {
            return Err(e.into());
        }
    }

    Ok(())
}
