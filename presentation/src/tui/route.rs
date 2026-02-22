//! Route primitives — the "mapping" layer connecting content to surfaces.
//!
//! A `RouteTable` declares which `ContentSlot` renders into which `SurfaceId`.
//! Phase 1 uses a fixed default mapping matching the current TUI layout.

use super::content::ContentSlot;
use super::surface::SurfaceId;

/// A single route entry mapping content to a surface.
#[derive(Debug, Clone, Copy)]
pub struct RouteEntry {
    pub content: ContentSlot,
    pub surface: SurfaceId,
}

/// Route table — maps content slots to surface ids.
pub struct RouteTable {
    entries: Vec<RouteEntry>,
}

impl RouteTable {
    /// Default layout matching the current TUI behaviour:
    /// - Conversation → MainPane
    /// - Progress → Sidebar
    /// - HilPrompt → Overlay
    /// - Help → Overlay
    /// - Notification → StatusBar
    pub fn default_layout() -> Self {
        Self {
            entries: vec![
                RouteEntry {
                    content: ContentSlot::Conversation,
                    surface: SurfaceId::MainPane,
                },
                RouteEntry {
                    content: ContentSlot::Progress,
                    surface: SurfaceId::Sidebar,
                },
                RouteEntry {
                    content: ContentSlot::HilPrompt,
                    surface: SurfaceId::Overlay,
                },
                RouteEntry {
                    content: ContentSlot::Help,
                    surface: SurfaceId::Overlay,
                },
                RouteEntry {
                    content: ContentSlot::Notification,
                    surface: SurfaceId::StatusBar,
                },
            ],
        }
    }

    /// Look up which surface a content slot should render into.
    pub fn surface_for(&self, content: ContentSlot) -> Option<SurfaceId> {
        self.entries
            .iter()
            .find(|e| e.content == content)
            .map(|e| e.surface)
    }

    /// Look up which content slots render into a given surface.
    pub fn content_for(&self, surface: SurfaceId) -> Vec<ContentSlot> {
        self.entries
            .iter()
            .filter(|e| e.surface == surface)
            .map(|e| e.content)
            .collect()
    }
}

impl Default for RouteTable {
    fn default() -> Self {
        Self::default_layout()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_conversation_to_main_pane() {
        let route = RouteTable::default();
        assert_eq!(
            route.surface_for(ContentSlot::Conversation),
            Some(SurfaceId::MainPane)
        );
    }

    #[test]
    fn test_default_progress_to_sidebar() {
        let route = RouteTable::default();
        assert_eq!(
            route.surface_for(ContentSlot::Progress),
            Some(SurfaceId::Sidebar)
        );
    }

    #[test]
    fn test_default_hil_to_overlay() {
        let route = RouteTable::default();
        assert_eq!(
            route.surface_for(ContentSlot::HilPrompt),
            Some(SurfaceId::Overlay)
        );
    }

    #[test]
    fn test_default_help_to_overlay() {
        let route = RouteTable::default();
        assert_eq!(
            route.surface_for(ContentSlot::Help),
            Some(SurfaceId::Overlay)
        );
    }

    #[test]
    fn test_default_notification_to_status_bar() {
        let route = RouteTable::default();
        assert_eq!(
            route.surface_for(ContentSlot::Notification),
            Some(SurfaceId::StatusBar)
        );
    }

    #[test]
    fn test_content_for_overlay_returns_two() {
        let route = RouteTable::default();
        let contents = route.content_for(SurfaceId::Overlay);
        assert_eq!(contents.len(), 2);
        assert!(contents.contains(&ContentSlot::HilPrompt));
        assert!(contents.contains(&ContentSlot::Help));
    }

    #[test]
    fn test_content_for_main_pane() {
        let route = RouteTable::default();
        let contents = route.content_for(SurfaceId::MainPane);
        assert_eq!(contents, vec![ContentSlot::Conversation]);
    }

    #[test]
    fn test_surface_for_unknown_returns_none() {
        let route = RouteTable {
            entries: Vec::new(),
        };
        assert_eq!(route.surface_for(ContentSlot::Conversation), None);
    }
}
