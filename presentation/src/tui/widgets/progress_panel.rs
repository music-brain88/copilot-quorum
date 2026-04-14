//! Progress panel widget — phase, tools, quorum status, tool execution lifecycle

use crate::tui::content::{ContentRenderer, ContentSlot};
use crate::tui::state::{ProgressState, ToolExecutionDisplay, ToolExecutionDisplayStatus, TuiState};
use quorum_domain::AgentPhase;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

/// ContentRenderer adapter for the progress panel.
pub struct ProgressRenderer;

impl ContentRenderer for ProgressRenderer {
    fn slot(&self) -> ContentSlot {
        ContentSlot::Progress
    }

    fn render_content(&self, state: &TuiState, area: Rect, buf: &mut Buffer) {
        ProgressPanelWidget::new(state).render(area, buf);
    }

    fn get_text_content(&self, state: &TuiState) -> String {
        format_progress_plain(&state.tabs.active_pane().progress)
    }
}

/// Plain-text rendering of the progress panel (used by yank).
pub(super) fn format_progress_plain(progress: &ProgressState) -> String {
    let mut lines: Vec<String> = Vec::new();

    if let Some(ref phase) = progress.current_phase {
        lines.push(format!(
            "Phase: {} {}",
            phase_emoji(phase),
            progress.phase_name
        ));
    } else {
        lines.push("Phase: Ready".to_string());
    }
    lines.push(String::new());

    if let Some(ref tp) = progress.task_progress {
        let completed = tp.completed_tasks.len();
        let all_done = !progress.is_running && completed == tp.total && tp.total > 0;
        if all_done {
            lines.push(format!("Tasks: {}/{} completed", completed, tp.total));
        } else {
            lines.push(format!(
                "Task {}/{}: {}",
                tp.current_index, tp.total, tp.description
            ));
        }

        let remaining = tp.total.saturating_sub(completed);
        lines.push(format!(
            "[{}{}] {}/{}",
            "=".repeat(completed),
            " ".repeat(remaining),
            completed,
            tp.total,
        ));

        for exec in &tp.active_tool_executions {
            lines.push(format_tool_execution_plain(exec));
        }

        for summary in &tp.completed_tasks {
            let icon = if summary.success { "✓" } else { "✗" };
            let dur = summary.duration_ms.map(format_duration).unwrap_or_default();
            lines.push(format!(
                "  {} Task {}: {}{}",
                icon, summary.index, summary.description, dur
            ));
            for exec in &summary.tool_executions {
                lines.push(format_tool_execution_plain(exec));
            }
        }
        lines.push(String::new());
    }

    if let Some(ref ep) = progress.ensemble_progress {
        if let Some((ref model, score)) = ep.selected {
            lines.push(format!("Ensemble: Selected {} ({:.1}/10)", model, score));
        } else if ep.voting_started {
            let plan_count = ep.plan_count.unwrap_or(0);
            lines.push(format!("Ensemble: Voting on {} plans...", plan_count));
        } else {
            lines.push(format!(
                "Ensemble: Planning {}/{} models done",
                ep.plans_generated, ep.total_models
            ));
        }
        for model in &ep.models_completed {
            lines.push(format!("  ✓ {}", model));
        }
        for (model, _err) in &ep.models_failed {
            lines.push(format!("  ✗ {}", model));
        }
        lines.push(String::new());
    }

    if let Some(ref quorum) = progress.quorum_status {
        lines.push(format!("Quorum: {}", quorum.phase));
        let filled = quorum.approved;
        let empty = quorum.total.saturating_sub(quorum.completed);
        let rejected = quorum.completed.saturating_sub(quorum.approved);
        lines.push(format!(
            "[{}{}{}] {}/{}",
            "●".repeat(filled),
            "○".repeat(rejected),
            "·".repeat(empty),
            quorum.completed,
            quorum.total,
        ));
    }

    lines.join("\n")
}

