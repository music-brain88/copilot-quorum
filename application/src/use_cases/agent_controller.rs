//! Agent Controller
//!
//! Extracts business logic from the REPL into the application layer.
//! Manages command processing, state changes, and use case orchestration.
//! Emits UiEvent messages to a channel for the presentation layer to render.

use crate::config::QuorumConfig;
use crate::ports::agent_progress::AgentProgressNotifier;
use crate::ports::config_accessor::ConfigAccessorPort;
use crate::ports::context_loader::ContextLoaderPort;
use crate::ports::conversation_logger::{
    ConversationEvent, ConversationLogger, NoConversationLogger,
};
use crate::ports::event_publisher::{
    CompositeEventPublisher, ConversationLogEventPublisher, EventPublisher, NoEventPublisher,
    ScriptEventPublisher,
};
use crate::ports::llm_gateway::LlmGateway;
use crate::ports::progress::QuorumProgressAdapter;
use crate::ports::tool_executor::ToolExecutorPort;
use crate::ports::ui_event::{
    AgentErrorEvent, AgentResultEvent, AskResultEvent, ConfigEntry, ConfigSnapshot,
    ContextInitResultEvent, InteractionCompletedEvent, InteractionSpawnedEvent, QuorumResultEvent,
    UiEvent, WelcomeInfo,
};
use crate::use_cases::init_context::{
    InitContextInput, InitContextProgressNotifier, InitContextUseCase,
};
use crate::use_cases::run_agent::RunAgentUseCase;
use crate::use_cases::run_ask::RunAskUseCase;
use crate::use_cases::run_quorum::RunQuorumUseCase;
use crate::use_cases::run_review::{RunReviewInput, RunReviewUseCase};
use quorum_domain::ContextMode;
use quorum_domain::interaction::{
    InteractionForm, InteractionId, InteractionResult, InteractionTree,
};
use quorum_domain::util::truncate_str;
use quorum_domain::{AgentPhase, ConsensusLevel, Model, OutputFormat, PhaseScope, QuorumResult};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::ports::human_intervention::HumanInterventionPort;
use crate::ports::reference_resolver::ReferenceResolverPort;
use crate::ports::scripting_engine::ScriptingEnginePort;
use crate::ports::tool_schema::ToolSchemaPort;
use crate::status_tracker::StatusTracker;

/// Entry in conversation history
#[derive(Debug, Clone)]
struct HistoryEntry {
    /// Interaction form (Agent/Ask/Discuss)
    form: InteractionForm,
    /// User's request
    request: String,
    /// Summary of agent's response
    summary: String,
}

/// Result of handling a command
pub enum CommandAction {
    /// Continue the REPL loop
    Continue,
    /// Exit the REPL
    Exit,
    /// Execute an interaction (Ask, Discuss, or Agent) — caller should spawn to JoinSet
    Execute {
        form: InteractionForm,
        query: String,
    },
}

/// Agent controller managing business logic for the REPL
///
/// This controller lives in the application layer and handles:
/// - Command processing (state changes, config updates)
/// - Use case orchestration (agent execution, quorum discussion, context init)
/// - Emitting UiEvents to a channel for the presentation layer
pub struct AgentController {
    gateway: Arc<dyn LlmGateway>,
    use_case: RunAgentUseCase,
    ask_use_case: RunAskUseCase,
    review_use_case: RunReviewUseCase,
    context_loader: Arc<dyn ContextLoaderPort>,
    config: Arc<Mutex<QuorumConfig>>,
    /// Moderator model for synthesis (if explicitly configured)
    moderator: Option<Model>,
    verbose: bool,
    /// Conversation history for /discuss context
    conversation_history: Vec<HistoryEntry>,
    /// Root cancellation token for graceful shutdown (Ctrl+C).
    /// Per-interaction tokens are derived as children of this one, so a
    /// root cancel stops every interaction while a child cancel stops just one.
    cancellation_token: Option<CancellationToken>,
    /// Per-interaction cancellation tokens (children of `cancellation_token`).
    /// Lets a closed tab cancel only its own agent (issue #282).
    interaction_tokens: HashMap<InteractionId, CancellationToken>,
    /// Channel sender for UI events
    tx: mpsc::UnboundedSender<UiEvent>,
    /// Conversation logger for structured event logging
    conversation_logger: Arc<dyn ConversationLogger>,
    /// Interaction tree for nesting management
    interaction_tree: InteractionTree,
    /// Currently active interaction ID
    active_interaction_id: InteractionId,
    /// Scripting engine for Lua command dispatch
    scripting_engine: Arc<dyn ScriptingEnginePort>,
    /// Aggregates working/blocked/idle across concurrent interactions
    /// (Issue #309). Shared into `RunAgentUseCase` (Blocked, from HiL) and
    /// `SpawnContext` (Working/Idle, from spawn/finalize).
    status_tracker: Arc<StatusTracker>,
    /// Extra `EventPublisher` subscribers injected from DI (e.g. a
    /// supervisor-reporting adapter) — infra-agnostic seam, see
    /// [`Self::with_event_subscriber`].
    extra_event_subscribers: Vec<Arc<dyn EventPublisher>>,
    /// The current composite event publisher, rebuilt whenever the logger,
    /// scripting engine, or extra subscribers change.
    event_publisher: Arc<dyn EventPublisher>,
    /// Human-in-the-loop port, propagated to `RunQuorumUseCase` instances
    /// built for `/discuss` (both inline and `SpawnContext`) so debate
    /// escalation checkpoints can prompt the user (issue #316).
    human_intervention: Arc<dyn HumanInterventionPort>,
}

impl AgentController {
    /// Create a new AgentController
    pub fn new(
        gateway: Arc<dyn LlmGateway>,
        tool_executor: Arc<dyn ToolExecutorPort>,
        tool_schema: Arc<dyn ToolSchemaPort>,
        context_loader: Arc<dyn ContextLoaderPort>,
        config: Arc<Mutex<QuorumConfig>>,
        human_intervention: Arc<dyn HumanInterventionPort>,
        tx: mpsc::UnboundedSender<UiEvent>,
    ) -> Self {
        let conversation_logger: Arc<dyn ConversationLogger> = Arc::new(NoConversationLogger);
        let ask_use_case =
            RunAskUseCase::new(gateway.clone(), tool_executor.clone(), tool_schema.clone())
                .with_conversation_logger(conversation_logger.clone());
        let review_use_case = RunReviewUseCase::new(gateway.clone());

        let mut interaction_tree = InteractionTree::default();
        // Agent form is the default root interaction
        let active_interaction_id = interaction_tree.create_root(InteractionForm::Agent);
        let status_tracker = StatusTracker::new();

        use crate::ports::scripting_engine::NoScriptingEngine;
        Self {
            gateway: gateway.clone(),
            use_case: RunAgentUseCase::with_context_loader(
                gateway,
                tool_executor,
                tool_schema,
                context_loader.clone(),
            )
            .with_human_intervention(human_intervention.clone())
            .with_status_tracker(status_tracker.clone()),
            ask_use_case,
            review_use_case,
            context_loader,
            config,
            moderator: None,
            verbose: false,
            conversation_history: Vec::new(),
            cancellation_token: None,
            interaction_tokens: HashMap::new(),
            tx,
            conversation_logger,
            interaction_tree,
            active_interaction_id,
            scripting_engine: Arc::new(NoScriptingEngine),
            status_tracker,
            extra_event_subscribers: Vec::new(),
            event_publisher: Arc::new(NoEventPublisher),
            human_intervention,
        }
    }

