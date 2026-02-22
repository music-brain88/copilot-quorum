//! Surface primitives — the "where" layer of the TUI architecture.
//!
//! A `SurfaceId` names a physical screen region. `ResolvedSurface` pairs an
//! id with its concrete `Rect` after layout computation. `SurfaceLayout`
//! collects all resolved surfaces for the current frame.

use super::widgets::MainLayout;
use ratatui::layout::Rect;

/// Named surface — a physical region of the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceId {
    MainPane,
    Sidebar,
    Overlay,
    Header,
    Input,
    StatusBar,
    TabBar,
    /// Third pane for Wide layout (tool log display).
    ToolPane,
    /// Floating overlay for tool display.
    ToolFloat,
}

/// A surface with its resolved screen area.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedSurface {
    pub id: SurfaceId,
    pub area: Rect,
}

/// All resolved surfaces for the current frame.
pub struct SurfaceLayout {
    surfaces: Vec<ResolvedSurface>,
}

impl SurfaceLayout {
    /// Look up the area for a given surface id.
    pub fn area_for(&self, id: SurfaceId) -> Option<Rect> {
        self.surfaces.iter().find(|s| s.id == id).map(|s| s.area)
    }

    /// Build from the existing `MainLayout`.
    ///
    /// Zero-size Rects (width=0 or height=0) are filtered out so that
    /// `area_for()` returns `None` for surfaces not present in the current preset.
    pub fn from_main_layout(layout: &MainLayout) -> Self {
        let mut candidates = vec![
            ResolvedSurface {
                id: SurfaceId::Header,
                area: layout.header,
            },
            ResolvedSurface {
                id: SurfaceId::MainPane,
                area: layout.conversation,
            },
            ResolvedSurface {
                id: SurfaceId::Sidebar,
                area: layout.progress,
            },
            ResolvedSurface {
                id: SurfaceId::Input,
                area: layout.input,
            },
            ResolvedSurface {
                id: SurfaceId::StatusBar,
                area: layout.status_bar,
            },
        ];
        if let Some(tab_bar) = layout.tab_bar {
            candidates.push(ResolvedSurface {
                id: SurfaceId::TabBar,
                area: tab_bar,
            });
        }
        if let Some(tool_pane) = layout.tool_pane {
            candidates.push(ResolvedSurface {
                id: SurfaceId::ToolPane,
                area: tool_pane,
            });
        }

        // Filter out zero-size surfaces (e.g. Minimal preset sets progress to ZERO)
        let surfaces = candidates
            .into_iter()
            .filter(|s| s.area.width > 0 && s.area.height > 0)
            .collect();

        Self { surfaces }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_main_layout_round_trip() {
        let layout = MainLayout {
            header: Rect::new(0, 0, 80, 3),
            tab_bar: None,
            conversation: Rect::new(0, 3, 56, 17),
            progress: Rect::new(56, 3, 24, 17),
            input: Rect::new(0, 20, 80, 3),
            status_bar: Rect::new(0, 23, 80, 1),
            tool_pane: None,
        };

        let surface = SurfaceLayout::from_main_layout(&layout);

        assert_eq!(
            surface.area_for(SurfaceId::Header),
            Some(Rect::new(0, 0, 80, 3))
        );
        assert_eq!(
            surface.area_for(SurfaceId::MainPane),
            Some(Rect::new(0, 3, 56, 17))
        );
        assert_eq!(
            surface.area_for(SurfaceId::Sidebar),
            Some(Rect::new(56, 3, 24, 17))
        );
        assert_eq!(
            surface.area_for(SurfaceId::Input),
            Some(Rect::new(0, 20, 80, 3))
        );
        assert_eq!(
            surface.area_for(SurfaceId::StatusBar),
            Some(Rect::new(0, 23, 80, 1))
        );
        assert_eq!(surface.area_for(SurfaceId::TabBar), None);
        assert_eq!(surface.area_for(SurfaceId::Overlay), None);
    }

    #[test]
    fn test_from_main_layout_with_tab_bar() {
        let layout = MainLayout {
            header: Rect::new(0, 0, 80, 3),
            tab_bar: Some(Rect::new(0, 3, 80, 1)),
            conversation: Rect::new(0, 4, 56, 16),
            progress: Rect::new(56, 4, 24, 16),
            input: Rect::new(0, 20, 80, 3),
            status_bar: Rect::new(0, 23, 80, 1),
            tool_pane: None,
        };

        let surface = SurfaceLayout::from_main_layout(&layout);

        assert_eq!(
            surface.area_for(SurfaceId::TabBar),
            Some(Rect::new(0, 3, 80, 1))
        );
    }

    #[test]
    fn test_zero_size_rect_filtered_out() {
        let layout = MainLayout {
            header: Rect::new(0, 0, 80, 3),
            tab_bar: None,
            conversation: Rect::new(0, 3, 80, 17),
            progress: Rect::ZERO, // Minimal preset sets this to ZERO
            input: Rect::new(0, 20, 80, 3),
            status_bar: Rect::new(0, 23, 80, 1),
            tool_pane: None,
        };

        let surface = SurfaceLayout::from_main_layout(&layout);
        assert_eq!(surface.area_for(SurfaceId::Sidebar), None);
        assert!(surface.area_for(SurfaceId::MainPane).is_some());
    }

    #[test]
    fn test_tool_pane_surface() {
        let layout = MainLayout {
            header: Rect::new(0, 0, 120, 3),
            tab_bar: None,
            conversation: Rect::new(0, 3, 72, 17),
            progress: Rect::new(72, 3, 24, 17),
            input: Rect::new(0, 20, 120, 3),
            status_bar: Rect::new(0, 23, 120, 1),
            tool_pane: Some(Rect::new(96, 3, 24, 17)),
        };

        let surface = SurfaceLayout::from_main_layout(&layout);
        assert_eq!(
            surface.area_for(SurfaceId::ToolPane),
            Some(Rect::new(96, 3, 24, 17))
        );
    }
}
