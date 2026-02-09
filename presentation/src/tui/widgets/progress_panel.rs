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

        // Current tool
        if let Some(ref tool) = progress.current_tool {
            lines.push(Line::from(vec![
                Span::styled("Tool: ", Style::default().fg(Color::White)),
                Span::styled(
                    format!("🔧 {}", tool),
                    Style::default().fg(Color::Cyan),
                ),
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
