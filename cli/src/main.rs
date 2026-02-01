//! CLI entrypoint for Copilot Quorum
//!
//! This is the main binary that wires together all layers using
//! dependency injection. Config conversion logic is centralized here.

use anyhow::{bail, Result};
use clap::Parser;
use quorum_application::{BehaviorConfig, RunAgentInput, RunAgentUseCase, RunQuorumInput, RunQuorumUseCase};
use quorum_domain::{AgentConfig, Model, OutputFormat};
use quorum_infrastructure::{ConfigLoader, CopilotLlmGateway, FileConfig, LocalToolExecutor};
use quorum_presentation::{
    AgentProgressReporter, AgentRepl, ChatRepl, Cli, ConsoleFormatter, OutputConfig,
    ProgressReporter, ReplConfig,
};
use std::sync::Arc;
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

    // Build layer-specific configs from FileConfig + CLI
    let (_behavior, output_config, repl_config) = build_configs(&cli, &config);

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

    // === Dependency Injection ===
    // Create infrastructure adapter (Copilot Gateway)
    let gateway = Arc::new(CopilotLlmGateway::new().await?);

    // Chat mode
    if cli.chat {
        let repl = ChatRepl::new(gateway, models)
            .with_progress(repl_config.show_progress)
            .with_skip_review(!enable_review);

        repl.run().await?;
        return Ok(());
    }

    // Agent mode
    if cli.agent || cli.agent_interactive {
        // Create tool executor
        let working_dir = cli
            .working_dir
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string()));

        let mut tool_executor = LocalToolExecutor::new();
        if let Some(ref dir) = working_dir {
            tool_executor = tool_executor.with_working_dir(dir);
        }
        let tool_executor = Arc::new(tool_executor);

        // Parse models for agent mode
        // First model is primary, rest are quorum reviewers
        let (primary_model, quorum_models) = if models.len() > 1 {
            (models[0].clone(), models[1..].to_vec())
        } else if !models.is_empty() {
            (models[0].clone(), Model::default_models())
        } else {
            (Model::default(), Model::default_models())
        };

        // Interactive agent REPL
        if cli.agent_interactive {
            let mut repl = AgentRepl::new(gateway, tool_executor, primary_model)
                .with_quorum_models(quorum_models)
                .with_verbose(cli.verbose > 0);

            if let Some(dir) = working_dir {
                repl = repl.with_working_dir(dir);
            }

            if cli.final_review {
                repl = repl.with_final_review(true);
            }

            repl.run().await?;
            return Ok(());
        }

        // Single request agent mode - request is required
        let request = match cli.question {
            Some(q) => q,
            None => bail!("Request is required. Use --agent-interactive for interactive mode."),
        };

        // Build agent config
        let mut agent_config = AgentConfig::new(primary_model.clone())
            .with_quorum_models(quorum_models.clone());

        if let Some(dir) = &working_dir {
            agent_config = agent_config.with_working_dir(dir);
        }

        if cli.final_review {
            agent_config = agent_config.with_final_review();
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
            println!(
                "Quorum Models: {}",
                quorum_models
                    .iter()
                    .map(|m| m.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!();
        }

        // Create and run agent
        let use_case = RunAgentUseCase::new(gateway, tool_executor);
        let input = RunAgentInput::new(request, agent_config);

        let result = if repl_config.show_progress {
            let progress = if cli.verbose > 0 {
                AgentProgressReporter::verbose()
            } else {
                AgentProgressReporter::new()
            };
            use_case.execute_with_progress(input, &progress).await?
        } else {
            use_case.execute(input).await?
        };

        // Output results
        println!();
        if result.success {
            println!("Agent completed successfully!");
        } else {
            println!("Agent completed with issues.");
        }
        println!();
        println!("Summary:\n{}", result.summary);

        return Ok(());
    }

    // Single question mode - question is required
    let question = match cli.question {
        Some(q) => q,
        None => bail!("Question is required. Use --chat for interactive mode, or --agent for agent mode."),
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
    if repl_config.show_progress {
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
    let result = if repl_config.show_progress {
        let progress = ProgressReporter::new();
        use_case.execute_with_progress(input, &progress).await?
    } else {
        use_case.execute(input).await?
    };

    // Output results using config format
    let output = match output_config.format {
        OutputFormat::Full => ConsoleFormatter::format(&result),
        OutputFormat::Synthesis => ConsoleFormatter::format_synthesis_only(&result),
        OutputFormat::Json => ConsoleFormatter::format_json(&result),
    };

    println!("{}", output);

    Ok(())
}
