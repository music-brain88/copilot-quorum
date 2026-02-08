//! REPL (Read-Eval-Print Loop) for agent mode
//!
//! This module provides a thin shell that wires together:
//! - AgentController (application layer) for business logic
//! - ReplPresenter (presentation layer) for display
//! - rustyline for terminal input
//! - mpsc channel for UiEvent delivery

use crate::agent::presenter::ReplPresenter;
use crate::agent::progress::AgentProgressReporter;
use quorum_application::{
    AgentController, CommandAction, ContextLoaderPort, LlmGateway, ToolExecutorPort, UiEvent,
};
use quorum_domain::{AgentConfig, ConsensusLevel, Model};
use rustyline::error::ReadlineError;
use rustyline::{DefaultEditor, Result as RlResult};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::agent::human_intervention::InteractiveHumanIntervention;

/// Interactive REPL for agent mode
///
/// A thin shell that delegates business logic to AgentController
/// and display to ReplPresenter, connected via UiEvent channel.
pub struct AgentRepl<
    G: LlmGateway + 'static,
    T: ToolExecutorPort + 'static,
    C: ContextLoaderPort + 'static,
> {
    controller: AgentController<G, T, C>,
    presenter: ReplPresenter,
    rx: mpsc::UnboundedReceiver<UiEvent>,
    verbose: bool,
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
        let (tx, rx) = mpsc::unbounded_channel();
        let human_intervention = Arc::new(InteractiveHumanIntervention::new());

        let controller = AgentController::new(
            gateway,
            tool_executor,
            context_loader,
            config,
            human_intervention,
            tx,
        );

        Self {
            controller,
            presenter: ReplPresenter::new(),
            rx,
            verbose: false,
        }
    }

    /// Set moderator model for synthesis
    pub fn with_moderator(mut self, model: Model) -> Self {
        self.controller = self.controller.with_moderator(model);
        self
    }

    /// Enable verbose output
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self.controller = self.controller.with_verbose(verbose);
        self
    }

    /// Set working directory
    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.controller = self.controller.with_working_dir(dir);
        self
    }

    /// Enable final review
    pub fn with_final_review(mut self, enable: bool) -> Self {
        self.controller = self.controller.with_final_review(enable);
        self
    }

    /// Set cancellation token for graceful shutdown
    pub fn with_cancellation(mut self, token: CancellationToken) -> Self {
        self.controller = self.controller.with_cancellation(token);
        self
    }

    /// Set initial consensus level (Solo or Ensemble)
    pub fn with_consensus_level(mut self, level: ConsensusLevel) -> Self {
        self.controller = self.controller.with_consensus_level(level);
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

        self.controller.send_welcome();
        self.drain_events();

        loop {
            let prompt = self.controller.prompt_string();

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
                        match self.controller.handle_command(line).await {
                            CommandAction::Exit => {
                                self.drain_events();
                                break;
                            }
                            CommandAction::Continue => {
                                self.drain_events();
                                continue;
                            }
                        }
                    }

                    // Add to history
                    let _ = rl.add_history_entry(line);

                    // Run agent with progress reporter from presentation layer
                    let progress = if self.verbose {
                        AgentProgressReporter::verbose()
                    } else {
                        AgentProgressReporter::new()
                    };
                    self.controller.process_request(line, &progress).await;
                    self.drain_events();
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

    /// Drain all pending UiEvents from the channel and render them
    fn drain_events(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            self.presenter.render(&event);
        }
    }
}
