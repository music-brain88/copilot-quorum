//! TuiEvent → state mapping (event dispatch)
//!
//! All TuiEvent processing extracted from TuiApp as free functions.

use super::content::{ContentRegistry, ContentSlot};
use super::event::{RoutedTuiEvent, ToolExecutionDisplayState, TuiEvent};
use super::state::{
    DisplayMessage, EnsembleProgress, ModelStreamState, ModelStreamStatus, QuorumStatus,
    TaskProgress, TaskSummary, ToolExecutionDisplay, ToolExecutionDisplayStatus, TuiState,
};
use super::surface::SurfaceId;
use super::widgets::model_stream::ModelStreamRenderer;
use quorum_domain::core::string::truncate;
use quorum_domain::interaction::InteractionId;
use std::cell::RefCell;

/// Apply a routed TuiEvent to the appropriate interaction pane.
pub(super) fn apply_routed_tui_event(
    state: &mut TuiState,
    content_registry: &RefCell<ContentRegistry>,
    routed: RoutedTuiEvent,
) {
    if let Some(id) = routed.interaction_id
        && state.tabs.find_tab_index_by_interaction(id).is_some()
    {
        apply_tui_event_to_interaction(state, content_registry, id, routed.event);
        return;
    }
    // Fallback to active pane (global event or untargeted)
    apply_tui_event(state, content_registry, routed.event);
}