    /// Lock and access the shared config.
    fn config(&self) -> MutexGuard<'_, QuorumConfig> {
        self.config.lock().expect("config lock poisoned")
    }

    /// Get the shared config reference for DI (e.g. passing to LuaScriptingEngine).
    pub fn shared_config(&self) -> Arc<Mutex<QuorumConfig>> {
        Arc::clone(&self.config)
    }

    /// Set a conversation logger for structured event logging.
    pub fn with_conversation_logger(mut self, logger: Arc<dyn ConversationLogger>) -> Self {
        self.conversation_logger = logger.clone();
        self.use_case = self.use_case.with_conversation_logger(logger.clone());
        self.ask_use_case.set_conversation_logger(logger);
        self.rebuild_event_publisher();
        self
    }

    /// Rebuild the typed-event seam from the current logger + scripting
    /// engine + extra subscribers, and store it for `SpawnContext` (Working/
    /// Idle transitions, see [`Self::build_spawn_context`]).
    ///
    /// Subscribers: JSONL conversation log, Lua scripting engine (the latter
    /// no-ops while the engine is unavailable), plus whatever was injected
    /// via [`Self::with_event_subscriber`] (e.g. a supervisor-reporting
    /// adapter — infra-agnostic: this layer never constructs one itself).
    fn rebuild_event_publisher(&mut self) {
        let mut subscribers: Vec<Arc<dyn EventPublisher>> = vec![
            Arc::new(ConversationLogEventPublisher::new(
                self.conversation_logger.clone(),
            )),
            Arc::new(ScriptEventPublisher::new(self.scripting_engine.clone())),
        ];
        subscribers.extend(self.extra_event_subscribers.iter().cloned());
        let publisher: Arc<dyn EventPublisher> =
            Arc::new(CompositeEventPublisher::new(subscribers));
        self.event_publisher = publisher.clone();
        self.use_case = self
            .use_case
            .clone()
            .with_event_publisher(publisher.clone());
        self.review_use_case = self.review_use_case.clone().with_event_publisher(publisher);
    }

    /// Inject an extra `EventPublisher` subscriber (e.g. a supervisor-
    /// reporting adapter built by the DI-assembly layer) and fold it into
    /// the composite. Call before the controller is handed off to a
    /// background task (`TuiApp::new_with_logger` does this at construction
    /// time) — builder methods on a running controller only reach it via
    /// `TuiCommand`, and this one doesn't have one (yet).
    pub fn with_event_subscriber(mut self, subscriber: Arc<dyn EventPublisher>) -> Self {
        self.extra_event_subscribers.push(subscriber);
        self.rebuild_event_publisher();
        self
    }

    /// Set moderator model for synthesis
    pub fn with_moderator(mut self, model: Model) -> Self {
        self.moderator = Some(model);
        self
    }

    /// Enable verbose output
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Set working directory
    pub fn with_working_dir(self, dir: impl Into<String>) -> Self {
        {
            let mut guard = self.config();
            let config = std::mem::take(&mut *guard);
            *guard = config.with_working_dir(dir);
        }
        self
    }

    /// Enable final review
    pub fn with_final_review(self, enable: bool) -> Self {
        if enable {
            let mut guard = self.config();
            let config = std::mem::take(&mut *guard);
            *guard = config.with_final_review();
        }
        self
    }

    /// Set cancellation token for graceful shutdown
    pub fn with_cancellation(mut self, token: CancellationToken) -> Self {
        self.cancellation_token = Some(token.clone());
        self.use_case = self.use_case.with_cancellation(token);
        self
    }

    /// Set initial consensus level (Solo or Ensemble)
    pub fn with_consensus_level(self, level: ConsensusLevel) -> Self {
        {
            let mut guard = self.config();
            let config = std::mem::take(&mut *guard);
            *guard = config.with_consensus_level(level);
        }
        self
    }

    /// Get the current consensus level
    pub fn consensus_level(&self) -> ConsensusLevel {
        self.config().mode().consensus_level
    }

    /// Whether verbose mode is enabled
    pub fn verbose(&self) -> bool {
        self.verbose
    }

    /// Set verbose output dynamically
    pub fn set_verbose(&mut self, verbose: bool) {
        self.verbose = verbose;
    }

    /// Set cancellation token dynamically
    pub fn set_cancellation(&mut self, token: CancellationToken) {
        self.cancellation_token = Some(token.clone());
        self.use_case = self.use_case.clone().with_cancellation(token);
    }

    /// Set reference resolver for automatic reference resolution
    pub fn set_reference_resolver(&mut self, resolver: Arc<dyn ReferenceResolverPort>) {
        self.use_case = self.use_case.clone().with_reference_resolver(resolver);
    }

    /// Set scripting engine for Lua command dispatch
    pub fn set_scripting_engine(&mut self, engine: Arc<dyn ScriptingEnginePort>) {
        self.scripting_engine = engine.clone();
        // Propagate to RunAgentUseCase for ToolCallBefore events
        self.use_case = self.use_case.clone().with_scripting_engine(engine);
        self.rebuild_event_publisher();
    }

    /// Get the scripting engine reference.
    pub fn scripting_engine(&self) -> &Arc<dyn ScriptingEnginePort> {
        &self.scripting_engine
    }

    /// Generate the prompt string for the REPL
    ///
    /// Format: `<level>>`
    /// Examples: `solo>`, `ens>`
    pub fn prompt_string(&self) -> String {
        let level = match self.config().mode().consensus_level {
            ConsensusLevel::Solo => "solo",
            ConsensusLevel::Ensemble => "ens",
        };
        format!("{}> ", level)
    }

    /// Send the welcome event
    pub fn send_welcome(&self) {
        let guard = self.config();
        let moderator = self
            .moderator
            .clone()
            .or_else(|| guard.models().review.first().cloned());

        let _ = self.tx.send(UiEvent::Welcome(WelcomeInfo {
            decision_model: guard.models().decision.clone(),
            review_models: guard.models().review.clone(),
            moderator,
            working_dir: guard.execution().working_dir.clone(),
            consensus_level: guard.mode().consensus_level,
        }));
        drop(guard);

        // Emit InteractionSpawned for the initial root interaction so the TUI
        // can bind its placeholder tab to this interaction ID.  Without this,
        // the initial tab stays PaneKind::Interaction(Agent, None) and all
        // progress events routed by interaction_id are silently dropped.
        let _ = self
            .tx
            .send(UiEvent::InteractionSpawned(InteractionSpawnedEvent {
                id: self.active_interaction_id,
                form: InteractionForm::Agent,
                parent_id: None,
                query: String::new(),
            }));
    }

    /// Handle a slash command. Returns whether to continue or exit the REPL.
    pub async fn handle_command(
        &mut self,
        cmd: &str,
        progress: &dyn AgentProgressNotifier,
    ) -> CommandAction {
        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        let command = parts.first().copied().unwrap_or("");
        let args = parts.get(1).copied().unwrap_or("").trim();

        // Vim-style bang: `:init!` / `:q!` — strip the trailing `!` and pass
        // it as a force flag to commands that support it.
        let (command, bang) = split_bang(command);

        match command {
            "/quit" | "/exit" | "/q" => {
                let _ = self.tx.send(UiEvent::Exit);
                CommandAction::Exit
            }
            "/help" | "/h" | "/?" => {
                let _ = self.tx.send(UiEvent::Help);
                CommandAction::Continue
            }
            "/mode" => {
                self.handle_mode_command(args);
                CommandAction::Continue
            }
            "/solo" => {
                self.config().mode_mut().consensus_level = ConsensusLevel::Solo;
                let _ = self.tx.send(UiEvent::ModeChanged {
                    level: ConsensusLevel::Solo,
                    description: "single model, quick execution".to_string(),
                });
                CommandAction::Continue
            }
            "/ens" | "/ensemble" => {
                self.config().mode_mut().consensus_level = ConsensusLevel::Ensemble;
                let _ = self.tx.send(UiEvent::ModeChanged {
                    level: ConsensusLevel::Ensemble,
                    description: "multi-model ensemble planning".to_string(),
                });
                CommandAction::Continue
            }
            "/fast" => {
                let new_scope = if self.config().mode().phase_scope == PhaseScope::Fast {
                    PhaseScope::Full
                } else {
                    PhaseScope::Fast
                };
                self.config().mode_mut().phase_scope = new_scope;
                let description = match new_scope {
                    PhaseScope::Fast => "reviews will be skipped".to_string(),
                    _ => "all review phases enabled".to_string(),
                };
                let _ = self.tx.send(UiEvent::ScopeChanged {
                    scope: new_scope,
                    description,
                });
                CommandAction::Continue
            }
            "/scope" => {
                self.handle_scope_command(args);
                CommandAction::Continue
            }
            "/strategy" => {
                self.handle_strategy_command(args);
                CommandAction::Continue
            }
            "/ask" => {
                if args.is_empty() {
                    let _ = self.tx.send(UiEvent::CommandError {
                        message: "Usage: /ask <question>".to_string(),
                    });
                    CommandAction::Continue
                } else {
                    CommandAction::Execute {
                        form: InteractionForm::Ask,
                        query: args.to_string(),
                    }
                }
            }
            "/discuss" | "/council" => {
                if args.is_empty() {
                    let _ = self.tx.send(UiEvent::CommandError {
                        message: "Usage: /discuss <question>".to_string(),
                    });
                    CommandAction::Continue
                } else {
                    CommandAction::Execute {
                        form: InteractionForm::Discuss,
                        query: args.to_string(),
                    }
                }
            }
            "/agent" => {
                if args.is_empty() {
                    let _ = self.tx.send(UiEvent::CommandError {
                        message: "Usage: /agent <task>".to_string(),
                    });
                    CommandAction::Continue
                } else {
                    CommandAction::Execute {
                        form: InteractionForm::Agent,
                        query: args.to_string(),
                    }
                }
            }
            "/config" => {
                let guard = self.config();
                let section_filter = if args.is_empty() {
                    None
                } else {
                    Some(args.to_string())
                };
                // Collect all known keys (registry order), optionally narrowed
                // to a section prefix (`:config models`, `:config tui.input`).
                let entries: Vec<ConfigEntry> = guard
                    .config_keys()
                    .into_iter()
                    .filter(|key| match &section_filter {
                        Some(section) => {
                            key == section || key.starts_with(&format!("{}.", section))
                        }
                        None => true,
                    })
                    .filter_map(|key| {
                        let value = guard.config_get(&key).ok()?.to_string();
                        Some(ConfigEntry { key, value })
                    })
                    .collect();
                if entries.is_empty() {
                    let sections = config_sections(&guard.config_keys()).join(", ");
                    drop(guard);
                    let _ = self.tx.send(UiEvent::CommandError {
                        message: format!(
                            "Unknown config section: '{}'. Valid sections: {}",
                            args, sections
                        ),
                    });
                    return CommandAction::Continue;
                }
                let snapshot = ConfigSnapshot {
                    entries,
                    section_filter,
                    working_dir: guard.execution().working_dir.clone(),
                    verbose: self.verbose,
                    history_count: self.conversation_history.len(),
                };
                drop(guard);
                let _ = self.tx.send(UiEvent::ConfigDisplay(snapshot));
                CommandAction::Continue
            }
            "/clear" => {
                self.conversation_history.clear();
                let _ = self.tx.send(UiEvent::HistoryCleared);
                CommandAction::Continue
            }
            "/init" => {
                self.run_init_context(args, bang, progress).await;
                CommandAction::Continue
            }
            "/verbose" => {
                let _ = self.tx.send(UiEvent::VerboseStatus {
                    enabled: self.verbose,
                });
                CommandAction::Continue
            }
            _ => {
                // Check for Lua-registered custom commands
                let cmd_name = command.strip_prefix('/').unwrap_or(command);
                let lua_cmd = self
                    .scripting_engine
                    .registered_commands()
                    .into_iter()
                    .find(|(name, ..)| name == cmd_name);

                if let Some((_name, _desc, _usage, callback_id)) = lua_cmd {
                    if let Err(e) = self
                        .scripting_engine
                        .execute_command_callback(callback_id, args)
                    {
                        let _ = self.tx.send(UiEvent::CommandError {
                            message: format!("Command /{} failed: {}", cmd_name, e),
                        });
                    }
                } else {
                    // Send the bare command name — presenters render their own
                    // prefix convention (`/` for REPL, `:` for TUI).
                    let _ = self.tx.send(UiEvent::UnknownCommand {
                        command: cmd_name.to_string(),
                    });
                }
                CommandAction::Continue
            }
        }
    }

    fn handle_mode_command(&mut self, args: &str) {
        if args.is_empty() {
            let level = self.config().mode().consensus_level;
            let _ = self.tx.send(UiEvent::CommandError {
                message: format!(
                    "Usage: /mode <level>\nAvailable levels: solo, ensemble\nCurrent level: {} ({})",
                    level,
                    level.short_description()
                ),
            });
            return;
        }

        if let Ok(level) = args.parse::<ConsensusLevel>() {
            self.config().mode_mut().consensus_level = level;
            let _ = self.tx.send(UiEvent::ModeChanged {
                level,
                description: level.description().to_string(),
            });
        } else {
            let _ = self.tx.send(UiEvent::CommandError {
                message: format!("Unknown mode: {}\nAvailable levels: solo, ensemble", args),
            });
        }
    }

    fn handle_scope_command(&mut self, args: &str) {
        if args.is_empty() {
            let _ = self.tx.send(UiEvent::CommandError {
                message: format!(
                    "Usage: /scope <scope>\nAvailable scopes: full, fast, plan\nCurrent scope: {}",
                    self.config().mode().phase_scope
                ),
            });
            return;
        }

        if let Ok(scope) = args.parse::<PhaseScope>() {
            self.config().mode_mut().phase_scope = scope;
            let _ = self.tx.send(UiEvent::ScopeChanged {
                scope,
                description: format!("Phase scope changed to: {}", scope),
            });
        } else {
            let _ = self.tx.send(UiEvent::CommandError {
                message: format!(
                    "Unknown scope: {}\nAvailable scopes: full, fast, plan",
                    args
                ),
            });
        }
    }

    fn handle_strategy_command(&mut self, args: &str) {
        if args.is_empty() {
            let _ = self.tx.send(UiEvent::CommandError {
                message: format!(
                    "Usage: /strategy <strategy>\nAvailable strategies: quorum, debate\nCurrent strategy: {}",
                    self.config().mode().strategy
                ),
            });
            return;
        }

        match args.split_whitespace().next().unwrap_or("") {
            "quorum" | "q" => {
                self.config().mode_mut().strategy = quorum_domain::OrchestrationStrategy::default();
                let _ = self.tx.send(UiEvent::StrategyChanged {
                    strategy: "quorum".to_string(),
                    description: "equal discussion + review + synthesis".to_string(),
                });
            }
            "debate" | "d" => {
                self.config().mode_mut().strategy = quorum_domain::OrchestrationStrategy::Debate(
                    quorum_domain::DebateConfig::default(),
                );
                let _ = self.tx.send(UiEvent::StrategyChanged {
                    strategy: "debate".to_string(),
                    description: "adversarial discussion + consensus building".to_string(),
                });
            }
            other => {
                let _ = self.tx.send(UiEvent::CommandError {
                    message: format!(
                        "Unknown strategy: {}\nAvailable strategies: quorum, debate",
                        other
                    ),
                });
            }
        }
    }

    /// Build context string from conversation history
    fn build_context_from_history(&self) -> String {
        if self.conversation_history.is_empty() {
            return String::new();
        }

        let mut context = String::from("## Previous Conversation Context\n\n");
        for (i, entry) in self.conversation_history.iter().enumerate() {
            context.push_str(&format!(
                "### Exchange {}\n**User**: {}\n**Agent Summary**: {}\n\n",
                i + 1,
                entry.request,
                entry.summary
            ));
        }
        context
    }

    /// Run Ask interaction — lightweight Q&A with read-only tool access (inline, no new tab)
    pub async fn run_ask(&mut self, question: &str, progress: &dyn AgentProgressNotifier) {
        let (clean_query, full_query) = self.prepare_inline(question);
        let context = self.build_spawn_context();
        let completion = context
            .execute(
                None,
                InteractionForm::Ask,
                clean_query,
                full_query,
                progress,
            )
            .await;
        self.finalize(completion);
    }

    /// Run Quorum Discussion with conversation context (inline, no new tab)
    pub async fn run_discuss(&mut self, question: &str, progress: &dyn AgentProgressNotifier) {
        let (clean_query, full_query) = self.prepare_inline(question);
        let context = self.build_spawn_context();
        let completion = context
            .execute(
                None,
                InteractionForm::Discuss,
                clean_query,
                full_query,
                progress,
            )
            .await;
        self.finalize(completion);
    }

    /// Run context initialization
    ///
    /// `bang` は Vim スタイルの強制フラグ（`:init!`）。`--force` / `-f` と等価。
    pub async fn run_init_context(
        &self,
        args: &str,
        bang: bool,
        progress: &dyn AgentProgressNotifier,
    ) {
        let force = bang || args.contains("--force") || args.contains("-f");

        let working_dir = self
            .config()
            .execution()
            .working_dir
            .clone()
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| ".".to_string())
            });

        // Check if context file already exists
        if !force
            && self
                .context_loader
                .context_file_exists(Path::new(&working_dir))
        {
            let _ = self.tx.send(UiEvent::ContextAlreadyExists);
            return;
        }

        let _ = self.tx.send(UiEvent::ContextInitStarting {
            model_count: self.config().models().review.len(),
        });

        // Create the init context input using review models
        let mut input = InitContextInput::new(&working_dir, self.config().models().review.clone());

        if let Some(moderator) = self.config().models().review.first() {
            input = input.with_moderator(moderator.clone());
        }

        if force {
            input = input.with_force(true);
        }

        // Run the initialization, bridging progress to both the Progress pane
        // (via AgentProgressNotifier) and the conversation log (via UiEvent)
        let bridge = InitContextProgressBridge {
            progress,
            tx: self.tx.clone(),
        };
        let use_case = InitContextUseCase::new(self.gateway.clone(), self.context_loader.clone());
        let result = use_case.execute_with_progress(input, &bridge).await;

        match result {
            Ok(output) => {
                let _ = self
                    .tx
                    .send(UiEvent::ContextInitResult(ContextInitResultEvent {
                        path: output.path,
                        content: output.content,
                        contributing_models: output.contributing_models,
                    }));
            }
            Err(e) => {
                let _ = self.tx.send(UiEvent::ContextInitError {
                    error: e.to_string(),
                });
            }
        }
    }

    /// Process a user request (run agent, inline in current tab)
    pub async fn process_request(&mut self, request: &str, progress: &dyn AgentProgressNotifier) {
        let (clean_query, full_query) = self.prepare_inline(request);
        let context = self.build_spawn_context();
        let completion = context
            .execute(
                None,
                InteractionForm::Agent,
                clean_query,
                full_query,
                progress,
            )
            .await;
        self.finalize(completion);
    }

    // =========================================================================
    // Interaction Nesting
    // =========================================================================

    /// Set the currently active interaction ID
    pub fn set_active_interaction(&mut self, id: InteractionId) {
        self.active_interaction_id = id;
    }

    /// Create (or replace) a cancellation token bound to an interaction, derived
    /// as a child of the root token. Returns `None` when no root token is set
    /// (cancellation simply isn't wired), in which case the agent runs uncancelled.
    ///
    /// The returned token is applied to that interaction's execution so a later
    /// [`Self::cancel_interaction`] (fired when the owning tab closes) stops just
    /// that agent — while a root cancel (Ctrl+C) still stops every interaction.
    fn bind_cancellation(&mut self, id: InteractionId) -> Option<CancellationToken> {
        let child = self.cancellation_token.as_ref()?.child_token();
        self.interaction_tokens.insert(id, child.clone());
        Some(child)
    }

    /// Cancel the in-flight interaction bound to `id` (its tab was closed).
    /// No-op if the interaction has no live token. See issue #282.
    pub fn cancel_interaction(&mut self, id: InteractionId) {
        if let Some(token) = self.interaction_tokens.remove(&id) {
            token.cancel();
        }
    }

    /// Prepare context for an inline execution (no tree node).
    ///
    /// Returns (clean_query, full_query) where:
    /// - clean_query: user query with flags stripped (for Agent which has its own context loader)
    /// - full_query: query with conversation history prepended (for Ask/Discuss)
    pub fn prepare_inline(&self, query: &str) -> (String, String) {
        let (ctx_override_flag, clean_query) = Self::parse_spawn_flags(query);
        let context_mode = ctx_override_flag.unwrap_or(ContextMode::Full);
        let context = match context_mode {
            ContextMode::Full => self.build_context_from_history(),
            ContextMode::Projected => self.build_projected_context(),
            ContextMode::Fresh => String::new(),
        };
        let full_query = if context.is_empty() {
            clean_query.clone()
        } else {
            format!("{}\n\n## Current Question\n\n{}", context, clean_query)
        };
        (clean_query, full_query)
    }

    /// Spawn a new interaction (Ask, Discuss, or Agent).
    /// Prepare a spawn operation (Phase 1: synchronous setup)
    ///
    /// Creates the interaction node, sends the spawn event, and builds the context.
    /// Returns the data needed to execute the spawn in a separate task.
    pub fn prepare_spawn(
        &mut self,
        form: InteractionForm,
        query: &str,
        context_mode_override: Option<ContextMode>,
    ) -> Result<(InteractionId, String, String), String> {
        let (ctx_override_flag, clean_query) = Self::parse_spawn_flags(query);
        let ctx_override = context_mode_override.or(ctx_override_flag);

        // 1. Add node to InteractionTree
        let child_res = match ctx_override {
            Some(mode) => self.interaction_tree.spawn_child_with_context(
                self.active_interaction_id,
                form,
                mode,
            ),
            None => self
                .interaction_tree
                .spawn_child(self.active_interaction_id, form),
        };

        let child_id = match child_res {
            Ok(c) => c,
            Err(e) => {
                let _ = self.tx.send(UiEvent::InteractionSpawnError {
                    error: e.to_string(),
                });
                return Err(e.to_string());
            }
        };

        let (parent_id, context_mode) = {
            let interaction = self.interaction_tree.get(child_id).unwrap();
            (interaction.parent, interaction.context_mode)
        };

        // 2. Send UiEvent::InteractionSpawned
        let _ = self
            .tx
            .send(UiEvent::InteractionSpawned(InteractionSpawnedEvent {
                id: child_id,
                form,
                parent_id,
                query: clean_query.clone(),
            }));

        // 3. Log spawn to ConversationLogger
        self.conversation_logger.log(ConversationEvent::new(
            "interaction_spawned",
            serde_json::json!({
                "id": child_id.0,
                "form": form.as_str(),
                "parent_id": parent_id.map(|id| id.0),
                "context_mode": format!("{:?}", context_mode),
            }),
        ));

        // 4. Build context based on ContextMode
        let context = match context_mode {
            ContextMode::Full => self.build_context_from_history(),
            ContextMode::Projected => self.build_projected_context(),
            ContextMode::Fresh => String::new(),
        };
        let full_query = if context.is_empty() {
            clean_query.clone()
        } else {
            format!("{}\n\n## Current Question\n\n{}", context, clean_query)
        };

        Ok((child_id, clean_query, full_query))
    }

    /// Prepare a root-level spawn (no parent, `context_mode` = `Fresh`) —
    /// used by headless entry points (e.g. #300's `review` subcommand) that
    /// start a brand new interaction rather than nesting under whichever
    /// interaction happens to be active. Unlike [`Self::prepare_spawn`],
    /// creating a root always succeeds (no depth limit to hit), and there is
    /// no conversation-history context to build — the caller supplies
    /// everything the models need via `material`.
    ///
    /// `label` is a short human-readable string used for the tab title,
    /// conversation echo, and history entries; `material` is the full prompt
    /// material sent to the models (e.g. diff + PR context for Review).
    pub fn prepare_root_spawn(
        &mut self,
        form: InteractionForm,
        label: impl Into<String>,
        material: impl Into<String>,
    ) -> (InteractionId, String, String) {
        let label = label.into();
        let material = material.into();
        let id = self.interaction_tree.create_root(form);

        let _ = self
            .tx
            .send(UiEvent::InteractionSpawned(InteractionSpawnedEvent {
                id,
                form,
                parent_id: None,
                query: label.clone(),
            }));

        self.conversation_logger.log(ConversationEvent::new(
            "interaction_spawned",
            serde_json::json!({
                "id": id.0,
                "form": form.as_str(),
                "parent_id": None::<usize>,
                "context_mode": format!("{:?}", form.default_context_mode()),
            }),
        ));

        (id, label, material)
    }

    /// Build a context object for executing a spawn in a background task
    pub fn build_spawn_context(&self) -> SpawnContext {
        SpawnContext {
            gateway: self.gateway.clone(),
            agent_use_case: self.use_case.clone(),
            ask_use_case: self.ask_use_case.clone(),
            review_use_case: self.review_use_case.clone(),
            config: self.config().clone(),
            tx: self.tx.clone(),
            verbose: self.verbose,
            scripting_engine: self.scripting_engine.clone(),
            status_tracker: self.status_tracker.clone(),
            event_publisher: self.event_publisher.clone(),
            human_intervention: self.human_intervention.clone(),
        }
    }

    /// Build a spawn context whose agent execution is cancellable per-interaction.
    ///
    /// Binds a fresh child cancellation token to `id` and applies it to the
    /// returned context, so closing the tab that owns `id` cancels just this
    /// agent (issue #282). Use this instead of [`Self::build_spawn_context`]
    /// for any execution bound to a tab.
    pub fn build_spawn_context_for(&mut self, id: InteractionId) -> SpawnContext {
        let token = self.bind_cancellation(id);
        self.build_spawn_context().with_cancellation(token)
    }

    /// Finalize a completed task (spawn or inline).
    ///
    /// Updates conversation history. For spawn tasks (interaction_id is Some),
    /// also emits InteractionCompleted event.
    pub fn finalize(&mut self, completion: TaskCompletion) {
        // Drop the per-interaction cancellation token now that the spawn is done.
        // (Inline executions carry no id here; their token is replaced on the
        // interaction's next request, so the map stays bounded either way.)
        if let Some(id) = completion.interaction_id {
            self.interaction_tokens.remove(&id);
        }
        if let Some(result) = &completion.result {
            self.conversation_history.push(HistoryEntry {
                form: completion.form,
                request: completion.query.clone(),
                summary: truncate_str(&result.to_context_injection(), 200).to_string(),
            });
        }
        // Spawn path: emit InteractionCompleted with a query-aware notification
        // so the parent tab shows what the result is answering (issue #274).
        if let Some(child_id) = completion.interaction_id
            && let Some(result) = &completion.result
        {
            let parent_id = self.interaction_tree.get(child_id).and_then(|i| i.parent);
            let _ = self
                .tx
                .send(UiEvent::InteractionCompleted(InteractionCompletedEvent {
                    id: child_id,
                    form: completion.form,
                    parent_id,
                    result_text: result.to_parent_notification(&completion.query),
                    result: Some(result.clone()),
                }));
        }
    }

    pub async fn spawn_interaction(
        &mut self,
        form: InteractionForm,
        query: &str,
        context_mode_override: Option<ContextMode>,
        progress: &dyn AgentProgressNotifier,
    ) {
        // Backward compatibility wrapper using new split methods
        if let Ok((child_id, clean_query, full_query)) =
            self.prepare_spawn(form, query, context_mode_override)
        {
            let context = self.build_spawn_context();
            let completion = context
                .execute(Some(child_id), form, clean_query, full_query, progress)
                .await;
            self.finalize(completion);
        }
    }

    fn parse_spawn_flags(input: &str) -> (Option<ContextMode>, String) {
        let trimmed = input.trim();
        if let Some(rest) = trimmed.strip_prefix("--fresh ") {
            (Some(ContextMode::Fresh), rest.trim().to_string())
        } else if let Some(rest) = trimmed.strip_prefix("--full ") {
            (Some(ContextMode::Full), rest.trim().to_string())
        } else if let Some(rest) = trimmed.strip_prefix("--projected ") {
            (Some(ContextMode::Projected), rest.trim().to_string())
        } else {
            (None, trimmed.to_string())
        }
    }

    fn build_projected_context(&self) -> String {
        // Summary of only the 3 most recent entries
        let recent: Vec<_> = self
            .conversation_history
            .iter()
            .rev()
            .take(3)
            .rev()
            .collect();
        if recent.is_empty() {
            return String::new();
        }
        let mut ctx = String::from("## Recent Context\n\n");
        for entry in recent {
            ctx.push_str(&format!(
                "- [{}] {}: {}\n",
                entry.form, entry.request, entry.summary
            ));
        }
        ctx
    }

    /// Get the active interaction ID
    pub fn active_interaction_id(&self) -> InteractionId {
        self.active_interaction_id
    }
}

