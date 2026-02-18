//! TUI Progress Bridge — AgentProgressNotifier → TuiEvent channel
//!
//! Converts progress callbacks into TuiEvents that flow through the
//! same mpsc channel as presenter events. No println!, no indicatif —
//! everything goes through the channel for ratatui rendering.

use super::event::{RoutedTuiEvent, TuiEvent};
use quorum_application::{AgentProgressNotifier, ErrorCategory};
use quorum_domain::{AgentPhase, InteractionId, Model, Plan, ReviewRound, Task, Thought};
use tokio::sync::mpsc;

/// Bridge from AgentProgressNotifier callbacks to TuiEvent channel
pub struct TuiProgressBridge {
    tx: mpsc::UnboundedSender<RoutedTuiEvent>,
    interaction_id: Option<InteractionId>,
}

impl TuiProgressBridge {
    pub fn new(tx: mpsc::UnboundedSender<RoutedTuiEvent>, interaction_id: Option<InteractionId>) -> Self {
        Self { tx, interaction_id }
    }

    fn emit(&self, event: TuiEvent) {
        let routed = match self.interaction_id {
            Some(id) => RoutedTuiEvent::for_interaction(id, event),
            None => RoutedTuiEvent::global(event),
        };
        let _ = self.tx.send(routed);
    }

    fn phase_name(phase: &AgentPhase) -> &'static str {
        match phase {
            AgentPhase::ContextGathering => "Gathering Context",
            AgentPhase::Planning => "Planning",
            AgentPhase::PlanReview => "Plan Review",
            AgentPhase::Executing => "Executing",
            AgentPhase::ActionReview => "Action Review",
            AgentPhase::FinalReview => "Final Review",
            AgentPhase::Completed => "Complete",
            AgentPhase::Failed => "Failed",
        }
    }
}

impl AgentProgressNotifier for TuiProgressBridge {
    fn on_phase_change(&self, phase: &AgentPhase) {
        self.emit(TuiEvent::PhaseChange {
            phase: phase.clone(),
            name: Self::phase_name(phase).to_string(),
        });
    }

    fn on_thought(&self, _thought: &Thought) {
        // Thoughts are displayed via streaming for now
    }

    fn on_task_start(&self, task: &Task, index: usize, total: usize) {
        self.emit(TuiEvent::TaskStart {
            description: task.description.clone(),
            index,
            total,
        });
    }

    fn on_task_complete(&self, task: &Task, success: bool, index: usize, total: usize) {
        let output = task
            .result
            .as_ref()
            .map(|r| r.output.clone())
            .filter(|o| !o.is_empty());
        self.emit(TuiEvent::TaskComplete {
            description: task.description.clone(),
            success,
            index,
            total,
            output,
        });
    }

    fn on_tool_execution_created(
        &self,
        _task_id: &str,
        execution_id: &str,
        tool_name: &str,
        _turn: usize,
    ) {
        self.emit(TuiEvent::ToolExecutionUpdate {
            task_index: 0, // Will be overridden by current task progress
            execution_id: execution_id.to_string(),
            tool_name: tool_name.to_string(),
            state: super::event::ToolExecutionDisplayState::Pending,
            duration_ms: None,
        });
    }

    fn on_tool_execution_started(&self, _task_id: &str, execution_id: &str, tool_name: &str) {
        self.emit(TuiEvent::ToolExecutionUpdate {
            task_index: 0,
            execution_id: execution_id.to_string(),
            tool_name: tool_name.to_string(),
            state: super::event::ToolExecutionDisplayState::Running,
            duration_ms: None,
        });
    }

    fn on_tool_execution_completed(
        &self,
        _task_id: &str,
        execution_id: &str,
        tool_name: &str,
        duration_ms: u64,
        output_preview: &str,
    ) {
        self.emit(TuiEvent::ToolExecutionUpdate {
            task_index: 0,
            execution_id: execution_id.to_string(),
            tool_name: tool_name.to_string(),
            state: super::event::ToolExecutionDisplayState::Completed {
                preview: output_preview.to_string(),
            },
            duration_ms: Some(duration_ms),
        });
    }

    fn on_tool_execution_failed(
        &self,
        _task_id: &str,
        execution_id: &str,
        tool_name: &str,
        error: &str,
    ) {
        self.emit(TuiEvent::ToolExecutionUpdate {
            task_index: 0,
            execution_id: execution_id.to_string(),
            tool_name: tool_name.to_string(),
            state: super::event::ToolExecutionDisplayState::Error {
                message: error.to_string(),
            },
            duration_ms: None,
        });
    }

    fn on_tool_call(&self, tool_name: &str, args: &str) {
        self.emit(TuiEvent::ToolCall {
            tool_name: tool_name.to_string(),
            args: args.to_string(),
        });
    }

