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

/// Adapter: `AgentProgressNotifier` → `ProgressNotifier`
///
/// Maps Quorum Discussion phase events to the existing quorum progress
/// callbacks on `AgentProgressNotifier`, enabling TUI progress display
/// during `:discuss` commands.
pub struct QuorumProgressAdapter<'a> {
    inner: &'a dyn super::agent_progress::AgentProgressNotifier,
}

impl<'a> QuorumProgressAdapter<'a> {
    pub fn new(inner: &'a dyn super::agent_progress::AgentProgressNotifier) -> Self {
        Self { inner }
    }
}

impl ProgressNotifier for QuorumProgressAdapter<'_> {
    fn on_phase_start(&self, phase: &Phase, total_tasks: usize) {
        self.inner.on_quorum_start(phase.as_str(), total_tasks);
    }

    fn on_task_complete(&self, _phase: &Phase, model: &Model, success: bool) {
        self.inner.on_quorum_model_complete(model, success);
    }

    fn on_phase_complete(&self, phase: &Phase) {
        self.inner.on_quorum_complete(phase.as_str(), true, None);
    }
}