/// Format quorum output based on output format
///
/// This is a helper that replaces the ConsoleFormatter usage from presentation.
/// In the future, this could be moved to a domain service.
/// Split a Vim-style bang off a command name.
///
/// `"/init!"` → `("/init", true)`, `"/q!"` → `("/q", true)`, `"/init"` → `("/init", false)`.
/// A bare `"/"` or `"!"` is returned unchanged.
fn split_bang(command: &str) -> (&str, bool) {
    match command.strip_suffix('!') {
        Some(base) if !base.is_empty() && base != "/" => (base, true),
        _ => (command, false),
    }
}

/// Collect unique config sections (key prefix before the last `.`) in registry order.
///
/// `["agent.hil_mode", "tui.input.submit_key"]` → `["agent", "tui.input"]`
fn config_sections(keys: &[String]) -> Vec<String> {
    let mut sections: Vec<String> = Vec::new();
    for key in keys {
        let section = key.rsplit_once('.').map(|(s, _)| s).unwrap_or("");
        if sections.last().map(String::as_str) != Some(section) {
            sections.push(section.to_string());
        }
    }
    sections
}

/// Bridges [`InitContextProgressNotifier`] callbacks to the generic
/// [`AgentProgressNotifier`] (Progress pane: phase + quorum vote bar) and the
/// [`UiEvent`] channel (conversation log lines).
struct InitContextProgressBridge<'a> {
    progress: &'a dyn AgentProgressNotifier,
    tx: mpsc::UnboundedSender<UiEvent>,
}

