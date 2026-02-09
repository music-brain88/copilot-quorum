//! TUI Human Intervention - Interactive decision making with TUI state
//!
//! This module provides a TUI-aware implementation of HumanInterventionPort,
//! updating TUI state and mode when intervention is required.

use super::event::TuiEvent;
use super::state::{TuiMode, TuiState};
use async_trait::async_trait;
use colored::Colorize;
use quorum_application::ports::human_intervention::{
    HumanInterventionError, HumanInterventionPort,
};
use quorum_domain::core::string::truncate;
use quorum_domain::{HumanDecision, Plan, ReviewRound};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

/// TUI-aware Human Intervention handler
///
/// Implements HumanInterventionPort by:
/// 1. Switching TUI mode to HumanIntervention
/// 2. Updating TUI state with plan details
/// 3. Reading user commands from stdin
/// 4. Returning decision back to the application layer
pub struct TuiHumanIntervention {
    state: Arc<Mutex<TuiState>>,
}

impl TuiHumanIntervention {
    pub fn new(state: Arc<Mutex<TuiState>>) -> Self {
        Self { state }
    }

    /// Display the intervention prompt UI
    fn display_intervention_prompt(
        &self,
        request: &str,
        plan: &Plan,
        review_history: &[ReviewRound],
    ) {
        // Update state
        {
            let mut state = self.state.lock().unwrap();
            state.set_mode(TuiMode::HumanIntervention);
            state.emit(TuiEvent::HumanInterventionPrompt {
                request: request.to_string(),
                objective: plan.objective.clone(),
                tasks: plan
                    .tasks
                    .iter()
                    .map(|t| t.description.clone())
                    .collect(),
                review_count: review_history.len(),
            });
        }

        println!();
        println!(
            "{}",
            "═══════════════════════════════════════════════════════════════"
                .yellow()
                .bold()
        );
        println!(
            "{}",
            "  ⚠️  Plan Requires Human Intervention".yellow().bold()
        );
        println!(
            "{}",
            "═══════════════════════════════════════════════════════════════"
                .yellow()
                .bold()
        );
        println!();

        let revision_count = review_history.iter().filter(|r| !r.approved).count();
        println!(
            "Revision limit ({}) exceeded. Quorum could not reach consensus.",
            revision_count
        );
        println!();

        println!("{}", "Request:".cyan().bold());
        println!("  {}", request.dimmed());
        println!();

        println!("{}", "Plan Objective:".cyan().bold());
        println!("  {}", plan.objective);
        println!();

        if !plan.tasks.is_empty() {
            println!("{}", "Tasks:".cyan().bold());
            for (i, task) in plan.tasks.iter().enumerate() {
                println!("  {}. {}", i + 1, task.description);
            }
            println!();
        }

        if !review_history.is_empty() {
            println!("{}", "Review History:".cyan().bold());
            for round in review_history {
                let status = if round.approved {
                    "APPROVED".green()
                } else {
                    "REJECTED".red()
                };
                println!("  Rev {}: {} {}", round.round, status, round.vote_summary());

                for vote in &round.votes {
                    if !vote.approved {
                        let feedback = truncate(&vote.reasoning, 80);
                        println!("    └─ {}: {}", vote.model.dimmed(), feedback);
                    }
                }
            }
            println!();
        }

        println!("{}", "Commands:".cyan().bold());
        println!("  {}  - Execute this plan as-is", "/approve".green());
        println!("  {}   - Abort the agent", "/reject".red());
        println!(
            "  {}     - Edit plan (feature coming soon)",
            "/edit".yellow()
        );
        println!();
    }

    /// Read user command
    fn read_command(&self) -> Result<String, HumanInterventionError> {
        print!("{} ", "agent-hil>".magenta().bold());
        io::stdout().flush().map_err(|e| {
            HumanInterventionError::IoError(format!("Failed to flush stdout: {}", e))
        })?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|e| HumanInterventionError::IoError(format!("Failed to read input: {}", e)))?;

        Ok(input.trim().to_string())
    }
}

impl Default for TuiHumanIntervention {
    fn default() -> Self {
        Self::new(Arc::new(Mutex::new(TuiState::default())))
    }
}

#[async_trait]
impl HumanInterventionPort for TuiHumanIntervention {
    async fn request_intervention(
        &self,
        request: &str,
        plan: &Plan,
        review_history: &[ReviewRound],
    ) -> Result<HumanDecision, HumanInterventionError> {
        self.display_intervention_prompt(request, plan, review_history);

        loop {
            let input = self.read_command()?;

            match input.to_lowercase().as_str() {
                "/approve" | "approve" | "a" => {
                    // Update state
                    {
                        let mut state = self.state.lock().unwrap();
                        state.emit(TuiEvent::HumanDecision {
                            decision: "approve".to_string(),
                        });
                        state.set_mode(TuiMode::Normal); // Return to normal mode
                    }

                    println!();
                    println!("{}", "✓ Plan approved by human intervention".green());
                    return Ok(HumanDecision::Approve);
                }
                "/reject" | "reject" | "r" | "q" => {
                    // Update state
                    {
                        let mut state = self.state.lock().unwrap();
                        state.emit(TuiEvent::HumanDecision {
                            decision: "reject".to_string(),
                        });
                        state.set_mode(TuiMode::Normal); // Return to normal mode
                    }

                    println!();
                    println!("{}", "✗ Plan rejected by human".red());
                    return Ok(HumanDecision::Reject);
                }
                "/edit" | "edit" | "e" => {
                    println!();
                    println!("{}", "⚠️  Plan editing is not yet implemented.".yellow());
                    println!("Please use /approve or /reject.");
                    println!();
                }
                "" => {
                    // Empty input, show prompt again
                    continue;
                }
                _ => {
                    println!();
                    println!("{} Unknown command: {}", "⚠️".yellow(), input.red());
                    println!("Available commands: /approve, /reject, /edit");
                    println!();
                }
            }
        }
    }
}
