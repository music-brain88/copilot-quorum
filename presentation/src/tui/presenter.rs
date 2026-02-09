//! TUI Presenter - Converts Application Events to TUI State
//!
//! This module serves as an adapter layer between the application layer
//! (which emits domain events) and the TUI layer (which manages view state).
//!
//! # Architecture
//!
//! ```text
//! Application Layer (UiEvent)
//!         ↓
//!   TuiPresenter (this module)
//!         ↓
//!   TuiState/TuiEvent (view state)
//!         ↓
//!   Rendering (future: Ratatui)
//! ```

use super::event::TuiEvent;
use super::state::TuiState;
use colored::Colorize;
use quorum_application::{
    AgentErrorEvent, AgentResultEvent, ConfigSnapshot, ContextInitResultEvent, QuorumResultEvent,
    UiEvent, WelcomeInfo,
};
use quorum_domain::{ConsensusLevel, PhaseScope};
use std::sync::{Arc, Mutex};

/// TUI Presenter - Converts UiEvents to TUI state updates
///
/// Maintains a shared TuiState that can be rendered by the UI.
/// Acts as the translation layer between application events and view state.
pub struct TuiPresenter {
    state: Arc<Mutex<TuiState>>,
}

impl TuiPresenter {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(TuiState::default())),
        }
    }

    /// Get a clone of the shared state handle
    pub fn state(&self) -> Arc<Mutex<TuiState>> {
        self.state.clone()
    }

    /// Render a UiEvent by updating state and emitting TUI events
    ///
    /// For now, this still uses println! for compatibility,
    /// but the state updates prepare us for true TUI rendering.
    pub fn render(&self, event: &UiEvent) {
        // Update state based on event
        match event {
            UiEvent::Welcome(info) => self.handle_welcome(info),
            UiEvent::Help => self.handle_help(),
            UiEvent::ConfigDisplay(snapshot) => self.handle_config(snapshot),
            UiEvent::ModeChanged { level, description } => {
                self.handle_mode_changed(*level, description)
            }
            UiEvent::ScopeChanged { scope, description } => {
                self.handle_scope_changed(*scope, description)
            }
            UiEvent::StrategyChanged {
                strategy,
                description,
            } => self.handle_strategy_changed(strategy, description),
            UiEvent::HistoryCleared => self.handle_history_cleared(),
            UiEvent::VerboseStatus { enabled } => self.handle_verbose_status(*enabled),
            UiEvent::AgentStarting { mode } => self.handle_agent_starting(*mode),
            UiEvent::AgentResult(result) => self.handle_agent_result(result),
            UiEvent::AgentError(error) => self.handle_agent_error(error),
            UiEvent::QuorumStarting => self.handle_quorum_starting(),
            UiEvent::QuorumResult(result) => self.handle_quorum_result(result),
            UiEvent::QuorumError { error } => self.handle_quorum_error(error),
            UiEvent::ContextInitStarting { model_count } => {
                self.handle_context_init_starting(*model_count)
            }
            UiEvent::ContextInitResult(result) => self.handle_context_init_result(result),
            UiEvent::ContextInitError { error } => self.handle_context_init_error(error),
            UiEvent::ContextAlreadyExists => self.handle_context_already_exists(),
            UiEvent::CommandError { message } => self.handle_command_error(message),
            UiEvent::UnknownCommand { command } => self.handle_unknown_command(command),
            UiEvent::Exit => self.handle_exit(),
        }
    }

    // ==================== Event Handlers ====================

    fn handle_welcome(&self, info: &WelcomeInfo) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::Welcome {
            decision_model: info.decision_model.to_string(),
            review_models: info
                .review_models
                .iter()
                .map(|m| m.to_string())
                .collect(),
            moderator: info.moderator.as_ref().map(|m| m.to_string()),
            working_dir: info.working_dir.clone(),
            consensus_level: info.consensus_level,
        });

        // Legacy: Still print for now
        self.print_welcome(info);
    }

    fn handle_help(&self) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::HelpRequested);

        // Legacy: Still print for now
        self.print_help();
    }

    fn handle_config(&self, snapshot: &ConfigSnapshot) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::ConfigDisplay {
            snapshot: snapshot.clone(),
        });

        // Legacy: Still print for now
        self.print_config(snapshot);
    }

    fn handle_mode_changed(&self, level: ConsensusLevel, description: &str) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::ModeChanged {
            level,
            description: description.to_string(),
        });

        // Legacy: Still print for now
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

    fn handle_scope_changed(&self, scope: PhaseScope, description: &str) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::ScopeChanged {
            scope,
            description: description.to_string(),
        });

        // Legacy: Still print for now
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

    fn handle_strategy_changed(&self, strategy: &str, description: &str) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::StrategyChanged {
            strategy: strategy.to_string(),
            description: description.to_string(),
        });

        // Legacy: Still print for now
        println!(
            "Strategy changed to: {} - {}",
            strategy.bold(),
            description.dimmed()
        );
    }

    fn handle_history_cleared(&self) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::HistoryCleared);

        // Legacy: Still print for now
        println!("{}", "Conversation history cleared.".green());
    }

    fn handle_verbose_status(&self, enabled: bool) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::VerboseStatus { enabled });

        // Legacy: Still print for now
        println!(
            "Verbose mode is currently: {}",
            if enabled { "ON" } else { "OFF" }
        );
        println!("Use --verbose flag when starting agent mode to enable.");
    }

    fn handle_agent_starting(&self, mode: ConsensusLevel) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::AgentStarting { mode });

        // Legacy: Still print for now
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

    fn handle_agent_result(&self, result: &AgentResultEvent) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::AgentResult {
            success: result.success,
            summary: result.summary.clone(),
            verbose: result.verbose,
        });

        // Legacy: Still print for now
        self.print_agent_result(result);
    }

    fn handle_agent_error(&self, error: &AgentErrorEvent) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::AgentError {
            cancelled: error.cancelled,
            error: error.error.clone(),
        });

        // Legacy: Still print for now
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

    fn handle_quorum_starting(&self) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::QuorumStarting);

        // Legacy: Still print for now
        println!();
        println!(
            "{} {}",
            "━".repeat(50).dimmed(),
            "Quorum Discussion".bold().magenta()
        );
        println!();
    }

    fn handle_quorum_result(&self, result: &QuorumResultEvent) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::QuorumResult {
            output: result.formatted_output.clone(),
        });

        // Legacy: Still print for now
        println!();
        println!("{}", "━".repeat(60).dimmed());
        println!();
        println!("{}", "Quorum Synthesis:".bold().magenta());
        println!();
        println!("{}", result.formatted_output);
        println!();
    }

    fn handle_quorum_error(&self, error: &str) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::QuorumError {
            error: error.to_string(),
        });

        // Legacy: Still print for now
        println!("{} {}", "❌".red(), "Quorum Discussion failed".red().bold());
        println!();
        println!("{} {}", "Error:".red().bold(), error);
        println!();
    }

    fn handle_context_init_starting(&self, model_count: usize) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::ContextInitStarting { model_count });

        // Legacy: Still print for now
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

    fn handle_context_init_result(&self, result: &ContextInitResultEvent) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::ContextInitResult {
            path: result.path.clone(),
            contributing_models: result.contributing_models.clone(),
        });

        // Legacy: Still print for now
        self.print_context_init_result(result);
    }

    fn handle_context_init_error(&self, error: &str) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::ContextInitError {
            error: error.to_string(),
        });

        // Legacy: Still print for now
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

    fn handle_context_already_exists(&self) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::ContextAlreadyExists);

        // Legacy: Still print for now
        println!();
        println!(
            "{} Context file already exists at {}",
            "⚠️".yellow(),
            ".quorum/context.md".cyan()
        );
        println!("Use {} to regenerate.", "/init --force".cyan());
        println!();
    }

    fn handle_command_error(&self, message: &str) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::CommandError {
            message: message.to_string(),
        });

        // Legacy: Still print for now
        println!("{} {}", "Error:".red().bold(), message);
    }

    fn handle_unknown_command(&self, command: &str) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::UnknownCommand {
            command: command.to_string(),
        });

        // Legacy: Still print for now
        println!("{} Unknown command: {}", "?".yellow(), command);
        println!("Type {} for available commands", "/help".cyan());
    }

    fn handle_exit(&self) {
        let mut state = self.state.lock().unwrap();
        state.emit(TuiEvent::Exit);

        // Legacy: Still print for now
        println!("Bye!");
    }

    // ==================== Legacy Print Functions ====================
    // TODO: Remove these once we migrate to Ratatui

    fn print_welcome(&self, info: &WelcomeInfo) {
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
        println!(
            "  {} - Consult quorum (Quorum Discussion)",
            "/discuss".cyan()
        );
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

    fn print_help(&self) {
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
    }

    fn print_config(&self, snapshot: &ConfigSnapshot) {
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

    fn print_agent_result(&self, result: &AgentResultEvent) {
        use crate::ConsoleFormatter;

        println!();
        println!("{}", "━".repeat(60).dimmed());
        println!();
        println!("{}", "═══════════════════════════════════════════".cyan());
        println!("{}", "  Agent Execution Summary".bold().cyan());
        println!("{}", "═══════════════════════════════════════════".cyan());
        println!();

        if result.success {
            println!("  {} {}", "Status:".bold(), "SUCCESS".green().bold());
        } else {
            println!("  {} {}", "Status:".bold(), "FAILED".red().bold());
        }
        println!();

        // Show plan details if available
        if let Some(plan) = &result.state.plan {
            // Show Quorum Journey
            if !plan.review_history.is_empty() {
                println!("  {} Quorum Journey:", "🗳️".bold());
                for round in &plan.review_history {
                    let status_icon = if round.approved { "✓" } else { "✗" };
                    let status_color: fn(&str) -> colored::ColoredString = if round.approved {
                        |s| s.green()
                    } else {
                        |s| s.red()
                    };

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

            // Show task details
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

                if task.status == quorum_domain::TaskStatus::Failed
                    && let Some(task_result) = &task.result
                    && let Some(error) = &task_result.error
                {
                    println!("       {} {}", "└─".dimmed(), error.red());
                }
            }
        } else {
            println!("{}", "Summary:".bold());
            println!("{}", ConsoleFormatter::indent(&result.summary, "  "));
        }

        println!();
        println!("{}", "═══════════════════════════════════════════".cyan());

        // Show thought summary in verbose mode
        if result.verbose && !result.thoughts.is_empty() {
            use crate::agent::thought::summarize_thoughts;
            println!();
            println!("{}", "Thought Process:".bold().dimmed());
            println!(
                "{}",
                ConsoleFormatter::indent(&summarize_thoughts(&result.thoughts), "  ")
            );
        }

        println!();
    }

    fn print_context_init_result(&self, result: &ContextInitResultEvent) {
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

impl Default for TuiPresenter {
    fn default() -> Self {
        Self::new()
    }
}

/// Truncate model name for compact display
fn truncate_model_name(model: &str) -> &str {
    model.split(['-', '_']).next().unwrap_or(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_presenter_creation() {
        let presenter = TuiPresenter::new();
        let state = presenter.state();
        let mut locked_state = state.lock().unwrap();
        
        // State should be initialized with no pending events
        assert!(locked_state.poll_event().is_none());
    }

    #[test]
    fn test_welcome_event() {
        let presenter = TuiPresenter::new();
        let info = WelcomeInfo {
            decision_model: quorum_domain::Model::ClaudeSonnet45,
            review_models: vec![
                quorum_domain::Model::Gpt52Codex,
            ],
            moderator: Some(quorum_domain::Model::ClaudeOpus45),
            working_dir: Some("/test/dir".to_string()),
            consensus_level: ConsensusLevel::Ensemble,
        };

        presenter.render(&UiEvent::Welcome(info.clone()));

        let state = presenter.state();
        let mut locked_state = state.lock().unwrap();
        
        if let Some(TuiEvent::Welcome { decision_model, review_models, consensus_level, .. }) = locked_state.poll_event() {
            assert_eq!(decision_model, "claude-sonnet-4.5");
            assert_eq!(review_models.len(), 1);
            assert_eq!(consensus_level, ConsensusLevel::Ensemble);
        } else {
            panic!("Expected Welcome event");
        }
    }

    #[test]
    fn test_mode_changed_event() {
        let presenter = TuiPresenter::new();
        
        presenter.render(&UiEvent::ModeChanged {
            level: ConsensusLevel::Solo,
            description: "Fast execution".to_string(),
        });

        let state = presenter.state();
        let mut locked_state = state.lock().unwrap();
        
        if let Some(TuiEvent::ModeChanged { level, description }) = locked_state.poll_event() {
            assert_eq!(level, ConsensusLevel::Solo);
            assert_eq!(description, "Fast execution");
        } else {
            panic!("Expected ModeChanged event");
        }
    }

    #[test]
    fn test_scope_changed_event() {
        let presenter = TuiPresenter::new();
        
        presenter.render(&UiEvent::ScopeChanged {
            scope: PhaseScope::Fast,
            description: "Skip reviews".to_string(),
        });

        let state = presenter.state();
        let mut locked_state = state.lock().unwrap();
        
        if let Some(TuiEvent::ScopeChanged { scope, description }) = locked_state.poll_event() {
            assert_eq!(scope, PhaseScope::Fast);
            assert_eq!(description, "Skip reviews");
        } else {
            panic!("Expected ScopeChanged event");
        }
    }

    #[test]
    fn test_agent_starting_event() {
        let presenter = TuiPresenter::new();
        
        presenter.render(&UiEvent::AgentStarting {
            mode: ConsensusLevel::Ensemble,
        });

        let state = presenter.state();
        let mut locked_state = state.lock().unwrap();
        
        if let Some(TuiEvent::AgentStarting { mode }) = locked_state.poll_event() {
            assert_eq!(mode, ConsensusLevel::Ensemble);
        } else {
            panic!("Expected AgentStarting event");
        }
    }

    #[test]
    fn test_quorum_starting_event() {
        let presenter = TuiPresenter::new();
        
        presenter.render(&UiEvent::QuorumStarting);

        let state = presenter.state();
        let mut locked_state = state.lock().unwrap();
        
        assert!(matches!(locked_state.poll_event(), Some(TuiEvent::QuorumStarting)));
    }

    #[test]
    fn test_exit_event() {
        let presenter = TuiPresenter::new();
        
        presenter.render(&UiEvent::Exit);

        let state = presenter.state();
        let mut locked_state = state.lock().unwrap();
        
        assert!(matches!(locked_state.poll_event(), Some(TuiEvent::Exit)));
    }

    #[test]
    fn test_multiple_events() {
        let presenter = TuiPresenter::new();
        
        presenter.render(&UiEvent::QuorumStarting);
        presenter.render(&UiEvent::HistoryCleared);
        presenter.render(&UiEvent::Exit);

        let state = presenter.state();
        let mut locked_state = state.lock().unwrap();
        
        assert!(matches!(locked_state.poll_event(), Some(TuiEvent::QuorumStarting)));
        assert!(matches!(locked_state.poll_event(), Some(TuiEvent::HistoryCleared)));
        assert!(matches!(locked_state.poll_event(), Some(TuiEvent::Exit)));
        assert!(locked_state.poll_event().is_none());
    }

    #[test]
    fn test_command_error_event() {
        let presenter = TuiPresenter::new();
        
        presenter.render(&UiEvent::CommandError {
            message: "Test error".to_string(),
        });

        let state = presenter.state();
        let mut locked_state = state.lock().unwrap();
        
        if let Some(TuiEvent::CommandError { message }) = locked_state.poll_event() {
            assert_eq!(message, "Test error");
        } else {
            panic!("Expected CommandError event");
        }
    }

    #[test]
    fn test_unknown_command_event() {
        let presenter = TuiPresenter::new();
        
        presenter.render(&UiEvent::UnknownCommand {
            command: "/unknown".to_string(),
        });

        let state = presenter.state();
        let mut locked_state = state.lock().unwrap();
        
        if let Some(TuiEvent::UnknownCommand { command }) = locked_state.poll_event() {
            assert_eq!(command, "/unknown");
        } else {
            panic!("Expected UnknownCommand event");
        }
    }

    #[test]
    fn test_truncate_model_name() {
        assert_eq!(truncate_model_name("claude-sonnet-4.5"), "claude");
        assert_eq!(truncate_model_name("gpt_5_2"), "gpt");
        assert_eq!(truncate_model_name("simple"), "simple");
        assert_eq!(truncate_model_name(""), "");
    }

    #[test]
    fn test_default_impl() {
        let presenter = TuiPresenter::default();
        let state = presenter.state();
        let mut locked_state = state.lock().unwrap();
        
        assert!(locked_state.poll_event().is_none());
    }
}
