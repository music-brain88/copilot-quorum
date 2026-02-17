//! Tab bar widget — renders tab labels when multiple tabs are open.

use crate::tui::tab::TabManager;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};

pub struct TabBarWidget<'a> {
    tabs: &'a TabManager,
}

impl<'a> TabBarWidget<'a> {
    pub fn new(tabs: &'a TabManager) -> Self {
        Self { tabs }
    }
}

impl<'a> Widget for TabBarWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Fill background
        let bg_style = Style::default().bg(Color::DarkGray).fg(Color::White);
        for x in area.left()..area.right() {
            buf[(x, area.y)].set_style(bg_style).set_char(' ');
        }

        let active_idx = self.tabs.active_index();
        let mut spans: Vec<Span> = Vec::new();

        for (i, tab) in self.tabs.tabs().iter().enumerate() {
            let label = format!(" {} {} ", i + 1, tab.pane.display_title());
            if i == active_idx {
                spans.push(Span::styled(
                    label,
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::styled(
                    label,
                    Style::default().fg(Color::Gray).bg(Color::DarkGray),
                ));
            }
            spans.push(Span::styled(" ", bg_style));
        }

        let line = Line::from(spans);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}
