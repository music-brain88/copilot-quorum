//! Content primitives — the "what" layer of the TUI architecture.
//!
//! Each `ContentSlot` variant identifies a logical content type. Concrete
//! structs (e.g. `ConversationContent`) own the data that widgets render.

use super::state::{DisplayMessage, ProgressState};

/// Logical content slot — identifies what kind of content occupies a surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentSlot {
    Conversation,
    Progress,
    Notification,
    HilPrompt,
    Help,
    /// Tool execution log — separable from Progress for independent routing.
    ToolLog,
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
}
