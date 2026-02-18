//! Agent Controller
//!
//! Extracts business logic from the REPL into the application layer.
//! Manages command processing, state changes, and use case orchestration.
//! Emits UiEvent messages to a channel for the presentation layer to render.

use crate::config::QuorumConfig;
use crate::ports::agent_progress::AgentProgressNotifier;
use crate::ports::context_loader::ContextLoaderPort;
use crate::ports::conversation_logger::{
    ConversationEvent, ConversationLogger, NoConversationLogger,
};
use crate::ports::llm_gateway::LlmGateway;
use crate::ports::progress::QuorumProgressAdapter;
use crate::ports::tool_executor::ToolExecutorPort;
use crate::ports::ui_event::{
    AgentErrorEvent, AgentResultEvent, AskResultEvent, ConfigSnapshot, ContextInitResultEvent,
    InteractionCompletedEvent, InteractionSpawnedEvent, QuorumResultEvent, UiEvent, WelcomeInfo,
};
use crate::use_cases::init_context::{InitContextInput, InitContextUseCase};
use crate::use_cases::run_agent::RunAgentUseCase;
use crate::use_cases::run_ask::RunAskUseCase;
use crate::use_cases::run_quorum::RunQuorumUseCase;
use quorum_domain::ContextMode;
use quorum_domain::interaction::{
    InteractionForm, InteractionId, InteractionResult, InteractionTree,
};
use quorum_domain::util::truncate_str;
use quorum_domain::{ConsensusLevel, Model, OutputFormat, PhaseScope, QuorumResult};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::ports::human_intervention::HumanInterventionPort;
use crate::ports::reference_resolver::ReferenceResolverPort;
use crate::ports::tool_schema::ToolSchemaPort;

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
}

/// Agent controller managing business logic for the REPL
///
/// This controller lives in the application layer and handles:
/// - Command processing (state changes, config updates)
/// - Use case orchestration (agent execution, quorum discussion, context init)
/// - Emitting UiEvents to a channel for the presentation layer
pub struct AgentController<
    G: LlmGateway + 'static,
    T: ToolExecutorPort + 'static,
    C: ContextLoaderPort + 'static,
> {
    gateway: Arc<G>,
    use_case: RunAgentUseCase<G, T, C>,
    ask_use_case: RunAskUseCase<G, T>,
    context_loader: Arc<C>,
    config: QuorumConfig,
    /// Moderator model for synthesis (if explicitly configured)
    moderator: Option<Model>,
    verbose: bool,
    /// Conversation history for /discuss context
    conversation_history: Vec<HistoryEntry>,
    /// Cancellation token for graceful shutdown
    cancellation_token: Option<CancellationToken>,
    /// Channel sender for UI events
    tx: mpsc::UnboundedSender<UiEvent>,
    /// Conversation logger for structured event logging
    conversation_logger: Arc<dyn ConversationLogger>,
    /// Interaction tree for nesting management
    interaction_tree: InteractionTree,
    /// Currently active interaction ID
    active_interaction_id: InteractionId,
}

