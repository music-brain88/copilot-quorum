//! Lua content renderer — simple text-based rendering for Lua-registered content slots.
//!
//! Renders text stored in `TuiState.lua_content` with a titled border.
//! This is the simplest possible renderer; future phases may support
//! richer Lua-driven drawing.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use super::super::content::{ContentRenderer, ContentSlot};
use super::super::state::TuiState;

/// A content renderer for a Lua-registered text slot.
pub struct LuaContentRenderer {
    slot_name: String,
}

impl LuaContentRenderer {
    pub fn new(slot_name: String) -> Self {
        Self { slot_name }
    }
}

impl ContentRenderer for LuaContentRenderer {
    fn slot(&self) -> ContentSlot {
        ContentSlot::LuaSlot(self.slot_name.clone())
    }

    fn render_content(&self, state: &TuiState, area: Rect, buf: &mut Buffer) {
        let text = state
            .lua_content
            .get(&self.slot_name)
            .map(|s| s.as_str())
            .unwrap_or("");

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", self.slot_name));

        Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}

// Use the ratatui Widget trait for rendering
use ratatui::widgets::Widget;
impl LuaContentRenderer {
    /// Render as a standalone widget (for testing).
    #[cfg(test)]
    fn render_standalone(&self, text: &str, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", self.slot_name));

        Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;

    #[test]
    fn test_lua_content_renderer_slot() {
        let renderer = LuaContentRenderer::new("my_panel".to_string());
        assert_eq!(
            renderer.slot(),
            ContentSlot::LuaSlot("my_panel".to_string())
        );
    }

    #[test]
    fn test_lua_content_renderer_renders_text() {
        let renderer = LuaContentRenderer::new("test".to_string());
        let area = Rect::new(0, 0, 20, 5);
        let mut buf = Buffer::empty(area);
        renderer.render_standalone("Hello", area, &mut buf);

        // The buffer should contain the text somewhere
        let content = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(content.contains("Hello"));
        assert!(content.contains("test"));
    }
}
