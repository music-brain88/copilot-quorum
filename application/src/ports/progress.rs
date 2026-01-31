//! Progress notification port
//!
//! Defines the interface for reporting progress during Quorum execution.

use quorum_domain::{Model, Phase};

/// Callback for progress updates during Quorum execution
///
/// Implementations live in the presentation layer and can display
/// progress in various ways (console, web UI, etc.)
pub trait ProgressNotifier: Send + Sync {
    /// Called when a phase starts
    fn on_phase_start(&self, phase: &Phase, total_tasks: usize);

    /// Called when a task completes within a phase
    fn on_task_complete(&self, phase: &Phase, model: &Model, success: bool);

    /// Called when a phase completes
    fn on_phase_complete(&self, phase: &Phase);
}

/// No-op progress notifier for when progress reporting is not needed
pub struct NoProgress;

impl ProgressNotifier for NoProgress {
    fn on_phase_start(&self, _phase: &Phase, _total_tasks: usize) {}
    fn on_task_complete(&self, _phase: &Phase, _model: &Model, _success: bool) {}
    fn on_phase_complete(&self, _phase: &Phase) {}
}