impl<G: LlmGateway + 'static, T: ToolExecutorPort + 'static, C: ContextLoaderPort + 'static>
    AgentController<G, T, C>
{
    /// Create a new AgentController
    pub fn new(
        gateway: Arc<G>,
        tool_executor: Arc<T>,
        tool_schema: Arc<dyn ToolSchemaPort>,
        context_loader: Arc<C>,
        config: QuorumConfig,
        human_intervention: Arc<dyn HumanInterventionPort>,
        tx: mpsc::UnboundedSender<UiEvent>,
    ) -> Self {
        let conversation_logger: Arc<dyn ConversationLogger> = Arc::new(NoConversationLogger);
        let ask_use_case =
            RunAskUseCase::new(gateway.clone(), tool_executor.clone(), tool_schema.clone())
                .with_conversation_logger(conversation_logger.clone());

        let mut interaction_tree = InteractionTree::default();
        // Agent form is the default root interaction
        let active_interaction_id = interaction_tree.create_root(InteractionForm::Agent);

        Self {
            gateway: gateway.clone(),
            use_case: RunAgentUseCase::with_context_loader(
                gateway,
                tool_executor,
                tool_schema,
                context_loader.clone(),
            )
            .with_human_intervention(human_intervention),
            ask_use_case,
            context_loader,
            config,
            moderator: None,
            verbose: false,
            conversation_history: Vec::new(),
            cancellation_token: None,
            tx,
            conversation_logger,
            interaction_tree,
            active_interaction_id,
        }
    }

    /// Set a conversation logger for structured event logging.
    pub fn with_conversation_logger(mut self, logger: Arc<dyn ConversationLogger>) -> Self {
        self.conversation_logger = logger.clone();
        self.use_case = self.use_case.with_conversation_logger(logger.clone());
        self.ask_use_case.set_conversation_logger(logger);
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
    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.config = self.config.with_working_dir(dir);
        self
    }

    /// Enable final review
    pub fn with_final_review(mut self, enable: bool) -> Self {
        if enable {
            self.config = self.config.with_final_review();
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
    pub fn with_consensus_level(mut self, level: ConsensusLevel) -> Self {
        self.config = self.config.with_consensus_level(level);
        self
    }

    /// Get the current consensus level
    pub fn consensus_level(&self) -> ConsensusLevel {
        self.config.mode().consensus_level
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

    /// Generate the prompt string for the REPL
    ///
    /// Format: `<level>>`
    /// Examples: `solo>`, `ens>`
    pub fn prompt_string(&self) -> String {
        let level = match self.config.mode().consensus_level {
            ConsensusLevel::Solo => "solo",
            ConsensusLevel::Ensemble => "ens",
        };
        format!("{}> ", level)
    }

    /// Send the welcome event
    pub fn send_welcome(&self) {
        let moderator = self
            .moderator
            .clone()
            .or_else(|| self.config.models().review.first().cloned());

        let _ = self.tx.send(UiEvent::Welcome(WelcomeInfo {
            decision_model: self.config.models().decision.clone(),
            review_models: self.config.models().review.clone(),
            moderator,
            working_dir: self.config.execution().working_dir.clone(),
            consensus_level: self.config.mode().consensus_level,
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
                self.config.mode_mut().consensus_level = ConsensusLevel::Solo;
                let _ = self.tx.send(UiEvent::ModeChanged {
                    level: ConsensusLevel::Solo,
                    description: "single model, quick execution".to_string(),
                });
                CommandAction::Continue
            }
            "/ens" | "/ensemble" => {
                self.config.mode_mut().consensus_level = ConsensusLevel::Ensemble;
                let _ = self.tx.send(UiEvent::ModeChanged {
                    level: ConsensusLevel::Ensemble,
                    description: "multi-model ensemble planning".to_string(),
                });
                CommandAction::Continue
            }
            "/fast" => {
                let new_scope = if self.config.mode().phase_scope == PhaseScope::Fast {
                    PhaseScope::Full
                } else {
                    PhaseScope::Fast
                };
                self.config.mode_mut().phase_scope = new_scope;
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
                } else {
                    self.execute_inline(InteractionForm::Ask, args, progress)
                        .await;
                }
                CommandAction::Continue
            }
            "/discuss" | "/council" => {
                if args.is_empty() {
                    let _ = self.tx.send(UiEvent::CommandError {
                        message: "Usage: /discuss <question>".to_string(),
                    });
                } else {
                    self.execute_inline(InteractionForm::Discuss, args, progress)
                        .await;
                }
                CommandAction::Continue
            }
            "/agent" => {
                if args.is_empty() {
                    let _ = self.tx.send(UiEvent::CommandError {
                        message: "Usage: /agent <task>".to_string(),
                    });
                } else {
                    self.execute_inline(InteractionForm::Agent, args, progress)
                        .await;
                }
                CommandAction::Continue
            }
            "/config" => {
                let _ = self.tx.send(UiEvent::ConfigDisplay(ConfigSnapshot {
                    exploration_model: self.config.models().exploration.clone(),
                    decision_model: self.config.models().decision.clone(),
                    review_models: self.config.models().review.clone(),
                    consensus_level: self.config.mode().consensus_level,
                    phase_scope: self.config.mode().phase_scope,
                    orchestration_strategy: self.config.mode().strategy.to_string(),
                    require_final_review: self.config.policy().require_final_review,
                    max_iterations: self.config.execution().max_iterations,
                    max_plan_revisions: self.config.policy().max_plan_revisions,
                    hil_mode: self.config.policy().hil_mode,
                    working_dir: self.config.execution().working_dir.clone(),
                    verbose: self.verbose,
                    history_count: self.conversation_history.len(),
                }));
                CommandAction::Continue
            }
            "/clear" => {
                self.conversation_history.clear();
                let _ = self.tx.send(UiEvent::HistoryCleared);
                CommandAction::Continue
            }
            "/init" => {
                self.run_init_context(args).await;
                CommandAction::Continue
            }
            "/verbose" => {
                let _ = self.tx.send(UiEvent::VerboseStatus {
                    enabled: self.verbose,
                });
                CommandAction::Continue
            }
            _ => {
                let _ = self.tx.send(UiEvent::UnknownCommand {
                    command: command.to_string(),
                });
                CommandAction::Continue
            }
        }
    }

    fn handle_mode_command(&mut self, args: &str) {
        if args.is_empty() {
            let level = self.config.mode().consensus_level;
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
            self.config.mode_mut().consensus_level = level;
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
                    self.config.mode().phase_scope
                ),
            });
            return;
        }

        if let Ok(scope) = args.parse::<PhaseScope>() {
            self.config.mode_mut().phase_scope = scope;
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
                    self.config.mode().strategy
                ),
            });
            return;
        }

        match args.split_whitespace().next().unwrap_or("") {
            "quorum" | "q" => {
                self.config.mode_mut().strategy = quorum_domain::OrchestrationStrategy::default();
                let _ = self.tx.send(UiEvent::StrategyChanged {
                    strategy: "quorum".to_string(),
                    description: "equal discussion + review + synthesis".to_string(),
                });
            }
            "debate" | "d" => {
                self.config.mode_mut().strategy = quorum_domain::OrchestrationStrategy::Debate(
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
        self.execute_inline(InteractionForm::Ask, question, progress)
            .await;
    }

    /// Run Quorum Discussion with conversation context (inline, no new tab)
    pub async fn run_discuss(&mut self, question: &str, progress: &dyn AgentProgressNotifier) {
        self.execute_inline(InteractionForm::Discuss, question, progress)
            .await;
    }

    /// Run context initialization
    pub async fn run_init_context(&self, args: &str) {
        let force = args.contains("--force") || args.contains("-f");

        let working_dir = self
            .config
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
            model_count: self.config.models().review.len(),
        });

        // Create the init context input using review models
        let mut input = InitContextInput::new(&working_dir, self.config.models().review.clone());

        if let Some(moderator) = self.config.models().review.first() {
            input = input.with_moderator(moderator.clone());
        }

        if force {
            input = input.with_force(true);
        }

        // Run the initialization
        let use_case = InitContextUseCase::new(self.gateway.clone(), self.context_loader.clone());
        let result = use_case.execute(input).await;

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
        self.execute_inline(InteractionForm::Agent, request, progress)
            .await;
    }

    // =========================================================================
    // Interaction Nesting
    // =========================================================================

    /// Set the currently active interaction ID
    pub fn set_active_interaction(&mut self, id: InteractionId) {
        self.active_interaction_id = id;
    }

    /// Execute an interaction inline in the current tab (no new tab, no tree node).
    ///
    /// Performs context construction, use case execution, and history tracking
    /// without creating an InteractionTree node or emitting InteractionSpawned/Completed events.
    async fn execute_inline(
        &mut self,
        form: InteractionForm,
        query: &str,
        progress: &dyn AgentProgressNotifier,
    ) {
        let (ctx_override_flag, clean_query) = Self::parse_spawn_flags(query);
        let context_mode = ctx_override_flag.unwrap_or(ContextMode::Full);

        // Build context
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

        // Agent uses clean_query (has its own context loader); Ask/Discuss use full_query
        let result = match form {
            InteractionForm::Ask => self.execute_ask_spawn(&full_query, progress).await,
            InteractionForm::Discuss => self.execute_discuss_spawn(&full_query, progress).await,
            InteractionForm::Agent => self.execute_agent_spawn(&clean_query, progress).await,
        };

        // Add to history
        if let Some(interaction_result) = result {
            let result_text = interaction_result.to_context_injection();
            self.conversation_history.push(HistoryEntry {
                form,
                request: clean_query,
                summary: truncate_str(&result_text, 200).to_string(),
            });
        }
    }

    /// Spawn a new interaction (Ask, Discuss, or Agent).
    pub async fn spawn_interaction(
        &mut self,
        form: InteractionForm,
        query: &str,
        context_mode_override: Option<ContextMode>,
        progress: &dyn AgentProgressNotifier,
    ) {
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
                return;
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

        // 5. Execute use case based on Form
        let result = match form {
            InteractionForm::Ask => self.execute_ask_spawn(&full_query, progress).await,
            InteractionForm::Discuss => self.execute_discuss_spawn(&full_query, progress).await,
            InteractionForm::Agent => self.execute_agent_spawn(&clean_query, progress).await,
        };

        // 6. Completion → Add to history + Send InteractionCompleted
        if let Some(interaction_result) = result {
            let result_text = interaction_result.to_context_injection();
            self.conversation_history.push(HistoryEntry {
                form,
                request: clean_query,
                summary: truncate_str(&result_text, 200).to_string(),
            });
            let _ = self
                .tx
                .send(UiEvent::InteractionCompleted(InteractionCompletedEvent {
                    id: child_id,
                    form,
                    parent_id,
                    result_text,
                }));
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

    async fn execute_ask_spawn(
        &mut self,
        query: &str,
        progress: &dyn AgentProgressNotifier,
    ) -> Option<InteractionResult> {
        let _ = self.tx.send(UiEvent::AskStarting);
        let input = self.config.to_ask_input(query.to_string());
        match self.ask_use_case.execute(input, progress).await {
            Ok(result @ InteractionResult::AskResult { .. }) => {
                // Send UiEvent for display (will appear in new tab's pane)
                if let InteractionResult::AskResult { ref answer } = result {
                    let _ = self.tx.send(UiEvent::AskResult(AskResultEvent {
                        answer: answer.clone(),
                    }));
                }
                Some(result)
            }
            Ok(_) => None,
            Err(e) => {
                let _ = self.tx.send(UiEvent::AskError {
                    error: e.to_string(),
                });
                None
            }
        }
    }

    async fn execute_discuss_spawn(
        &mut self,
        query: &str,
        progress: &dyn AgentProgressNotifier,
    ) -> Option<InteractionResult> {
        let _ = self.tx.send(UiEvent::QuorumStarting);
        let input = self.config.to_quorum_input(query.to_string());
        let use_case = RunQuorumUseCase::new(self.gateway.clone());
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

    async fn execute_agent_spawn(
        &mut self,
        query: &str,
        progress: &dyn AgentProgressNotifier,
    ) -> Option<InteractionResult> {
        let _ = self.tx.send(UiEvent::AgentStarting {
            mode: self.config.mode().consensus_level,
        });
        let input = self.config.to_agent_input(query);
        match self.use_case.execute_with_progress(input, progress).await {
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
}

/// Format quorum output based on output format
///
/// This is a helper that replaces the ConsoleFormatter usage from presentation.
/// In the future, this could be moved to a domain service.
fn format_quorum_output(result: &QuorumResult, format: OutputFormat) -> String {
    // Simple formatting — the detailed formatting will be done by the presenter
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(result).unwrap_or_default(),
        OutputFormat::Full | OutputFormat::Synthesis => {
            // Return the synthesis text as-is; presenter will handle formatting
            result.synthesis.conclusion.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn create_test_controller() -> (
        AgentController<MockGateway, MockToolExecutor, MockContextLoader>,
        mpsc::UnboundedReceiver<UiEvent>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        let gateway = Arc::new(MockGateway::new(vec![Box::new(MockSession(
            Model::default(),
        ))]));
        let tool_executor = Arc::new(MockToolExecutor::new());
        let tool_schema: Arc<dyn ToolSchemaPort> = Arc::new(MockToolSchema);
        let context_loader = Arc::new(MockContextLoader);
        let human_intervention = Arc::new(MockHumanIntervention);
        let config = QuorumConfig::default();

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
        assert!(matches!(event, UiEvent::ConfigDisplay(_)));
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
            UiEvent::UnknownCommand { command } => assert_eq!(command, "/foobar"),
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
    async fn test_ask_with_question_sends_ask_starting() {
        let (mut controller, mut rx) = create_test_controller();

        controller
            .handle_command("/ask What is Rust?", &NoAgentProgress)
            .await;
        // Inline execution: no InteractionSpawned, directly AskStarting
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, UiEvent::AskStarting));
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
    }

    // === parse_spawn_flags tests ===

    type TestController = AgentController<MockGateway, MockToolExecutor, MockContextLoader>;

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
}
