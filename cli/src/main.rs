//! CLI entrypoint for Copilot Quorum
//!
//! This is the main binary that wires together all layers using
//! dependency injection. Configuration flows through Lua (init.lua + plugins),
//! not TOML.
//!
//! Boot sequence:
//!   Rust defaults → QuorumConfig → Lua engine → init.lua → CLI overrides → DI wiring

use anyhow::Result;
use clap::Parser;
use quorum_application::LlmGateway;
#[cfg(feature = "scripting")]
use quorum_application::ScriptingEnginePort;
use quorum_application::ToolExecutorPort;
use quorum_application::{ConfigAccessorPort, ConfigValue};
use quorum_application::{QuorumConfig, RunAgentUseCase};
use quorum_domain::ConsensusLevel;
use quorum_domain::OutputFormat;
#[cfg(feature = "bedrock")]
use quorum_infrastructure::BedrockProviderAdapter;
use quorum_infrastructure::{
    CopilotLlmGateway, CopilotProviderAdapter, GitHubReferenceResolver, JsonSchemaToolConverter,
    JsonlConversationLogger, LocalContextLoader, LocalToolExecutor,
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
fn generate_session_prefix() -> String {
    let now = chrono::Local::now();
    let pid = std::process::id();
    format!("session-{}-{}", now.format("%Y-%m-%dT%H-%M-%S"), pid)
}

/// Logging initialization result.
struct LoggingOutput {
    _guard: Option<WorkerGuard>,
    conversation_log_path: Option<PathBuf>,
}

/// Initialize multi-layer logging (console + optional file).
fn init_logging(
    verbose: u8,
    log_dir_override: Option<&Path>,
    no_log_file: bool,
    tui_mode: bool,
) -> LoggingOutput {
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

    let log_dir = resolve_log_dir(log_dir_override);
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
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

/// Apply CLI argument overrides on top of Lua-configured QuorumConfig.
///
/// CLI flags take precedence over init.lua settings.
fn apply_cli_overrides(config: &mut QuorumConfig, cli: &Cli) {
    if cli.ensemble {
        config.mode_mut().consensus_level = ConsensusLevel::Ensemble;
    }
    match cli.model.len() {
        0 => {}
        1 => {
            let model = cli.model[0]
                .parse::<quorum_domain::Model>()
                .expect("Invalid model format");
            config.models_mut().decision = model;
            config.models_mut().review = vec![];
        }
        _ => {
            let decision_model = cli.model[0]
                .parse::<quorum_domain::Model>()
                .expect("Invalid model format");
            let review_models = cli.model[1..]
                .iter()
                .map(|m| {
                    m.parse::<quorum_domain::Model>()
                        .expect("Invalid model format")
                })
                .collect();
            config.models_mut().decision = decision_model;
            config.models_mut().review = review_models;
        }
    }

    if cli.no_quorum {
        config.models_mut().review = vec![];
        config.policy_mut().require_plan_review = false;
    }

    if cli.final_review {
        config.policy_mut().require_final_review = true;
    }

    if cli.quiet {
        config
            .config_set("repl.show_progress", ConfigValue::Boolean(false))
            .ok();
    }
    if let Some(output) = cli.output {
        let format: OutputFormat = output.into();
        config
            .config_set("output.format", ConfigValue::String(format.to_string()))
            .ok();
    }
}

/// Build presentation-layer output and repl configs from QuorumConfig.
fn build_presentation_configs(config: &QuorumConfig, cli: &Cli) -> (OutputConfig, ReplConfig) {
    let format = cli
        .output
        .map(OutputFormat::from)
        .unwrap_or(config.output_format());
    let output = OutputConfig::new(format, config.color());
    let repl = ReplConfig::new(
        !cli.quiet && config.show_progress(),
        config.history_file().map(|s| s.to_string()),
    );
    (output, repl)
}

/// Build TUI input config from QuorumConfig.
fn build_tui_input_config(config: &QuorumConfig) -> TuiInputConfig {
    TuiInputConfig {
        max_input_height: config.tui_max_input_height(),
        context_header: config.tui_context_header(),
    }
}

/// Build TUI layout config from QuorumConfig + scripting engine.
fn build_tui_layout_config_from_quorum(
    config: &QuorumConfig,
    #[cfg(feature = "scripting")] scripting_engine: &dyn ScriptingEnginePort,
) -> TuiLayoutConfig {
    let preset = config
        .tui_layout_preset()
        .parse::<LayoutPreset>()
        .unwrap_or_default();

    let layout = TuiLayoutConfig {
        preset,
        flex_threshold: config.tui_flex_threshold(),
        surface_config: Default::default(),
        route_overrides: Vec::new(),
        strategy_presets: std::collections::HashMap::new(),
        custom_presets: std::collections::HashMap::new(),
    };

    // Apply route overrides and custom presets from TUI accessor (set via quorum.tui.* Lua API)
    #[cfg(feature = "scripting")]
    {
        let _ = scripting_engine; // Routes/surfaces are applied via TuiAccessorPort in TuiApp
    }

    layout
}

/// Create the scripting engine (init.lua + plugins), returns engine + provider config.
#[cfg(feature = "scripting")]
fn create_scripting_engine(
    shared_config: Arc<std::sync::Mutex<dyn quorum_application::ConfigAccessorPort>>,
    tui_accessor: Arc<std::sync::Mutex<dyn quorum_application::TuiAccessorPort>>,
) -> Arc<dyn quorum_application::ScriptingEnginePort> {
    match quorum_infrastructure::LuaScriptingEngine::new(shared_config, tui_accessor) {
        Ok(engine) => {
            if let Some(config_dir) = dirs::config_dir() {
                let copilot_config_dir = config_dir.join("copilot-quorum");
                let init_lua = copilot_config_dir.join("init.lua");
                if init_lua.exists() {
                    if let Err(e) = engine.load_script(&init_lua) {
                        eprintln!("Warning: Failed to load {}: {}", init_lua.display(), e);
                    } else {
                        info!("Loaded init.lua from {}", init_lua.display());
                    }
                }

                // Load plugins/*.lua in alphabetical order
                let plugins_dir = copilot_config_dir.join("plugins");
                if plugins_dir.is_dir() {
                    let mut plugin_files: Vec<PathBuf> = std::fs::read_dir(&plugins_dir)
                        .into_iter()
                        .flatten()
                        .filter_map(|entry| entry.ok())
                        .map(|entry| entry.path())
                        .filter(|path| path.extension().is_some_and(|ext| ext == "lua"))
                        .collect();
                    plugin_files.sort();
                    for plugin_path in &plugin_files {
                        if let Err(e) = engine.load_script(plugin_path) {
                            eprintln!(
                                "Warning: Failed to load plugin {}: {}",
                                plugin_path.display(),
                                e
                            );
                        } else {
                            info!("Loaded plugin: {}", plugin_path.display());
                        }
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut cli = Cli::parse();

    // Handle --show-config: print Lua config paths
    if cli.show_config {
        if let Some(config_dir) = dirs::config_dir() {
            let copilot_dir = config_dir.join("copilot-quorum");
            println!("init.lua: {}", copilot_dir.join("init.lua").display());
            println!("plugins:  {}", copilot_dir.join("plugins").display());
        } else {
            println!("Config directory not found");
        }
        return Ok(());
    }

    // 1. Create QuorumConfig with Rust defaults
    let quorum_config = QuorumConfig::default();
    let shared_config = Arc::new(std::sync::Mutex::new(quorum_config));

    // Determine TUI mode before logging init
    let is_tui = cli.question.is_none();

    // Initialize logging
    let logging = init_logging(cli.verbose, cli.log_dir.as_deref(), cli.no_log_file, is_tui);
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

    // Cancellation token for graceful shutdown
    let cancellation_token = CancellationToken::new();
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

    // 2. Set up scripting engine + load init.lua (configures QuorumConfig via Lua)
    let tui_accessor: Arc<std::sync::Mutex<dyn quorum_application::TuiAccessorPort>> = Arc::new(
        std::sync::Mutex::new(quorum_application::TuiAccessorState::with_default_routes()),
    );

    #[cfg(feature = "scripting")]
    let scripting_engine = create_scripting_engine(shared_config.clone(), tui_accessor.clone());
    #[cfg(not(feature = "scripting"))]
    let scripting_engine: Arc<dyn quorum_application::ScriptingEnginePort> =
        Arc::new(quorum_application::NoScriptingEngine);

    // 3. Apply CLI argument overrides (after Lua, so CLI wins)
    {
        let mut config = shared_config.lock().unwrap();
        apply_cli_overrides(&mut config, &cli);
    }

    // 4. Read back Lua-configured values for DI wiring
    let provider_config = {
        let config = shared_config.lock().unwrap();
        config.provider_config().clone()
    };
    // Merge: Lua provider_config from scripting engine (if set) takes precedence
    let provider_config = scripting_engine
        .provider_config()
        .unwrap_or(provider_config);
    let custom_tools = scripting_engine.registered_custom_tools();

    // 5. Build providers
    let copilot = CopilotLlmGateway::new_with_logger(conversation_logger.clone()).await?;
    #[allow(unused_mut)]
    let mut providers: Vec<Arc<dyn ProviderAdapter>> =
        vec![Arc::new(CopilotProviderAdapter::new(copilot))];

    #[cfg(feature = "bedrock")]
    {
        if let Some(bedrock) = BedrockProviderAdapter::try_new(&provider_config.bedrock).await {
            info!("Bedrock provider registered");
            providers.push(Arc::new(bedrock));
        }
    }

    let gateway: Arc<dyn LlmGateway> = Arc::new(RoutingGateway::new(providers, &provider_config));

    // 6. Build tool executor (custom tools from Lua)
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
    if !custom_tools.is_empty() {
        tool_executor = tool_executor.with_custom_tool_defs(&custom_tools);
        info!("Registered {} custom tool(s) from Lua", custom_tools.len());
    }
    let tool_executor: Arc<dyn ToolExecutorPort> = Arc::new(tool_executor);

    let tool_schema: Arc<dyn quorum_application::ToolSchemaPort> =
        Arc::new(JsonSchemaToolConverter);
    let context_loader: Arc<dyn quorum_application::ContextLoaderPort> =
        Arc::new(LocalContextLoader::new());

    // Apply working dir to config
    if let Some(ref dir) = working_dir {
        let mut config = shared_config.lock().unwrap();
        config.execution_mut().working_dir = Some(dir.clone());
    }

    // 7. Branch: TUI or single-request mode
    if is_tui {
        let (_output_config, _repl_config) = {
            let config = shared_config.lock().unwrap();
            build_presentation_configs(&config, &cli)
        };

        let tui_input_config = {
            let config = shared_config.lock().unwrap();
            build_tui_input_config(&config)
        };

        let tui_layout_config = {
            let config = shared_config.lock().unwrap();
            build_tui_layout_config_from_quorum(
                &config,
                #[cfg(feature = "scripting")]
                scripting_engine.as_ref(),
            )
        };

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

    // Single request agent mode
    let request = cli.question.take().unwrap();
    let quorum_config = shared_config.lock().unwrap().clone();

    let (_, repl_config) = build_presentation_configs(&quorum_config, &cli);

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

    let human_intervention = Arc::new(InteractiveHumanIntervention::new());
    let reference_resolver = GitHubReferenceResolver::try_new(working_dir.clone()).await;

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
