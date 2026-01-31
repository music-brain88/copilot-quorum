//! Progress reporting for Quorum execution

use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use quorum_application::ports::progress::ProgressNotifier;
use quorum_domain::{Model, Phase};
use std::collections::HashMap;
use std::sync::Mutex;

/// Reports progress during Quorum execution with fancy progress bars
pub struct ProgressReporter {
    multi: MultiProgress,
    #[allow(dead_code)]
    bars: Mutex<HashMap<String, ProgressBar>>,
    phase_bar: Mutex<Option<ProgressBar>>,
}

impl ProgressReporter {
    pub fn new() -> Self {
        Self {
            multi: MultiProgress::new(),
            bars: Mutex::new(HashMap::new()),
            phase_bar: Mutex::new(None),
        }
    }

    fn phase_style() -> ProgressStyle {
        ProgressStyle::default_bar()
            .template("{spinner:.green} {prefix:.bold.cyan} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("=>-")
    }

    #[allow(dead_code)]
    fn spinner_style() -> ProgressStyle {
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {prefix:.bold} {msg}")
            .unwrap()
    }

    fn phase_display_name(phase: &Phase) -> &'static str {
        match phase {
            Phase::Initial => "Phase 1: Initial Query",
            Phase::Review => "Phase 2: Peer Review",
            Phase::Synthesis => "Phase 3: Synthesis",
        }
    }

    fn phase_short_name(phase: &Phase) -> &'static str {
        match phase {
            Phase::Initial => "Phase 1",
            Phase::Review => "Phase 2",
            Phase::Synthesis => "Phase 3",
        }
    }
}

impl Default for ProgressReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgressNotifier for ProgressReporter {
    fn on_phase_start(&self, phase: &Phase, total_tasks: usize) {
        let phase_name = Self::phase_display_name(phase);

        let pb = self.multi.add(ProgressBar::new(total_tasks as u64));
        pb.set_style(Self::phase_style());
        pb.set_prefix(phase_name.to_string());
        pb.set_message("Starting...");

        *self.phase_bar.lock().unwrap() = Some(pb);
    }

    fn on_task_complete(&self, _phase: &Phase, model: &Model, success: bool) {
        if let Some(pb) = self.phase_bar.lock().unwrap().as_ref() {
            let status = if success {
                format!("{} {}", "v".green(), model)
            } else {
                format!("{} {}", "x".red(), model)
            };
            pb.set_message(status);
            pb.inc(1);
        }
    }

    fn on_phase_complete(&self, phase: &Phase) {
        if let Some(pb) = self.phase_bar.lock().unwrap().take() {
            let phase_name = Self::phase_short_name(phase);
            pb.finish_with_message(format!("{} complete!", phase_name.green()));
        }
    }
}

/// Simple text-based progress (no fancy UI)
pub struct SimpleProgress;

impl ProgressNotifier for SimpleProgress {
    fn on_phase_start(&self, phase: &Phase, total_tasks: usize) {
        let phase_name = ProgressReporter::phase_display_name(phase);
        println!(
            "{} {} ({} tasks)",
            "->".cyan(),
            phase_name.bold(),
            total_tasks
        );
    }

    fn on_task_complete(&self, _phase: &Phase, model: &Model, success: bool) {
        if success {
            println!("  {} {}", "v".green(), model);
        } else {
            println!("  {} {} (failed)", "x".red(), model);
        }
    }

    fn on_phase_complete(&self, _phase: &Phase) {
        println!();
    }
}