impl InitContextProgressBridge<'_> {
    fn log(&self, message: impl Into<String>) {
        let _ = self.tx.send(UiEvent::ContextInitProgress {
            message: message.into(),
        });
    }
}

impl InitContextProgressNotifier for InitContextProgressBridge<'_> {
    fn on_loading_files(&self) {
        self.progress.on_phase_change(&AgentPhase::ContextGathering);
        self.log("Loading project files...");
    }

    fn on_analysis_start(&self, model_count: usize) {
        self.progress
            .on_quorum_start("Context Analysis", model_count);
        self.log(format!("Analyzing project with {} models...", model_count));
    }

    fn on_model_complete(&self, model: &Model) {
        self.progress.on_quorum_model_complete(model, true);
        self.log(format!("✓ {} analysis complete", model));
    }

    fn on_model_failed(&self, model: &Model, error: &str) {
        self.progress.on_quorum_model_complete(model, false);
        self.log(format!(
            "✗ {} unavailable — skipped ({}). Check models.review in init.lua.",
            model, error
        ));
    }

    fn on_synthesis_start(&self) {
        self.progress.on_quorum_start("Context Synthesis", 1);
        self.log("Synthesizing analyses...");
    }

    fn on_complete(&self, _path: &str) {
        self.progress.on_phase_change(&AgentPhase::Completed);
    }
}

