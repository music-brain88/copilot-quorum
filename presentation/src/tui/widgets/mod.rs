//! TUI widgets — ratatui components for the main layout
//!
//! Layout:
//! ┌── Header (3) ────────────────────────────────────┐
//! ├── Panes (flex, dynamically split) ───────────────┤
//! ├── Input (3) ─────────────────────────────────────┤
//! └── StatusBar (1) ─────────────────────────────────┘

pub mod conversation;
pub mod header;
pub mod input;
pub mod model_stream;
pub mod progress_panel;
pub mod status_bar;
pub mod tab_bar;
pub mod tool_log;

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use super::layout::LayoutPreset;

/// Compute the main layout regions from a terminal area.
///
/// `panes` is a dynamically-sized Vec of content pane areas, ordered to match
/// the `SurfaceId` list from `RouteTable::required_pane_surfaces()`.
pub struct MainLayout {
    pub header: Rect,
    pub tab_bar: Option<Rect>,
    /// Dynamic content panes — ordered to match the route table's pane surfaces.
    pub panes: Vec<Rect>,
    pub input: Rect,
    pub status_bar: Rect,
}

impl MainLayout {
    /// Compute layout with dynamic input height.
    ///
    /// `input_lines` is the number of text lines in the input buffer.
    /// `max_input_height` is the maximum number of text lines (from config).
    /// `show_tab_bar` adds a 1-row tab bar between header and main area.
    /// The input area grows from 3 (1 line + borders) up to max_input_height + 2 (borders),
    /// but is capped to prevent pushing other widgets out of the terminal.
    ///
    /// Delegates to `compute_with_layout()` with `LayoutPreset::Default`, threshold=0, pane_count=2.
    #[allow(dead_code)]
    pub fn compute_with_input_config(
        area: Rect,
        input_lines: u16,
        max_input_height: u16,
        show_tab_bar: bool,
    ) -> Self {
        Self::compute_with_layout(
            area,
            input_lines,
            max_input_height,
            show_tab_bar,
            LayoutPreset::Default,
            0,
            2,
        )
    }

    /// Compute layout with a preset, flex threshold, and dynamic pane count.
    ///
    /// If `area.width < flex_threshold`, the preset falls back to Minimal.
    /// `pane_count` determines how many content panes to split the main area into.
    pub fn compute_with_layout(
        area: Rect,
        input_lines: u16,
        max_input_height: u16,
        show_tab_bar: bool,
        preset: LayoutPreset,
        flex_threshold: u16,
        pane_count: usize,
    ) -> Self {
        // Responsive fallback: narrow terminal → Minimal
        let effective_preset = if flex_threshold > 0 && area.width < flex_threshold {
            LayoutPreset::Minimal
        } else {
            preset
        };

        let (header, tab_bar, main_area, input, status_bar) =
            Self::compute_outer_regions(area, input_lines, max_input_height, show_tab_bar);

        let pane_count = if effective_preset == LayoutPreset::Minimal {
            1
        } else {
            pane_count.max(1)
        };

        let splits = effective_preset.default_splits(pane_count);
        let direction = effective_preset.split_direction();

        let constraints: Vec<Constraint> = splits
            .iter()
            .map(|&pct| Constraint::Percentage(pct))
            .collect();

        let panes = if constraints.is_empty() {
            vec![main_area]
        } else {
            Layout::default()
                .direction(direction)
                .constraints(constraints)
                .split(main_area)
                .to_vec()
        };

        Self {
            header,
            tab_bar,
            panes,
            input,
            status_bar,
        }
    }

    /// Shared outer region computation: header, optional tab_bar, main fill, input, status bar.
    fn compute_outer_regions(
        area: Rect,
        input_lines: u16,
        max_input_height: u16,
        show_tab_bar: bool,
    ) -> (Rect, Option<Rect>, Rect, Rect, Rect) {
        let header_h: u16 = 3;
        let tab_bar_h: u16 = if show_tab_bar { 1 } else { 0 };
        let status_h: u16 = 1;

        let max_for_input = area.height.saturating_sub(header_h + tab_bar_h + status_h);
        let desired_h = (input_lines + 2).clamp(3, max_input_height + 2);
        let input_h = desired_h.min(max_for_input).max(1);

        if show_tab_bar {
            let vertical = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(header_h),
                    Constraint::Length(tab_bar_h),
                    Constraint::Fill(1),
                    Constraint::Length(input_h),
                    Constraint::Length(status_h),
                ])
                .split(area);

            (
                vertical[0],
                Some(vertical[1]),
                vertical[2],
                vertical[3],
                vertical[4],
            )
        } else {
            let vertical = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(header_h),
                    Constraint::Fill(1),
                    Constraint::Length(input_h),
                    Constraint::Length(status_h),
                ])
                .split(area);

            (vertical[0], None, vertical[1], vertical[2], vertical[3])
        }
    }

    /// Centered overlay rectangle for help dialog
    pub fn centered_overlay(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
        let vert = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(area);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(vert[1])[1]
    }
}
