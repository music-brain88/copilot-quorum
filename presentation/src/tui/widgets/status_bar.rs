//! Status bar widget — mode indicator + key hints + flash messages

use crate::tui::state::TuiState;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};

pub struct StatusBarWidget<'a> {
    state: &'a TuiState,
}

impl<'a> StatusBarWidget<'a> {
    pub fn new(state: &'a TuiState) -> Self {
        Self { state }
    }
}

impl<'a> Widget for StatusBarWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Fill background
        let bg_style = Style::default().bg(Color::DarkGray).fg(Color::White);
        for x in area.left()..area.right() {
            buf[(x, area.y)]
                .set_style(bg_style)
                .set_char(' ');
        }

        let mode = &self.state.mode;

        // Left: mode indicator
        let mode_text = mode.indicator();
        let mode_style = Style::default()
            .fg(Color::Black)
            .bg(mode.color())
            .add_modifier(Modifier::BOLD);

        let mode_span = Span::styled(format!(" {} ", mode_text), mode_style);

        // Flash message or key hints on the right
        let right_text = if let Some((ref flash, _)) = self.state.flash_message {
            flash.clone()
        } else {
            match self.state.mode {
                crate::tui::mode::InputMode::Normal => {
                    "i:insert  ::command  j/k:scroll  ?:help  Ctrl+C:quit".into()
                }
                crate::tui::mode::InputMode::Insert => {
                    "Enter:send  Esc:normal  Ctrl+C:quit".into()
                }
                crate::tui::mode::InputMode::Command => {
                    "Enter:execute  Esc:cancel  q:quit  help:commands".into()
                }
            }
        };

        let right_span = Span::styled(
            right_text.clone(),
            Style::default().fg(Color::White).bg(Color::DarkGray),
        );

        // Render mode indicator on the left
        let mode_line = Line::from(vec![mode_span]);
        let mode_width = mode_text.len() as u16 + 2; // padding

        buf.set_line(area.x, area.y, &mode_line, mode_width);

        // Render right-side hints, right-aligned
        let right_width = right_text.len() as u16;
        let right_x = area.right().saturating_sub(right_width + 1);
        if right_x > area.x + mode_width {
            let right_line = Line::from(vec![right_span]);
            buf.set_line(right_x, area.y, &right_line, right_width + 1);
        }
    }
}
