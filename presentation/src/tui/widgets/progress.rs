//! Progress indicator widget

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::tui::state::ProgressState;

/// Widget for rendering progress information
pub struct ProgressWidget<'a> {
    progress: &'a ProgressState,
}

impl<'a> ProgressWidget<'a> {
    pub fn new(progress: &'a ProgressState) -> Self {
        Self { progress }
    }
}

impl<'a> Widget for ProgressWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let lines = if let Some(ref current) = self.progress.current_phase {
            vec![
                Line::from(vec![
                    Span::styled("Phase: ", Style::default().fg(Color::White)),
                    Span::styled(current, Style::default().fg(Color::Green)),
                ]),
                Line::from(vec![
                    Span::styled("Status: ", Style::default().fg(Color::White)),
                    Span::styled(
                        &self.progress.current_status,
                        Style::default().fg(Color::Yellow),
                    ),
                ]),
                Line::from(""),
            ]
        } else {
            vec![
                Line::from(Span::styled(
                    "Ready",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
                Line::from(""),
            ]
        };

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Progress ")
                .style(Style::default().fg(Color::White)),
        );

        paragraph.render(area, buf);
    }
}
