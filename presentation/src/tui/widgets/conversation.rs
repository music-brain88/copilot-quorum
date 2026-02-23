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
        let total_lines = count_wrapped_lines(&text, content_width);

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

        Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .render(area, buf);
    }
}

/// Count the total number of physical (wrapped) lines for a given available width.
///
/// `Paragraph::scroll((row, 0))` operates on physical lines after word-wrap,
/// but `Text::lines.len()` returns the logical (pre-wrap) count. This function
/// estimates the physical line count so scroll calculations stay accurate.
fn count_wrapped_lines(text: &Text<'_>, available_width: u16) -> u16 {
    if available_width == 0 {
        return text.lines.len() as u16;
    }
    let w = available_width as usize;
    text.lines
        .iter()
        .map(|line| {
            let lw = line.width();
            if lw == 0 { 1u16 } else { lw.div_ceil(w) as u16 }
        })
        .sum()
}