fn format_tool_execution_plain(exec: &ToolExecutionDisplay) -> String {
    let (icon, suffix) = match &exec.state {
        ToolExecutionDisplayStatus::Pending => ("…", String::new()),
        ToolExecutionDisplayStatus::Running => ("▸", String::new()),
        ToolExecutionDisplayStatus::Completed { .. } => {
            let dur = exec.duration_ms.map(format_duration).unwrap_or_default();
            ("✓", dur)
        }
        ToolExecutionDisplayStatus::Error { message } => ("✗", format!(" — {}", message)),
    };
    let args = exec
        .args_preview
        .as_deref()
        .map(|p| format!("  {}", p))
        .unwrap_or_default();
    format!("    {} {}{}{}", icon, exec.tool_name, args, suffix)
}

pub struct ProgressPanelWidget<'a> {
    state: &'a TuiState,
}

impl<'a> ProgressPanelWidget<'a> {
    pub fn new(state: &'a TuiState) -> Self {
        Self { state }
    }
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

impl<'a> Widget for ProgressPanelWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let progress = &self.state.tabs.active_pane().progress;
        let mut lines: Vec<Line> = Vec::new();

        // Phase
        if let Some(ref phase) = progress.current_phase {
            lines.push(Line::from(vec![
                Span::styled("Phase: ", Style::default().fg(Color::White)),
                Span::styled(
                    format!("{} {}", phase_emoji(phase), progress.phase_name),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                "Phase: Ready",
                Style::default().fg(Color::DarkGray),
            )));
        }

        lines.push(Line::from(""));

        // Task progress
        if let Some(ref tp) = progress.task_progress {
            let completed = tp.completed_tasks.len();
            let all_done = !progress.is_running && completed == tp.total && tp.total > 0;
            let has_failures = tp.completed_tasks.iter().any(|t| !t.success);

            // Header line: completed summary vs in-progress indicator
            if all_done {
                let (icon, color) = if has_failures {
                    ("⚠", Color::Yellow)
                } else {
                    ("✓", Color::Green)
                };
                lines.push(Line::from(vec![
                    Span::styled("Tasks: ", Style::default().fg(Color::White)),
                    Span::styled(
                        format!("{} {}/{} completed", icon, completed, tp.total),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("Task: ", Style::default().fg(Color::White)),
                    Span::styled(
                        format!(
                            "⚡ {}/{}: {}",
                            tp.current_index,
                            tp.total,
                            truncate_str(&tp.description, 25)
                        ),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
            }

            // Progress bar
            let remaining = tp.total.saturating_sub(completed);
            let bar = format!(
                "[{}{}] {}/{}",
                "=".repeat(completed),
                " ".repeat(remaining),
                completed,
                tp.total,
            );
            lines.push(Line::from(Span::styled(
                bar,
                Style::default().fg(if all_done {
                    Color::Green
                } else {
                    Color::Yellow
                }),
            )));

            // Show active task tool executions (current task in progress)
            if !tp.active_tool_executions.is_empty() {
                for exec in &tp.active_tool_executions {
                    render_tool_execution_line(&mut lines, exec);
                }
            }

            // Show completed tasks (last 3) with their tool executions
            for summary in tp
                .completed_tasks
                .iter()
                .rev()
                .take(3)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                let icon = if summary.success {
                    Span::styled("✓ ", Style::default().fg(Color::Green))
                } else {
                    Span::styled("✗ ", Style::default().fg(Color::Red))
                };
                let duration_str = summary.duration_ms.map(format_duration).unwrap_or_default();
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    icon,
                    Span::styled(
                        format!("Task {}", summary.index),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(duration_str, Style::default().fg(Color::DarkGray)),
                ]));
                // Show tool executions for completed tasks (last 3)
                for exec in summary
                    .tool_executions
                    .iter()
                    .rev()
                    .take(3)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                {
                    render_tool_execution_line(&mut lines, exec);
                }
            }
            lines.push(Line::from(""));
        }

        // Ensemble progress
        if let Some(ref ep) = progress.ensemble_progress {
            if let Some((ref model, score)) = ep.selected {
                lines.push(Line::from(vec![
                    Span::styled("Ensemble: ", Style::default().fg(Color::White)),
                    Span::styled(
                        format!("Selected {} ({:.1}/10)", model, score),
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
            } else if ep.voting_started {
                let plan_count = ep.plan_count.unwrap_or(0);
                lines.push(Line::from(vec![
                    Span::styled("Ensemble: ", Style::default().fg(Color::White)),
                    Span::styled(
                        format!("Voting on {} plans...", plan_count),
                        Style::default().fg(Color::Yellow),
                    ),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("Ensemble: ", Style::default().fg(Color::White)),
                    Span::styled(
                        format!(
                            "Planning {}/{} models done",
                            ep.plans_generated, ep.total_models
                        ),
                        Style::default().fg(Color::Cyan),
                    ),
                ]));
            }
            // Show completed/failed models
            for model in &ep.models_completed {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("✓ ", Style::default().fg(Color::Green)),
                    Span::styled(model.as_str(), Style::default().fg(Color::DarkGray)),
                ]));
            }
            for (model, _error) in &ep.models_failed {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("✗ ", Style::default().fg(Color::Red)),
                    Span::styled(model.as_str(), Style::default().fg(Color::DarkGray)),
                ]));
            }
            lines.push(Line::from(""));
        }

        // Quorum status
        if let Some(ref quorum) = progress.quorum_status {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Quorum: ", Style::default().fg(Color::White)),
                Span::styled(&quorum.phase, Style::default().fg(Color::Magenta)),
            ]));

            // Simple vote bar
            let filled = quorum.approved;
            let empty = quorum.total.saturating_sub(quorum.completed);
            let rejected = quorum.completed.saturating_sub(quorum.approved);
            let bar = format!(
                "[{}{}{}] {}/{}",
                "●".repeat(filled),
                "○".repeat(rejected),
                "·".repeat(empty),
                quorum.completed,
                quorum.total,
            );
            lines.push(Line::from(Span::styled(
                bar,
                Style::default().fg(Color::Yellow),
            )));
        }

        let block = super::focus_block(self.state, &ContentSlot::Progress, " Progress ");

        Paragraph::new(lines).block(block).render(area, buf);
    }
}

