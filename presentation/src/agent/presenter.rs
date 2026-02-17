//! REPL Presenter - Renders UiEvents to the terminal
//!
//! This module contains the presentation logic for the Agent REPL.
//! All println!/colored output is concentrated here, separating display
//! from business logic (which lives in AgentController in the application layer).

use crate::ConsoleFormatter;
use crate::agent::thought::summarize_thoughts;
use colored::Colorize;
use quorum_application::{
    AgentErrorEvent, AgentResultEvent, ConfigSnapshot, ContextInitResultEvent, QuorumResultEvent,
    UiEvent, WelcomeInfo,
};
use quorum_domain::{ConsensusLevel, PhaseScope};

/// Renders UiEvents to the terminal for the REPL
pub struct ReplPresenter;

impl Default for ReplPresenter {
    fn default() -> Self {
        Self::new()
    }
}

impl ReplPresenter {
    pub fn new() -> Self {
        Self
    }

    /// Render a single UiEvent to the terminal
    pub fn render(&self, event: &UiEvent) {
        match event {
            UiEvent::Welcome(info) => self.render_welcome(info),
            UiEvent::Help => self.render_help(),
            UiEvent::ConfigDisplay(snapshot) => self.render_config(snapshot),
            UiEvent::ModeChanged { level, description } => {
                self.render_mode_changed(*level, description)
            }
            UiEvent::ScopeChanged { scope, description } => {
                self.render_scope_changed(*scope, description)
            }
            UiEvent::StrategyChanged {
                strategy,
                description,
            } => self.render_strategy_changed(strategy, description),
            UiEvent::HistoryCleared => {
                println!("{}", "Conversation history cleared.".green());
            }
            UiEvent::VerboseStatus { enabled } => {
                println!(
                    "Verbose mode is currently: {}",
                    if *enabled { "ON" } else { "OFF" }
                );
                println!("Use --verbose flag when starting agent mode to enable.");
            }
            UiEvent::AgentStarting { mode } => self.render_agent_starting(*mode),
            UiEvent::AgentResult(result) => self.render_agent_result(result),
            UiEvent::AgentError(error) => self.render_agent_error(error),
            UiEvent::AskStarting => {
                println!();
                println!("{} {}", "━".repeat(50).dimmed(), "Ask".bold().cyan());
                println!();
            }
            UiEvent::AskResult(result) => {
                println!();
                println!("{}", result.answer);
                println!();
            }
            UiEvent::AskError { error } => {
                println!("{} {}", "❌".red(), "Ask failed".red().bold());
                println!();
                println!("{} {}", "Error:".red().bold(), error);
                println!();
            }
            UiEvent::QuorumStarting => self.render_quorum_starting(),
            UiEvent::QuorumResult(result) => self.render_quorum_result(result),
            UiEvent::QuorumError { error } => {
                println!("{} {}", "❌".red(), "Quorum Discussion failed".red().bold());
                println!();
                println!("{} {}", "Error:".red().bold(), error);
                println!();
            }
            UiEvent::ContextInitStarting { model_count } => {
                println!();
                println!(
                    "{} {}",
                    "━".repeat(50).dimmed(),
                    "Context Initialization".bold().magenta()
                );
                println!();
                println!("Analyzing project with {} models...", model_count);
                println!();
            }
            UiEvent::ContextInitResult(result) => self.render_context_init_result(result),
            UiEvent::ContextInitError { error } => {
                println!();
                println!(
                    "{} {}",
                    "❌".red(),
                    "Context initialization failed".red().bold()
                );
                println!();
                println!("{} {}", "Error:".red().bold(), error);
                println!();
            }
            UiEvent::ContextAlreadyExists => {
                println!();
                println!(
                    "{} Context file already exists at {}",
                    "⚠️".yellow(),
                    ".quorum/context.md".cyan()
                );
                println!("Use {} to regenerate.", "/init --force".cyan());
                println!();
            }
            UiEvent::CommandError { message } => {
                println!("{} {}", "Error:".red().bold(), message);
            }
            UiEvent::UnknownCommand { command } => {
                println!("{} Unknown command: {}", "?".yellow(), command);
                println!("Type {} for available commands", "/help".cyan());
            }
            UiEvent::Exit => {
                println!("Bye!");
            }
        }
    }

