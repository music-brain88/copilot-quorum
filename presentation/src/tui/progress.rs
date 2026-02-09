//! TUI Progress Reporter - Converts AgentProgressNotifier callbacks to TUI state
//!
//! This module adapts the AgentProgressNotifier trait to emit TUI events,
//! maintaining compatibility with the existing progress reporting system
//! while preparing for full TUI rendering.

use super::event::TuiEvent;
use super::state::TuiState;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use quorum_application::{AgentProgressNotifier, ErrorCategory};
use quorum_domain::core::string::truncate;
use quorum_domain::{AgentPhase, Model, Task, Thought};
use std::io::Write;
use std::sync::{Arc, Mutex};

/// TUI-aware Progress Reporter
///
/// Implements AgentProgressNotifier by updating TUI state and (for now)
/// also printing to stdout for compatibility.
pub struct TuiProgressReporter {
    state: Arc<Mutex<TuiState>>,
    multi: MultiProgress,
    phase_bar: Mutex<Option<ProgressBar>>,
    task_bar: Mutex<Option<ProgressBar>>,
    quorum_bar: Mutex<Option<ProgressBar>>,
    verbose: bool,
    show_votes: bool,
}

impl TuiProgressReporter {
    pub fn new(state: Arc<Mutex<TuiState>>) -> Self {
        Self {
            state,
            multi: MultiProgress::new(),
            phase_bar: Mutex::new(None),
            task_bar: Mutex::new(None),
            quorum_bar: Mutex::new(None),
            verbose: false,
            show_votes: false,
        }
    }

    pub fn with_options(state: Arc<Mutex<TuiState>>, verbose: bool, show_votes: bool) -> Self {
        Self {
            state,
            multi: MultiProgress::new(),
            phase_bar: Mutex::new(None),
            task_bar: Mutex::new(None),
            quorum_bar: Mutex::new(None),
            verbose,
            show_votes,
        }
    }

    fn phase_style() -> ProgressStyle {
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {prefix:.bold.cyan} {msg}")
            .unwrap()
    }

    fn quorum_style() -> ProgressStyle {
        ProgressStyle::default_bar()
            .template("    {spinner:.yellow} {prefix:.bold.yellow} [{bar:20.yellow/dim}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("●○-")
    }

    fn phase_emoji(phase: &AgentPhase) -> &'static str {
        match phase {
            AgentPhase::ContextGathering => "🔍",
            AgentPhase::Planning => "📝",
            AgentPhase::PlanReview => "🗳️",
            AgentPhase::Executing => "⚡",
            AgentPhase::ActionReview => "🔒",
            AgentPhase::FinalReview => "✅",
            AgentPhase::Completed => "🎉",
            AgentPhase::Failed => "❌",
        }
    }

    fn phase_name(phase: &AgentPhase) -> &'static str {
        match phase {
            AgentPhase::ContextGathering => "Gathering Context",
            AgentPhase::Planning => "Planning",
            AgentPhase::PlanReview => "Plan Review (Quorum)",
            AgentPhase::Executing => "Executing Tasks",
            AgentPhase::ActionReview => "Action Review (Quorum)",
            AgentPhase::FinalReview => "Final Review (Quorum)",
            AgentPhase::Completed => "Complete",
            AgentPhase::Failed => "Failed",
        }
    }

    fn finish_current_phase(&self) {
        if let Some(pb) = self.phase_bar.lock().unwrap().take() {
            pb.finish_and_clear();
        }
        if let Some(pb) = self.task_bar.lock().unwrap().take() {
            pb.finish_and_clear();
        }
        if let Some(pb) = self.quorum_bar.lock().unwrap().take() {
            pb.finish_and_clear();
        }
    }
}

impl AgentProgressNotifier for TuiProgressReporter {
    fn on_phase_change(&self, phase: &AgentPhase) {
        // Update state
        {
            let mut state = self.state.lock().unwrap();
            state.emit(TuiEvent::PhaseChange {
                phase: phase.clone(),
                name: Self::phase_name(phase).to_string(),
            });
        }

        self.finish_current_phase();

        let emoji = Self::phase_emoji(phase);
        let name = Self::phase_name(phase);

        if matches!(phase, AgentPhase::Completed | AgentPhase::Failed) {
            println!();
            if matches!(phase, AgentPhase::Completed) {
                println!("{} {}", emoji, name.green().bold());
            } else {
                println!("{} {}", emoji, name.red().bold());
            }
            return;
        }

        let pb = self.multi.add(ProgressBar::new_spinner());
        pb.set_style(Self::phase_style());
        pb.set_prefix(format!("{} {}", emoji, name));
        pb.set_message("...");
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        *self.phase_bar.lock().unwrap() = Some(pb);
    }

