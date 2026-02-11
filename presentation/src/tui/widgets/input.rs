//! Input widget — text input area with mode-aware prompt
//!
//! Supports multiline input: text is split on `\n` and rendered as
//! multiple `Line`s inside a `Paragraph`. The prompt prefix is shown
//! only on the first line; continuation lines get a "  " indent.

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

        let prompt_span = Span::styled(
            prompt,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        );
        let continuation = "  ";

        let border_style = if active {
            Style::default().fg(color)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Input ")
            .style(border_style);

        // Inner area height (excluding borders) — used for scroll
        let inner_height = area.height.saturating_sub(2) as usize;

        let lines = if active {
            build_active_lines(text, cursor_pos, color, &prompt_span, continuation)
        } else {
            build_inactive_lines(text, color, &prompt_span, continuation)
        };

        // Scroll so the cursor line (last line with cursor) is visible
        let total_lines = lines.len();
        let scroll_offset = if total_lines > inner_height {
            // Find cursor line index — it's the line containing the cursor position
            let cursor_line = find_cursor_line(text, cursor_pos);
            if cursor_line >= inner_height {
                (cursor_line + 1).saturating_sub(inner_height)
            } else {
                0
            }
        } else {
            0
        };

        Paragraph::new(lines)
            .block(block)
            .scroll((scroll_offset as u16, 0))
            .render(area, buf);
    }
}

/// Build lines for active (Insert/Command) mode with cursor rendering
fn build_active_lines<'a>(
    text: &str,
    cursor_pos: usize,
    color: Color,
    prompt_span: &Span<'a>,
    continuation: &str,
) -> Vec<Line<'a>> {
    let cursor_style = Style::default().fg(Color::Black).bg(color);

    // Split text into lines, preserving trailing newline as empty line
    let raw_lines: Vec<&str> = if text.is_empty() {
        vec![""]
    } else if text.ends_with('\n') {
        // split() on "foo\n" gives ["foo", ""], which is correct
        text.split('\n').collect()
    } else {
        text.split('\n').collect()
    };

    let mut lines = Vec::with_capacity(raw_lines.len());
    let mut byte_offset = 0;

    for (i, line_text) in raw_lines.iter().enumerate() {
        let line_start = byte_offset;
        let line_end = line_start + line_text.len();
        let is_first = i == 0;

        let mut spans: Vec<Span<'a>> = Vec::new();

        // Prompt / continuation prefix
        if is_first {
            spans.push(prompt_span.clone());
        } else {
            spans.push(Span::styled(
                continuation.to_string(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
        }

        // Check if cursor is on this line
        let cursor_on_line = cursor_pos >= line_start && cursor_pos <= line_end;

        if cursor_on_line {
            let local_cursor = cursor_pos - line_start;
            let before = &line_text[..local_cursor];
            let after = &line_text[local_cursor..];

            spans.push(Span::raw(before.to_string()));

            if after.is_empty() {
                // Cursor at end of line — show block cursor on space
                spans.push(Span::styled(" ", cursor_style));
            } else {
                // Cursor on a character
                let ch = after.chars().next().unwrap();
                let ch_len = ch.len_utf8();
                spans.push(Span::styled(after[..ch_len].to_string(), cursor_style));
                if ch_len < after.len() {
                    spans.push(Span::raw(after[ch_len..].to_string()));
                }
            }
        } else {
            spans.push(Span::raw(line_text.to_string()));
        }

        lines.push(Line::from(spans));

        // Advance byte_offset: line content + '\n' separator
        byte_offset = line_end + 1; // +1 for the '\n'
    }

    lines
}

/// Build lines for inactive (Normal) mode — no cursor
fn build_inactive_lines<'a>(
    text: &str,
    color: Color,
    prompt_span: &Span<'a>,
    continuation: &str,
) -> Vec<Line<'a>> {
    let raw_lines: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.split('\n').collect()
    };

    let inactive_style = Style::default().fg(color);

    raw_lines
        .iter()
        .enumerate()
        .map(|(i, line_text)| {
            let prefix = if i == 0 {
                prompt_span.clone()
            } else {
                Span::styled(
                    continuation.to_string(),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )
            };
            Line::from(vec![
                prefix,
                Span::styled(line_text.to_string(), inactive_style),
            ])
        })
        .collect()
}

/// Find which line (0-indexed) the cursor is on
fn find_cursor_line(text: &str, cursor_pos: usize) -> usize {
    text[..cursor_pos.min(text.len())]
        .chars()
        .filter(|&c| c == '\n')
        .count()
}