/// Render a single tool execution line in the progress panel.
///
/// Format: `    ▸ read_file  src/main.rs`
///         `    ✓ run_command  cargo test (1.2s)`
pub(super) fn render_tool_execution_line<'a>(
    lines: &mut Vec<Line<'a>>,
    exec: &ToolExecutionDisplay,
) {
    let (icon, color, suffix) = match &exec.state {
        ToolExecutionDisplayStatus::Pending => ("…", Color::DarkGray, String::new()),
        ToolExecutionDisplayStatus::Running => ("▸", Color::Yellow, String::new()),
        ToolExecutionDisplayStatus::Completed { .. } => {
            let dur = exec.duration_ms.map(format_duration).unwrap_or_default();
            ("✓", Color::Green, dur)
        }
        ToolExecutionDisplayStatus::Error { message } => {
            let msg = format!(" — {}", truncate_str(message, 27));
            ("✗", Color::Red, msg)
        }
    };

    let mut spans = vec![
        Span::raw("    "),
        Span::styled(format!("{} ", icon), Style::default().fg(color)),
        Span::styled(exec.tool_name.clone(), Style::default().fg(Color::DarkGray)),
    ];

    // Show args preview after tool name (e.g., "  src/main.rs")
    if let Some(ref preview) = exec.args_preview {
        spans.push(Span::styled(
            format!("  {}", preview),
            Style::default().fg(Color::DarkGray),
        ));
    }

    spans.push(Span::styled(suffix, Style::default().fg(Color::DarkGray)));
    lines.push(Line::from(spans));
}

/// Format a duration in milliseconds to a human-readable string.
pub(super) fn format_duration(ms: u64) -> String {
    if ms < 1000 {
        format!(" ({}ms)", ms)
    } else {
        format!(" ({:.1}s)", ms as f64 / 1000.0)
    }
}

/// Truncate a string to max_len characters, appending "..." if truncated.
pub(super) fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}
