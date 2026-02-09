//! Conversation widget — message history + streaming text

use crate::tui::state::TuiState;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

pub struct ConversationWidget<'a> {
    state: &'a TuiState,
}

impl<'a> ConversationWidget<'a> {
    pub fn new(state: &'a TuiState) -> Self {
        Self { state }
    }

    fn format_messages(&self) -> Text<'_> {
        let mut lines: Vec<Line> = Vec::new();

        for msg in &self.state.messages {
            let role_style = Style::default()
                .fg(msg.role.color())
                .add_modifier(Modifier::BOLD);

            lines.push(Line::from(Span::styled(
                format!("{}: ", msg.role.label()),
                role_style,
            )));

            for content_line in msg.content.lines() {
                lines.push(Line::from(format!("  {}", content_line)));
            }
            lines.push(Line::from(""));
        }

        // Append streaming text if present
        if !self.state.streaming_text.is_empty() {
            lines.push(Line::from(Span::styled(
                "Agent: ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )));
            for content_line in self.state.streaming_text.lines() {
                lines.push(Line::from(format!("  {}", content_line)));
            }
            lines.push(Line::from(Span::styled(
                "  ▌",
                Style::default().fg(Color::Green),
            )));
        }

        Text::from(lines)
    }
}

impl<'a> Widget for ConversationWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let text = self.format_messages();
        let total_lines = text.lines.len() as u16;
        let visible_height = area.height.saturating_sub(2); // borders

        // Calculate scroll: scroll_offset=0 means "show bottom"
        let scroll = if total_lines > visible_height {
            let max_scroll = total_lines - visible_height;
            let offset = (self.state.scroll_offset as u16).min(max_scroll);
            max_scroll - offset
        } else {
            0
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Conversation ")
            .style(Style::default().fg(Color::White));

        Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .render(area, buf);
    }
}