/// Apply event to a specific interaction pane — the single source of truth
/// for all TuiEvent → state mapping.
///
/// Called directly by [`apply_routed_tui_event`] for targeted events, and
/// indirectly by [`apply_tui_event`] for untargeted (active-pane) events.
fn apply_tui_event_to_interaction(
    state: &mut TuiState,
    content_registry: &RefCell<ContentRegistry>,
    id: InteractionId,
    event: TuiEvent,
) {
    match event {
        TuiEvent::StreamChunk(chunk) => {
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                pane.conversation.streaming_text.push_str(&chunk);
                if pane.conversation.auto_scroll {
                    pane.conversation.scroll_offset = 0;
                }
            }
        }
        TuiEvent::StreamEnd => {
            state.finalize_stream_for(id);
        }
        TuiEvent::PhaseChange { phase, name } => {
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                let progress = &mut pane.progress;
                progress.current_phase = Some(phase);
                progress.phase_name = name;
            }
        }
        TuiEvent::TaskStart {
            description,
            index,
            total,
        } => {
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                let progress = &mut pane.progress;
                let completed_tasks = progress
                    .task_progress
                    .as_ref()
                    .map(|tp| tp.completed_tasks.clone())
                    .unwrap_or_default();
                progress.task_progress = Some(TaskProgress {
                    current_index: index,
                    total,
                    description: description.clone(),
                    completed_tasks,
                    active_tool_executions: Vec::new(),
                });
            }
            state.push_message_to(
                id,
                DisplayMessage::system(format!(
                    "Executing Task {}/{}: {}",
                    index, total, description
                )),
            );
        }
        TuiEvent::TaskComplete {
            description,
            success,
            index,
            total: _,
            output,
        } => {
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                let progress = &mut pane.progress;
                let active_execs = if let Some(ref mut tp) = progress.task_progress {
                    std::mem::take(&mut tp.active_tool_executions)
                } else {
                    Vec::new()
                };
                if let Some(ref mut tp) = progress.task_progress {
                    tp.completed_tasks.push(TaskSummary {
                        index,
                        description: description.clone(),
                        success,
                        output: output.clone(),
                        duration_ms: None,
                        tool_executions: active_execs,
                    });
                }
            }

            let tool_exec_lines = if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                if let Some(ref tp) = pane.progress.task_progress {
                    tp.completed_tasks
                        .last()
                        .map(|summary| {
                            summary
                                .tool_executions
                                .iter()
                                .map(|exec| {
                                    let (icon, dur) = match &exec.state {
                                        ToolExecutionDisplayStatus::Completed { .. } => {
                                            let d = exec
                                                .duration_ms
                                                .map(|ms| {
                                                    if ms < 1000 {
                                                        format!("{}ms", ms)
                                                    } else {
                                                        format!("{:.1}s", ms as f64 / 1000.0)
                                                    }
                                                })
                                                .unwrap_or_default();
                                            ("✓", d)
                                        }
                                        ToolExecutionDisplayStatus::Error { message } => {
                                            ("✗", truncate(message, 40))
                                        }
                                        _ => ("…", String::new()),
                                    };
                                    format!("  {} {} ({})", icon, exec.tool_name, dur)
                                })
                                .collect::<Vec<_>>()
                                .join(
                                    "
",
                                )
                        })
                        .unwrap_or_default()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            let status = if success { "✓" } else { "✗" };
            let mut msg = if let Some(ref out) = output {
                let extracted = extract_response_text(out);
                if extracted.is_empty() {
                    format!("Task {} {} {}", index, status, description)
                } else {
                    format!(
                        "Task {} {} {}\n  Output: {}",
                        index, status, description, extracted
                    )
                }
            } else {
                format!("Task {} {} {}", index, status, description)
            };
            if !tool_exec_lines.is_empty() {
                msg.push('\n');
                msg.push_str(&tool_exec_lines);
            }
            state.push_message_to(id, DisplayMessage::system(msg));
        }
        TuiEvent::InteractionCompleted {
            parent_id,
            result_text,
        } => {
            let target_id = parent_id.unwrap_or(id);
            state.push_message_to(target_id, DisplayMessage::system(result_text));
            state.set_flash("Child interaction completed");
        }
        TuiEvent::QuorumStart { phase, model_count } => {
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                pane.progress.quorum_status = Some(QuorumStatus {
                    phase,
                    total: model_count,
                    completed: 0,
                    approved: 0,
                });
            }
        }
        TuiEvent::QuorumModelVote { model: _, approved } => {
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id)
                && let Some(ref mut qs) = pane.progress.quorum_status
            {
                qs.completed += 1;
                if approved {
                    qs.approved += 1;
                }
            }
        }
        TuiEvent::QuorumComplete {
            phase,
            approved,
            feedback: _,
        } => {
            let status = if approved { "APPROVED" } else { "REJECTED" };
            state.set_flash(format!("{}: {}", phase, status));
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                pane.progress.quorum_status = None;
            }
        }
        TuiEvent::PlanRevision { revision, feedback } => {
            state.push_message_to(
                id,
                DisplayMessage::system(format!("Plan revision #{}: {}", revision, feedback)),
            );
        }
        TuiEvent::EnsembleStart(count) => {
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                pane.progress.ensemble_progress = Some(EnsembleProgress {
                    total_models: count,
                    plans_generated: 0,
                    models_completed: Vec::new(),
                    models_failed: Vec::new(),
                    voting_started: false,
                    plan_count: None,
                    selected: None,
                });
            }
        }
        TuiEvent::EnsemblePlanGenerated(model) => {
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id)
                && let Some(ref mut ep) = pane.progress.ensemble_progress
            {
                ep.plans_generated += 1;
                ep.models_completed.push(model);
            }
        }
        TuiEvent::EnsembleVotingStart(plan_count) => {
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id)
                && let Some(ref mut ep) = pane.progress.ensemble_progress
            {
                ep.voting_started = true;
                ep.plan_count = Some(plan_count);
            }
        }
        TuiEvent::EnsembleModelFailed { model, error } => {
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id)
                && let Some(ref mut ep) = pane.progress.ensemble_progress
            {
                ep.models_failed.push((model, error));
            }
        }
        TuiEvent::EnsembleComplete {
            selected_model,
            score,
        } => {
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id)
                && let Some(ref mut ep) = pane.progress.ensemble_progress
            {
                ep.selected = Some((selected_model.clone(), score));
            }
            state.push_message_to(
                id,
                DisplayMessage::system(format!(
                    "Selected plan from {} (score: {:.1}/10)",
                    selected_model, score
                )),
            );
        }
        TuiEvent::EnsembleFallback(reason) => {
            state.push_message_to(
                id,
                DisplayMessage::system(format!("Ensemble failed, solo fallback: {}", reason)),
            );
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                pane.progress.ensemble_progress = None;
            }
        }
        TuiEvent::ModelStreamStart { model, context: _ } => {
            // Register dynamic route + renderer for this model
            state.route.set_route(
                ContentSlot::ModelStream(model.clone()),
                SurfaceId::DynamicPane(model.clone()),
            );
            content_registry
                .borrow_mut()
                .register_mut(Box::new(ModelStreamRenderer::new(model.clone())));

            if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                pane.progress.model_streams.insert(
                    model.clone(),
                    ModelStreamState {
                        model_name: model,
                        streaming_text: String::new(),
                        status: ModelStreamStatus::Streaming,
                        score: None,
                        duration_ms: None,
                    },
                );
            }
        }
        TuiEvent::ModelStreamChunk { model, chunk } => {
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id)
                && let Some(ms) = pane.progress.model_streams.get_mut(&model)
            {
                ms.streaming_text.push_str(&chunk);
            }
        }
        TuiEvent::ModelStreamEnd(model) => {
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id)
                && let Some(ms) = pane.progress.model_streams.get_mut(&model)
            {
                ms.status = ModelStreamStatus::Complete;
            }
        }
        TuiEvent::EnsembleVoteScore { model, score } => {
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id)
                && let Some(ms) = pane.progress.model_streams.get_mut(&model)
            {
                ms.score = Some(score);
            }
        }
        TuiEvent::AgentStarting => {
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                let progress = &mut pane.progress;
                progress.is_running = true;
                progress.quorum_status = None;
                progress.task_progress = None;
                progress.ensemble_progress = None;
            }
        }
        TuiEvent::AgentResult {
            success,
            summary: _,
        } => {
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                let progress = &mut pane.progress;
                progress.is_running = false;
                progress.current_phase = None;
            }
            if success {
                state.set_flash("Agent completed successfully");
            } else {
                state.set_flash("Agent completed with issues");
            }
        }
        TuiEvent::AgentError(msg) => {
            if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                pane.progress.is_running = false;
            }
            state.set_flash(msg);
        }
        TuiEvent::Flash(msg) => {
            state.set_flash(msg);
        }
        TuiEvent::HistoryCleared => {}
        TuiEvent::Exit => {
            state.should_quit = true;
        }
        TuiEvent::ToolExecutionUpdate {
            task_index: _,
            execution_id,
            tool_name,
            state: exec_state,
            duration_ms,
            args_preview,
        } => {
            // Build flash message outside the mutable borrow scope
            let flash_msg = if let ToolExecutionDisplayState::Error { ref message } = exec_state {
                Some(format!("Tool error: {} - {}", tool_name, message))
            } else {
                None
            };

            if let Some(pane) = state.tabs.pane_for_interaction_mut(id) {
                // Auto-initialize task_progress for non-agent contexts
                // (gather_context, run_ask) that don't emit TaskStart
                let tp = pane
                    .progress
                    .task_progress
                    .get_or_insert_with(|| TaskProgress {
                        current_index: 0,
                        total: 0,
                        description: String::new(),
                        completed_tasks: Vec::new(),
                        active_tool_executions: Vec::new(),
                    });

                let display_status = match exec_state {
                    ToolExecutionDisplayState::Pending => ToolExecutionDisplayStatus::Pending,
                    ToolExecutionDisplayState::Running => ToolExecutionDisplayStatus::Running,
                    ToolExecutionDisplayState::Completed { preview } => {
                        ToolExecutionDisplayStatus::Completed { preview }
                    }
                    ToolExecutionDisplayState::Error { message } => {
                        ToolExecutionDisplayStatus::Error { message }
                    }
                };

                if let Some(existing) = tp
                    .active_tool_executions
                    .iter_mut()
                    .find(|e| e.execution_id == execution_id)
                {
                    existing.state = display_status;
                    existing.duration_ms = duration_ms;
                } else {
                    tp.active_tool_executions.push(ToolExecutionDisplay {
                        execution_id,
                        tool_name,
                        state: display_status,
                        duration_ms,
                        args_preview,
                    });
                }
            }

            if let Some(msg) = flash_msg {
                state.set_flash(msg);
            }
        }
        // Config/mode events handled by presenter already
        TuiEvent::Welcome { .. }
        | TuiEvent::ConfigDisplay(_)
        | TuiEvent::ModeChanged { .. }
        | TuiEvent::ScopeChanged(_)
        | TuiEvent::StrategyChanged(_)
        | TuiEvent::CommandError(_) => {}
    }
}

