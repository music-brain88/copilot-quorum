//! Composite progress notifier — delegates to multiple notifiers.
//!
//! Used to fan out agent lifecycle events to both the TUI progress bridge
//! and the scripting event bridge simultaneously.

use super::agent_progress::AgentProgressNotifier;
use quorum_domain::{
    AgentPhase, ErrorCategory, Model, Plan, ReviewRound, StreamContext, Task, Thought,
};

/// A progress notifier that delegates to multiple inner notifiers.
///
/// Uses borrowed references with a lifetime parameter so both owned and
/// borrowed notifiers can be composed without wrapper types.
///
/// ```text
/// RunAgentUseCase.execute_with_progress(input, &composite_progress)
///                                                |
///                     +--------------------------+---------------------------+
///                     |                                                      |
///         TuiProgressBridge (existing)                  ScriptProgressBridge (new)
///         → TuiEvent channel                            → ScriptingEnginePort::emit_event()
/// ```
pub struct CompositeProgressNotifier<'a> {
    delegates: Vec<&'a dyn AgentProgressNotifier>,
}

impl<'a> CompositeProgressNotifier<'a> {
    pub fn new(delegates: Vec<&'a dyn AgentProgressNotifier>) -> Self {
        Self { delegates }
    }
}

/// Macro to delegate a method call to all inner notifiers.
macro_rules! delegate {
    ($self:ident, $method:ident $(, $arg:expr)*) => {
        for d in &$self.delegates {
            d.$method($($arg),*);
        }
    };
}

