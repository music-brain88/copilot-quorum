//! REPL (Read-Eval-Print Loop) for agent mode

use crate::ConsoleFormatter;
use crate::agent::human_intervention::InteractiveHumanIntervention;
use crate::agent::progress::AgentProgressReporter;
use crate::agent::thought::summarize_thoughts;
use crate::progress::reporter::ProgressReporter;
use colored::Colorize;
use quorum_application::{
    ContextLoaderPort, InitContextInput, InitContextUseCase, LlmGateway, RunAgentInput,
    RunAgentUseCase, RunQuorumInput, RunQuorumUseCase, ToolExecutorPort,
};
use quorum_domain::{AgentConfig, Model, OrchestrationMode, OutputFormat, PlanningMode};
use rustyline::error::ReadlineError;
use rustyline::{DefaultEditor, Result as RlResult};
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Entry in conversation history
#[derive(Debug, Clone)]
struct HistoryEntry {
    /// User's request
    request: String,
    /// Summary of agent's response
    summary: String,
}

/// Interactive REPL for agent mode
pub struct AgentRepl<
    G: LlmGateway + 'static,
    T: ToolExecutorPort + 'static,
    C: ContextLoaderPort + 'static,
> {
    gateway: Arc<G>,
    use_case: RunAgentUseCase<G, T, C>,
    context_loader: Arc<C>,
    config: AgentConfig,
    /// Moderator model for synthesis (if explicitly configured)
    moderator: Option<Model>,
    verbose: bool,
    working_dir: Option<String>,
    /// Conversation history for /council context
    conversation_history: Vec<HistoryEntry>,
    /// Current orchestration mode
    current_mode: OrchestrationMode,
    /// Cancellation token for graceful shutdown
    cancellation_token: Option<CancellationToken>,
}