/// Apply a TuiEvent (from progress bridge or presenter) to state.
///
/// Delegates to [`apply_tui_event_to_interaction`] when the active pane has an
/// interaction id. When no interaction is active, only global events are handled.
fn apply_tui_event(
    state: &mut TuiState,
    content_registry: &RefCell<ContentRegistry>,
    event: TuiEvent,
) {
    if let Some(id) = state.active_interaction_id() {
        apply_tui_event_to_interaction(state, content_registry, id, event);
        return;
    }
    // No active interaction — handle global events only
    match event {
        TuiEvent::Flash(msg) => state.set_flash(msg),
        TuiEvent::Exit => {
            state.should_quit = true;
        }
        TuiEvent::HistoryCleared => {}
        TuiEvent::Welcome { .. }
        | TuiEvent::ConfigDisplay(_)
        | TuiEvent::ModeChanged { .. }
        | TuiEvent::ScopeChanged(_)
        | TuiEvent::StrategyChanged(_)
        | TuiEvent::CommandError(_) => {}
        _ => { /* dropped — no active interaction to target */ }
    }
}

/// Extract the meaningful LLM analysis text from task output.
///
/// Task output contains interleaved tool results and LLM text separated by `\n---\n`.
/// This function filters out tool result sections (lines starting with `[tool_name]:`)
/// and returns the last LLM text block, which is typically the final analysis/summary.
fn extract_response_text(output: &str) -> String {
    let sections: Vec<&str> = output.split("\n---\n").collect();

    // Find the last section that isn't a tool result
    sections
        .iter()
        .rev()
        .find(|section| {
            let trimmed = section.trim();
            !trimmed.is_empty()
                && !trimmed
                    .lines()
                    .next()
                    .is_some_and(|first| first.contains("]:") && first.starts_with('['))
        })
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_plain_text() {
        let output = "The code is well-structured and follows best practices.";
        assert_eq!(extract_response_text(output), output);
    }

    #[test]
    fn test_extract_filters_tool_results() {
        let output = "[read_file]: contents of foo.rs\n---\nThe code looks clean.";
        assert_eq!(extract_response_text(output), "The code looks clean.");
    }

    #[test]
    fn test_extract_returns_last_llm_block() {
        let output =
            "Initial analysis\n---\n[grep_search]: found 3 matches\n---\nFinal summary here.";
        assert_eq!(extract_response_text(output), "Final summary here.");
    }

    #[test]
    fn test_extract_empty_output() {
        assert_eq!(extract_response_text(""), String::new());
    }

    #[test]
    fn test_extract_only_tool_results() {
        let output = "[read_file]: file contents\n---\n[grep_search]: matches";
        assert_eq!(extract_response_text(output), String::new());
    }

    #[test]
    fn test_extract_preserves_long_text() {
        let long_text = "A".repeat(12000);
        let result = extract_response_text(&long_text);
        assert_eq!(result.len(), 12000);
    }

    #[test]
    fn test_extract_ignores_brackets_mid_line() {
        // Text that has brackets but not at start of line
        let output = "The function returns [Ok] or [Err]: both are valid.";
        assert_eq!(extract_response_text(output), output);
    }
}
