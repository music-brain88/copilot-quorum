//! Status bar widget — mode indicator + key hints + flash messages

use crate::tui::state::{TuiState, content_slot_label};
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
            buf[(x, area.y)].set_style(bg_style).set_char(' ');
        }

        let mode = &self.state.mode;

        // Left: mode indicator
        let mode_text = mode.indicator();
        let mode_style = Style::default()
            .fg(Color::Black)
            .bg(mode.color())
            .add_modifier(Modifier::BOLD);

        let mode_span = Span::styled(format!(" {} ", mode_text), mode_style);

        // Focus indicator (Normal / Visual only — other modes don't act on focus)
        let focus_text = match mode {
            crate::tui::mode::InputMode::Normal | crate::tui::mode::InputMode::Visual => Some(
                format!(" ◆ {} ", content_slot_label(&self.state.focused_slot)),
            ),
            _ => None,
        };
        let focus_span = focus_text.as_deref().map(|t| {
            Span::styled(
                t,
                Style::default()
                    .fg(Color::Yellow)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
        });

        // Flash message or key hints on the right
        let right_text = if let Some((ref flash, _)) = self.state.flash_message {
            flash.clone()
        } else {
            match self.state.mode {
                crate::tui::mode::InputMode::Normal => {
                    "i:insert  yy/ya/Y:yank  v:visual  Ctrl+w:focus  gt:tab  ?:help".into()
                }
                crate::tui::mode::InputMode::Insert => "Enter:send  Esc:normal  Ctrl+C:quit".into(),
                crate::tui::mode::InputMode::Command => {
                    "Enter:execute  Esc:cancel  q:close tab  qa:quit  help:commands".into()
                }
                crate::tui::mode::InputMode::Visual => {
                    "hjkl/wb:extend  y/Enter:yank  Esc/v:cancel".into()
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

        // Render focus indicator right after the mode indicator
        let focus_width = if let Some(span) = focus_span {
            let w = span.content.len() as u16;
            let line = Line::from(vec![span]);
            buf.set_line(area.x + mode_width, area.y, &line, w);
            w
        } else {
            0
        };

        // Visual selection position indicator (Visual mode only)
        let sel_width = if self.state.mode == crate::tui::mode::InputMode::Visual {
            if let Some(sel) = self.state.visual_selection {
                let (s, e) = sel.range();
                let text = format!(" L{}→L{} ({} lines) ", s + 1, e + 1, e - s + 1);
                let w = text.len() as u16;
                let span = Span::styled(
                    text,
                    Style::default()
                        .fg(Color::Magenta)
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                );
                let line = Line::from(vec![span]);
                buf.set_line(area.x + mode_width + focus_width, area.y, &line, w);
                w
            } else {
                0
            }
        } else {
            0
        };

        // Render right-side hints, right-aligned — skip if it would collide
        // with the mode + focus + sel indicators.
        let right_width = right_text.len() as u16;
        let right_x = area.right().saturating_sub(right_width + 1);
        if right_x > area.x + mode_width + focus_width + sel_width {
            let right_line = Line::from(vec![right_span]);
            buf.set_line(right_x, area.y, &right_line, right_width + 1);
        }
    }
}
