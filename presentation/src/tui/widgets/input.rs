//! Input widget — text input area with mode-aware prompt

use crate::tui::mode::InputMode;
use crate::tui::state::TuiState;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

pub struct InputWidget<'a> {
    state: &'a TuiState,
}

impl<'a> InputWidget<'a> {
    pub fn new(state: &'a TuiState) -> Self {
        Self { state }
    }
}

impl<'a> Widget for InputWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Build mode-aware prompt: "solo:ask> " or "ens:discuss> "
        let level_short = match self.state.consensus_level {
            quorum_domain::ConsensusLevel::Solo => "solo",
            quorum_domain::ConsensusLevel::Ensemble => "ens",
        };
        let mode_prompt = format!("{}:{}> ", level_short, self.state.interaction_type);

        let (prompt, text, cursor_pos, color, active) = match self.state.mode {
            InputMode::Insert => (
                mode_prompt.as_str(),
                &self.state.input,
                self.state.cursor_pos,
                Color::Green,
                true,
            ),
            InputMode::Command => (
                ":",
                &self.state.command_input,
                self.state.command_cursor,
                Color::Yellow,
                true,
            ),
            InputMode::Normal => (
                mode_prompt.as_str(),
                &self.state.input,
                self.state.cursor_pos,
                Color::DarkGray,
                false,
            ),
        };

        let mut spans = vec![Span::styled(
            prompt,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )];

        if active {
            // Split text at cursor position for cursor rendering
            let before = &text[..cursor_pos.min(text.len())];
            let after = &text[cursor_pos.min(text.len())..];

            spans.push(Span::raw(before.to_string()));
            // Cursor block
            if after.is_empty() {
                spans.push(Span::styled(
                    " ",
                    Style::default().fg(Color::Black).bg(color),
                ));
            } else {
                let cursor_char = &after[..after.chars().next().map(|c| c.len_utf8()).unwrap_or(1)];
                spans.push(Span::styled(
                    cursor_char.to_string(),
                    Style::default().fg(Color::Black).bg(color),
                ));
                spans.push(Span::raw(after[cursor_char.len()..].to_string()));
            }
        } else {
            spans.push(Span::styled(
                text.to_string(),
                Style::default().fg(Color::DarkGray),
            ));
        }

        let line = Line::from(spans);
        let border_style = if active {
            Style::default().fg(color)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Input ")
            .style(border_style);

        Paragraph::new(line).block(block).render(area, buf);
    }
}