    fn on_thought(&self, thought: &Thought) {
        // Update state
        {
            let mut state = self.state.lock().unwrap();
            state.emit(TuiEvent::Thought {
                thought_type: format!("{:?}", thought.thought_type),
                content: thought.content.clone(),
            });
        }

        if self.verbose {
            let type_str = format!("{:?}", thought.thought_type);
            println!(
                "    {} {}: {}",
                "💭".dimmed(),
                type_str.dimmed(),
                thought.content.dimmed()
            );
        }
    }

    fn on_task_start(&self, task: &Task) {
        // Update state
        {
            let mut state = self.state.lock().unwrap();
            state.emit(TuiEvent::TaskStart {
                description: task.description.clone(),
            });
        }

        if let Some(pb) = self.phase_bar.lock().unwrap().as_ref() {
            pb.set_message(format!("Task: {}", truncate(&task.description, 40)));
        }

        if self.verbose {
            println!("    {} Starting: {}", "→".blue(), task.description);
        }
    }

    fn on_task_complete(&self, task: &Task, success: bool) {
        // Update state
        {
            let mut state = self.state.lock().unwrap();
            state.emit(TuiEvent::TaskComplete {
                description: task.description.clone(),
                success,
            });
        }

        if self.verbose {
            if success {
                println!(
                    "    {} Completed: {}",
                    "✓".green(),
                    task.description.green()
                );
            } else {
                println!("    {} Failed: {}", "✗".red(), task.description.red());
            }
        }
    }

    fn on_tool_call(&self, tool_name: &str, args: &str) {
        // Update state
        {
            let mut state = self.state.lock().unwrap();
            state.emit(TuiEvent::ToolCall {
                tool_name: tool_name.to_string(),
                args: args.to_string(),
            });
        }

        let args_display = truncate(args, 50);
        println!(
            "      {} {} {}",
            "🔧".dimmed(),
            tool_name.cyan(),
            args_display.dimmed()
        );

        if let Some(pb) = self.phase_bar.lock().unwrap().as_ref() {
            pb.set_message(format!("Running: {}", tool_name));
        }
    }

    fn on_tool_result(&self, tool_name: &str, success: bool) {
        // Update state
        {
            let mut state = self.state.lock().unwrap();
            state.emit(TuiEvent::ToolResult {
                tool_name: tool_name.to_string(),
                success,
            });
        }

        if success {
            println!(
                "      {} {} {}",
                "✓".green(),
                tool_name.green(),
                "OK".dimmed()
            );
        } else {
            println!(
                "      {} {} {}",
                "✗".red(),
                tool_name.red(),
                "FAILED".dimmed()
            );
        }
    }

    fn on_tool_error(&self, tool_name: &str, category: ErrorCategory, message: &str) {
        // Update state
        {
            let mut state = self.state.lock().unwrap();
            state.emit(TuiEvent::ToolError {
                tool_name: tool_name.to_string(),
                category: format!("{:?}", category),
                message: message.to_string(),
            });
        }

        let emoji = category.emoji();
        let desc = category.description();

        println!(
            "      {} {} {}: {}",
            emoji,
            tool_name.red(),
            desc.red(),
            truncate(message, 60).dimmed()
        );
    }

    fn on_tool_retry(&self, tool_name: &str, attempt: usize, max_retries: usize, error: &str) {
        // Update state
        {
            let mut state = self.state.lock().unwrap();
            state.emit(TuiEvent::ToolRetry {
                tool_name: tool_name.to_string(),
                attempt,
                max_retries,
                error: error.to_string(),
            });
        }

        println!(
            "      {} {} {} ({}/{})",
            "🔄".yellow(),
            tool_name.yellow(),
            format!("Retrying: {}", truncate(error, 40)).dimmed(),
            attempt,
            max_retries
        );
    }

    fn on_tool_not_found(&self, tool_name: &str, available_tools: &[&str]) {
        println!(
            "      ⚠️ Tool {} {} [available: {}]",
            tool_name.red(),
            "not found, resolving...".yellow(),
            available_tools.join(", ").dimmed()
        );
    }