fn format_quorum_output(result: &QuorumResult, format: OutputFormat) -> String {
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(result).unwrap_or_default(),
        OutputFormat::Full | OutputFormat::Synthesis => result.synthesis.conclusion.clone(),
    }
}

/// Context for executing a spawn in a background task
pub struct SpawnContext {
    pub(crate) gateway: Arc<dyn LlmGateway>,
    pub(crate) agent_use_case: RunAgentUseCase,
    pub(crate) ask_use_case: RunAskUseCase,
    pub(crate) review_use_case: RunReviewUseCase,
    pub(crate) config: QuorumConfig,
    pub(crate) tx: mpsc::UnboundedSender<UiEvent>,
    pub(crate) verbose: bool,
    pub(crate) scripting_engine: Arc<dyn ScriptingEnginePort>,
    pub(crate) status_tracker: Arc<StatusTracker>,
    pub(crate) event_publisher: Arc<dyn EventPublisher>,
    pub(crate) human_intervention: Arc<dyn HumanInterventionPort>,
}

/// Completion result of a task (spawn or inline execution)
pub struct TaskCompletion {
    /// Some(id) for spawn (emits InteractionCompleted), None for inline (history only)
    pub interaction_id: Option<InteractionId>,
    pub form: InteractionForm,
    pub query: String,
    pub result: Option<InteractionResult>,
}

impl SpawnContext {
    /// Override the agent use case's cancellation token for this execution.
    ///
    /// `None` leaves the inherited (root) token in place. Passing a per-interaction
    /// child token makes this execution cancellable independently (issue #282).
    pub fn with_cancellation(mut self, token: Option<CancellationToken>) -> Self {
        if let Some(token) = token {
            self.agent_use_case = self.agent_use_case.with_cancellation(token);
        }
        self
    }

    pub async fn execute(
        self,
        interaction_id: Option<InteractionId>,
        form: InteractionForm,
        clean_query: String,
        full_query: String,
        progress: &dyn AgentProgressNotifier,
    ) -> TaskCompletion {
        // Working for the duration of this call (any form) — dropped at the
        // end of this function (all return paths), so a cancelled or
        // panicking execution still reverts to Idle (Issue #309).
        let _working_guard = self
            .status_tracker
            .enter_working(self.event_publisher.clone());

        // Wrap progress with ScriptProgressBridge via CompositeProgress
        use crate::ports::composite_progress::CompositeProgressNotifier;
        use crate::ports::script_progress_bridge::ScriptProgressBridge;

        let script_bridge = ScriptProgressBridge::new(self.scripting_engine.clone());
        let composite = CompositeProgressNotifier::new(vec![progress, &script_bridge]);
        let progress: &dyn AgentProgressNotifier = &composite;

        let result = match form {
            InteractionForm::Ask => self.execute_ask(&full_query, progress).await,
            InteractionForm::Discuss => self.execute_discuss(&full_query, progress).await,
            InteractionForm::Agent => self.execute_agent(&clean_query, progress).await,
            InteractionForm::Review => self.execute_review(&full_query, progress).await,
        };

        TaskCompletion {
            interaction_id,
            form,
            query: clean_query,
            result,
        }
    }

    async fn execute_ask(
        &self,
        query: &str,
        progress: &dyn AgentProgressNotifier,
    ) -> Option<InteractionResult> {
        let _ = self.tx.send(UiEvent::AskStarting);
        let input = crate::use_cases::run_ask::RunAskInput::new(
            query,
            self.config.models().clone(),
            self.config.execution().clone(),
        );

        match self.ask_use_case.execute(input, progress).await {
            Ok(result) => {
                if let InteractionResult::AskResult { ref answer } = result {
                    let _ = self.tx.send(UiEvent::AskResult(AskResultEvent {
                        answer: answer.clone(),
                    }));
                }
                Some(result)
            }
            Err(e) => {
                let _ = self.tx.send(UiEvent::AskError {
                    error: e.to_string(),
                });
                None
            }
        }
    }

    async fn execute_discuss(
        &self,
        query: &str,
        progress: &dyn AgentProgressNotifier,
    ) -> Option<InteractionResult> {
        let _ = self.tx.send(UiEvent::QuorumStarting);
        let input = self.config.to_quorum_input(query.to_string());
        let use_case = RunQuorumUseCase::new(self.gateway.clone())
            .with_event_publisher(self.event_publisher.clone())
            .with_human_intervention(self.human_intervention.clone());
        let adapter = QuorumProgressAdapter::new(progress);
        match use_case.execute_with_progress(input, &adapter).await {
            Ok(output) => {
                let formatted = format_quorum_output(&output, OutputFormat::Synthesis);
                let _ = self.tx.send(UiEvent::QuorumResult(QuorumResultEvent {
                    formatted_output: formatted.clone(),
                    output_format: OutputFormat::Synthesis,
                }));
                Some(InteractionResult::DiscussResult {
                    synthesis: formatted,
                    participant_count: output.models.len(),
                })
            }
            Err(e) => {
                let _ = self.tx.send(UiEvent::QuorumError {
                    error: e.to_string(),
                });
                None
            }
        }
    }

    async fn execute_agent(
        &self,
        query: &str,
        progress: &dyn AgentProgressNotifier,
    ) -> Option<InteractionResult> {
        let _ = self.tx.send(UiEvent::AgentStarting {
            mode: self.config.mode().consensus_level,
        });

        // Use the factory method from QuorumConfig
        let input = self.config.to_agent_input(query);

        match self
            .agent_use_case
            .execute_with_progress(input, progress)
            .await
        {
            Ok(output) => {
                let _ = self
                    .tx
                    .send(UiEvent::AgentResult(Box::new(AgentResultEvent {
                        success: output.success,
                        summary: output.summary.clone(),
                        state: output.state.clone(),
                        verbose: self.verbose,
                        thoughts: output.state.thoughts.clone(),
                    })));
                Some(InteractionResult::AgentResult {
                    summary: output.summary,
                    success: output.success,
                })
            }
            Err(e) => {
                let cancelled = e.is_cancelled();
                let _ = self.tx.send(UiEvent::AgentError(AgentErrorEvent {
                    error: e.to_string(),
                    cancelled,
                }));
                None
            }
        }
    }

