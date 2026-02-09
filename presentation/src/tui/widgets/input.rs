//! Input box widget

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::tui::mode::Mode;
use crate::tui::state::TuiMode;
use quorum_domain::ConsensusLevel;

/// Widget for rendering input box with prompt
pub struct InputWidget<'a> {
    input: &'a str,
    prompt: String,
    color: Color,
}

impl<'a> InputWidget<'a> {
    pub fn new(input: &'a str, tui_mode: TuiMode) -> Self {
        let (prompt, color) = match tui_mode {
            TuiMode::Normal => ("normal> ".to_string(), Color::Blue),
            TuiMode::HumanIntervention => ("intervention> ".to_string(), Color::Red),
        };
        Self {
            input,
            prompt,
            color,
        }
    }

    pub fn with_mode(input: &'a str, mode: Mode) -> Self {
        let (prompt, color) = match mode {
            Mode::Normal => ("normal> ".to_string(), Color::Blue),
            Mode::Insert => ("insert> ".to_string(), Color::Green),
            Mode::Command => ("command> ".to_string(), Color::Yellow),
            Mode::Confirm => ("confirm> ".to_string(), Color::Magenta),
        };
        Self {
            input,
            prompt,
            color,
        }
    }

    pub fn with_consensus(input: &'a str, level: ConsensusLevel) -> Self {
        let (prompt, color) = match level {
            ConsensusLevel::Solo => ("solo> ".to_string(), Color::Cyan),
            ConsensusLevel::Ensemble => ("ensemble> ".to_string(), Color::Magenta),
        };
        Self {
            input,
            prompt,
            color,
        }
    }
}

impl<'a> Widget for InputWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let line = Line::from(vec![
            Span::styled(
                &self.prompt,
                Style::default()
                    .fg(self.color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(self.input, Style::default().fg(Color::White)),
            Span::styled("_", Style::default().fg(Color::DarkGray)), // Cursor
        ]);

        let paragraph = Paragraph::new(line).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Input ")
                .style(Style::default().fg(Color::White)),
        );

        paragraph.render(area, buf);
    }
}
