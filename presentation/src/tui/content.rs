//! Content primitives — the "what" layer of the TUI architecture.
//!
//! Each `ContentSlot` variant identifies a logical content type. Concrete
//! structs (e.g. `ConversationContent`) own the data that widgets render.
//!
//! `ContentRenderer` trait and `ContentRegistry` provide registry-driven
//! dispatch (same pattern as ToolSpec / ToolProvider).

use std::collections::HashMap;

use ratatui::{buffer::Buffer, layout::Rect};

use super::state::{DisplayMessage, ProgressState, TuiState};

/// Logical content slot — identifies what kind of content occupies a surface.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ContentSlot {
    Conversation,
    Progress,
    Notification,
    HilPrompt,
    Help,
    /// Tool execution log — separable from Progress for independent routing.
    ToolLog,
    /// Dynamic: Ensemble model output stream (keyed by model name).
    ModelStream(String),
}

/// Content rendering capability (analogous to ToolProvider).
///
/// Each renderer knows which `ContentSlot` it handles and how to paint
/// into a given area. Renderers are registered in `ContentRegistry` and
/// dispatched by the render loop via route table lookup.
pub trait ContentRenderer {
    /// Which content slot this renderer handles.
    fn slot(&self) -> ContentSlot;

    /// Render content into the given area.
    fn render_content(&self, state: &TuiState, area: Rect, buf: &mut Buffer);
}

/// ContentSlot → Renderer registry (analogous to ToolSpec).
///
/// Holds a mapping from each `ContentSlot` to its renderer implementation.
/// Built via the builder pattern: `ContentRegistry::new().register(...)`.
pub struct ContentRegistry {
    renderers: HashMap<ContentSlot, Box<dyn ContentRenderer>>,
}

impl ContentRegistry {
    pub fn new() -> Self {
        Self {
            renderers: HashMap::new(),
        }
    }

    /// Register a renderer (builder pattern, consumes self).
    pub fn register(mut self, renderer: Box<dyn ContentRenderer>) -> Self {
        let slot = renderer.slot();
        self.renderers.insert(slot, renderer);
        self
    }

    /// Register a renderer mutably (for dynamic registration at runtime).
    pub fn register_mut(&mut self, renderer: Box<dyn ContentRenderer>) {
        let slot = renderer.slot();
        self.renderers.insert(slot, renderer);
    }

    /// Look up the renderer for a given slot.
    pub fn get(&self, slot: &ContentSlot) -> Option<&dyn ContentRenderer> {
        self.renderers.get(slot).map(|r| r.as_ref())
    }
}

impl Default for ContentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Conversation content — messages, streaming text, and scroll state.
#[derive(Debug, Clone)]
pub struct ConversationContent {
    pub messages: Vec<DisplayMessage>,
    pub streaming_text: String,
    pub scroll_offset: usize,
    pub auto_scroll: bool,
}

impl Default for ConversationContent {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            streaming_text: String::new(),
            scroll_offset: 0,
            auto_scroll: true,
        }
    }
}

/// Progress content — type alias for existing `ProgressState`.
pub type ProgressContent = ProgressState;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversation_content_default() {
        let content = ConversationContent::default();
        assert!(content.messages.is_empty());
        assert!(content.streaming_text.is_empty());
        assert_eq!(content.scroll_offset, 0);
        assert!(content.auto_scroll);
    }

    #[test]
    fn test_content_slot_eq() {
        assert_eq!(ContentSlot::Conversation, ContentSlot::Conversation);
        assert_ne!(ContentSlot::Conversation, ContentSlot::Progress);
    }

    #[test]
    fn test_content_slot_model_stream() {
        let slot_a = ContentSlot::ModelStream("claude".to_string());
        let slot_b = ContentSlot::ModelStream("claude".to_string());
        let slot_c = ContentSlot::ModelStream("gpt".to_string());
        assert_eq!(slot_a, slot_b);
        assert_ne!(slot_a, slot_c);
    }

    #[test]
    fn test_content_registry_register_and_get() {
        struct DummyRenderer;
        impl ContentRenderer for DummyRenderer {
            fn slot(&self) -> ContentSlot {
                ContentSlot::Progress
            }
            fn render_content(&self, _state: &TuiState, _area: Rect, _buf: &mut Buffer) {}
        }

        let registry = ContentRegistry::new().register(Box::new(DummyRenderer));
        assert!(registry.get(&ContentSlot::Progress).is_some());
        assert!(registry.get(&ContentSlot::Conversation).is_none());
    }

    #[test]
    fn test_content_registry_register_mut() {
        struct DummyRenderer;
        impl ContentRenderer for DummyRenderer {
            fn slot(&self) -> ContentSlot {
                ContentSlot::Help
            }
            fn render_content(&self, _state: &TuiState, _area: Rect, _buf: &mut Buffer) {}
        }

        let mut registry = ContentRegistry::new();
        registry.register_mut(Box::new(DummyRenderer));
        assert!(registry.get(&ContentSlot::Help).is_some());
    }
}
