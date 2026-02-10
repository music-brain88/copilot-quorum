//! Agent execution progress port.
//!
//! [`AgentProgressNotifier`] is an **output port** that the presentation layer
//! implements to display real-time agent execution progress to the user.
//! All callback argument types come from the domain layer.
//!
//! # Callback Categories
//!
//! - **Phase callbacks**: Track high-level execution phases
//! - **Task callbacks**: Track individual task execution
//! - **Tool callbacks**: Track tool calls and results
//! - **Quorum callbacks**: Track multi-model voting (Solo mode)
//! - **Ensemble callbacks**: Track multi-model planning (Ensemble mode)
//!
//! # Example Implementation
//!
//! ```ignore
//! use quorum_application::ports::agent_progress::AgentProgressNotifier;
//!
//! struct MyProgress;
//!
//! impl AgentProgressNotifier for MyProgress {
//!     fn on_phase_change(&self, phase: &AgentPhase) {
//!         println!("Phase: {}", phase);
//!     }
//!
//!     fn on_ensemble_complete(&self, model: &Model, score: f64) {
//!         println!("Selected: {} ({:.1}/10)", model, score);
//!     }
//! }
//! ```

use quorum_domain::{AgentPhase, ErrorCategory, Model, Plan, ReviewRound, Task, Thought};

/// Progress notifier for agent execution.
///
/// This trait provides callbacks for various stages of agent execution,
/// allowing UI implementations to display progress to the user.
///
/// All methods have default no-op implementations, so implementers only
/// need to override the callbacks they care about.
pub trait AgentProgressNotifier: Send + Sync {
    /// Called when the agent transitions to a new phase
    fn on_phase_change(&self, _phase: &AgentPhase) {}

    /// Called when the agent records a reasoning step
    fn on_thought(&self, _thought: &Thought) {}

    /// Called when a task begins execution
    fn on_task_start(&self, _task: &Task) {}

    /// Called when a task completes (success or failure)
    fn on_task_complete(&self, _task: &Task, _success: bool) {}

    /// Called when a tool is invoked
    fn on_tool_call(&self, _tool_name: &str, _args: &str) {}

    /// Called when a tool returns a result
    fn on_tool_result(&self, _tool_name: &str, _success: bool) {}

    /// Called when a tool execution fails with details about the error
    fn on_tool_error(&self, _tool_name: &str, _category: ErrorCategory, _message: &str) {}

    /// Called when retrying a tool call after an error
    fn on_tool_retry(&self, _tool_name: &str, _attempt: usize, _max_retries: usize, _error: &str) {}

    /// Called when a tool name is not found in the registry
    fn on_tool_not_found(&self, _tool_name: &str, _available_tools: &[&str]) {}

    /// Called when an unknown tool name has been resolved to a valid tool
    fn on_tool_resolved(&self, _original_name: &str, _resolved_name: &str) {}

    // ==================== LLM Streaming Callbacks ====================

    /// Called for each text chunk received during LLM streaming.
    fn on_llm_chunk(&self, _chunk: &str) {}

    /// Called when LLM streaming begins.
    fn on_llm_stream_start(&self, _purpose: &str) {}

    /// Called when LLM streaming ends.
    fn on_llm_stream_end(&self) {}

    // ==================== Plan Revision Callbacks ====================

    /// Called when a plan revision is triggered after rejection
    fn on_plan_revision(&self, _revision: usize, _feedback: &str) {}

    /// Called when an action is being retried after rejection
    fn on_action_retry(&self, _task: &Task, _attempt: usize, _feedback: &str) {}

    // ==================== Quorum Callbacks (Solo Mode) ====================

    /// Called when quorum voting begins
    ///
    /// # Arguments
    /// * `phase` - The review phase (e.g., "plan_review", "action_review")
    /// * `model_count` - Number of models participating in the vote
    fn on_quorum_start(&self, _phase: &str, _model_count: usize) {}

    /// Called when a single model completes its vote
    fn on_quorum_model_complete(&self, _model: &Model, _approved: bool) {}

    /// Called when quorum voting completes
    fn on_quorum_complete(&self, _phase: &str, _approved: bool, _feedback: Option<&str>) {}

    /// Called when quorum voting completes with detailed vote information
    ///
    /// # Arguments
    /// * `votes` - Vec of (model_name, approved, reasoning) tuples
    fn on_quorum_complete_with_votes(
        &self,
        _phase: &str,
        _approved: bool,
        _votes: &[(String, bool, String)],
        _feedback: Option<&str>,
    ) {
    }

    /// Called when human intervention is required due to plan revision limit
    fn on_human_intervention_required(
        &self,
        _request: &str,
        _plan: &Plan,
        _review_history: &[ReviewRound],
        _max_revisions: usize,
    ) {
    }

    /// Called when execution confirmation is required before task execution.
    ///
    /// Only triggered when `PhaseScope::Full` is active.
    fn on_execution_confirmation_required(&self, _request: &str, _plan: &Plan) {}

    // ==================== Ensemble Planning Callbacks ====================

    /// Called when ensemble planning starts (Ensemble mode only)
    ///
    /// In ensemble mode, multiple models generate plans independently.
    /// This is called at the start of that process.
    ///
    /// # Arguments
    /// * `model_count` - Number of models that will generate plans
    fn on_ensemble_start(&self, _model_count: usize) {}

    /// Called when a model finishes generating its plan
    ///
    /// Called once per model as they complete plan generation.
    fn on_ensemble_plan_generated(&self, _model: &Model) {}

    /// Called when ensemble voting starts
    ///
    /// After all plans are generated, models vote on each other's plans.
    ///
    /// # Arguments
    /// * `plan_count` - Number of plans to be voted on
    fn on_ensemble_voting_start(&self, _plan_count: usize) {}

    /// Called when a model fails to generate a plan during ensemble planning
    ///
    /// Called for each model that returns an error or text-only response.
    /// Useful for showing per-model failure status to the user.
    fn on_ensemble_model_failed(&self, _model: &Model, _error: &str) {}

    /// Called when ensemble planning completes with the selected plan
    ///
    /// # Arguments
    /// * `selected_model` - The model whose plan was selected
    /// * `score` - The average score (1-10) the selected plan received
    fn on_ensemble_complete(&self, _selected_model: &Model, _score: f64) {}

    /// Called when ensemble planning fails and falls back to solo planning
    ///
    /// This happens when all models fail to generate plans in ensemble mode.
    fn on_ensemble_fallback(&self, _reason: &str) {}
}

/// No-op implementation for when progress isn't needed
pub struct NoAgentProgress;

impl AgentProgressNotifier for NoAgentProgress {}
