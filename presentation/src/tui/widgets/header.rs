//! Header widget — shows mode, model, and current phase

use crate::tui::state::TuiState;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

pub struct HeaderWidget<'a> {
    state: &'a TuiState,
}

impl<'a> HeaderWidget<'a> {
    pub fn new(state: &'a TuiState) -> Self {
        Self { state }
    }
}

impl<'a> Widget for HeaderWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mode_label = format!("{}", self.state.consensus_level);
        let mode_color = if self.state.consensus_level.is_ensemble() {
            Color::Magenta
        } else {
            Color::Cyan
        };

        let phase_text = if self.state.progress.is_running {
            self.state.progress.phase_name.as_str()
        } else {
            "Ready"
        };

        let line = Line::from(vec![
            Span::styled("◉ ", Style::default().fg(Color::Green)),
            Span::styled(
                mode_label,
                Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | "),
            Span::styled(&self.state.model_name, Style::default().fg(Color::White)),
            Span::raw(" | "),
            Span::styled(phase_text, Style::default().fg(Color::Yellow)),
        ]);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Copilot Quorum ")
            .style(Style::default().fg(Color::White));

        Paragraph::new(line).block(block).render(area, buf);
    }
}