    fn on_tool_result(&self, tool_name: &str, success: bool) {
        self.emit(TuiEvent::ToolResult {
            tool_name: tool_name.to_string(),
            success,
        });
    }

    fn on_tool_error(&self, tool_name: &str, _category: ErrorCategory, message: &str) {
        self.emit(TuiEvent::ToolError {
            tool_name: tool_name.to_string(),
            message: message.to_string(),
        });
    }

    fn on_tool_retry(&self, tool_name: &str, attempt: usize, max_retries: usize, error: &str) {
        self.emit(TuiEvent::Flash(format!(
            "Retrying {} ({}/{}) : {}",
            tool_name, attempt, max_retries, error
        )));
    }

    fn on_tool_not_found(&self, tool_name: &str, _available: &[&str]) {
        self.emit(TuiEvent::Flash(format!("Tool not found: {}", tool_name)));
    }

    fn on_tool_resolved(&self, original: &str, resolved: &str) {
        self.emit(TuiEvent::Flash(format!(
            "Resolved {} → {}",
            original, resolved
        )));
    }

    fn on_llm_chunk(&self, chunk: &str) {
        self.emit(TuiEvent::StreamChunk(chunk.to_string()));
    }

    fn on_llm_stream_start(&self, _purpose: &str) {
        // Stream rendering handled by StreamChunk events
    }

    fn on_llm_stream_end(&self) {
        self.emit(TuiEvent::StreamEnd);
    }

    fn on_plan_revision(&self, revision: usize, feedback: &str) {
        self.emit(TuiEvent::PlanRevision {
            revision,
            feedback: feedback.to_string(),
        });
    }

    fn on_action_retry(&self, task: &Task, attempt: usize, feedback: &str) {
        self.emit(TuiEvent::Flash(format!(
            "Retrying task '{}' (#{}) — {}",
            task.description, attempt, feedback
        )));
    }

    fn on_quorum_start(&self, phase: &str, model_count: usize) {
        self.emit(TuiEvent::QuorumStart {
            phase: phase.to_string(),
            model_count,
        });
    }

    fn on_quorum_model_complete(&self, model: &Model, approved: bool) {
        self.emit(TuiEvent::QuorumModelVote {
            model: model.to_string(),
            approved,
        });
    }

    fn on_quorum_complete(&self, phase: &str, approved: bool, feedback: Option<&str>) {
        self.emit(TuiEvent::QuorumComplete {
            phase: phase.to_string(),
            approved,
            feedback: feedback.map(|s| s.to_string()),
        });
    }

    fn on_quorum_complete_with_votes(
        &self,
        phase: &str,
        approved: bool,
        votes: &[(String, bool, String)],
        feedback: Option<&str>,
    ) {
        // Emit individual votes then completion
        for (model, vote, _reasoning) in votes {
            self.emit(TuiEvent::QuorumModelVote {
                model: model.clone(),
                approved: *vote,
            });
        }
        self.emit(TuiEvent::QuorumComplete {
            phase: phase.to_string(),
            approved,
            feedback: feedback.map(|s| s.to_string()),
        });
    }

    fn on_human_intervention_required(
        &self,
        _request: &str,
        _plan: &Plan,
        _review_history: &[ReviewRound],
        max_revisions: usize,
    ) {
        self.emit(TuiEvent::Flash(format!(
            "Human intervention required (max revisions: {})",
            max_revisions
        )));
    }

    fn on_execution_confirmation_required(&self, _request: &str, _plan: &Plan) {
        self.emit(TuiEvent::Flash(
            "Execution confirmation required".to_string(),
        ));
    }

    fn on_ensemble_start(&self, model_count: usize) {
        self.emit(TuiEvent::EnsembleStart(model_count));
    }

    fn on_ensemble_plan_generated(&self, model: &Model) {
        self.emit(TuiEvent::EnsemblePlanGenerated(model.to_string()));
    }

    fn on_ensemble_voting_start(&self, plan_count: usize) {
        self.emit(TuiEvent::EnsembleVotingStart(plan_count));
    }

    fn on_ensemble_model_failed(&self, model: &Model, error: &str) {
        self.emit(TuiEvent::EnsembleModelFailed {
            model: model.to_string(),
            error: error.to_string(),
        });
    }

    fn on_ensemble_complete(&self, selected_model: &Model, score: f64) {
        self.emit(TuiEvent::EnsembleComplete {
            selected_model: selected_model.to_string(),
            score,
        });
    }