    fn on_tool_resolved(&self, original_name: &str, resolved_name: &str) {
        println!(
            "      {} {} {} {}",
            "✓".green(),
            original_name.dimmed().strikethrough(),
            "→".green(),
            resolved_name.cyan()
        );
        if let Some(pb) = self.phase_bar.lock().unwrap().as_ref() {
            pb.set_message(format!("Running: {}", resolved_name));
        }
    }

    fn on_quorum_start(&self, phase: &str, model_count: usize) {
        // Update state
        {
            let mut state = self.state.lock().unwrap();
            state.emit(TuiEvent::QuorumStart {
                phase: phase.to_string(),
                model_count,
            });
        }

        let pb = self.multi.add(ProgressBar::new(model_count as u64));
        pb.set_style(Self::quorum_style());
        pb.set_prefix(format!("🗳️  {} ", phase));
        pb.set_message(format!("{} models voting...", model_count));

        *self.quorum_bar.lock().unwrap() = Some(pb);
    }

    fn on_quorum_model_complete(&self, model: &Model, approved: bool) {
        if let Some(pb) = self.quorum_bar.lock().unwrap().as_ref() {
            let vote = if approved {
                format!("{} ✓", model).green().to_string()
            } else {
                format!("{} ✗", model).red().to_string()
            };
            pb.set_message(vote);
            pb.inc(1);
        }
    }

    fn on_quorum_complete(&self, phase: &str, approved: bool, feedback: Option<&str>) {
        // Update state
        {
            let mut state = self.state.lock().unwrap();
            state.emit(TuiEvent::QuorumComplete {
                phase: phase.to_string(),
                approved,
                feedback: feedback.map(|s| s.to_string()),
            });
        }

        if let Some(pb) = self.quorum_bar.lock().unwrap().take() {
            if approved {
                pb.finish_with_message(format!("{} {}", phase.green(), "APPROVED".green().bold()));
            } else {
                pb.finish_with_message(format!("{} {}", phase.red(), "REJECTED".red().bold()));
            }
        }

        if !approved && let Some(fb) = feedback {
            println!();
            println!("    {} Feedback:", "ℹ".yellow());
            for line in fb.lines().take(20) {
                println!("      {}", line.yellow());
            }
            let total_lines = fb.lines().count();
            if total_lines > 20 {
                println!(
                    "      {} ...and {} more lines",
                    "".dimmed(),
                    total_lines - 20
                );
            }
        }
    }

    fn on_quorum_complete_with_votes(
        &self,
        phase: &str,
        approved: bool,
        votes: &[(String, bool, String)],
        feedback: Option<&str>,
    ) {
        if let Some(pb) = self.quorum_bar.lock().unwrap().take() {
            let vote_summary: String = votes
                .iter()
                .map(|(_, approved, _)| if *approved { '●' } else { '○' })
                .collect();
            let approve_count = votes.iter().filter(|(_, a, _)| *a).count();
            let total = votes.len();
            let unanimous = approve_count == total || approve_count == 0;

            let status = if approved {
                "APPROVED".green().bold()
            } else {
                "REJECTED".red().bold()
            };

            let consensus = if unanimous {
                "(unanimous)".dimmed()
            } else {
                "(majority)".dimmed()
            };

            pb.finish_with_message(format!(
                "{} {} [{}] {}/{} {}",
                phase, status, vote_summary, approve_count, total, consensus
            ));
        }

        if self.verbose || self.show_votes {
            println!();
            for (model, approved, reasoning) in votes {
                let vote_icon = if *approved {
                    "✓".green()
                } else {
                    "✗".red()
                };
                println!("      {} {}", vote_icon, model);
                if self.show_votes && !reasoning.is_empty() {
                    let snippet = truncate(reasoning, 80);
                    println!("        {} {}", "└─".dimmed(), snippet.dimmed());
                }
            }
        }

        if !approved && let Some(fb) = feedback {
            println!();
            println!("    {} Feedback:", "ℹ".yellow());
            for line in fb.lines().take(20) {
                println!("      {}", line.yellow());
            }
            let total_lines = fb.lines().count();
            if total_lines > 20 {
                println!(
                    "      {} ...and {} more lines",
                    "".dimmed(),
                    total_lines - 20
                );
            }
        }
    }

