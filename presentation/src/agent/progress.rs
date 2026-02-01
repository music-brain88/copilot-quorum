//! Progress reporting for Agent execution

use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use quorum_application::AgentProgressNotifier;
use quorum_domain::{AgentPhase, Model, Task, Thought};
use std::sync::Mutex;

/// Reports progress during Agent execution with fancy UI
pub struct AgentProgressReporter {
    multi: MultiProgress,
    phase_bar: Mutex<Option<ProgressBar>>,
    task_bar: Mutex<Option<ProgressBar>>,
    quorum_bar: Mutex<Option<ProgressBar>>,
    verbose: bool,
}

impl AgentProgressReporter {
    /// Create a new progress reporter
    pub fn new() -> Self {
        Self {
            multi: MultiProgress::new(),
            phase_bar: Mutex::new(None),
            task_bar: Mutex::new(None),
            quorum_bar: Mutex::new(None),
            verbose: false,
        }
    }

    /// Create with verbose output (shows all thoughts)
    pub fn verbose() -> Self {
        Self {
            multi: MultiProgress::new(),
            phase_bar: Mutex::new(None),
            task_bar: Mutex::new(None),
            quorum_bar: Mutex::new(None),
            verbose: true,
        }
    }

    fn phase_style() -> ProgressStyle {
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {prefix:.bold.cyan} {msg}")
            .unwrap()
    }

    #[allow(dead_code)]
    fn task_style() -> ProgressStyle {
        ProgressStyle::default_bar()
            .template("  {spinner:.blue} {prefix:.bold} [{bar:30.blue/dim}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("=>-")
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

impl Default for AgentProgressReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentProgressNotifier for AgentProgressReporter {
    fn on_phase_change(&self, phase: &AgentPhase) {
        self.finish_current_phase();

        let emoji = Self::phase_emoji(phase);
        let name = Self::phase_name(phase);

        // Print phase header for terminal phases
        if matches!(phase, AgentPhase::Completed | AgentPhase::Failed) {
            println!();
            if matches!(phase, AgentPhase::Completed) {
                println!("{} {}", emoji, name.green().bold());
            } else {
                println!("{} {}", emoji, name.red().bold());
            }
            return;
        }

        // Create spinner for active phases
        let pb = self.multi.add(ProgressBar::new_spinner());
        pb.set_style(Self::phase_style());
        pb.set_prefix(format!("{} {}", emoji, name));
        pb.set_message("...");
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        *self.phase_bar.lock().unwrap() = Some(pb);
    }

    fn on_thought(&self, thought: &Thought) {
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
        // Update phase bar message
        if let Some(pb) = self.phase_bar.lock().unwrap().as_ref() {
            pb.set_message(format!("Task: {}", truncate(&task.description, 40)));
        }

        if self.verbose {
            println!("    {} Starting: {}", "→".blue(), task.description);
        }
    }

    fn on_task_complete(&self, task: &Task, success: bool) {
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
        if self.verbose {
            let args_display = truncate(args, 50);
            println!(
                "      {} {} {}",
                "🔧".dimmed(),
                tool_name.cyan(),
                args_display.dimmed()
            );
        }

        // Update phase bar
        if let Some(pb) = self.phase_bar.lock().unwrap().as_ref() {
            pb.set_message(format!("Running: {}", tool_name));
        }
    }

    fn on_tool_result(&self, tool_name: &str, success: bool) {
        if self.verbose {
            if success {
                println!("      {} {} {}", "✓".green(), tool_name.green(), "OK".dimmed());
            } else {
                println!(
                    "      {} {} {}",
                    "✗".red(),
                    tool_name.red(),
                    "FAILED".dimmed()
                );
            }
        }
    }

    fn on_quorum_start(&self, phase: &str, model_count: usize) {
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
        if let Some(pb) = self.quorum_bar.lock().unwrap().take() {
            if approved {
                pb.finish_with_message(format!("{} {}", phase.green(), "APPROVED".green().bold()));
            } else {
                pb.finish_with_message(format!("{} {}", phase.red(), "REJECTED".red().bold()));
            }
        }

        if !approved {
            if let Some(fb) = feedback {
                println!();
                println!("    {} Feedback:", "ℹ".yellow());
                for line in fb.lines().take(5) {
                    println!("      {}", line.yellow());
                }
            }
        }
    }
}

/// Simple text-based progress (no spinners)
pub struct SimpleAgentProgress {
    verbose: bool,
}

impl SimpleAgentProgress {
    pub fn new(verbose: bool) -> Self {
        Self { verbose }
    }
}

impl AgentProgressNotifier for SimpleAgentProgress {
    fn on_phase_change(&self, phase: &AgentPhase) {
        let emoji = AgentProgressReporter::phase_emoji(phase);
        let name = AgentProgressReporter::phase_name(phase);
        println!("{} {}", emoji, name.bold());
    }

    fn on_thought(&self, thought: &Thought) {
        if self.verbose {
            println!("  💭 {:?}: {}", thought.thought_type, thought.content);
        }
    }

    fn on_task_start(&self, task: &Task) {
        println!("  → {}", task.description);
    }

    fn on_task_complete(&self, task: &Task, success: bool) {
        if success {
            println!("  {} {}", "✓".green(), task.description);
        } else {
            println!("  {} {}", "✗".red(), task.description);
        }
    }

    fn on_tool_call(&self, tool_name: &str, _args: &str) {
        if self.verbose {
            println!("    🔧 {}", tool_name);
        }
    }

    fn on_tool_result(&self, tool_name: &str, success: bool) {
        if self.verbose {
            if success {
                println!("    ✓ {}", tool_name);
            } else {
                println!("    ✗ {} FAILED", tool_name);
            }
        }
    }

    fn on_quorum_start(&self, phase: &str, model_count: usize) {
        println!("  🗳️  {} ({} models)", phase, model_count);
    }

    fn on_quorum_model_complete(&self, model: &Model, approved: bool) {
        let vote = if approved { "APPROVE" } else { "REJECT" };
        println!("    {} {}: {}", if approved { "✓" } else { "✗" }, model, vote);
    }

    fn on_quorum_complete(&self, phase: &str, approved: bool, feedback: Option<&str>) {
        if approved {
            println!("  ✓ {} APPROVED", phase);
        } else {
            println!("  ✗ {} REJECTED", phase);
            if let Some(fb) = feedback {
                println!("    Feedback: {}", fb);
            }
        }
    }
}

/// Truncate a string to a maximum length
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
