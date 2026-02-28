//! Route primitives — the "mapping" layer connecting content to surfaces.
//!
//! A `RouteTable` declares which `ContentSlot` renders into which `SurfaceId`.
//! Phase 1 uses a fixed default mapping matching the current TUI layout.

use std::collections::HashSet;

use super::content::ContentSlot;
use super::layout::{LayoutPreset, RouteOverride};
use super::surface::SurfaceId;

/// A single route entry mapping content to a surface.
#[derive(Debug, Clone)]
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

    /// Minimal layout: no sidebar, conversation only.
    pub fn minimal_layout() -> Self {
        Self {
            entries: vec![
                RouteEntry {
                    content: ContentSlot::Conversation,
                    surface: SurfaceId::MainPane,
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

    /// Wide layout: 3-pane with tool log in ToolPane.
    pub fn wide_layout() -> Self {
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
                    content: ContentSlot::ToolLog,
                    surface: SurfaceId::ToolPane,
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

    /// Stacked layout: vertical split (conversation top, progress bottom).
    pub fn stacked_layout() -> Self {
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

    /// Build a route table from a preset with optional user overrides applied.
    pub fn from_preset_and_overrides(preset: LayoutPreset, overrides: &[RouteOverride]) -> Self {
        let mut table = match preset {
            LayoutPreset::Default => Self::default_layout(),
            LayoutPreset::Minimal => Self::minimal_layout(),
            LayoutPreset::Wide => Self::wide_layout(),
            LayoutPreset::Stacked => Self::stacked_layout(),
            // Custom presets start with default routes
            LayoutPreset::Custom(_) => Self::default_layout(),
        };
        for ov in overrides {
            table.set_route(ov.content.clone(), ov.surface.clone());
        }
        table
    }

    /// Set or replace a route entry for the given content slot.
    pub fn set_route(&mut self, content: ContentSlot, surface: SurfaceId) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.content == content) {
            entry.surface = surface;
        } else {
            self.entries.push(RouteEntry { content, surface });
        }
    }

    /// Look up which surface a content slot should render into.
    pub fn surface_for(&self, content: &ContentSlot) -> Option<SurfaceId> {
        self.entries
            .iter()
            .find(|e| &e.content == content)
            .map(|e| e.surface.clone())
    }

    /// Look up which content slots render into a given surface.
    pub fn content_for(&self, surface: &SurfaceId) -> Vec<ContentSlot> {
        self.entries
            .iter()
            .filter(|e| &e.surface == surface)
            .map(|e| e.content.clone())
            .collect()
    }

    /// Get all route entries (immutable).
    pub fn entries(&self) -> &[RouteEntry] {
        &self.entries
    }

    /// Collect the unique content-pane SurfaceIds referenced by this route table,
    /// preserving first-seen order.
    ///
    /// Used by the render loop to determine how many panes to create and which
    /// SurfaceId each pane maps to.
    pub fn required_pane_surfaces(&self) -> Vec<SurfaceId> {
        let mut seen = HashSet::new();
        self.entries
            .iter()
            .filter(|e| e.surface.is_content_pane())
            .filter(|e| seen.insert(e.surface.clone()))
            .map(|e| e.surface.clone())
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
            route.surface_for(&ContentSlot::Conversation),
            Some(SurfaceId::MainPane)
        );
    }

    #[test]
    fn test_default_progress_to_sidebar() {
        let route = RouteTable::default();
        assert_eq!(
            route.surface_for(&ContentSlot::Progress),
            Some(SurfaceId::Sidebar)
        );
    }

    #[test]
    fn test_default_hil_to_overlay() {
        let route = RouteTable::default();
        assert_eq!(
            route.surface_for(&ContentSlot::HilPrompt),
            Some(SurfaceId::Overlay)
        );
    }

    #[test]
    fn test_default_help_to_overlay() {
        let route = RouteTable::default();
        assert_eq!(
            route.surface_for(&ContentSlot::Help),
            Some(SurfaceId::Overlay)
        );
    }

    #[test]
    fn test_default_notification_to_status_bar() {
        let route = RouteTable::default();
        assert_eq!(
            route.surface_for(&ContentSlot::Notification),
            Some(SurfaceId::StatusBar)
        );
    }

    #[test]
    fn test_content_for_overlay_returns_two() {
        let route = RouteTable::default();
        let contents = route.content_for(&SurfaceId::Overlay);
        assert_eq!(contents.len(), 2);
        assert!(contents.contains(&ContentSlot::HilPrompt));
        assert!(contents.contains(&ContentSlot::Help));
    }

    #[test]
    fn test_content_for_main_pane() {
        let route = RouteTable::default();
        let contents = route.content_for(&SurfaceId::MainPane);
        assert_eq!(contents, vec![ContentSlot::Conversation]);
    }

    #[test]
    fn test_surface_for_unknown_returns_none() {
        let route = RouteTable {
            entries: Vec::new(),
        };
        assert_eq!(route.surface_for(&ContentSlot::Conversation), None);
    }

    #[test]
    fn test_minimal_layout_no_progress() {
        let route = RouteTable::minimal_layout();
        assert_eq!(route.surface_for(&ContentSlot::Progress), None);
        assert_eq!(
            route.surface_for(&ContentSlot::Conversation),
            Some(SurfaceId::MainPane)
        );
    }

    #[test]
    fn test_wide_layout_has_tool_log() {
        let route = RouteTable::wide_layout();
        assert_eq!(
            route.surface_for(&ContentSlot::ToolLog),
            Some(SurfaceId::ToolPane)
        );
        assert_eq!(
            route.surface_for(&ContentSlot::Progress),
            Some(SurfaceId::Sidebar)
        );
    }

    #[test]
    fn test_stacked_layout() {
        let route = RouteTable::stacked_layout();
        assert_eq!(
            route.surface_for(&ContentSlot::Progress),
            Some(SurfaceId::Sidebar)
        );
    }

    #[test]
    fn test_set_route_overrides_existing() {
        let mut route = RouteTable::default_layout();
        route.set_route(ContentSlot::Progress, SurfaceId::ToolFloat);
        assert_eq!(
            route.surface_for(&ContentSlot::Progress),
            Some(SurfaceId::ToolFloat)
        );
    }

    #[test]
    fn test_set_route_adds_new() {
        let mut route = RouteTable::default_layout();
        route.set_route(ContentSlot::ToolLog, SurfaceId::Sidebar);
        assert_eq!(
            route.surface_for(&ContentSlot::ToolLog),
            Some(SurfaceId::Sidebar)
        );
    }

    #[test]
    fn test_from_preset_and_overrides() {
        use super::super::layout::RouteOverride;

        let overrides = vec![RouteOverride {
            content: ContentSlot::Progress,
            surface: SurfaceId::ToolFloat,
        }];
        let route = RouteTable::from_preset_and_overrides(LayoutPreset::Default, &overrides);
        assert_eq!(
            route.surface_for(&ContentSlot::Progress),
            Some(SurfaceId::ToolFloat)
        );
        // Other entries unchanged
        assert_eq!(
            route.surface_for(&ContentSlot::Conversation),
            Some(SurfaceId::MainPane)
        );
    }

    #[test]
    fn test_required_pane_surfaces_default() {
        let route = RouteTable::default_layout();
        let panes = route.required_pane_surfaces();
        assert_eq!(panes, vec![SurfaceId::MainPane, SurfaceId::Sidebar]);
    }

    #[test]
    fn test_required_pane_surfaces_minimal() {
        let route = RouteTable::minimal_layout();
        let panes = route.required_pane_surfaces();
        assert_eq!(panes, vec![SurfaceId::MainPane]);
    }

    #[test]
    fn test_required_pane_surfaces_wide() {
        let route = RouteTable::wide_layout();
        let panes = route.required_pane_surfaces();
        assert_eq!(
            panes,
            vec![SurfaceId::MainPane, SurfaceId::Sidebar, SurfaceId::ToolPane]
        );
    }

    #[test]
    fn test_entries_returns_all() {
        let route = RouteTable::default_layout();
        assert_eq!(route.entries().len(), 5);
    }

    #[test]
    fn test_model_stream_route() {
        let mut route = RouteTable::default_layout();
        route.set_route(
            ContentSlot::ModelStream("claude".into()),
            SurfaceId::DynamicPane("claude".into()),
        );
        assert_eq!(
            route.surface_for(&ContentSlot::ModelStream("claude".into())),
            Some(SurfaceId::DynamicPane("claude".into()))
        );
    }
}