    fn on_plan_revision(&self, revision: usize, feedback: &str) {
        // Update state
        {
            let mut state = self.state.lock().unwrap();
            state.emit(TuiEvent::PlanRevision {
                revision,
                feedback: feedback.to_string(),
            });
        }

        println!();
        println!(
            "    {} Plan rejected, starting revision #{}",
            "🔄".yellow(),
            revision
        );
        println!(
            "      {} {}",
            "Feedback:".dimmed(),
            truncate(feedback, 60).yellow()
        );
    }

    fn on_action_retry(&self, task: &Task, attempt: usize, feedback: &str) {
        println!(
            "    {} Action retry #{} for task: {}",
            "🔄".yellow(),
            attempt,
            truncate(&task.description, 40)
        );
        println!(
            "      {} {}",
            "Feedback:".dimmed(),
            truncate(feedback, 60).yellow()
        );
    }

    fn on_human_intervention_required(
        &self,
        _request: &str,
        _plan: &quorum_domain::Plan,
        _review_history: &[quorum_domain::ReviewRound],
        max_revisions: usize,
    ) {
        // Update state
        {
            let mut state = self.state.lock().unwrap();
            state.emit(TuiEvent::HumanInterventionRequired { max_revisions });
        }

        self.finish_current_phase();

        println!();
        println!(
            "    {} Plan revision limit ({}) exceeded - human intervention required",
            "⚠️".yellow(),
            max_revisions
        );
    }

    fn on_ensemble_start(&self, model_count: usize) {
        let pb = self.multi.add(ProgressBar::new(model_count as u64));
        pb.set_style(Self::quorum_style());
        pb.set_prefix("🎭  Ensemble Planning ");
        pb.set_message(format!("{} models generating plans...", model_count));

        *self.quorum_bar.lock().unwrap() = Some(pb);
    }

    fn on_ensemble_plan_generated(&self, model: &quorum_domain::Model) {
        if let Some(pb) = self.quorum_bar.lock().unwrap().as_ref() {
            pb.set_message(format!("{} generated plan", model).green().to_string());
            pb.inc(1);
        }
    }

    fn on_ensemble_voting_start(&self, plan_count: usize) {
        if let Some(pb) = self.quorum_bar.lock().unwrap().as_ref() {
            pb.set_message(format!("Voting on {} plans...", plan_count));
        }
    }

    fn on_ensemble_complete(&self, selected_model: &quorum_domain::Model, score: f64) {
        if let Some(pb) = self.quorum_bar.lock().unwrap().take() {
            pb.finish_with_message(format!(
                "{} {} (score: {:.1}/10)",
                "Selected:".green().bold(),
                selected_model.to_string().cyan(),
                score
            ));
        }
    }

    fn on_llm_chunk(&self, chunk: &str) {
        if let Some(pb) = self.phase_bar.lock().unwrap().as_ref() {
            pb.suspend(|| {
                print!("{}", chunk);
                let _ = std::io::stdout().flush();
            });
        } else {
            print!("{}", chunk);
            let _ = std::io::stdout().flush();
        }
    }

    fn on_llm_stream_start(&self, purpose: &str) {
        if self.verbose && !purpose.is_empty() {
            println!("  {} Streaming: {}", "📡".dimmed(), purpose.dimmed());
        }
    }

