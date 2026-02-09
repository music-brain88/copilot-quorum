//! Help overlay widget

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

/// Widget for rendering help overlay
pub struct HelpWidget;

impl HelpWidget {
    pub fn new() -> Self {
        Self
    }

    fn build_help_text() -> Vec<Line<'static>> {
        vec![
            Line::from(Span::styled(
                "Keyboard Shortcuts",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("Ctrl+C", Style::default().fg(Color::Yellow)),
                Span::raw(" - Exit"),
            ]),
            Line::from(vec![
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::raw(" - Submit input"),
            ]),
            Line::from(vec![
                Span::styled("Backspace", Style::default().fg(Color::Yellow)),
                Span::raw(" - Delete character"),
            ]),
            Line::from(vec![
                Span::styled("F1", Style::default().fg(Color::Yellow)),
                Span::raw(" - Toggle this help"),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Commands",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("/help", Style::default().fg(Color::Green)),
                Span::raw(" - Show command help"),
            ]),
            Line::from(vec![
                Span::styled("/mode <level>", Style::default().fg(Color::Green)),
                Span::raw(" - Change mode (solo, ensemble)"),
            ]),
            Line::from(vec![
                Span::styled("/solo", Style::default().fg(Color::Green)),
                Span::raw(" - Switch to Solo mode"),
            ]),
            Line::from(vec![
                Span::styled("/ens", Style::default().fg(Color::Green)),
                Span::raw(" - Switch to Ensemble mode"),
            ]),
            Line::from(vec![
                Span::styled("/discuss", Style::default().fg(Color::Green)),
                Span::raw(" - Run Quorum Discussion"),
            ]),
            Line::from(vec![
                Span::styled("/scope", Style::default().fg(Color::Green)),
                Span::raw(" - Change phase scope"),
            ]),
            Line::from(vec![
                Span::styled("/strategy", Style::default().fg(Color::Green)),
                Span::raw(" - Change orchestration strategy"),
            ]),
            Line::from(vec![
                Span::styled("/config", Style::default().fg(Color::Green)),
                Span::raw(" - Show configuration"),
            ]),
            Line::from(vec![
                Span::styled("/clear", Style::default().fg(Color::Green)),
                Span::raw(" - Clear conversation history"),
            ]),
            Line::from(vec![
                Span::styled("/quit", Style::default().fg(Color::Green)),
                Span::raw(" - Exit"),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Press F1 or ESC to close",
                Style::default().fg(Color::DarkGray),
            )),
        ]
    }
}

impl Widget for HelpWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let lines = Self::build_help_text();

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Help ")
                    .style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: true });

        paragraph.render(area, buf);
    }
}

impl Default for HelpWidget {
    fn default() -> Self {
        Self::new()
    }
}
