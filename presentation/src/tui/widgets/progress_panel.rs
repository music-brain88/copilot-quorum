//! Progress panel widget — phase, tools, quorum status

use crate::tui::state::TuiState;
use quorum_domain::AgentPhase;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

pub struct ProgressPanelWidget<'a> {
    state: &'a TuiState,
}

impl<'a> ProgressPanelWidget<'a> {
    pub fn new(state: &'a TuiState) -> Self {
        Self { state }
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
}

impl<'a> Widget for ProgressPanelWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let progress = &self.state.progress;
        let mut lines: Vec<Line> = Vec::new();

        // Phase
        if let Some(ref phase) = progress.current_phase {
            lines.push(Line::from(vec![
                Span::styled("Phase: ", Style::default().fg(Color::White)),
                Span::styled(
                    format!("{} {}", Self::phase_emoji(phase), progress.phase_name),
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
            lines.push(Line::from(vec![
                Span::styled("Task: ", Style::default().fg(Color::White)),
                Span::styled(
                    format!("⚡ {}/{}: {}", tp.current_index, tp.total, truncate_str(&tp.description, 25)),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));

            // Progress bar
            let completed = tp.completed_tasks.len();
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
                Style::default().fg(Color::Yellow),
            )));

            // Show completed tasks (last 3)
            for summary in tp.completed_tasks.iter().rev().take(3).collect::<Vec<_>>().into_iter().rev() {
                let icon = if summary.success {
                    Span::styled("✓ ", Style::default().fg(Color::Green))
                } else {
                    Span::styled("✗ ", Style::default().fg(Color::Red))
                };
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    icon,
                    Span::styled(
                        format!("Task {}", summary.index),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
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
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
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
                        format!("Planning {}/{} models done", ep.plans_generated, ep.total_models),
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

        // Current tool
        if let Some(ref tool) = progress.current_tool {
            lines.push(Line::from(vec![
                Span::styled("Tool: ", Style::default().fg(Color::White)),
                Span::styled(format!("🔧 {}", tool), Style::default().fg(Color::Cyan)),
            ]));
        }

        // Recent tool log (last 5)
        let recent_tools: Vec<_> = progress
            .tool_log
            .iter()
            .rev()
            .take(5)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        if !recent_tools.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Recent:",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )));
            for entry in recent_tools {
                let icon = match entry.success {
                    Some(true) => Span::styled("✓ ", Style::default().fg(Color::Green)),
                    Some(false) => Span::styled("✗ ", Style::default().fg(Color::Red)),
                    None => Span::styled("… ", Style::default().fg(Color::Yellow)),
                };
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    icon,
                    Span::styled(&entry.tool_name, Style::default().fg(Color::DarkGray)),
                ]));
            }
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

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Progress ")
            .style(Style::default().fg(Color::White));

        Paragraph::new(lines).block(block).render(area, buf);
    }
}

/// Truncate a string to max_len characters, appending "..." if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}