    fn on_ensemble_fallback(&self, reason: &str) {
        self.emit(TuiEvent::EnsembleFallback(reason.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_change_emits_event() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let bridge = TuiProgressBridge::new(tx, None);

        bridge.on_phase_change(&AgentPhase::Planning);

        let event = rx.try_recv().unwrap();
        assert!(event.interaction_id.is_none());
        let event = event.event;
        assert!(matches!(event, TuiEvent::PhaseChange { .. }));
    }

    #[test]
    fn test_tool_call_emits_event() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let bridge = TuiProgressBridge::new(tx, None);

        bridge.on_tool_call("read_file", "/test.rs");

        let event = rx.try_recv().unwrap().event;
        if let TuiEvent::ToolCall { tool_name, args } = event {
            assert_eq!(tool_name, "read_file");
            assert_eq!(args, "/test.rs");
        } else {
            panic!("Expected ToolCall event");
        }
    }

    #[test]
    fn test_stream_chunk_emits_event() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let bridge = TuiProgressBridge::new(tx, None);

        bridge.on_llm_chunk("hello ");
        bridge.on_llm_chunk("world");
        bridge.on_llm_stream_end();

        assert!(matches!(rx.try_recv().unwrap().event, TuiEvent::StreamChunk(_)));
        assert!(matches!(rx.try_recv().unwrap().event, TuiEvent::StreamChunk(_)));
        assert!(matches!(rx.try_recv().unwrap().event, TuiEvent::StreamEnd));
    }

    #[test]
    fn test_routed_event_includes_interaction_id() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let bridge = TuiProgressBridge::new(tx, Some(InteractionId(42)));

        bridge.on_llm_chunk("hello");

        let event = rx.try_recv().unwrap();
        assert_eq!(event.interaction_id, Some(InteractionId(42)));
        assert!(matches!(event.event, TuiEvent::StreamChunk(_)));
    }

    #[test]
    fn test_quorum_events() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let bridge = TuiProgressBridge::new(tx, None);

        bridge.on_quorum_start("Plan Review", 3);
        bridge.on_quorum_model_complete(&Model::ClaudeSonnet45, true);
        bridge.on_quorum_complete("Plan Review", true, Some("LGTM"));

        assert!(matches!(
            rx.try_recv().unwrap().event,
            TuiEvent::QuorumStart { .. }
        ));
        assert!(matches!(
            rx.try_recv().unwrap().event,
            TuiEvent::QuorumModelVote { .. }
        ));
        assert!(matches!(
            rx.try_recv().unwrap().event,
            TuiEvent::QuorumComplete { .. }
        ));
    }

    #[test]
    fn test_task_lifecycle() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let bridge = TuiProgressBridge::new(tx, None);

        let task = Task::new("t1", "Fix bug");
        bridge.on_task_start(&task, 1, 3);
        bridge.on_task_complete(&task, true, 1, 3);

        let event = rx.try_recv().unwrap().event;
        if let TuiEvent::TaskStart {
            description,
            index,
            total,
        } = event
        {
            assert_eq!(description, "Fix bug");
            assert_eq!(index, 1);
            assert_eq!(total, 3);
        } else {
            panic!("Expected TaskStart event");
        }

        assert!(matches!(
            rx.try_recv().unwrap().event,
            TuiEvent::TaskComplete {
                index: 1,
                total: 3,
                success: true,
                ..
            }
        ));
    }

    #[test]
    fn test_task_complete_with_output() {
        use quorum_domain::TaskResult;

        let (tx, mut rx) = mpsc::unbounded_channel();
        let bridge = TuiProgressBridge::new(tx, None);

        let mut task = Task::new("t1", "Analyze code");
        task.mark_completed(TaskResult::success(
            "The code looks clean and well-structured.",
        ));
        bridge.on_task_complete(&task, true, 1, 1);

        let event = rx.try_recv().unwrap().event;
        if let TuiEvent::TaskComplete {
            output, success, ..
        } = event
        {
            assert!(success);
            assert_eq!(
                output.as_deref(),
                Some("The code looks clean and well-structured.")
            );
        } else {
            panic!("Expected TaskComplete event");
        }
    }

    #[test]
    fn test_task_complete_without_result() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let bridge = TuiProgressBridge::new(tx, None);

        let task = Task::new("t1", "Do something");
        bridge.on_task_complete(&task, true, 1, 1);

        let event = rx.try_recv().unwrap().event;
        if let TuiEvent::TaskComplete { output, .. } = event {
            assert!(output.is_none());
        } else {
            panic!("Expected TaskComplete event");
        }
    }

    #[test]
    fn test_task_complete_with_empty_output() {
        use quorum_domain::TaskResult;

        let (tx, mut rx) = mpsc::unbounded_channel();
        let bridge = TuiProgressBridge::new(tx, None);

        let mut task = Task::new("t1", "Empty result");
        task.mark_completed(TaskResult::success(""));
        bridge.on_task_complete(&task, true, 1, 1);

        let event = rx.try_recv().unwrap().event;
        if let TuiEvent::TaskComplete { output, .. } = event {
            assert!(output.is_none()); // Empty output filtered to None
        } else {
            panic!("Expected TaskComplete event");
        }
    }
}
