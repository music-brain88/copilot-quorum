//! Interactive human intervention for agent execution.
//!
//! This module provides a CLI interface for users to make decisions when
//! the plan revision limit is exceeded and `HilMode::Interactive` is set.
//!
//! # User Interface
//!
//! When intervention is required, the user sees:
//!
//! ```text
//! ═══════════════════════════════════════════════════════════════
//!   ⚠️  Plan Requires Human Intervention
//! ═══════════════════════════════════════════════════════════════
//!
//! Revision limit (3) exceeded. Quorum could not reach consensus.
//!
//! Request:
//!   <original user request>
//!
//! Plan Objective:
//!   <plan objective>
//!
//! Tasks:
//!   1. <task 1>
//!   2. <task 2>
//!
//! Review History:
//!   Rev 1: REJECTED [○●○]
//!     └─ model-name: <feedback>
//!
//! Commands:
//!   /approve  - Execute this plan as-is
//!   /reject   - Abort the agent
//!   /edit     - Edit plan manually (coming soon)
//!
//! agent-hil>
//! ```
//!
//! # Commands
//!
//! | Command | Aliases | Description |
//! |---------|---------|-------------|
//! | `/approve` | `approve`, `a` | Execute the current plan |
//! | `/reject` | `reject`, `r`, `q` | Abort the agent |
//! | `/edit` | `edit`, `e` | Edit plan (not yet implemented) |

use async_trait::async_trait;
use colored::Colorize;
use quorum_application::ports::human_intervention::{
    HumanInterventionError, HumanInterventionPort,
};
use quorum_domain::core::string::truncate;
use quorum_domain::{HumanDecision, Plan, ReviewRound};
use std::io::{self, Write};

/// Interactive human intervention handler for CLI.
///
/// Implements [`HumanInterventionPort`] to provide a terminal-based
/// interface for user decisions during agent execution.
///
/// # Example
///
/// ```ignore
/// use quorum_presentation::InteractiveHumanIntervention;
/// use std::sync::Arc;
///
/// let intervention = Arc::new(InteractiveHumanIntervention::new());
/// let use_case = RunAgentUseCase::new(gateway, executor)
///     .with_human_intervention(intervention);
/// ```
pub struct InteractiveHumanIntervention;

impl InteractiveHumanIntervention {
    pub fn new() -> Self {
        Self
    }

    /// Display the intervention prompt UI
    fn display_intervention_prompt(
        &self,
        request: &str,
        plan: &Plan,
        review_history: &[ReviewRound],
    ) {
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

        // Show revision count
        let revision_count = review_history.iter().filter(|r| !r.approved).count();
        println!(
            "Revision limit ({}) exceeded. Quorum could not reach consensus.",
            revision_count
        );
        println!();

        // Show original request
        println!("{}", "Request:".cyan().bold());
        println!("  {}", request.dimmed());
        println!();

        // Show plan objective
        println!("{}", "Plan Objective:".cyan().bold());
        println!("  {}", plan.objective);
        println!();

        // Show tasks
        if !plan.tasks.is_empty() {
            println!("{}", "Tasks:".cyan().bold());
            for (i, task) in plan.tasks.iter().enumerate() {
                println!("  {}. {}", i + 1, task.description);
            }
            println!();
        }

        // Show review history
        if !review_history.is_empty() {
            println!("{}", "Review History:".cyan().bold());
            for round in review_history {
                let status = if round.approved {
                    "APPROVED".green()
                } else {
                    "REJECTED".red()
                };
                println!("  Rev {}: {} {}", round.round, status, round.vote_summary());

                // Show feedback from rejecting models
                for vote in &round.votes {
                    if !vote.approved {
                        let feedback = truncate(&vote.reasoning, 80);
                        println!("    └─ {}: {}", vote.model.dimmed(), feedback);
                    }
                }
            }
            println!();
        }

        // Show commands
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

impl Default for InteractiveHumanIntervention {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HumanInterventionPort for InteractiveHumanIntervention {
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
                    println!();
                    println!("{}", "✓ Plan approved by human intervention".green());
                    return Ok(HumanDecision::Approve);
                }
                "/reject" | "reject" | "r" | "q" => {
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
