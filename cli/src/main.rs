//! CLI entrypoint for Copilot Quorum
//!
//! This is the main binary that wires together all layers using
//! dependency injection. Config conversion logic is centralized here.

use anyhow::{Result, bail};
use clap::Parser;
use quorum_application::{BehaviorConfig, RunAgentInput, RunAgentUseCase};
use quorum_domain::{AgentConfig, ConsensusLevel, Model, OutputFormat};
use quorum_infrastructure::{
    ConfigLoader, CopilotLlmGateway, FileConfig, LocalContextLoader, LocalToolExecutor,
};
use quorum_presentation::{
    AgentProgressReporter, AgentRepl, Cli, InteractiveHumanIntervention, OutputConfig, ReplConfig,
};
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
    // Register custom tools from config
    if !config.tools.custom.is_empty() {
        tool_executor = tool_executor.with_custom_tools(&config.tools.custom);
        info!("Registered {} custom tool(s)", config.tools.custom.len());
    }
    let tool_executor = Arc::new(tool_executor);

    // Create context loader
    let context_loader = Arc::new(LocalContextLoader::new());

    // Build agent config with role-based model configuration
    // Start with defaults, then apply config file overrides, then CLI overrides
    let mut agent_config = AgentConfig::default();

    // Apply role-based model settings from config file
    if let Some(model) = config.agent.parse_exploration_model() {
        agent_config = agent_config.with_exploration_model(model);
    }
    if let Some(model) = config.agent.parse_decision_model() {
        agent_config = agent_config.with_decision_model(model);
    }
    if let Some(models) = config.agent.parse_review_models() {
        agent_config = agent_config.with_review_models(models);
    }

    // CLI --model flag overrides decision_model (for backward compatibility)
    // First model from CLI becomes decision model, rest become review models
    if !models.is_empty() {
        agent_config = agent_config.with_decision_model(models[0].clone());
        if models.len() > 1 {
            agent_config = agent_config.with_review_models(models[1..].to_vec());
        }
    }

    // Apply HiL settings from config
    agent_config = agent_config
        .with_max_plan_revisions(config.agent.max_plan_revisions)
        .with_hil_mode(config.agent.parse_hil_mode());

    // Apply consensus level and phase scope from config file
    agent_config = agent_config
        .with_consensus_level(config.agent.parse_consensus_level())
        .with_phase_scope(config.agent.parse_phase_scope());

    // Apply --no-quorum flag
    if cli.no_quorum {
        agent_config = agent_config
            .with_review_models(vec![])
            .with_skip_plan_review();
    }

    // Determine initial consensus level
    // --ensemble flag overrides config file setting
    let initial_level = if cli.ensemble {
        agent_config = agent_config.with_ensemble();
        ConsensusLevel::Ensemble
    } else {
        agent_config.consensus_level
    };

    // No question provided -> Start Agent REPL (default)
    if cli.question.is_none() {
        let mut repl = AgentRepl::new(
            gateway.clone(),
            tool_executor,
            context_loader.clone(),
            agent_config.clone(),
        )
        .with_verbose(cli.verbose > 0)
        .with_cancellation(cancellation_token.clone())
        .with_consensus_level(initial_level);

        // Set moderator from config
        repl = repl.with_moderator(config.council.moderator.clone());

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
        println!("Decision Model: {}", agent_config.decision_model);
        if cli.no_quorum {
            println!("Quorum: Disabled (--no-quorum)");
        } else {
            println!(
                "Review Models: {}",
                agent_config
                    .review_models
                    .iter()
                    .map(|m| m.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        println!();
    }

    // Create human intervention handler for interactive mode
    let human_intervention = Arc::new(InteractiveHumanIntervention::new());

    // Create and run agent with cancellation support
    let use_case = RunAgentUseCase::with_context_loader(gateway, tool_executor, context_loader)
        .with_cancellation(cancellation_token.clone())
        .with_human_intervention(human_intervention);
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
