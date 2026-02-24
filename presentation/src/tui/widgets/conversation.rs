//! Conversation widget — message history + streaming text

use crate::tui::content::{ContentRenderer, ContentSlot};
use crate::tui::state::TuiState;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

/// ContentRenderer adapter for the conversation pane.
pub struct ConversationRenderer;

impl ContentRenderer for ConversationRenderer {
    fn slot(&self) -> ContentSlot {
        ContentSlot::Conversation
    }

    fn render_content(&self, state: &TuiState, area: Rect, buf: &mut Buffer) {
        ConversationWidget::new(state).render(area, buf);
    }
}

pub struct ConversationWidget<'a> {
    state: &'a TuiState,
}

impl<'a> ConversationWidget<'a> {
    pub fn new(state: &'a TuiState) -> Self {
        Self { state }
    }

    fn format_messages(&self) -> Text<'_> {
        let mut lines: Vec<Line> = Vec::new();

        let pane = self.state.tabs.active_pane();
        for msg in &pane.conversation.messages {
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
        if !pane.conversation.streaming_text.is_empty() {
            lines.push(Line::from(Span::styled(
                "Agent: ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )));
            for content_line in pane.conversation.streaming_text.lines() {
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
        let visible_height = area.height.saturating_sub(2); // borders
        let content_width = area.width.saturating_sub(2); // borders

        // Use Paragraph's own line_count() which uses WordWrapper internally,
        // matching the exact wrapping algorithm used during rendering.
        // Built without block so line_count returns pure content lines.
        let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
        let total_lines = paragraph.line_count(content_width) as u16;

        // Calculate scroll: scroll_offset=0 means "show bottom"
        let pane = self.state.tabs.active_pane();
        let scroll = if total_lines > visible_height {
            let max_scroll = total_lines - visible_height;
            let offset = (pane.conversation.scroll_offset as u16).min(max_scroll);
            max_scroll - offset
        } else {
            0
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Conversation ")
            .style(Style::default().fg(Color::White));

        paragraph.block(block).scroll((scroll, 0)).render(area, buf);
    }
}
