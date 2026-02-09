//! Conversation history widget

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Widget},
};

use crate::tui::state::MessageEntry;

/// Widget for rendering conversation history
pub struct ConversationWidget<'a> {
    messages: &'a [MessageEntry],
}

impl<'a> ConversationWidget<'a> {
    pub fn new(messages: &'a [MessageEntry]) -> Self {
        Self { messages }
    }

    fn format_message<'b>(&self, entry: &'b MessageEntry) -> ListItem<'b> {
        let role_style = match entry.role.as_str() {
            "user" => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            "assistant" => Style::default().fg(Color::Green),
            "system" => Style::default().fg(Color::Yellow),
            _ => Style::default().fg(Color::White),
        };

        let lines = vec![
            Line::from(vec![
                Span::styled(format!("[{}] ", entry.role), role_style),
                Span::styled(&entry.timestamp, Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(entry.content.as_str()),
            Line::from(""),
        ];

        ListItem::new(lines)
    }
}

impl<'a> Widget for ConversationWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let items: Vec<ListItem> = self
            .messages
            .iter()
            .map(|msg| self.format_message(msg))
            .collect();

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Conversation ")
                .style(Style::default().fg(Color::White)),
        );

        Widget::render(list, area, buf);
    }
}
