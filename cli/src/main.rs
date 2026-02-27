//! CLI entrypoint for Copilot Quorum
//!
//! This is the main binary that wires together all layers using
//! dependency injection. Config conversion logic is centralized here.

use anyhow::{Result, bail};
use clap::Parser;
use quorum_application::ContextLoaderPort;
use quorum_application::ExecutionParams;
use quorum_application::LlmGateway;
#[cfg(feature = "scripting")]
use quorum_application::ScriptingEnginePort;
use quorum_application::ToolExecutorPort;
use quorum_application::{QuorumConfig, RunAgentUseCase};
use quorum_domain::{AgentPolicy, ConsensusLevel, Model, ModelConfig, OutputFormat, SessionMode};
#[cfg(feature = "bedrock")]
use quorum_infrastructure::BedrockProviderAdapter;
use quorum_infrastructure::{
    ConfigLoader, CopilotLlmGateway, CopilotProviderAdapter, FileConfig, GitHubReferenceResolver,
    JsonSchemaToolConverter, JsonlConversationLogger, LocalContextLoader, LocalToolExecutor,
};
use quorum_infrastructure::{ProviderAdapter, RoutingGateway};
use quorum_presentation::{
    AgentProgressReporter, Cli, InteractiveHumanIntervention, LayoutPreset, OutputConfig,
    ReplConfig, TuiApp, TuiInputConfig, TuiLayoutConfig,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Format timestamps using local time (via chrono).
struct LocalTimer;

impl tracing_subscriber::fmt::time::FormatTime for LocalTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        write!(
            w,
            "{}",
            chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z")
        )
    }
}

/// Resolve the log directory path.
///
/// Priority: CLI `--log-dir` → `dirs::data_dir()/copilot-quorum/logs/` → `.copilot-quorum/logs/`
fn resolve_log_dir(override_path: Option<&Path>) -> PathBuf {
    if let Some(path) = override_path {
        return path.to_path_buf();
    }
    if let Some(data_dir) = dirs::data_dir() {
        return data_dir.join("copilot-quorum").join("logs");
    }
    PathBuf::from(".copilot-quorum").join("logs")
}

/// Generate a timestamped session prefix for log files.
///
/// Returns a prefix like `session-2026-02-17T14-30-00-12345` that is shared
/// between the operation log (`.log`) and conversation transcript (`.conversation.jsonl`).
fn generate_session_prefix() -> String {
    let now = chrono::Local::now();
    let pid = std::process::id();
    format!("session-{}-{}", now.format("%Y-%m-%dT%H-%M-%S"), pid)
}

/// Logging initialization result.
struct LoggingOutput {
    /// Guard that must be held to ensure file log flushing.
    _guard: Option<WorkerGuard>,
    /// Path to the conversation JSONL log (if file logging is enabled).
    conversation_log_path: Option<PathBuf>,
}

/// Initialize multi-layer logging (console + optional file).
///
/// Returns a [`LoggingOutput`] containing the worker guard and the path
/// to the conversation JSONL log file.
///
/// When `tui_mode` is true, the console (stderr) layer is disabled to avoid
/// corrupting ratatui's alternate screen. Logs are still written to the file layer.
fn init_logging(
    verbose: u8,
    log_dir_override: Option<&Path>,
    no_log_file: bool,
    tui_mode: bool,
) -> LoggingOutput {
    // Console layer: stderr — disabled in TUI mode to prevent alternate screen corruption.
    // `Option<Layer>` with `None` acts as a no-op layer in tracing_subscriber.
    let console_layer = if tui_mode {
        None
    } else {
        let console_filter = match verbose {
            0 => EnvFilter::new("warn"),
            1 => EnvFilter::new("info"),
            2 => EnvFilter::new("debug"),
            _ => EnvFilter::new("trace"),
        };
        Some(
            fmt::layer()
                .with_timer(LocalTimer)
                .with_target(false)
                .with_writer(std::io::stderr)
                .with_filter(console_filter),
        )
    };

    if no_log_file {
        tracing_subscriber::registry().with(console_layer).init();
        return LoggingOutput {
            _guard: None,
            conversation_log_path: None,
        };
    }

    // File layer: debug by default, trace at -vvv
    let log_dir = resolve_log_dir(log_dir_override);
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        // Fallback: console only
        eprintln!(
            "Warning: Could not create log directory {}: {}",
            log_dir.display(),
            e
        );
        tracing_subscriber::registry().with(console_layer).init();
        return LoggingOutput {
            _guard: None,
            conversation_log_path: None,
        };
    }

    let session_prefix = generate_session_prefix();
    let log_filename = format!("{}.log", session_prefix);
    let conversation_filename = format!("{}.conversation.jsonl", session_prefix);

    let file_appender = tracing_appender::rolling::never(&log_dir, &log_filename);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let file_filter = match verbose {
        0..=2 => EnvFilter::new("debug"),
        _ => EnvFilter::new("trace"),
    };
    let file_layer = fmt::layer()
        .with_timer(LocalTimer)
        .with_ansi(false)
        .with_target(true)
        .with_thread_names(true)
        .with_writer(non_blocking)
        .with_filter(file_filter);

    tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer)
        .init();

    info!("Log file: {}", log_dir.join(&log_filename).display());
    info!(
        "Conversation log: {}",
        log_dir.join(&conversation_filename).display()
    );

    LoggingOutput {
        _guard: Some(guard),
        conversation_log_path: Some(log_dir.join(conversation_filename)),
    }
}