    /// Execute a Review interaction (#300). `material` is the full review
    /// material built by the caller (diff + optional PR/focus context, see
    /// `ReviewPromptTemplate::build_material`) — Review has no conversation
    /// history to fold in, so unlike Ask/Discuss it is passed through as-is.
    async fn execute_review(
        &self,
        material: &str,
        progress: &dyn AgentProgressNotifier,
    ) -> Option<InteractionResult> {
        let input = RunReviewInput::new(material, self.config.models().clone());

        match self.review_use_case.execute(input, progress).await {
            Ok(output) => Some(InteractionResult::ReviewResult {
                approved: output.approved,
                votes: output.votes,
                synthesis: output.synthesis,
            }),
            Err(e) => {
                let _ = self.tx.send(UiEvent::ReviewError {
                    error: e.to_string(),
                });
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_bang() {
        assert_eq!(split_bang("/init!"), ("/init", true));
        assert_eq!(split_bang("/q!"), ("/q", true));
        assert_eq!(split_bang("/init"), ("/init", false));
        assert_eq!(split_bang("/quit"), ("/quit", false));
        // degenerate inputs stay unchanged
        assert_eq!(split_bang("!"), ("!", false));
        assert_eq!(split_bang("/!"), ("/!", false));
        assert_eq!(split_bang(""), ("", false));
    }
    use crate::ports::agent_progress::NoAgentProgress;
    use crate::ports::context_loader::ContextLoaderPort;
    use crate::ports::human_intervention::{HumanInterventionError, HumanInterventionPort};
    use crate::ports::llm_gateway::{GatewayError, LlmGateway, LlmSession};
    use crate::ports::tool_executor::ToolExecutorPort;
    use crate::ports::tool_schema::ToolSchemaPort;
    use async_trait::async_trait;
    use quorum_domain::{
        HumanDecision, LlmResponse, LoadedContextFile, Model, Plan, ReviewRound, ToolCall,
        ToolDefinition, ToolResult, ToolSpec,
    };
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::Mutex;

    // === Mock implementations ===

    struct MockGateway {
        sessions: Mutex<VecDeque<Box<dyn LlmSession>>>,
    }

    impl MockGateway {
        fn new(sessions: Vec<Box<dyn LlmSession>>) -> Self {
            Self {
                sessions: Mutex::new(VecDeque::from(sessions)),
            }
        }
    }

    #[async_trait]
    impl LlmGateway for MockGateway {
        async fn create_session(
            &self,
            _model: &Model,
        ) -> Result<Box<dyn LlmSession>, GatewayError> {
            self.sessions
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| GatewayError::Other("No more sessions".to_string()))
        }

        async fn create_session_with_system_prompt(
            &self,
            _model: &Model,
            _system_prompt: &str,
        ) -> Result<Box<dyn LlmSession>, GatewayError> {
            self.create_session(_model).await
        }

        async fn available_models(&self) -> Result<Vec<Model>, GatewayError> {
            Ok(vec![Model::default()])
        }
    }

    struct MockSession(Model);

    #[async_trait]
    impl LlmSession for MockSession {
        fn model(&self) -> &Model {
            &self.0
        }

        async fn send(&self, _content: &str) -> Result<String, GatewayError> {
            Ok("mock response".to_string())
        }

        async fn send_with_tools(
            &self,
            content: &str,
            _tools: &[serde_json::Value],
        ) -> Result<LlmResponse, GatewayError> {
            let text = self.send(content).await?;
            Ok(LlmResponse::from_text(text))
        }
    }

    struct MockToolExecutor {
        spec: ToolSpec,
    }

    impl MockToolExecutor {
        fn new() -> Self {
            Self {
                spec: ToolSpec::new(),
            }
        }
    }

    #[async_trait]
    impl ToolExecutorPort for MockToolExecutor {
        fn tool_spec(&self) -> &ToolSpec {
            &self.spec
        }

        async fn execute(&self, _call: &ToolCall) -> ToolResult {
            ToolResult::success("mock-tool", "mock output")
        }

        fn execute_sync(&self, _call: &ToolCall) -> ToolResult {
            ToolResult::success("mock-tool", "mock output")
        }
    }

    struct MockContextLoader;

    impl ContextLoaderPort for MockContextLoader {
        fn load_known_files(&self, _project_root: &Path) -> Vec<LoadedContextFile> {
            vec![]
        }

        fn context_file_exists(&self, _project_root: &Path) -> bool {
            false
        }

        fn write_context_file(&self, _project_root: &Path, _content: &str) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct MockHumanIntervention;

    #[async_trait]
    impl HumanInterventionPort for MockHumanIntervention {
        async fn request_intervention(
            &self,
            _request: &str,
            _plan: &Plan,
            _review_history: &[ReviewRound],
        ) -> Result<HumanDecision, HumanInterventionError> {
            Ok(HumanDecision::Approve)
        }
    }

    struct MockToolSchema;

    impl ToolSchemaPort for MockToolSchema {
        fn tool_to_schema(&self, tool: &ToolDefinition) -> serde_json::Value {
            serde_json::json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": { "type": "object", "properties": {}, "required": [] }
            })
        }

        fn all_tools_schema(&self, spec: &ToolSpec) -> Vec<serde_json::Value> {
            let mut tools: Vec<_> = spec.all().collect();
            tools.sort_by_key(|t| &t.name);
            tools.into_iter().map(|t| self.tool_to_schema(t)).collect()
        }

        fn low_risk_tools_schema(&self, spec: &ToolSpec) -> Vec<serde_json::Value> {
            let mut tools: Vec<_> = spec.low_risk_tools().collect();
            tools.sort_by_key(|t| &t.name);
            tools.into_iter().map(|t| self.tool_to_schema(t)).collect()
        }
    }

    fn create_test_controller() -> (AgentController, mpsc::UnboundedReceiver<UiEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let gateway = Arc::new(MockGateway::new(vec![Box::new(MockSession(
            Model::default(),
        ))]));
        let tool_executor = Arc::new(MockToolExecutor::new());
        let tool_schema: Arc<dyn ToolSchemaPort> = Arc::new(MockToolSchema);
        let context_loader = Arc::new(MockContextLoader);
        let human_intervention = Arc::new(MockHumanIntervention);
        let config = Arc::new(Mutex::new(QuorumConfig::default()));

        let controller = AgentController::new(
            gateway,
            tool_executor,
            tool_schema,
            context_loader,
            config,
            human_intervention,
            tx,
        );
        (controller, rx)
    }

    #[test]
    fn init_bridge_surfaces_model_failure_to_ui() {
        // Regression for #262: a failed model in `/init` must reach the UI
        // conversation log, not only the WARN tracing log.
        let (tx, mut rx) = mpsc::unbounded_channel();
        let progress = NoAgentProgress;
        let bridge = InitContextProgressBridge {
            progress: &progress,
            tx,
        };

        bridge.on_model_failed(&Model::Gpt52Codex, "Model not available");

        match rx.try_recv() {
            Ok(UiEvent::ContextInitProgress { message }) => {
                assert!(message.contains("gpt-5.2-codex"), "message: {message}");
                assert!(
                    message.contains("Model not available"),
                    "message: {message}"
                );
            }
            other => panic!("expected ContextInitProgress, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_solo_command_sends_mode_changed_event() {
        let (mut controller, mut rx) = create_test_controller();
        let action = controller.handle_command("/solo", &NoAgentProgress).await;

        assert!(matches!(action, CommandAction::Continue));
        assert_eq!(controller.consensus_level(), ConsensusLevel::Solo);

        let event = rx.try_recv().unwrap();
        assert!(matches!(
            event,
            UiEvent::ModeChanged {
                level: ConsensusLevel::Solo,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn test_ens_command_sends_mode_changed_event() {
        let (mut controller, mut rx) = create_test_controller();
        let action = controller.handle_command("/ens", &NoAgentProgress).await;

        assert!(matches!(action, CommandAction::Continue));
        assert_eq!(controller.consensus_level(), ConsensusLevel::Ensemble);

        let event = rx.try_recv().unwrap();
        assert!(matches!(
            event,
            UiEvent::ModeChanged {
                level: ConsensusLevel::Ensemble,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn test_fast_toggle() {
        let (mut controller, mut rx) = create_test_controller();

        // Default is Full, toggle to Fast
        controller.handle_command("/fast", &NoAgentProgress).await;
        let event = rx.try_recv().unwrap();
        assert!(matches!(
            event,
            UiEvent::ScopeChanged {
                scope: PhaseScope::Fast,
                ..
            }
        ));

        // Toggle back to Full
        controller.handle_command("/fast", &NoAgentProgress).await;
        let event = rx.try_recv().unwrap();
        assert!(matches!(
            event,
            UiEvent::ScopeChanged {
                scope: PhaseScope::Full,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn test_strategy_change() {
        let (mut controller, mut rx) = create_test_controller();

        controller
            .handle_command("/strategy debate", &NoAgentProgress)
            .await;
        let event = rx.try_recv().unwrap();
        match event {
            UiEvent::StrategyChanged { strategy, .. } => assert_eq!(strategy, "debate"),
            other => panic!("Expected StrategyChanged, got {:?}", other),
        }

        controller
            .handle_command("/strategy quorum", &NoAgentProgress)
            .await;
        let event = rx.try_recv().unwrap();
        match event {
            UiEvent::StrategyChanged { strategy, .. } => assert_eq!(strategy, "quorum"),
            other => panic!("Expected StrategyChanged, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_config_display() {
        let (mut controller, mut rx) = create_test_controller();
        controller.handle_command("/config", &NoAgentProgress).await;

        let event = rx.try_recv().unwrap();
        match event {
            UiEvent::ConfigDisplay(snapshot) => {
                // All registry keys are included (no filter)
                assert_eq!(snapshot.entries.len(), quorum_domain::known_keys().len());
                assert!(snapshot.section_filter.is_none());
                assert!(
                    snapshot
                        .entries
                        .iter()
                        .any(|e| e.key == "models.exploration")
                );
                assert!(snapshot.entries.iter().any(|e| e.key == "output.format"));
            }
            other => panic!("Expected ConfigDisplay, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_config_display_section_filter() {
        let (mut controller, mut rx) = create_test_controller();
        controller
            .handle_command("/config models", &NoAgentProgress)
            .await;

        let event = rx.try_recv().unwrap();
        match event {
            UiEvent::ConfigDisplay(snapshot) => {
                assert_eq!(snapshot.section_filter.as_deref(), Some("models"));
                assert!(!snapshot.entries.is_empty());
                assert!(snapshot.entries.iter().all(|e| e.section() == "models"));
            }
            other => panic!("Expected ConfigDisplay, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_config_display_nested_section_filter() {
        let (mut controller, mut rx) = create_test_controller();
        controller
            .handle_command("/config tui.input", &NoAgentProgress)
            .await;

        let event = rx.try_recv().unwrap();
        match event {
            UiEvent::ConfigDisplay(snapshot) => {
                assert!(!snapshot.entries.is_empty());
                assert!(snapshot.entries.iter().all(|e| e.section() == "tui.input"));
            }
            other => panic!("Expected ConfigDisplay, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_config_display_unknown_section() {
        let (mut controller, mut rx) = create_test_controller();
        controller
            .handle_command("/config nonexistent", &NoAgentProgress)
            .await;

        let event = rx.try_recv().unwrap();
        match event {
            UiEvent::CommandError { message } => {
                assert!(message.contains("Unknown config section"));
                assert!(message.contains("models"));
            }
            other => panic!("Expected CommandError, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_clear_history() {
        let (mut controller, mut rx) = create_test_controller();
        controller.handle_command("/clear", &NoAgentProgress).await;

        let event = rx.try_recv().unwrap();
        assert!(matches!(event, UiEvent::HistoryCleared));
    }

    #[tokio::test]
    async fn test_quit_returns_exit() {
        let (mut controller, mut rx) = create_test_controller();
        let action = controller.handle_command("/quit", &NoAgentProgress).await;

        assert!(matches!(action, CommandAction::Exit));
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, UiEvent::Exit));
    }

    #[tokio::test]
    async fn test_unknown_command() {
        let (mut controller, mut rx) = create_test_controller();
        controller.handle_command("/foobar", &NoAgentProgress).await;

        let event = rx.try_recv().unwrap();
        match event {
            UiEvent::UnknownCommand { command } => assert_eq!(command, "foobar"),
            other => panic!("Expected UnknownCommand, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_help_command() {
        let (mut controller, mut rx) = create_test_controller();
        controller.handle_command("/help", &NoAgentProgress).await;

        let event = rx.try_recv().unwrap();
        assert!(matches!(event, UiEvent::Help));
    }

    #[tokio::test]
    async fn test_mode_command_with_args() {
        let (mut controller, mut rx) = create_test_controller();

        controller
            .handle_command("/mode ensemble", &NoAgentProgress)
            .await;
        let event = rx.try_recv().unwrap();
        assert!(matches!(
            event,
            UiEvent::ModeChanged {
                level: ConsensusLevel::Ensemble,
                ..
            }
        ));
        assert_eq!(controller.consensus_level(), ConsensusLevel::Ensemble);
    }

    #[tokio::test]
    async fn test_mode_command_without_args() {
        let (mut controller, mut rx) = create_test_controller();

        controller.handle_command("/mode", &NoAgentProgress).await;
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, UiEvent::CommandError { .. }));
    }

    #[tokio::test]
    async fn test_discuss_without_args_shows_usage() {
        let (mut controller, mut rx) = create_test_controller();

        controller
            .handle_command("/discuss", &NoAgentProgress)
            .await;
        let event = rx.try_recv().unwrap();
        match event {
            UiEvent::CommandError { message } => {
                assert!(message.contains("Usage"));
            }
            other => panic!("Expected CommandError, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_ask_without_args_shows_usage() {
        let (mut controller, mut rx) = create_test_controller();

        controller.handle_command("/ask", &NoAgentProgress).await;
        let event = rx.try_recv().unwrap();
        match event {
            UiEvent::CommandError { message } => {
                assert!(message.contains("Usage"));
            }
            other => panic!("Expected CommandError, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_cancel_interaction_cancels_only_that_child() {
        let (mut controller, _rx) = create_test_controller();
        let root = CancellationToken::new();
        controller.set_cancellation(root.clone());

        let a = InteractionId(1);
        let b = InteractionId(2);
        let tok_a = controller.bind_cancellation(a).expect("root token present");
        let tok_b = controller.bind_cancellation(b).expect("root token present");

        // Cancelling one interaction leaves the other running.
        controller.cancel_interaction(a);
        assert!(tok_a.is_cancelled());
        assert!(!tok_b.is_cancelled());

        // The token is removed on cancel, so a second cancel is a harmless no-op.
        controller.cancel_interaction(a);

        // A root cancel (Ctrl+C) still stops every remaining interaction.
        root.cancel();
        assert!(tok_b.is_cancelled());
    }

    #[tokio::test]
    async fn test_bind_cancellation_without_root_is_none() {
        let (mut controller, _rx) = create_test_controller();
        // No root token wired → interactions run uncancelled, no panic.
        assert!(controller.bind_cancellation(InteractionId(1)).is_none());
        controller.cancel_interaction(InteractionId(1));
    }

    #[tokio::test]
    async fn test_finalize_drops_interaction_token() {
        let (mut controller, _rx) = create_test_controller();
        controller.set_cancellation(CancellationToken::new());
        let id = InteractionId(7);
        controller.bind_cancellation(id);
        assert!(controller.interaction_tokens.contains_key(&id));
        controller.finalize(TaskCompletion {
            interaction_id: Some(id),
            form: InteractionForm::Agent,
            query: "q".into(),
            result: None,
        });
        assert!(!controller.interaction_tokens.contains_key(&id));
    }

    #[tokio::test]
    async fn test_agent_without_args_shows_usage() {
        let (mut controller, mut rx) = create_test_controller();

        controller.handle_command("/agent", &NoAgentProgress).await;
        let event = rx.try_recv().unwrap();
        match event {
            UiEvent::CommandError { message } => {
                assert!(message.contains("Usage"));
            }
            other => panic!("Expected CommandError, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_ask_command_returns_execute() {
        let (mut controller, _rx) = create_test_controller();

        let action = controller
            .handle_command("/ask What is Rust?", &NoAgentProgress)
            .await;
        match action {
            CommandAction::Execute { form, query } => {
                assert_eq!(form, InteractionForm::Ask);
                assert_eq!(query, "What is Rust?");
            }
            _ => panic!("Expected CommandAction::Execute"),
        }
    }

    #[tokio::test]
    async fn test_discuss_command_returns_execute() {
        let (mut controller, _rx) = create_test_controller();

        let action = controller
            .handle_command("/discuss Design the auth", &NoAgentProgress)
            .await;
        match action {
            CommandAction::Execute { form, query } => {
                assert_eq!(form, InteractionForm::Discuss);
                assert_eq!(query, "Design the auth");
            }
            _ => panic!("Expected CommandAction::Execute"),
        }
    }

    #[tokio::test]
    async fn test_agent_command_returns_execute() {
        let (mut controller, _rx) = create_test_controller();

        let action = controller
            .handle_command("/agent Fix the bug", &NoAgentProgress)
            .await;
        match action {
            CommandAction::Execute { form, query } => {
                assert_eq!(form, InteractionForm::Agent);
                assert_eq!(query, "Fix the bug");
            }
            _ => panic!("Expected CommandAction::Execute"),
        }
    }

    #[tokio::test]
    async fn test_council_without_args_shows_usage() {
        let (mut controller, mut rx) = create_test_controller();

        controller
            .handle_command("/council", &NoAgentProgress)
            .await;
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, UiEvent::CommandError { .. }));
    }

    #[tokio::test]
    async fn test_scope_command() {
        let (mut controller, mut rx) = create_test_controller();

        controller
            .handle_command("/scope fast", &NoAgentProgress)
            .await;
        let event = rx.try_recv().unwrap();
        assert!(matches!(
            event,
            UiEvent::ScopeChanged {
                scope: PhaseScope::Fast,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn test_prompt_string() {
        let (controller, _rx) = create_test_controller();
        assert_eq!(controller.prompt_string(), "solo> ");
    }

    #[tokio::test]
    async fn test_prompt_string_ensemble() {
        let (controller, _rx) = create_test_controller();
        let controller = controller.with_consensus_level(ConsensusLevel::Ensemble);
        assert_eq!(controller.prompt_string(), "ens> ");
    }

    #[tokio::test]
    async fn test_send_welcome() {
        let (controller, mut rx) = create_test_controller();
        controller.send_welcome();

        let event = rx.try_recv().unwrap();
        assert!(matches!(event, UiEvent::Welcome(_)));

        // send_welcome also emits InteractionSpawned for the initial root
        // interaction so the TUI can bind tab 0 to an interaction ID.
        let event = rx.try_recv().unwrap();
        match event {
            UiEvent::InteractionSpawned(spawned) => {
                assert_eq!(spawned.form, InteractionForm::Agent);
                assert!(spawned.parent_id.is_none());
            }
            other => panic!("Expected InteractionSpawned, got {:?}", other),
        }
    }

    // === parse_spawn_flags tests ===

    type TestController = AgentController;

    #[test]
    fn test_parse_spawn_flags_no_flag() {
        let (flag, query) = TestController::parse_spawn_flags("What is Rust?");
        assert_eq!(flag, None);
        assert_eq!(query, "What is Rust?");
    }

    #[test]
    fn test_parse_spawn_flags_fresh() {
        let (flag, query) = TestController::parse_spawn_flags("--fresh How does auth work?");
        assert_eq!(flag, Some(ContextMode::Fresh));
        assert_eq!(query, "How does auth work?");
    }

    #[test]
    fn test_parse_spawn_flags_full() {
        let (flag, query) = TestController::parse_spawn_flags("--full Explain the architecture");
        assert_eq!(flag, Some(ContextMode::Full));
        assert_eq!(query, "Explain the architecture");
    }

    #[test]
    fn test_parse_spawn_flags_projected() {
        let (flag, query) = TestController::parse_spawn_flags("--projected Summarize changes");
        assert_eq!(flag, Some(ContextMode::Projected));
        assert_eq!(query, "Summarize changes");
    }

    #[test]
    fn test_parse_spawn_flags_no_space_after_flag_treated_as_query() {
        // "--fresh" without a trailing space is NOT recognized as a flag
        let (flag, query) = TestController::parse_spawn_flags("--fresh");
        assert_eq!(flag, None);
        assert_eq!(query, "--fresh");
    }

    #[test]
    fn test_parse_spawn_flags_flag_in_middle_not_recognized() {
        let (flag, query) = TestController::parse_spawn_flags("query --fresh option");
        assert_eq!(flag, None);
        assert_eq!(query, "query --fresh option");
    }

    #[test]
    fn test_parse_spawn_flags_trims_whitespace() {
        let (flag, query) = TestController::parse_spawn_flags("  --fresh   spaced query  ");
        assert_eq!(flag, Some(ContextMode::Fresh));
        assert_eq!(query, "spaced query");
    }

    // === build_projected_context tests ===

    #[test]
    fn test_projected_context_empty_history() {
        let (controller, _rx) = create_test_controller();
        assert_eq!(controller.build_projected_context(), "");
    }

    #[test]
    fn test_projected_context_one_entry() {
        let (mut controller, _rx) = create_test_controller();
        controller.conversation_history.push(HistoryEntry {
            form: InteractionForm::Ask,
            request: "What is X?".to_string(),
            summary: "X is Y".to_string(),
        });

        let ctx = controller.build_projected_context();
        assert!(ctx.starts_with("## Recent Context"));
        assert!(ctx.contains("[ask] What is X?: X is Y"));
    }

    #[test]
    fn test_projected_context_caps_at_three() {
        let (mut controller, _rx) = create_test_controller();
        for i in 0..5 {
            controller.conversation_history.push(HistoryEntry {
                form: InteractionForm::Agent,
                request: format!("Task {}", i),
                summary: format!("Done {}", i),
            });
        }

        let ctx = controller.build_projected_context();
        // Should contain only the last 3 entries (Task 2, 3, 4)
        assert!(!ctx.contains("Task 0"));
        assert!(!ctx.contains("Task 1"));
        assert!(ctx.contains("Task 2"));
        assert!(ctx.contains("Task 3"));
        assert!(ctx.contains("Task 4"));
    }

    #[test]
    fn test_projected_context_preserves_form_label() {
        let (mut controller, _rx) = create_test_controller();
        controller.conversation_history.push(HistoryEntry {
            form: InteractionForm::Discuss,
            request: "Design auth".to_string(),
            summary: "Use JWT".to_string(),
        });

        let ctx = controller.build_projected_context();
        assert!(ctx.contains("[discuss]"));
    }

    #[test]
    fn test_prepare_spawn_emits_interaction_spawned() {
        let (mut controller, mut rx) = create_test_controller();

        let (child_id, clean_query, full_query) = controller
            .prepare_spawn(InteractionForm::Ask, "hello", None)
            .unwrap();

        assert_eq!(clean_query, "hello");
        assert_eq!(full_query, "hello");

        let event = rx.try_recv().unwrap();
        match event {
            UiEvent::InteractionSpawned(spawned) => {
                assert_eq!(spawned.id, child_id);
                assert_eq!(spawned.form, InteractionForm::Ask);
                assert_eq!(spawned.query, "hello");
                assert!(spawned.parent_id.is_some());
            }
            other => panic!("Expected InteractionSpawned, got {:?}", other),
        }
    }

    #[test]
    fn test_prepare_root_spawn_has_no_parent() {
        // Regression coverage for #300: headless review spawns a genuine root
        // interaction (not a child of the default Agent root), so its
        // InteractionCompleted later routes to its own tab (presenter.rs).
        let (mut controller, mut rx) = create_test_controller();
        // Drain the InteractionSpawned emitted for the default root at construction time.
        // (AgentController::new creates one root Agent interaction; nothing sends it here
        // since send_welcome isn't called, so the channel should be empty at this point.)

        let (id, label, material) =
            controller.prepare_root_spawn(InteractionForm::Review, "Review PR #123", "diff text");

        assert_eq!(label, "Review PR #123");
        assert_eq!(material, "diff text");

        let event = rx.try_recv().unwrap();
        match event {
            UiEvent::InteractionSpawned(spawned) => {
                assert_eq!(spawned.id, id);
                assert_eq!(spawned.form, InteractionForm::Review);
                assert_eq!(spawned.query, "Review PR #123");
                assert_eq!(spawned.parent_id, None);
            }
            other => panic!("Expected InteractionSpawned, got {:?}", other),
        }
        // The root created at construction time is id 0; this new root must
        // be distinct from it.
        assert_ne!(id, InteractionId(0));
    }

    #[test]
    fn test_finalize_with_spawn_emits_completion() {
        let (mut controller, mut rx) = create_test_controller();
        let (child_id, clean_query, _) = controller
            .prepare_spawn(InteractionForm::Ask, "ship it", None)
            .unwrap();
        let _ = rx.try_recv(); // drain InteractionSpawned

        controller.finalize(TaskCompletion {
            interaction_id: Some(child_id),
            form: InteractionForm::Ask,
            query: clean_query,
            result: Some(InteractionResult::AskResult {
                answer: "done".to_string(),
            }),
        });

        let event = rx.try_recv().unwrap();
        match event {
            UiEvent::InteractionCompleted(completed) => {
                assert_eq!(completed.id, child_id);
                assert_eq!(completed.form, InteractionForm::Ask);
                // Parent notification includes the query for context (issue #274)
                assert_eq!(completed.result_text, "[Ask] \"ship it\" → done");
                assert!(completed.parent_id.is_some());
            }
            other => panic!("Expected InteractionCompleted, got {:?}", other),
        }
        assert_eq!(controller.conversation_history.len(), 1);
    }

    #[test]
    fn test_finalize_inline_adds_history_only() {
        let (mut controller, mut rx) = create_test_controller();

        controller.finalize(TaskCompletion {
            interaction_id: None,
            form: InteractionForm::Agent,
            query: "do something".to_string(),
            result: Some(InteractionResult::AgentResult {
                summary: "done it".to_string(),
                success: true,
            }),
        });

        // No InteractionCompleted event for inline
        assert!(rx.try_recv().is_err());
        assert_eq!(controller.conversation_history.len(), 1);
    }

    #[test]
    fn test_prepare_inline_no_history() {
        let (controller, _rx) = create_test_controller();
        let (clean, full) = controller.prepare_inline("hello world");
        assert_eq!(clean, "hello world");
        assert_eq!(full, "hello world"); // no history = no context
    }

    #[test]
    fn test_prepare_inline_with_history() {
        let (mut controller, _rx) = create_test_controller();
        controller.conversation_history.push(HistoryEntry {
            form: InteractionForm::Ask,
            request: "What is X?".to_string(),
            summary: "X is Y".to_string(),
        });

        let (clean, full) = controller.prepare_inline("follow up");
        assert_eq!(clean, "follow up");
        assert!(full.contains("Previous Conversation Context"));
        assert!(full.contains("follow up"));
    }

    #[test]
    fn test_prepare_inline_fresh_flag() {
        let (mut controller, _rx) = create_test_controller();
        controller.conversation_history.push(HistoryEntry {
            form: InteractionForm::Ask,
            request: "What is X?".to_string(),
            summary: "X is Y".to_string(),
        });

        let (clean, full) = controller.prepare_inline("--fresh no context please");
        assert_eq!(clean, "no context please");
        assert_eq!(full, "no context please"); // --fresh = no context
    }
}