    fn render_welcome(&self, info: &WelcomeInfo) {
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
        println!("{} {}", "Decision Model:".bold(), info.decision_model);
        if !info.review_models.is_empty() {
            println!(
                "{} {}",
                "Review Models:".bold(),
                info.review_models
                    .iter()
                    .map(|m| m.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if let Some(ref moderator) = info.moderator {
                println!("{} {}", "Moderator:".bold(), moderator);
            }
        }
        if let Some(ref dir) = info.working_dir {
            println!("{} {}", "Working Dir:".bold(), dir);
        }
        println!("{} {}", "Mode:".bold(), info.consensus_level);
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
        println!("  {}     - Ask (lightweight Q&A)", "/ask".cyan());
        println!("  {} - Discuss (quorum discussion)", "/discuss".cyan());
        println!("  {}    - Toggle fast mode (skip reviews)", "/fast".cyan());
        println!(
            "  {}   - Change phase scope (full, fast, plan)",
            "/scope".cyan()
        );
        println!(
            "  {}- Change strategy (quorum, debate)",
            "/strategy ".cyan()
        );
        println!("  {}    - Initialize project context", "/init".cyan());
        println!("  {}  - Show current configuration", "/config".cyan());
        println!("  {}   - Clear conversation history", "/clear".cyan());
        println!("  {}    - Exit", "/quit".cyan());
        println!();
    }

    fn render_help(&self) {
        println!();
        println!("{}", "Commands:".bold());
        println!("  /help, /h, /?        - Show this help");
        println!();
        println!("{}", "Mode Commands:".bold().cyan());
        println!("  /mode <level>        - Change consensus level (solo, ensemble)");
        println!("  /solo                - Switch to Solo mode (single model, quick)");
        println!("  /ens                 - Switch to Ensemble mode (multi-model)");
        println!();
        println!("{}", "Scope Commands:".bold().yellow());
        println!("  /fast                - Toggle fast mode (skip reviews)");
        println!("  /scope <scope>       - Change phase scope (full, fast, plan)");
        println!();
        println!("{}", "Strategy Commands:".bold().blue());
        println!("  /strategy <s>        - Change strategy (quorum, debate)");
        println!();
        println!("{}", "Interaction Commands:".bold().magenta());
        println!("  /ask <question>      - Ask (lightweight Q&A with read-only tools)");
        println!("  /discuss <question>  - Discuss (quorum discussion, consult multiple models)");
        println!();
        println!("{}", "Other Commands:".bold());
        println!("  /init [--force]      - Initialize project context");
        println!("  /config              - Show current configuration");
        println!("  /clear               - Clear conversation history");
        println!("  /verbose             - Toggle verbose mode");
        println!("  /quit, /exit, /q     - Exit");
        println!();
    }

    fn render_config(&self, snapshot: &ConfigSnapshot) {
        println!();
        println!("{}", "Current Configuration:".bold().cyan());
        println!(
            "  Exploration Model: {} {}",
            snapshot.exploration_model,
            "(context + low-risk tools)".dimmed()
        );
        println!(
            "  Decision Model:    {} {}",
            snapshot.decision_model,
            "(planning + high-risk tools)".dimmed()
        );
        println!(
            "  Review Models:     {}",
            if snapshot.review_models.is_empty() {
                "None (auto-approve)".to_string()
            } else {
                snapshot
                    .review_models
                    .iter()
                    .map(|m| m.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        );
        println!(
            "  Consensus Level:   {} {}",
            snapshot.consensus_level,
            if snapshot.consensus_level.is_ensemble() {
                "(multi-model planning + voting)".dimmed()
            } else {
                "(single model planning)".dimmed()
            }
        );
        println!("  Phase Scope:       {}", snapshot.phase_scope);
        println!("  Strategy:          {}", snapshot.orchestration_strategy);
        println!("  Plan Review:       {}", "Always required".green());
        println!(
            "  Final Review:      {}",
            if snapshot.require_final_review {
                "Enabled"
            } else {
                "Disabled"
            }
        );
        println!("  Max Iterations:    {}", snapshot.max_iterations);
        println!("  Max Plan Revisions: {}", snapshot.max_plan_revisions);
        println!("  HiL Mode:          {}", snapshot.hil_mode);
        println!(
            "  Working Dir:       {}",
            snapshot.working_dir.as_deref().unwrap_or("(current)")
        );
        println!("  Verbose:           {}", snapshot.verbose);
        println!("  History:           {} entries", snapshot.history_count);
        println!();
    }

    fn render_mode_changed(&self, level: ConsensusLevel, description: &str) {
        match level {
            ConsensusLevel::Solo => {
                println!(
                    "Switched to {} - {}",
                    "Solo mode".green().bold(),
                    description.dimmed()
                );
                println!(
                    "Use {} for ad-hoc multi-model consultation.",
                    "/discuss".cyan()
                );
            }
            ConsensusLevel::Ensemble => {
                println!(
                    "Switched to {} - {}",
                    "Ensemble mode".magenta().bold(),
                    description.dimmed()
                );
                println!("Plans will be generated by multiple models and voted on.");
            }
        }
    }

    fn render_scope_changed(&self, scope: PhaseScope, description: &str) {
        match scope {
            PhaseScope::Fast => println!(
                "Switched to {} - {}",
                "Fast scope".yellow().bold(),
                description.dimmed()
            ),
            _ => println!(
                "Switched to {} - {}",
                "Full scope".green().bold(),
                description.dimmed()
            ),
        }
    }

    fn render_strategy_changed(&self, strategy: &str, description: &str) {
        println!(
            "Strategy changed to: {} - {}",
            strategy.bold(),
            description.dimmed()
        );
    }

    fn render_agent_starting(&self, mode: ConsensusLevel) {
        println!();
        let mode_label = if mode.is_ensemble() {
            "Ensemble Agent Starting".bold().magenta()
        } else {
            "Solo Agent Starting".bold().cyan()
        };
        println!("{} {}", "━".repeat(50).dimmed(), mode_label);
        if mode.is_ensemble() {
            println!(
                "{}",
                "  (Multi-model mode: higher accuracy, more thorough)".dimmed()
            );
        }
        println!();
    }

    fn render_agent_result(&self, result: &AgentResultEvent) {
        println!();
        println!("{}", "━".repeat(60).dimmed());

        // Print execution summary header
        println!();
        println!("{}", "═══════════════════════════════════════════".cyan());
        println!("{}", "  Agent Execution Summary".bold().cyan());
        println!("{}", "═══════════════════════════════════════════".cyan());
        println!();

        // Status
        if result.success {
            println!("  {} {}", "Status:".bold(), "SUCCESS".green().bold());
        } else {
            println!("  {} {}", "Status:".bold(), "FAILED".red().bold());
        }
        println!();

        // Show Quorum Journey if there was any review history
        if let Some(plan) = &result.state.plan {
            if !plan.review_history.is_empty() {
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
                    && let Some(task_result) = &task.result
                    && let Some(error) = &task_result.error
                {
                    println!("       {} {}", "└─".dimmed(), error.red());
                }
            }
        } else {
            // Fallback to old summary if no plan
            println!("{}", "Summary:".bold());
            println!("{}", ConsoleFormatter::indent(&result.summary, "  "));
        }

        println!();
        println!("{}", "═══════════════════════════════════════════".cyan());

        // Show thought summary in verbose mode
        if result.verbose && !result.thoughts.is_empty() {
            println!();
            println!("{}", "Thought Process:".bold().dimmed());
            println!(
                "{}",
                ConsoleFormatter::indent(&summarize_thoughts(&result.thoughts), "  ")
            );
        }

        println!();
    }

    fn render_agent_error(&self, error: &AgentErrorEvent) {
        if error.cancelled {
            println!();
            println!(
                "{} {}",
                "⚠️".yellow(),
                "Operation cancelled".yellow().bold()
            );
        } else {
            println!("{} {}", "❌".red(), "Agent failed".red().bold());
            println!();
            println!("{} {}", "Error:".red().bold(), error.error);
        }
        println!();
    }

    fn render_quorum_starting(&self) {
        println!();
        println!(
            "{} {}",
            "━".repeat(50).dimmed(),
            "Quorum Discussion".bold().magenta()
        );
        println!();
    }

    fn render_quorum_result(&self, result: &QuorumResultEvent) {
        println!();
        println!("{}", "━".repeat(60).dimmed());
        println!();
        println!("{}", "Quorum Synthesis:".bold().magenta());
        println!();
        println!("{}", result.formatted_output);
        println!();
    }

    fn render_context_init_result(&self, result: &ContextInitResultEvent) {
        println!();
        println!("{}", "━".repeat(60).dimmed());
        println!();
        println!(
            "{} {} {}",
            "✅".green(),
            "Created:".green().bold(),
            result.path.cyan()
        );
        println!();
        println!("{}", "Contributing models:".bold());
        for model in &result.contributing_models {
            println!("  - {}", model);
        }
        println!();
        println!("{}", "Preview:".bold());
        println!("{}", "─".repeat(40).dimmed());
        // Show first 20 lines
        let preview: String = result
            .content
            .lines()
            .take(20)
            .collect::<Vec<_>>()
            .join("\n");
        println!("{}", preview);
        if result.content.lines().count() > 20 {
            println!("{}", "...".dimmed());
        }
        println!();
    }
}

/// Truncate model name for compact display (e.g., "claude-sonnet-4.5" -> "claude")
fn truncate_model_name(model: &str) -> &str {
    model.split(['-', '_']).next().unwrap_or(model)
}