/// Convert FileConfig + CLI args to layer-specific configs
/// This is the single place where FileConfig is translated to application/presentation types
fn build_configs(cli: &Cli, file: &FileConfig) -> (OutputConfig, ReplConfig) {
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

    (output, repl)
}

/// Build `TuiLayoutConfig` from TOML `FileConfig`.
///
/// Parses preset, flex_threshold, route overrides, and surface config.
/// Invalid values fall back to defaults (warnings are already emitted by `validate()`).
fn build_tui_layout_config(config: &FileConfig) -> TuiLayoutConfig {
    use quorum_presentation::tui::content::ContentSlot;
    use quorum_presentation::tui::layout::{
        BorderStyle, RouteOverride, SurfaceConfig, SurfacePosition, parse_route_target,
    };

    let preset = config
        .tui
        .layout
        .preset
        .parse::<LayoutPreset>()
        .unwrap_or_default();

    let flex_threshold = config.tui.layout.flex_threshold;

    // Build route overrides from [tui.routes]
    let mut route_overrides = Vec::new();
    if let Some(ref target) = config.tui.routes.tool_log
        && let Some(surface) = parse_route_target(target)
    {
        route_overrides.push(RouteOverride {
            content: ContentSlot::ToolLog,
            surface,
        });
    }
    if let Some(ref target) = config.tui.routes.notification
        && let Some(surface) = parse_route_target(target)
    {
        route_overrides.push(RouteOverride {
            content: ContentSlot::Notification,
            surface,
        });
    }
    if let Some(ref target) = config.tui.routes.hil_prompt
        && let Some(surface) = parse_route_target(target)
    {
        route_overrides.push(RouteOverride {
            content: ContentSlot::HilPrompt,
            surface,
        });
    }

    // Build surface config from [tui.surfaces.progress_pane]
    let mut surface_config = SurfaceConfig::default();
    if let Some(ref progress) = config.tui.surfaces.progress_pane {
        if let Some(ref pos) = progress.position
            && let Ok(p) = pos.parse::<SurfacePosition>()
        {
            surface_config.position = p;
        }
        if let Some(ref width) = progress.width
            && let Some(pct) = width.strip_suffix('%').and_then(|s| s.parse::<u16>().ok())
        {
            surface_config.width_percent = pct;
        }
        if let Some(ref border) = progress.border
            && let Ok(b) = border.parse::<BorderStyle>()
        {
            surface_config.border = b;
        }
    }

    // Build strategy preset overrides from [tui.layout.strategy]
    let strategy_presets = config
        .tui
        .layout
        .strategy
        .iter()
        .filter_map(|(k, v)| v.parse::<LayoutPreset>().ok().map(|p| (k.clone(), p)))
        .collect();

    TuiLayoutConfig {
        preset,
        flex_threshold,
        surface_config,
        route_overrides,
        strategy_presets,
        custom_presets: std::collections::HashMap::new(),
    }
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

    // Validate configuration (unified: file-level parse + enum + dead section checks)
    let config_issues = config.validate();
    for issue in &config_issues {
        match issue.severity {
            quorum_domain::Severity::Warning => eprintln!("Warning: {}", issue.message),
            quorum_domain::Severity::Error => eprintln!("Error: {}", issue.message),
        }
    }
    if config_issues
        .iter()
        .any(|i| i.severity == quorum_domain::Severity::Error)
    {
        bail!("Invalid configuration");
    }

    // Determine TUI mode before initializing logging so we can suppress console output
    let is_tui = cli.question.is_none();

    // Initialize logging (console + file + conversation JSONL)
    // In TUI mode, console layer is disabled to avoid corrupting the alternate screen
    let logging = init_logging(cli.verbose, cli.log_dir.as_deref(), cli.no_log_file, is_tui);

    // Create conversation logger (JSONL file alongside the operation log)
    let conversation_logger: Arc<dyn quorum_application::ConversationLogger> =
        if let Some(ref path) = logging.conversation_log_path {
            match JsonlConversationLogger::new(path) {
                Some(logger) => Arc::new(logger),
                None => Arc::new(quorum_application::NoConversationLogger),
            }
        } else {
            Arc::new(quorum_application::NoConversationLogger)
        };

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
    let (output_config, repl_config) = build_configs(&cli, &config);

    // Parse models from CLI --model flag (empty if not specified)
    let cli_models: Vec<Model> = cli.model.iter().map(|s| s.parse().unwrap()).collect();

    // === Dependency Injection ===
    // Create infrastructure adapter (Copilot Gateway)
    let copilot = CopilotLlmGateway::new_with_logger(conversation_logger.clone()).await?;
    #[allow(unused_mut)]
    let mut providers: Vec<Arc<dyn ProviderAdapter>> =
        vec![Arc::new(CopilotProviderAdapter::new(copilot))];

    #[cfg(feature = "bedrock")]
    {
        if let Some(bedrock) = BedrockProviderAdapter::try_new(&config.providers.bedrock).await {
            info!("Bedrock provider registered");
            providers.push(Arc::new(bedrock));
        }
    }

    let gateway: Arc<dyn LlmGateway> = Arc::new(RoutingGateway::new(providers, &config.providers));

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
    let tool_executor: Arc<dyn ToolExecutorPort> = Arc::new(tool_executor);

    // Create tool schema converter
    let tool_schema: Arc<dyn quorum_application::ToolSchemaPort> =
        Arc::new(JsonSchemaToolConverter);

    // Create context loader
    let context_loader: Arc<dyn ContextLoaderPort> = Arc::new(LocalContextLoader::new());

    // === Build QuorumConfig from split types ===

    // Build ModelConfig (issues already collected by config.validate() above)
    let mut models = ModelConfig::default();
    if let Some(model) = config.models.parse_exploration().0 {
        models = models.with_exploration(model);
    }
    if let Some(model) = config.models.parse_decision().0 {
        models = models.with_decision(model);
    }
    if let Some(review) = config.models.parse_review().0 {
        models = models.with_review(review);
    }
    if let Some(participants) = config.models.parse_participants().0 {
        models = models.with_participants(participants);
    }
    if let Some(moderator) = config.models.parse_moderator().0 {
        models = models.with_moderator(moderator);
    }
    if let Some(ask) = config.models.parse_ask().0 {
        models = models.with_ask(ask);
    }

    // CLI --model flag overrides config file models (for backward compatibility)
    // First model from CLI becomes decision model, rest become review models
    if !cli_models.is_empty() {
        models = models.with_decision(cli_models[0].clone());
        if cli_models.len() > 1 {
            models = models.with_review(cli_models[1..].to_vec());
        }
    }

    // Build AgentPolicy
    let mut policy = AgentPolicy::default()
        .with_max_plan_revisions(config.agent.max_plan_revisions)
        .with_hil_mode(config.agent.parse_hil_mode().0);

    // Apply --no-quorum flag
    if cli.no_quorum {
        models = models.with_review(vec![]);
        policy = policy.with_require_plan_review(false);
    }

    // Build SessionMode (strategy is also parsed and wired here)
    let (strategy_name, _) = config.agent.parse_strategy();
    let strategy = match strategy_name {
        "debate" => {
            quorum_domain::OrchestrationStrategy::Debate(quorum_domain::DebateConfig::default())
        }
        _ => quorum_domain::OrchestrationStrategy::default(),
    };
    let mut mode = SessionMode::default()
        .with_consensus_level(config.agent.parse_consensus_level().0)
        .with_phase_scope(config.agent.parse_phase_scope().0)
        .with_strategy(strategy);

    // --ensemble flag overrides config file setting
    if cli.ensemble {
        mode = mode.with_consensus_level(ConsensusLevel::Ensemble);
    }

    // Build ExecutionParams
    let (context_budget, budget_issues) = config.context_budget.to_context_budget();
    for issue in &budget_issues {
        eprintln!("Warning: {}", issue.message);
    }
    let mut execution = ExecutionParams::default().with_context_budget(context_budget);

    // Validate configuration combination
    let issues = mode.validate_combination();
    for issue in &issues {
        match issue.severity {
            quorum_domain::Severity::Warning => eprintln!("Warning: {}", issue.message),
            quorum_domain::Severity::Error => eprintln!("Error: {}", issue.message),
        }
    }
    if SessionMode::has_errors(&issues) {
        bail!("Invalid configuration combination");
    }

    // No question provided -> Start TUI (default)
    if cli.question.is_none() {
        if let Some(dir) = &working_dir {
            execution = execution.with_working_dir(dir);
        }
        if cli.final_review {
            policy = policy.with_require_final_review(true);
        }

        let quorum_config = QuorumConfig::new(mode, models, policy, execution)
            .with_output_format(output_config.format)
            .with_color(output_config.color)
            .with_show_progress(repl_config.show_progress)
            .with_history_file(repl_config.history_file.clone());
        let shared_config = Arc::new(std::sync::Mutex::new(quorum_config));
        let tui_accessor: Arc<std::sync::Mutex<dyn quorum_application::TuiAccessorPort>> =
            Arc::new(std::sync::Mutex::new(
                quorum_application::TuiAccessorState::with_default_routes(),
            ));

        // Set up scripting engine (feature-gated)
        #[cfg(feature = "scripting")]
        let scripting_engine: Arc<dyn quorum_application::ScriptingEnginePort> = {
            match quorum_infrastructure::LuaScriptingEngine::new(
                shared_config.clone(),
                tui_accessor.clone(),
            ) {
                Ok(engine) => {
                    // Load init.lua from user config directory
                    if let Some(config_dir) = dirs::config_dir() {
                        let init_lua = config_dir.join("copilot-quorum").join("init.lua");
                        if init_lua.exists() {
                            if let Err(e) = engine.load_script(&init_lua) {
                                eprintln!("Warning: Failed to load {}: {}", init_lua.display(), e);
                            } else {
                                info!("Loaded init.lua from {}", init_lua.display());
                            }
                        }
                    }
                    Arc::new(engine)
                }
                Err(e) => {
                    eprintln!("Warning: Failed to initialize Lua scripting engine: {}", e);
                    Arc::new(quorum_application::NoScriptingEngine)
                }
            }
        };
        #[cfg(not(feature = "scripting"))]
        let scripting_engine: Arc<dyn quorum_application::ScriptingEnginePort> =
            Arc::new(quorum_application::NoScriptingEngine);

        let tui_input_config = TuiInputConfig {
            max_input_height: config.tui.input.max_height,
            context_header: config.tui.input.context_header,
        };

        // Build TUI layout config from TOML
        let tui_layout_config = build_tui_layout_config(&config);

        // Create reference resolver (graceful: None if gh CLI not available)
        let reference_resolver = GitHubReferenceResolver::try_new(working_dir.clone()).await;

        let mut tui_app = TuiApp::new_with_logger(
            gateway.clone(),
            tool_executor.clone(),
            tool_schema.clone(),
            context_loader.clone(),
            shared_config,
            conversation_logger.clone(),
        )
        .with_tui_config(tui_input_config)
        .with_layout_config(tui_layout_config)
        .with_scripting_engine(scripting_engine)
        .with_tui_accessor(tui_accessor);
        if let Some(resolver) = reference_resolver {
            tui_app = tui_app.with_reference_resolver(Arc::new(resolver));
        }
        tui_app.run().await?;
        return Ok(());
    }

    // Question provided -> Single request agent mode
    let request = cli.question.unwrap();

    if let Some(dir) = &working_dir {
        execution = execution.with_working_dir(dir);
    }

    if cli.final_review {
        policy = policy.with_require_final_review(true);
    }

    let quorum_config = QuorumConfig::new(mode, models, policy, execution)
        .with_output_format(output_config.format)
        .with_color(output_config.color)
        .with_show_progress(repl_config.show_progress)
        .with_history_file(repl_config.history_file.clone());

    // Print header
    if repl_config.show_progress {
        println!();
        println!("+============================================================+");
        println!("|           Copilot Quorum - Agent Mode                      |");
        println!("+============================================================+");
        println!();
        println!("Request: {}", request);
        println!("Decision Model: {}", quorum_config.models().decision);
        if cli.no_quorum {
            println!("Quorum: Disabled (--no-quorum)");
        } else {
            println!(
                "Review Models: {}",
                quorum_config
                    .models()
                    .review
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

    // Create reference resolver (graceful: None if gh CLI not available)
    let reference_resolver = GitHubReferenceResolver::try_new(working_dir.clone()).await;

    // Create and run agent with cancellation support
    let mut use_case =
        RunAgentUseCase::with_context_loader(gateway, tool_executor, tool_schema, context_loader)
            .with_cancellation(cancellation_token.clone())
            .with_human_intervention(human_intervention)
            .with_conversation_logger(conversation_logger);
    if let Some(resolver) = reference_resolver {
        use_case = use_case.with_reference_resolver(Arc::new(resolver));
    }
    let input = quorum_config.to_agent_input(request);

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