impl AgentProgressNotifier for CompositeProgressNotifier<'_> {
    fn on_phase_change(&self, phase: &AgentPhase) {
        delegate!(self, on_phase_change, phase);
    }

    fn on_thought(&self, thought: &Thought) {
        delegate!(self, on_thought, thought);
    }

    fn on_task_start(&self, task: &Task, index: usize, total: usize) {
        delegate!(self, on_task_start, task, index, total);
    }

    fn on_task_complete(&self, task: &Task, success: bool, index: usize, total: usize) {
        delegate!(self, on_task_complete, task, success, index, total);
    }

    fn on_tool_call(&self, tool_name: &str, args: &str) {
        delegate!(self, on_tool_call, tool_name, args);
    }

    fn on_tool_result(&self, tool_name: &str, success: bool) {
        delegate!(self, on_tool_result, tool_name, success);
    }

    fn on_tool_error(&self, tool_name: &str, category: ErrorCategory, message: &str) {
        delegate!(self, on_tool_error, tool_name, category, message);
    }

    fn on_tool_retry(&self, tool_name: &str, attempt: usize, max_retries: usize, error: &str) {
        delegate!(self, on_tool_retry, tool_name, attempt, max_retries, error);
    }

    fn on_tool_not_found(&self, tool_name: &str, available_tools: &[&str]) {
        delegate!(self, on_tool_not_found, tool_name, available_tools);
    }

    fn on_tool_resolved(&self, original_name: &str, resolved_name: &str) {
        delegate!(self, on_tool_resolved, original_name, resolved_name);
    }

    fn on_tool_execution_created(
        &self,
        task_id: &str,
        execution_id: &str,
        tool_name: &str,
        turn: usize,
        args_preview: &str,
    ) {
        delegate!(
            self,
            on_tool_execution_created,
            task_id,
            execution_id,
            tool_name,
            turn,
            args_preview
        );
    }

    fn on_tool_execution_started(&self, task_id: &str, execution_id: &str, tool_name: &str) {
        delegate!(
            self,
            on_tool_execution_started,
            task_id,
            execution_id,
            tool_name
        );
    }

    fn on_tool_execution_completed(
        &self,
        task_id: &str,
        execution_id: &str,
        tool_name: &str,
        duration_ms: u64,
        output_preview: &str,
    ) {
        delegate!(
            self,
            on_tool_execution_completed,
            task_id,
            execution_id,
            tool_name,
            duration_ms,
            output_preview
        );
    }

    fn on_tool_execution_failed(
        &self,
        task_id: &str,
        execution_id: &str,
        tool_name: &str,
        error: &str,
    ) {
        delegate!(
            self,
            on_tool_execution_failed,
            task_id,
            execution_id,
            tool_name,
            error
        );
    }

    fn on_llm_chunk(&self, chunk: &str) {
        delegate!(self, on_llm_chunk, chunk);
    }

    fn on_llm_stream_start(&self, purpose: &str) {
        delegate!(self, on_llm_stream_start, purpose);
    }

    fn on_llm_stream_end(&self) {
        delegate!(self, on_llm_stream_end);
    }

    fn on_plan_created(&self, plan: &Plan) {
        delegate!(self, on_plan_created, plan);
    }

    fn on_plan_revision(&self, revision: usize, feedback: &str) {
        delegate!(self, on_plan_revision, revision, feedback);
    }

    fn on_action_retry(&self, task: &Task, attempt: usize, feedback: &str) {
        delegate!(self, on_action_retry, task, attempt, feedback);
    }

    fn on_quorum_start(&self, phase: &str, model_count: usize) {
        delegate!(self, on_quorum_start, phase, model_count);
    }

    fn on_quorum_model_complete(&self, model: &Model, approved: bool) {
        delegate!(self, on_quorum_model_complete, model, approved);
    }

    fn on_quorum_complete(&self, phase: &str, approved: bool, feedback: Option<&str>) {
        delegate!(self, on_quorum_complete, phase, approved, feedback);
    }

    fn on_quorum_complete_with_votes(
        &self,
        phase: &str,
        approved: bool,
        votes: &[(String, bool, String)],
        feedback: Option<&str>,
    ) {
        delegate!(
            self,
            on_quorum_complete_with_votes,
            phase,
            approved,
            votes,
            feedback
        );
    }

    fn on_human_intervention_required(
        &self,
        request: &str,
        plan: &Plan,
        review_history: &[ReviewRound],
        max_revisions: usize,
    ) {
        delegate!(
            self,
            on_human_intervention_required,
            request,
            plan,
            review_history,
            max_revisions
        );
    }

    fn on_execution_confirmation_required(&self, request: &str, plan: &Plan) {
        delegate!(self, on_execution_confirmation_required, request, plan);
    }

    fn on_ensemble_start(&self, model_count: usize) {
        delegate!(self, on_ensemble_start, model_count);
    }

    fn on_ensemble_plan_generated(&self, model: &Model) {
        delegate!(self, on_ensemble_plan_generated, model);
    }

    fn on_ensemble_voting_start(&self, plan_count: usize) {
        delegate!(self, on_ensemble_voting_start, plan_count);
    }

    fn on_ensemble_model_failed(&self, model: &Model, error: &str) {
        delegate!(self, on_ensemble_model_failed, model, error);
    }

    fn on_ensemble_complete(&self, selected_model: &Model, score: f64) {
        delegate!(self, on_ensemble_complete, selected_model, score);
    }

    fn on_ensemble_fallback(&self, reason: &str) {
        delegate!(self, on_ensemble_fallback, reason);
    }

    fn on_model_stream_start(&self, model: &str, context: &StreamContext) {
        delegate!(self, on_model_stream_start, model, context);
    }

    fn on_model_stream_chunk(&self, model: &str, chunk: &str) {
        delegate!(self, on_model_stream_chunk, model, chunk);
    }

    fn on_model_stream_end(&self, model: &str) {
        delegate!(self, on_model_stream_end, model);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingNotifier {
        phase_count: AtomicUsize,
        plan_count: AtomicUsize,
        tool_completed_count: AtomicUsize,
        tool_failed_count: AtomicUsize,
    }

    impl CountingNotifier {
        fn new() -> Self {
            Self {
                phase_count: AtomicUsize::new(0),
                plan_count: AtomicUsize::new(0),
                tool_completed_count: AtomicUsize::new(0),
                tool_failed_count: AtomicUsize::new(0),
            }
        }
    }

    impl AgentProgressNotifier for CountingNotifier {
        fn on_phase_change(&self, _phase: &AgentPhase) {
            self.phase_count.fetch_add(1, Ordering::Relaxed);
        }
        fn on_plan_created(&self, _plan: &Plan) {
            self.plan_count.fetch_add(1, Ordering::Relaxed);
        }
        fn on_tool_execution_completed(
            &self,
            _task_id: &str,
            _execution_id: &str,
            _tool_name: &str,
            _duration_ms: u64,
            _output_preview: &str,
        ) {
            self.tool_completed_count.fetch_add(1, Ordering::Relaxed);
        }
        fn on_tool_execution_failed(
            &self,
            _task_id: &str,
            _execution_id: &str,
            _tool_name: &str,
            _error: &str,
        ) {
            self.tool_failed_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn test_composite_delegates_to_all_notifiers() {
        let n1 = CountingNotifier::new();
        let n2 = CountingNotifier::new();

        let composite = CompositeProgressNotifier::new(vec![&n1, &n2]);

        composite.on_phase_change(&AgentPhase::Planning);
        composite.on_phase_change(&AgentPhase::Executing);
        composite.on_plan_created(&Plan::new("objective", "reasoning"));
        composite.on_tool_execution_completed("t1", "e1", "read_file", 100, "ok");
        composite.on_tool_execution_failed("t1", "e2", "write_file", "permission denied");

        assert_eq!(n1.phase_count.load(Ordering::Relaxed), 2);
        assert_eq!(n2.phase_count.load(Ordering::Relaxed), 2);
        assert_eq!(n1.plan_count.load(Ordering::Relaxed), 1);
        assert_eq!(n2.plan_count.load(Ordering::Relaxed), 1);
        assert_eq!(n1.tool_completed_count.load(Ordering::Relaxed), 1);
        assert_eq!(n1.tool_failed_count.load(Ordering::Relaxed), 1);
    }
}