impl<G: LlmGateway + 'static, T: ToolExecutorPort + 'static, C: ContextLoaderPort + 'static>
    AgentRepl<G, T, C>
{
    /// Create a new AgentRepl with role-based agent configuration
    pub fn new(
        gateway: Arc<G>,
        tool_executor: Arc<T>,
        context_loader: Arc<C>,
        config: AgentConfig,
    ) -> Self {
        // Set up human intervention handler for HiL (Human-in-the-Loop)
        let human_intervention = Arc::new(InteractiveHumanIntervention::new());

        Self {
            gateway: gateway.clone(),
            use_case: RunAgentUseCase::with_context_loader(
                gateway,
                tool_executor,
                context_loader.clone(),
            )
            .with_human_intervention(human_intervention),
            context_loader,
            config,
            moderator: None,
            verbose: false,
            working_dir: None,
            conversation_history: Vec::new(),
            current_mode: OrchestrationMode::Agent,
            cancellation_token: None,
        }
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
        let dir = dir.into();
        self.working_dir = Some(dir.clone());
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

    /// Set initial orchestration mode (Solo or Ensemble)
    pub fn with_mode(mut self, mode: OrchestrationMode) -> Self {
        self.current_mode = mode;
        self
    }

    /// Run the interactive REPL
    pub async fn run(&mut self) -> RlResult<()> {
        let mut rl = DefaultEditor::new()?;

        // Try to load history
        let history_path =
            dirs::data_dir().map(|p| p.join("copilot-quorum").join("agent_history.txt"));

        if let Some(ref path) = history_path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = rl.load_history(path);
        }

        self.print_welcome();

        loop {
            let prompt = match self.current_mode {
                OrchestrationMode::Agent => format!("{} ", "solo>".green()),
                OrchestrationMode::Quorum => format!("{} ", "ensemble>".magenta()),
                OrchestrationMode::Fast => format!("{} ", "fast>".yellow()),
                OrchestrationMode::Debate => format!("{} ", "debate>".blue()),
                OrchestrationMode::Plan => format!("{} ", "plan>".cyan()),
            };

            let readline = rl.readline(&prompt);

            match readline {
                Ok(line) => {
                    let line = line.trim();

                    // Skip empty lines
                    if line.is_empty() {
                        continue;
                    }

                    // Handle commands
                    if line.starts_with('/') {
                        match self.handle_command(line).await {
                            CommandResult::Exit => break,
                            CommandResult::Continue => continue,
                        }
                    }

                    // Add to history
                    let _ = rl.add_history_entry(line);

                    // Run agent
                    self.process_request(line).await;
                }
                Err(ReadlineError::Interrupted) => {
                    println!("^C");
                    continue;
                }
                Err(ReadlineError::Eof) => {
                    println!("Bye!");
                    break;
                }
                Err(err) => {
                    eprintln!("Error: {:?}", err);
                    break;
                }
            }
        }

        // Save history
        if let Some(ref path) = history_path {
            let _ = rl.save_history(path);
        }

        Ok(())
    }

    fn print_welcome(&self) {
        println!();
        println!(
            "{}",
            "╭─────────────────────────────────────────────╮".cyan()
        );
        println!(
            "{}",
            "│      Copilot Quorum - Agent Mode            │".cyan()
        );
        println!(
            "{}",
            "╰─────────────────────────────────────────────╯".cyan()
        );
        println!();
        // Show role-based model configuration
        println!(
            "{} {}",
            "Decision Model:".bold(),
            self.config.decision_model
        );
        if !self.config.review_models.is_empty() {
            println!(
                "{} {}",
                "Review Models:".bold(),
                self.config
                    .review_models
                    .iter()
                    .map(|m| m.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            // Show moderator (explicit or default to first review model)
            let moderator_display = self
                .moderator
                .as_ref()
                .or(self.config.review_models.first())
                .map(|m| m.to_string())
                .unwrap_or_default();
            if !moderator_display.is_empty() {
                println!("{} {}", "Moderator:".bold(), moderator_display);
            }
        }
        if let Some(ref dir) = self.working_dir {
            println!("{} {}", "Working Dir:".bold(), dir);
        }
        println!("{} {}", "Mode:".bold(), self.current_mode);
        println!();
        println!("{}", "The agent will:".dimmed());
        println!("{}", "  1. Gather context about your project".dimmed());
        println!("{}", "  2. Create a plan (reviewed by quorum)".dimmed());
        println!("{}", "  3. Execute tasks (high-risk ops reviewed)".dimmed());
        println!();
        println!("Commands:");
        println!("  {}    - Show this help", "/help".cyan());
        println!("  {}    - Change mode (solo, ensemble)", "/mode".cyan());
        println!("  {}    - Shortcut: switch to Solo mode", "/solo".cyan());
        println!(
            "  {}     - Shortcut: switch to Ensemble mode",
            "/ens".cyan()
        );
        println!(
            "  {} - Consult quorum (Quorum Discussion)",
            "/discuss".cyan()
        );
        println!("  {}    - Initialize project context", "/init".cyan());
        println!("  {}  - Show current configuration", "/config".cyan());
        println!("  {}   - Clear conversation history", "/clear".cyan());
        println!("  {}    - Exit", "/quit".cyan());
        println!();
    }

    /// Handle slash commands. Returns whether to continue or exit.
    async fn handle_command(&mut self, cmd: &str) -> CommandResult {
        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        let command = parts.first().copied().unwrap_or("");
        let args = parts.get(1).copied().unwrap_or("").trim();

        match command {
            "/quit" | "/exit" | "/q" => {
                println!("Bye!");
                CommandResult::Exit
            }
            "/help" | "/h" | "/?" => {
                println!();
                println!("{}", "Commands:".bold());
                println!("  /help, /h, /?        - Show this help");
                println!();
                println!("{}", "Mode Commands:".bold().cyan());
                println!(
                    "  /mode <mode>         - Change mode (solo, ensemble, fast, debate, plan)"
                );
                println!("  /solo                - Switch to Solo mode (single model, quick)");
                println!("  /ens                 - Switch to Ensemble mode (multi-model)");
                println!();
                println!("{}", "Quorum Commands:".bold().magenta());
                println!("  /discuss <question>  - Quorum Discussion (consult multiple models)");
                println!("  /council <question>  - Alias for /discuss");
                println!();
                println!("{}", "Other Commands:".bold());
                println!("  /init [--force]      - Initialize project context");
                println!("  /config              - Show current configuration");
                println!("  /clear               - Clear conversation history");
                println!("  /verbose             - Toggle verbose mode");
                println!("  /quit, /exit, /q     - Exit");
                println!();
                println!("{}", "Modes:".bold());
                println!("  solo (agent)   - Single model driven, quick execution");
                println!("  ensemble (ens) - Multi-model Quorum Discussion for all");
                println!("  fast           - No review, immediate response");
                println!("  debate         - Inter-model debate");
                println!("  plan           - Plan only, no execution");
                println!();
                println!("{}", "Usage:".bold());
                println!("  Type your request and press Enter.");
                println!("  In Solo mode: Single model executes, /discuss for quorum");
                println!("  In Ensemble mode: All queries go through Quorum Discussion");
                println!();
                println!("{}", "/discuss:".bold());
                println!("  Use /discuss to trigger a Quorum Discussion.");
                println!("  Multiple models provide perspectives and reach consensus.");
                println!("  The conversation history provides context.");
                println!();
                println!("{}", "Examples:".bold());
                println!("  - \"Fix the bug in login.rs\"");
                println!("  - \"/discuss What's the best approach for auth?\"");
                println!("  - \"/ens\" then \"Design the API\"");
                println!("  - \"/init --force\"");
                println!();
                CommandResult::Continue
            }
            "/mode" => {
                if args.is_empty() {
                    println!("{} Usage: /mode <mode>", "Error:".red().bold());
                    println!("Available modes: solo, ensemble, fast, debate, plan");
                    println!("Aliases: agent=solo, quorum=ensemble, ens=ensemble");
                    println!(
                        "Current mode: {} ({})",
                        self.current_mode,
                        self.current_mode.short_description()
                    );
                    return CommandResult::Continue;
                }

                if let Ok(mode) = args.parse::<OrchestrationMode>() {
                    self.current_mode = mode;
                    println!(
                        "Mode changed to: {} ({})",
                        mode,
                        mode.description().dimmed()
                    );
                } else {
                    println!("{} Unknown mode: {}", "Error:".red().bold(), args);
                    println!("Available modes: solo, ensemble, fast, debate, plan");
                }
                CommandResult::Continue
            }
            // Solo mode shortcut
            "/solo" => {
                self.current_mode = OrchestrationMode::Agent;
                self.config = self.config.clone().with_planning_mode(PlanningMode::Single);
                println!(
                    "Switched to {} - {}",
                    "Solo mode".green().bold(),
                    "single model, quick execution".dimmed()
                );
                println!(
                    "Use {} for ad-hoc multi-model consultation.",
                    "/discuss".cyan()
                );
                CommandResult::Continue
            }
            // Ensemble mode shortcut
            "/ens" | "/ensemble" => {
                self.current_mode = OrchestrationMode::Quorum;
                self.config = self
                    .config
                    .clone()
                    .with_planning_mode(PlanningMode::Ensemble);
                println!(
                    "Switched to {} - {}",
                    "Ensemble mode".magenta().bold(),
                    "multi-model ensemble planning".dimmed()
                );
                println!("Plans will be generated by multiple models and voted on.");
                CommandResult::Continue
            }
            // Quorum Discussion
            "/discuss" | "/council" => {
                if args.is_empty() {
                    println!("{} Usage: /discuss <your question>", "Error:".red().bold());
                    println!("Example: /discuss What's the best approach for this design?");
                    return CommandResult::Continue;
                }

                self.run_council(args).await;
                CommandResult::Continue
            }
            "/config" => {
                println!();
                println!("{}", "Current Configuration:".bold().cyan());
                // Role-based model configuration (3 fields)
                println!(
                    "  Exploration Model: {} {}",
                    self.config.exploration_model,
                    "(context + low-risk tools)".dimmed()
                );
                println!(
                    "  Decision Model:    {} {}",
                    self.config.decision_model,
                    "(planning + high-risk tools)".dimmed()
                );
                println!(
                    "  Review Models:     {}",
                    if self.config.review_models.is_empty() {
                        "None (auto-approve)".to_string()
                    } else {
                        self.config
                            .review_models
                            .iter()
                            .map(|m| m.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                );
                println!(
                    "  Planning Mode:     {} {}",
                    self.config.planning_mode,
                    if self.config.planning_mode.is_ensemble() {
                        "(multi-model planning + voting)".dimmed()
                    } else {
                        "(single model planning)".dimmed()
                    }
                );
                println!("  Plan Review:       {}", "Always required".green());
                println!(
                    "  Final Review:      {}",
                    if self.config.require_final_review {
                        "Enabled"
                    } else {
                        "Disabled"
                    }
                );
                println!("  Max Iterations:    {}", self.config.max_iterations);
                println!("  Max Plan Revisions: {}", self.config.max_plan_revisions);
                println!("  HiL Mode:          {}", self.config.hil_mode);
                println!(
                    "  Working Dir:       {}",
                    self.working_dir.as_deref().unwrap_or("(current)")
                );
                println!("  Verbose:           {}", self.verbose);
                println!(
                    "  History:           {} entries",
                    self.conversation_history.len()
                );
                println!();
                CommandResult::Continue
            }
            "/clear" => {
                self.conversation_history.clear();
                println!("{}", "Conversation history cleared.".green());
                CommandResult::Continue
            }
            "/init" => {
                self.run_init_context(args).await;
                CommandResult::Continue
            }
            "/verbose" => {
                // Note: Can't mutate self here, but this shows the pattern
                println!(
                    "Verbose mode is currently: {}",
                    if self.verbose { "ON" } else { "OFF" }
                );
                println!("Use --verbose flag when starting agent mode to enable.");
                CommandResult::Continue
            }
            _ => {
                println!("{} Unknown command: {}", "?".yellow(), command);
                println!("Type {} for available commands", "/help".cyan());
                CommandResult::Continue
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

    /// Run Quorum Discussion with conversation context
    async fn run_council(&self, question: &str) {
        println!();
        println!(
            "{} {}",
            "━".repeat(50).dimmed(),
            "Quorum Discussion".bold().magenta()
        );
        println!();

        // Build the question with context
        let context = self.build_context_from_history();
        let full_question = if context.is_empty() {
            question.to_string()
        } else {
            format!("{}\n\n## Current Question\n\n{}", context, question)
        };

        // Create quorum input using review models
        let mut input = RunQuorumInput::new(full_question, self.config.review_models.clone());

        // Use first review model as moderator if available
        if let Some(moderator) = self.config.review_models.first() {
            input = input.with_moderator(moderator.clone());
        }

        // Run quorum
        let use_case = RunQuorumUseCase::new(self.gateway.clone());
        let progress = ProgressReporter::new();
        let result = use_case.execute_with_progress(input, &progress).await;

        println!();
        println!("{}", "━".repeat(60).dimmed());

        match result {
            Ok(output) => {
                // Show synthesis
                println!();
                println!("{}", "Quorum Synthesis:".bold().magenta());
                println!();

                let formatted = match OutputFormat::Synthesis {
                    OutputFormat::Synthesis => ConsoleFormatter::format_synthesis_only(&output),
                    OutputFormat::Full => ConsoleFormatter::format(&output),
                    OutputFormat::Json => ConsoleFormatter::format_json(&output),
                };
                println!("{}", formatted);
            }
            Err(e) => {
                println!("{} {}", "❌".red(), "Quorum Discussion failed".red().bold());
                println!();
                println!("{} {}", "Error:".red().bold(), e);
            }
        }

        println!();
    }

    /// Run context initialization using quorum
    async fn run_init_context(&self, args: &str) {
        let force = args.contains("--force") || args.contains("-f");

        let working_dir = self.working_dir.clone().unwrap_or_else(|| {
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
            println!();
            println!(
                "{} Context file already exists at {}",
                "⚠️".yellow(),
                ".quorum/context.md".cyan()
            );
            println!("Use {} to regenerate.", "/init --force".cyan());
            println!();
            return;
        }

        println!();
        println!(
            "{} {}",
            "━".repeat(50).dimmed(),
            "Context Initialization".bold().magenta()
        );
        println!();
        println!(
            "Analyzing project with {} models...",
            self.config.review_models.len()
        );
        println!();

        // Create the init context input using review models
        let mut input = InitContextInput::new(&working_dir, self.config.review_models.clone());

        if let Some(moderator) = self.config.review_models.first() {
            input = input.with_moderator(moderator.clone());
        }

        if force {
            input = input.with_force(true);
        }

        // Run the initialization
        let use_case = InitContextUseCase::new(self.gateway.clone(), self.context_loader.clone());
        let result = use_case.execute(input).await;

        println!();
        println!("{}", "━".repeat(60).dimmed());

        match result {
            Ok(output) => {
                println!();
                println!(
                    "{} {} {}",
                    "✅".green(),
                    "Created:".green().bold(),
                    output.path.cyan()
                );
                println!();
                println!("{}", "Contributing models:".bold());
                for model in &output.contributing_models {
                    println!("  - {}", model);
                }
                println!();
                println!("{}", "Preview:".bold());
                println!("{}", "─".repeat(40).dimmed());
                // Show first 20 lines
                let preview: String = output
                    .content
                    .lines()
                    .take(20)
                    .collect::<Vec<_>>()
                    .join("\n");
                println!("{}", preview);
                if output.content.lines().count() > 20 {
                    println!("{}", "...".dimmed());
                }
            }
            Err(e) => {
                println!();
                println!(
                    "{} {}",
                    "❌".red(),
                    "Context initialization failed".red().bold()
                );
                println!();
                println!("{} {}", "Error:".red().bold(), e);
            }
        }

        println!();
    }

    async fn process_request(&mut self, request: &str) {
        // Route based on mode
        match self.current_mode {
            // Solo mode: Single model driven, quick execution
            OrchestrationMode::Agent => {
                // Fall through to agent logic
            }
            // Ensemble mode: Multi-model driven, more thorough
            // Uses the same agent flow but with ensemble flag for future enhancements
            OrchestrationMode::Quorum => {
                // For now, use the same agent flow
                // TODO: Add multi-model planning discussion in Ensemble mode
            }
            OrchestrationMode::Fast | OrchestrationMode::Debate | OrchestrationMode::Plan => {
                println!();
                println!(
                    "{} Mode '{}' is not yet implemented.",
                    "⚠️".yellow(),
                    self.current_mode
                );
                println!("Please switch back to 'solo' or 'ensemble' using /mode.");
                println!();
                return;
            }
        }

        println!();
        let mode_label = if self.current_mode.is_ensemble() {
            "Ensemble Agent Starting".bold().magenta()
        } else {
            "Solo Agent Starting".bold().cyan()
        };
        println!("{} {}", "━".repeat(50).dimmed(), mode_label);
        if self.current_mode.is_ensemble() {
            println!(
                "{}",
                "  (Multi-model mode: higher accuracy, more thorough)".dimmed()
            );
        }
        println!();

        let input = RunAgentInput::new(request, self.config.clone());

        let result = if self.verbose {
            let progress = AgentProgressReporter::verbose();
            self.use_case.execute_with_progress(input, &progress).await
        } else {
            let progress = AgentProgressReporter::new();
            self.use_case.execute_with_progress(input, &progress).await
        };

        println!();
        println!("{}", "━".repeat(60).dimmed());

        match result {
            Ok(output) => {
                // Add to conversation history
                self.conversation_history.push(HistoryEntry {
                    request: request.to_string(),
                    summary: output.summary.clone(),
                });

                // Print execution summary header
                println!();
                println!("{}", "═══════════════════════════════════════════".cyan());
                println!("{}", "  Agent Execution Summary".bold().cyan());
                println!("{}", "═══════════════════════════════════════════".cyan());
                println!();

                // Status
                if output.success {
                    println!("  {} {}", "Status:".bold(), "SUCCESS".green().bold());
                } else {
                    println!("  {} {}", "Status:".bold(), "FAILED".red().bold());
                }
                println!();

                // Show Quorum Journey if there was any review history
                if let Some(plan) = &output.state.plan
                    && !plan.review_history.is_empty()
                {
                    println!("  {} Quorum Journey:", "🗳️".bold());
                    for round in &plan.review_history {
                        let status_icon = if round.approved { "✓" } else { "✗" };
                        let status_color: fn(&str) -> colored::ColoredString = if round.approved {
                            |s| s.green()
                        } else {
                            |s| s.red()
                        };

                        // Build vote details like [claude: ✓, gpt: ✗, gemini: ✓]
                        let vote_details: Vec<String> = round
                            .votes
                            .iter()
                            .map(|v| {
                                let icon = if v.approved { "✓" } else { "✗" };
                                format!("{}: {}", truncate_model_name(&v.model), icon)
                            })
                            .collect();

                        println!(
                            "    {} Rev {}: {} [{}]",
                            status_color(status_icon),
                            round.round,
                            status_color(if round.approved {
                                "Approved"
                            } else {
                                "Rejected"
                            }),
                            vote_details.join(", ")
                        );
                    }

                    let revision_count = plan.revision_count();
                    if revision_count > 0 {
                        println!(
                            "    {} Approved after {} revision(s)",
                            "📝".dimmed(),
                            revision_count
                        );
                    }
                    println!();
                }

                // Show task details with status
                if let Some(plan) = &output.state.plan {
                    let (completed, total) = plan.progress();
                    println!("  {} {}/{} completed", "📋 Tasks:".bold(), completed, total);

                    for (i, task) in plan.tasks.iter().enumerate() {
                        let (icon, status_color): (&str, fn(&str) -> colored::ColoredString) =
                            match task.status {
                                quorum_domain::TaskStatus::Completed => ("✅", |s| s.green()),
                                quorum_domain::TaskStatus::Failed => ("❌", |s| s.red()),
                                quorum_domain::TaskStatus::Skipped => ("⏭️", |s| s.dimmed()),
                                _ => ("⏳", |s| s.yellow()),
                            };

                        println!(
                            "    {} Task {}: {}",
                            icon,
                            i + 1,
                            status_color(&task.description)
                        );

                        // Show failure reason if task failed
                        if task.status == quorum_domain::TaskStatus::Failed
                            && let Some(result) = &task.result
                            && let Some(error) = &result.error
                        {
                            println!("       {} {}", "└─".dimmed(), error.red());
                        }
                    }
                } else {
                    // Fallback to old summary if no plan
                    println!("{}", "Summary:".bold());
                    println!("{}", ConsoleFormatter::indent(&output.summary, "  "));
                }

                println!();
                println!("{}", "═══════════════════════════════════════════".cyan());

                // Show thought summary in verbose mode
                if self.verbose && !output.state.thoughts.is_empty() {
                    println!();
                    println!("{}", "Thought Process:".bold().dimmed());
                    println!(
                        "{}",
                        ConsoleFormatter::indent(&summarize_thoughts(&output.state.thoughts), "  ")
                    );
                }
            }
            Err(e) => {
                // Check if this was a cancellation
                if e.is_cancelled() {
                    println!();
                    println!(
                        "{} {}",
                        "⚠️".yellow(),
                        "Operation cancelled".yellow().bold()
                    );
                } else {
                    println!("{} {}", "❌".red(), "Agent failed".red().bold());
                    println!();
                    println!("{} {}", "Error:".red().bold(), e);
                }
            }
        }

        println!();
    }
}

/// Result of handling a command
enum CommandResult {
    Continue,
    Exit,
}

/// Truncate model name for compact display (e.g., "claude-sonnet-4.5" -> "claude")
fn truncate_model_name(model: &str) -> &str {
    // Take the first part before any dash/underscore
    model.split(['-', '_']).next().unwrap_or(model)
}
