//! REPL (Read-Eval-Print Loop) for interactive chat

use crate::ConsoleFormatter;
use crate::ProgressReporter;
use quorum_application::{LlmGateway, RunQuorumInput, RunQuorumUseCase};
use quorum_domain::Model;
use rustyline::error::ReadlineError;
use rustyline::{DefaultEditor, Result as RlResult};
use std::sync::Arc;

/// Interactive chat REPL
pub struct ChatRepl<G: LlmGateway + 'static> {
    use_case: RunQuorumUseCase<G>,
    models: Vec<Model>,
    show_progress: bool,
    skip_review: bool,
}

impl<G: LlmGateway + 'static> ChatRepl<G> {
    /// Create a new ChatRepl
    pub fn new(gateway: Arc<G>, models: Vec<Model>) -> Self {
        Self {
            use_case: RunQuorumUseCase::new(gateway),
            models,
            show_progress: true,
            skip_review: false,
        }
    }

    /// Set whether to show progress
    pub fn with_progress(mut self, show: bool) -> Self {
        self.show_progress = show;
        self
    }

    /// Set whether to skip review phase
    pub fn with_skip_review(mut self, skip: bool) -> Self {
        self.skip_review = skip;
        self
    }

    /// Run the interactive REPL
    pub async fn run(&self) -> RlResult<()> {
        let mut rl = DefaultEditor::new()?;

        // Try to load history
        let history_path = dirs::data_dir().map(|p| p.join("copilot-quorum").join("history.txt"));

        if let Some(ref path) = history_path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = rl.load_history(path);
        }

        self.print_welcome();

        loop {
            let readline = rl.readline(">>> ");

            match readline {
                Ok(line) => {
                    let line = line.trim();

                    // Skip empty lines
                    if line.is_empty() {
                        continue;
                    }

                    // Handle commands
                    if line.starts_with('/') {
                        if self.handle_command(line) {
                            break;
                        }
                        continue;
                    }

                    // Add to history
                    let _ = rl.add_history_entry(line);

                    // Run quorum
                    self.process_question(line).await;
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
        println!("╭─────────────────────────────────────────────╮");
        println!("│         Copilot Quorum - Chat Mode          │");
        println!("╰─────────────────────────────────────────────╯");
        println!();
        println!(
            "Models: {}",
            self.models
                .iter()
                .map(|m| m.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!();
        println!("Commands:");
        println!("  /help     - Show this help");
        println!("  /models   - Show current models");
        println!("  /quit     - Exit chat");
        println!();
    }

    /// Handle slash commands. Returns true if should exit.
    fn handle_command(&self, cmd: &str) -> bool {
        match cmd {
            "/quit" | "/exit" | "/q" => {
                println!("Bye!");
                true
            }
            "/help" | "/h" | "/?" => {
                println!();
                println!("Commands:");
                println!("  /help, /h, /?   - Show this help");
                println!("  /models         - Show current models");
                println!("  /quit, /exit, /q - Exit chat");
                println!();
                false
            }
            "/models" => {
                println!();
                println!("Current models:");
                for model in &self.models {
                    println!("  - {}", model);
                }
                println!();
                false
            }
            _ => {
                println!("Unknown command: {}", cmd);
                println!("Type /help for available commands");
                false
            }
        }
    }

    async fn process_question(&self, question: &str) {
        println!();

        let mut input = RunQuorumInput::new(question.to_string(), self.models.clone());

        if self.skip_review {
            input = input.without_review();
        }

        let result = if self.show_progress {
            let progress = ProgressReporter::new();
            self.use_case.execute_with_progress(input, &progress).await
        } else {
            self.use_case.execute(input).await
        };

        match result {
            Ok(result) => {
                let output = ConsoleFormatter::format_synthesis_only(&result);
                println!("{}", output);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
        println!();
    }
}
