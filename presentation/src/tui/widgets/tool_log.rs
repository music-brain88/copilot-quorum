//! Tool log widget — full tool execution history (no last-3 limit).
//!
//! Unlike ProgressPanel which shows only recent tool executions,
//! ToolLogRenderer displays the complete tool execution log for all tasks
//! in the current interaction.

use crate::tui::content::{ContentRenderer, ContentSlot};
use crate::tui::state::{ToolExecutionDisplay, ToolExecutionDisplayStatus, TuiState};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use super::progress_panel::{format_duration, render_tool_execution_line, truncate_str};

/// ContentRenderer for the tool execution log.
///
/// Shows all tool executions across all tasks (not limited to last 3 like ProgressPanel).
pub struct ToolLogRenderer;

impl ContentRenderer for ToolLogRenderer {
    fn slot(&self) -> ContentSlot {
        ContentSlot::ToolLog
    }

    fn render_content(&self, state: &TuiState, area: Rect, buf: &mut Buffer) {
        ToolLogWidget::new(state).render(area, buf);
    }

    fn get_text_content(&self, state: &TuiState) -> String {
        let progress = &state.tabs.active_pane().progress;
        let mut lines: Vec<String> = Vec::new();

        if let Some(ref tp) = progress.task_progress {
            for summary in &tp.completed_tasks {
                let icon = if summary.success { "✓" } else { "✗" };
                let dur = summary.duration_ms.map(format_duration).unwrap_or_default();
                lines.push(format!(
                    "{} Task {}: {}{}",
                    icon, summary.index, summary.description, dur
                ));
                for exec in &summary.tool_executions {
                    lines.push(format_tool_execution_plain(exec));
                }
            }

            if !tp.active_tool_executions.is_empty() {
                if !lines.is_empty() {
                    lines.push(String::new());
                }
                lines.push(format!(
                    "▸ Task {}: {}",
                    tp.current_index, tp.description
                ));
                for exec in &tp.active_tool_executions {
                    lines.push(format_tool_execution_plain(exec));
                }
            }
        }

        if lines.is_empty() {
            return "No tool executions".to_string();
        }
        lines.join("\n")
    }
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

struct ToolLogWidget<'a> {
    state: &'a TuiState,
}

impl<'a> ToolLogWidget<'a> {
    fn new(state: &'a TuiState) -> Self {
        Self { state }
    }
}

impl<'a> Widget for ToolLogWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let progress = &self.state.tabs.active_pane().progress;
        let mut lines: Vec<Line> = Vec::new();

        if let Some(ref tp) = progress.task_progress {
            // Show all completed task tool executions (no limit)
            for summary in &tp.completed_tasks {
                let icon = if summary.success {
                    Span::styled("✓ ", Style::default().fg(Color::Green))
                } else {
                    Span::styled("✗ ", Style::default().fg(Color::Red))
                };
                let duration_str = summary.duration_ms.map(format_duration).unwrap_or_default();
                lines.push(Line::from(vec![
                    icon,
                    Span::styled(
                        format!(
                            "Task {}: {}",
                            summary.index,
                            truncate_str(&summary.description, 30)
                        ),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(duration_str, Style::default().fg(Color::DarkGray)),
                ]));

                // All tool executions for this task (no limit)
                for exec in &summary.tool_executions {
                    render_tool_execution_line(&mut lines, exec);
                }
            }

            // Show active task tool executions
            if !tp.active_tool_executions.is_empty() {
                if !lines.is_empty() {
                    lines.push(Line::from(""));
                }
                lines.push(Line::from(Span::styled(
                    format!(
                        "▸ Task {}: {}",
                        tp.current_index,
                        truncate_str(&tp.description, 30)
                    ),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
                for exec in &tp.active_tool_executions {
                    render_tool_execution_line(&mut lines, exec);
                }
            }
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "No tool executions",
                Style::default().fg(Color::DarkGray),
            )));
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Tool Log ")
            .style(Style::default().fg(Color::White));

        Paragraph::new(lines).block(block).render(area, buf);
    }
}
