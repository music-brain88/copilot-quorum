//! Surface primitives — the "where" layer of the TUI architecture.
//!
//! A `SurfaceId` names a physical screen region. `ResolvedSurface` pairs an
//! id with its concrete `Rect` after layout computation. `SurfaceLayout`
//! collects all resolved surfaces for the current frame.

use super::widgets::MainLayout;
use ratatui::layout::Rect;

/// Named surface — a physical region of the terminal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SurfaceId {
    // Fixed chrome
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
    /// Dynamically added pane (e.g. Ensemble model streams).
    DynamicPane(String),
}

impl SurfaceId {
    /// Whether this surface is a content pane (participates in dynamic layout).
    pub fn is_content_pane(&self) -> bool {
        matches!(
            self,
            Self::MainPane | Self::Sidebar | Self::ToolPane | Self::DynamicPane(_)
        )
    }

    /// Whether this surface is an overlay (rendered on top of content).
    pub fn is_overlay(&self) -> bool {
        matches!(self, Self::Overlay | Self::ToolFloat)
    }
}

/// A surface with its resolved screen area.
#[derive(Debug, Clone)]
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
    pub fn area_for(&self, id: &SurfaceId) -> Option<Rect> {
        self.surfaces.iter().find(|s| &s.id == id).map(|s| s.area)
    }

    /// Build from the existing `MainLayout` with dynamic pane mapping.
    ///
    /// `pane_surfaces` provides the SurfaceId for each pane in order.
    /// `layout.panes[i]` is mapped to `pane_surfaces[i]`.
    ///
    /// Zero-size Rects (width=0 or height=0) are filtered out so that
    /// `area_for()` returns `None` for surfaces not present in the current preset.
    pub fn from_main_layout(layout: &MainLayout, pane_surfaces: &[SurfaceId]) -> Self {
        let mut candidates = vec![
            ResolvedSurface {
                id: SurfaceId::Header,
                area: layout.header,
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

        // Map panes[i] → pane_surfaces[i]
        for (i, pane_area) in layout.panes.iter().enumerate() {
            if let Some(surface_id) = pane_surfaces.get(i) {
                candidates.push(ResolvedSurface {
                    id: surface_id.clone(),
                    area: *pane_area,
                });
            }
        }

        // Filter out zero-size surfaces (e.g. Minimal preset sets progress to ZERO)
        let surfaces = candidates
            .into_iter()
            .filter(|s| s.area.width > 0 && s.area.height > 0)
            .collect();

        Self { surfaces }
    }

    /// Add an overlay surface (for help dialog, HiL modal, etc.).
    #[allow(dead_code)]
    pub fn add_overlay(&mut self, id: SurfaceId, area: Rect) {
        self.surfaces.push(ResolvedSurface { id, area });
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
            panes: vec![
                Rect::new(0, 3, 56, 17),  // MainPane
                Rect::new(56, 3, 24, 17), // Sidebar
            ],
            input: Rect::new(0, 20, 80, 3),
            status_bar: Rect::new(0, 23, 80, 1),
        };

        let pane_surfaces = vec![SurfaceId::MainPane, SurfaceId::Sidebar];
        let surface = SurfaceLayout::from_main_layout(&layout, &pane_surfaces);

        assert_eq!(
            surface.area_for(&SurfaceId::Header),
            Some(Rect::new(0, 0, 80, 3))
        );
        assert_eq!(
            surface.area_for(&SurfaceId::MainPane),
            Some(Rect::new(0, 3, 56, 17))
        );
        assert_eq!(
            surface.area_for(&SurfaceId::Sidebar),
            Some(Rect::new(56, 3, 24, 17))
        );
        assert_eq!(
            surface.area_for(&SurfaceId::Input),
            Some(Rect::new(0, 20, 80, 3))
        );
        assert_eq!(
            surface.area_for(&SurfaceId::StatusBar),
            Some(Rect::new(0, 23, 80, 1))
        );
        assert_eq!(surface.area_for(&SurfaceId::TabBar), None);
        assert_eq!(surface.area_for(&SurfaceId::Overlay), None);
    }

    #[test]
    fn test_from_main_layout_with_tab_bar() {
        let layout = MainLayout {
            header: Rect::new(0, 0, 80, 3),
            tab_bar: Some(Rect::new(0, 3, 80, 1)),
            panes: vec![Rect::new(0, 4, 56, 16), Rect::new(56, 4, 24, 16)],
            input: Rect::new(0, 20, 80, 3),
            status_bar: Rect::new(0, 23, 80, 1),
        };

        let pane_surfaces = vec![SurfaceId::MainPane, SurfaceId::Sidebar];
        let surface = SurfaceLayout::from_main_layout(&layout, &pane_surfaces);

        assert_eq!(
            surface.area_for(&SurfaceId::TabBar),
            Some(Rect::new(0, 3, 80, 1))
        );
    }

    #[test]
    fn test_zero_size_rect_filtered_out() {
        let layout = MainLayout {
            header: Rect::new(0, 0, 80, 3),
            tab_bar: None,
            panes: vec![
                Rect::new(0, 3, 80, 17),
                Rect::ZERO, // Minimal preset sets this to ZERO
            ],
            input: Rect::new(0, 20, 80, 3),
            status_bar: Rect::new(0, 23, 80, 1),
        };

        let pane_surfaces = vec![SurfaceId::MainPane, SurfaceId::Sidebar];
        let surface = SurfaceLayout::from_main_layout(&layout, &pane_surfaces);
        assert_eq!(surface.area_for(&SurfaceId::Sidebar), None);
        assert!(surface.area_for(&SurfaceId::MainPane).is_some());
    }

    #[test]
    fn test_tool_pane_surface() {
        let layout = MainLayout {
            header: Rect::new(0, 0, 120, 3),
            tab_bar: None,
            panes: vec![
                Rect::new(0, 3, 72, 17),
                Rect::new(72, 3, 24, 17),
                Rect::new(96, 3, 24, 17),
            ],
            input: Rect::new(0, 20, 120, 3),
            status_bar: Rect::new(0, 23, 120, 1),
        };

        let pane_surfaces = vec![SurfaceId::MainPane, SurfaceId::Sidebar, SurfaceId::ToolPane];
        let surface = SurfaceLayout::from_main_layout(&layout, &pane_surfaces);
        assert_eq!(
            surface.area_for(&SurfaceId::ToolPane),
            Some(Rect::new(96, 3, 24, 17))
        );
    }

    #[test]
    fn test_dynamic_pane_surface() {
        let layout = MainLayout {
            header: Rect::new(0, 0, 120, 3),
            tab_bar: None,
            panes: vec![
                Rect::new(0, 3, 60, 17),
                Rect::new(60, 3, 30, 17),
                Rect::new(90, 3, 30, 17),
            ],
            input: Rect::new(0, 20, 120, 3),
            status_bar: Rect::new(0, 23, 120, 1),
        };

        let pane_surfaces = vec![
            SurfaceId::MainPane,
            SurfaceId::DynamicPane("claude".into()),
            SurfaceId::DynamicPane("gpt".into()),
        ];
        let surface = SurfaceLayout::from_main_layout(&layout, &pane_surfaces);
        assert_eq!(
            surface.area_for(&SurfaceId::DynamicPane("claude".into())),
            Some(Rect::new(60, 3, 30, 17))
        );
        assert_eq!(
            surface.area_for(&SurfaceId::DynamicPane("gpt".into())),
            Some(Rect::new(90, 3, 30, 17))
        );
    }

    #[test]
    fn test_surface_id_classification() {
        assert!(SurfaceId::MainPane.is_content_pane());
        assert!(SurfaceId::Sidebar.is_content_pane());
        assert!(SurfaceId::ToolPane.is_content_pane());
        assert!(SurfaceId::DynamicPane("test".into()).is_content_pane());
        assert!(!SurfaceId::Header.is_content_pane());
        assert!(!SurfaceId::Overlay.is_content_pane());

        assert!(SurfaceId::Overlay.is_overlay());
        assert!(SurfaceId::ToolFloat.is_overlay());
        assert!(!SurfaceId::MainPane.is_overlay());
    }

    #[test]
    fn test_add_overlay() {
        let layout = MainLayout {
            header: Rect::new(0, 0, 80, 3),
            tab_bar: None,
            panes: vec![Rect::new(0, 3, 80, 17)],
            input: Rect::new(0, 20, 80, 3),
            status_bar: Rect::new(0, 23, 80, 1),
        };

        let pane_surfaces = vec![SurfaceId::MainPane];
        let mut surface = SurfaceLayout::from_main_layout(&layout, &pane_surfaces);
        assert_eq!(surface.area_for(&SurfaceId::Overlay), None);

        surface.add_overlay(SurfaceId::Overlay, Rect::new(10, 5, 60, 10));
        assert_eq!(
            surface.area_for(&SurfaceId::Overlay),
            Some(Rect::new(10, 5, 60, 10))
        );
    }
}
