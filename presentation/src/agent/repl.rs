//! REPL (Read-Eval-Print Loop) for agent mode

use crate::agent::progress::AgentProgressReporter;
use crate::agent::thought::summarize_thoughts;
use crate::ConsoleFormatter;
use colored::Colorize;
use quorum_application::{LlmGateway, RunAgentInput, RunAgentUseCase, ToolExecutorPort};
use quorum_domain::{AgentConfig, Model};
use rustyline::error::ReadlineError;
use rustyline::{DefaultEditor, Result as RlResult};
use std::sync::Arc;

/// Interactive REPL for agent mode
pub struct AgentRepl<G: LlmGateway + 'static, T: ToolExecutorPort + 'static> {
    use_case: RunAgentUseCase<G, T>,
    config: AgentConfig,
    verbose: bool,
    working_dir: Option<String>,
}

impl<G: LlmGateway + 'static, T: ToolExecutorPort + 'static> AgentRepl<G, T> {
    /// Create a new AgentRepl
    pub fn new(gateway: Arc<G>, tool_executor: Arc<T>, primary_model: Model) -> Self {
        Self {
            use_case: RunAgentUseCase::new(gateway, tool_executor),
            config: AgentConfig::new(primary_model),
            verbose: false,
            working_dir: None,
        }
    }

    /// Set quorum models for review
    pub fn with_quorum_models(mut self, models: Vec<Model>) -> Self {
        self.config = self.config.with_quorum_models(models);
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

    /// Run the interactive REPL
    pub async fn run(&self) -> RlResult<()> {
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
            let readline = rl.readline("agent> ");

            match readline {
                Ok(line) => {
                    let line = line.trim();

                    // Skip empty lines
                    if line.is_empty() {
                        continue;
                    }

                    // Handle commands
                    if line.starts_with('/') {
                        match self.handle_command(line) {
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
        println!(
            "{} {}",
            "Primary Model:".bold(),
            self.config.primary_model
        );
        if !self.config.quorum_models.is_empty() {
            println!(
                "{} {}",
                "Quorum Models:".bold(),
                self.config
                    .quorum_models
                    .iter()
                    .map(|m| m.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if let Some(ref dir) = self.working_dir {
            println!("{} {}", "Working Dir:".bold(), dir);
        }
        println!();
        println!("{}", "The agent will:".dimmed());
        println!("{}", "  1. Gather context about your project".dimmed());
        println!("{}", "  2. Create a plan (reviewed by quorum)".dimmed());
        println!("{}", "  3. Execute tasks (high-risk ops reviewed)".dimmed());
        println!();
        println!("Commands:");
        println!("  {}    - Show this help", "/help".cyan());
        println!("  {}  - Show current configuration", "/config".cyan());
        println!("  {}    - Exit agent mode", "/quit".cyan());
        println!();
    }

    /// Handle slash commands. Returns whether to continue or exit.
    fn handle_command(&self, cmd: &str) -> CommandResult {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let cmd = parts.first().copied().unwrap_or("");

        match cmd {
            "/quit" | "/exit" | "/q" => {
                println!("Bye!");
                CommandResult::Exit
            }
            "/help" | "/h" | "/?" => {
                println!();
                println!("{}", "Commands:".bold());
                println!("  /help, /h, /?   - Show this help");
                println!("  /config         - Show current configuration");
                println!("  /verbose        - Toggle verbose mode");
                println!("  /quit, /exit, /q - Exit agent mode");
                println!();
                println!("{}", "Usage:".bold());
                println!("  Type your request and press Enter.");
                println!("  The agent will create a plan and execute it.");
                println!("  High-risk operations require quorum approval.");
                println!();
                println!("{}", "Examples:".bold());
                println!("  - \"Fix the bug in login.rs\"");
                println!("  - \"Add unit tests for the User struct\"");
                println!("  - \"Refactor the error handling in api.rs\"");
                println!();
                CommandResult::Continue
            }
            "/config" => {
                println!();
                println!("{}", "Current Configuration:".bold().cyan());
                println!("  Primary Model:    {}", self.config.primary_model);
                println!(
                    "  Quorum Models:    {}",
                    if self.config.quorum_models.is_empty() {
                        "None (auto-approve)".to_string()
                    } else {
                        self.config
                            .quorum_models
                            .iter()
                            .map(|m| m.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                );
                println!("  Plan Review:      {}", "Always required".green());
                println!(
                    "  Final Review:     {}",
                    if self.config.require_final_review {
                        "Enabled"
                    } else {
                        "Disabled"
                    }
                );
                println!("  Max Iterations:   {}", self.config.max_iterations);
                println!(
                    "  Working Dir:      {}",
                    self.working_dir.as_deref().unwrap_or("(current)")
                );
                println!("  Verbose:          {}", self.verbose);
                println!();
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
                println!("{} Unknown command: {}", "?".yellow(), cmd);
                println!("Type {} for available commands", "/help".cyan());
                CommandResult::Continue
            }
        }
    }

    async fn process_request(&self, request: &str) {
        println!();
        println!(
            "{} {}",
            "━".repeat(50).dimmed(),
            "Agent Starting".bold().cyan()
        );
        println!();

        let input = RunAgentInput::new(request, self.config.clone());

        let result = if self.verbose {
            let progress = AgentProgressReporter::verbose();
            self.use_case
                .execute_with_progress(input, &progress)
                .await
        } else {
            let progress = AgentProgressReporter::new();
            self.use_case
                .execute_with_progress(input, &progress)
                .await
        };

        println!();
        println!(
            "{}",
            "━".repeat(60).dimmed()
        );

        match result {
            Ok(output) => {
                if output.success {
                    println!("{} {}", "✅".green(), "Agent completed successfully".green().bold());
                } else {
                    println!("{} {}", "⚠️".yellow(), "Agent completed with issues".yellow().bold());
                }
                println!();
                println!("{}", "Summary:".bold());
                println!("{}", ConsoleFormatter::indent(&output.summary, "  "));

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
                println!("{} {}", "❌".red(), "Agent failed".red().bold());
                println!();
                println!("{} {}", "Error:".red().bold(), e);
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