    fn on_llm_stream_end(&self) {
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reporter_creation() {
        let state = Arc::new(Mutex::new(TuiState::default()));
        let reporter = TuiProgressReporter::new(state.clone());
        
        // Should create without panic
        assert!(reporter.phase_bar.lock().unwrap().is_none());
        assert!(reporter.task_bar.lock().unwrap().is_none());
        assert!(reporter.quorum_bar.lock().unwrap().is_none());
    }

    #[test]
    fn test_reporter_with_options() {
        let state = Arc::new(Mutex::new(TuiState::default()));
        let reporter = TuiProgressReporter::with_options(state.clone(), true, true);
        
        assert!(reporter.verbose);
        assert!(reporter.show_votes);
    }

    #[test]
    fn test_phase_emoji() {
        assert_eq!(TuiProgressReporter::phase_emoji(&AgentPhase::ContextGathering), "🔍");
        assert_eq!(TuiProgressReporter::phase_emoji(&AgentPhase::Planning), "📝");
        assert_eq!(TuiProgressReporter::phase_emoji(&AgentPhase::PlanReview), "🗳️");
        assert_eq!(TuiProgressReporter::phase_emoji(&AgentPhase::Executing), "⚡");
        assert_eq!(TuiProgressReporter::phase_emoji(&AgentPhase::ActionReview), "🔒");
        assert_eq!(TuiProgressReporter::phase_emoji(&AgentPhase::FinalReview), "✅");
        assert_eq!(TuiProgressReporter::phase_emoji(&AgentPhase::Completed), "🎉");
        assert_eq!(TuiProgressReporter::phase_emoji(&AgentPhase::Failed), "❌");
    }

    #[test]
    fn test_phase_name() {
        assert_eq!(TuiProgressReporter::phase_name(&AgentPhase::ContextGathering), "Gathering Context");
        assert_eq!(TuiProgressReporter::phase_name(&AgentPhase::Planning), "Planning");
        assert_eq!(TuiProgressReporter::phase_name(&AgentPhase::PlanReview), "Plan Review (Quorum)");
        assert_eq!(TuiProgressReporter::phase_name(&AgentPhase::Executing), "Executing Tasks");
        assert_eq!(TuiProgressReporter::phase_name(&AgentPhase::ActionReview), "Action Review (Quorum)");
        assert_eq!(TuiProgressReporter::phase_name(&AgentPhase::FinalReview), "Final Review (Quorum)");
        assert_eq!(TuiProgressReporter::phase_name(&AgentPhase::Completed), "Complete");
        assert_eq!(TuiProgressReporter::phase_name(&AgentPhase::Failed), "Failed");
    }

    #[test]
    fn test_on_phase_change_emits_event() {
        let state = Arc::new(Mutex::new(TuiState::default()));
        let reporter = TuiProgressReporter::new(state.clone());
        
        reporter.on_phase_change(&AgentPhase::Planning);
        
        let mut locked_state = state.lock().unwrap();
        
        if let Some(TuiEvent::PhaseChange { phase, name }) = locked_state.poll_event() {
            assert_eq!(phase, AgentPhase::Planning);
            assert_eq!(name, "Planning");
        } else {
            panic!("Expected PhaseChange event");
        }
    }

    #[test]
    fn test_on_thought_emits_event() {
        let state = Arc::new(Mutex::new(TuiState::default()));
        let reporter = TuiProgressReporter::new(state.clone());
        
        let thought = Thought {
            thought_type: quorum_domain::ThoughtType::Observation,
            content: "Test observation".to_string(),
            timestamp: 0,
        };
        
        reporter.on_thought(&thought);
        
        let mut locked_state = state.lock().unwrap();
        
        if let Some(TuiEvent::Thought { thought_type, content }) = locked_state.poll_event() {
            assert_eq!(thought_type, "Observation");
            assert_eq!(content, "Test observation");
        } else {
            panic!("Expected Thought event");
        }
    }

    #[test]
    fn test_on_task_start_emits_event() {
        let state = Arc::new(Mutex::new(TuiState::default()));
        let reporter = TuiProgressReporter::new(state.clone());
        
        let task = Task::new("task-1", "Test task");
        
        reporter.on_task_start(&task);
        
        let mut locked_state = state.lock().unwrap();
        
        if let Some(TuiEvent::TaskStart { description }) = locked_state.poll_event() {
            assert_eq!(description, "Test task");
        } else {
            panic!("Expected TaskStart event");
        }
    }

    #[test]
    fn test_on_task_complete_emits_event() {
        let state = Arc::new(Mutex::new(TuiState::default()));
        let reporter = TuiProgressReporter::new(state.clone());
        
        let task = Task::new("task-1", "Test task");
        
        reporter.on_task_complete(&task, true);
        
        let mut locked_state = state.lock().unwrap();
        
        if let Some(TuiEvent::TaskComplete { description, success }) = locked_state.poll_event() {
            assert_eq!(description, "Test task");
            assert_eq!(success, true);
        } else {
            panic!("Expected TaskComplete event");
        }
    }

    #[test]
    fn test_on_tool_call_emits_event() {
        let state = Arc::new(Mutex::new(TuiState::default()));
        let reporter = TuiProgressReporter::new(state.clone());
        
        reporter.on_tool_call("read_file", "/path/to/file");
        
        let mut locked_state = state.lock().unwrap();
        
        if let Some(TuiEvent::ToolCall { tool_name, args }) = locked_state.poll_event() {
            assert_eq!(tool_name, "read_file");
            assert_eq!(args, "/path/to/file");
        } else {
            panic!("Expected ToolCall event");
        }
    }

    #[test]
    fn test_on_tool_result_emits_event() {
        let state = Arc::new(Mutex::new(TuiState::default()));
        let reporter = TuiProgressReporter::new(state.clone());
        
        reporter.on_tool_result("read_file", true);
        
        let mut locked_state = state.lock().unwrap();
        
        if let Some(TuiEvent::ToolResult { tool_name, success }) = locked_state.poll_event() {
            assert_eq!(tool_name, "read_file");
            assert_eq!(success, true);
        } else {
            panic!("Expected ToolResult event");
        }
    }

    #[test]
    fn test_on_quorum_start_emits_event() {
        let state = Arc::new(Mutex::new(TuiState::default()));
        let reporter = TuiProgressReporter::new(state.clone());
        
        reporter.on_quorum_start("Plan Review", 3);
        
        let mut locked_state = state.lock().unwrap();
        
        if let Some(TuiEvent::QuorumStart { phase, model_count }) = locked_state.poll_event() {
            assert_eq!(phase, "Plan Review");
            assert_eq!(model_count, 3);
        } else {
            panic!("Expected QuorumStart event");
        }
    }

    #[test]
    fn test_on_quorum_complete_emits_event() {
        let state = Arc::new(Mutex::new(TuiState::default()));
        let reporter = TuiProgressReporter::new(state.clone());
        
        reporter.on_quorum_complete("Plan Review", true, Some("Good job"));
        
        let mut locked_state = state.lock().unwrap();
        
        if let Some(TuiEvent::QuorumComplete { phase, approved, feedback }) = locked_state.poll_event() {
            assert_eq!(phase, "Plan Review");
            assert_eq!(approved, true);
            assert_eq!(feedback.as_ref().unwrap(), "Good job");
        } else {
            panic!("Expected QuorumComplete event");
        }
    }

    #[test]
    fn test_on_plan_revision_emits_event() {
        let state = Arc::new(Mutex::new(TuiState::default()));
        let reporter = TuiProgressReporter::new(state.clone());
        
        reporter.on_plan_revision(2, "Needs improvement");
        
        let mut locked_state = state.lock().unwrap();
        
        if let Some(TuiEvent::PlanRevision { revision, feedback }) = locked_state.poll_event() {
            assert_eq!(revision, 2);
            assert_eq!(feedback, "Needs improvement");
        } else {
            panic!("Expected PlanRevision event");
        }
    }

    #[test]
    fn test_on_human_intervention_required_emits_event() {
        let state = Arc::new(Mutex::new(TuiState::default()));
        let reporter = TuiProgressReporter::new(state.clone());
        
        let plan = quorum_domain::Plan::new("Test goal", "Test reasoning");
        reporter.on_human_intervention_required("Test request", &plan, &[], 5);
        
        let mut locked_state = state.lock().unwrap();
        
        if let Some(TuiEvent::HumanInterventionRequired { max_revisions }) = locked_state.poll_event() {
            assert_eq!(max_revisions, 5);
        } else {
            panic!("Expected HumanInterventionRequired event");
        }
    }

    #[test]
    fn test_multiple_events_sequence() {
        let state = Arc::new(Mutex::new(TuiState::default()));
        let reporter = TuiProgressReporter::new(state.clone());
        
        // Simulate a typical workflow
        reporter.on_phase_change(&AgentPhase::Planning);
        reporter.on_task_start(&Task::new("task-1", "Task 1"));
        reporter.on_tool_call("read_file", "test.txt");
        reporter.on_tool_result("read_file", true);
        reporter.on_task_complete(&Task::new("task-1", "Task 1"), true);
        
        let mut locked_state = state.lock().unwrap();
        
        // Verify event sequence
        assert!(matches!(locked_state.poll_event(), Some(TuiEvent::PhaseChange { .. })));
        assert!(matches!(locked_state.poll_event(), Some(TuiEvent::TaskStart { .. })));
        assert!(matches!(locked_state.poll_event(), Some(TuiEvent::ToolCall { .. })));
        assert!(matches!(locked_state.poll_event(), Some(TuiEvent::ToolResult { .. })));
        assert!(matches!(locked_state.poll_event(), Some(TuiEvent::TaskComplete { .. })));
        assert!(locked_state.poll_event().is_none());
    }
}
