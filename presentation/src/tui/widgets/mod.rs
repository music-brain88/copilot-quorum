//! TUI widgets — ratatui components for the main layout
//!
//! Layout:
//! ┌── Header (3) ────────────────────────────────────┐
//! ├── Conversation (flex) ──┬── Progress (30%) ──────┤
//! ├── Input (3) ────────────┴────────────────────────┤
//! └── StatusBar (1) ─────────────────────────────────┘

pub mod conversation;
pub mod header;
pub mod input;
pub mod progress_panel;
pub mod status_bar;
pub mod tab_bar;

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use super::layout::LayoutPreset;

/// Compute the main layout regions from a terminal area
pub struct MainLayout {
    pub header: Rect,
    pub tab_bar: Option<Rect>,
    pub conversation: Rect,
    pub progress: Rect,
    pub input: Rect,
    pub status_bar: Rect,
    /// Third pane for Wide layout (tool log display). None for other presets.
    pub tool_pane: Option<Rect>,
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
    /// Delegates to `compute_with_layout()` with `LayoutPreset::Default` and threshold=0.
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
        )
    }

    /// Compute layout with a preset and flex threshold.
    ///
    /// If `area.width < flex_threshold`, the preset falls back to Minimal.
    pub fn compute_with_layout(
        area: Rect,
        input_lines: u16,
        max_input_height: u16,
        show_tab_bar: bool,
        preset: LayoutPreset,
        flex_threshold: u16,
    ) -> Self {
        // Responsive fallback: narrow terminal → Minimal
        let effective_preset = if flex_threshold > 0 && area.width < flex_threshold {
            LayoutPreset::Minimal
        } else {
            preset
        };

        match effective_preset {
            LayoutPreset::Default => {
                Self::compute_default(area, input_lines, max_input_height, show_tab_bar)
            }
            LayoutPreset::Minimal => {
                Self::compute_minimal(area, input_lines, max_input_height, show_tab_bar)
            }
            LayoutPreset::Wide => {
                Self::compute_wide(area, input_lines, max_input_height, show_tab_bar)
            }
            LayoutPreset::Stacked => {
                Self::compute_stacked(area, input_lines, max_input_height, show_tab_bar)
            }
        }
    }

    /// Default: 70/30 horizontal split (conversation + sidebar).
    fn compute_default(
        area: Rect,
        input_lines: u16,
        max_input_height: u16,
        show_tab_bar: bool,
    ) -> Self {
        let (header, tab_bar, main_area, input, status_bar) =
            Self::compute_outer_regions(area, input_lines, max_input_height, show_tab_bar);

        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(main_area);

        Self {
            header,
            tab_bar,
            conversation: horizontal[0],
            progress: horizontal[1],
            input,
            status_bar,
            tool_pane: None,
        }
    }

    /// Minimal: full-width conversation, no sidebar.
    fn compute_minimal(
        area: Rect,
        input_lines: u16,
        max_input_height: u16,
        show_tab_bar: bool,
    ) -> Self {
        let (header, tab_bar, main_area, input, status_bar) =
            Self::compute_outer_regions(area, input_lines, max_input_height, show_tab_bar);

        Self {
            header,
            tab_bar,
            conversation: main_area,
            progress: Rect::ZERO,
            input,
            status_bar,
            tool_pane: None,
        }
    }

    /// Wide: 60/20/20 three-pane horizontal split.
    fn compute_wide(
        area: Rect,
        input_lines: u16,
        max_input_height: u16,
        show_tab_bar: bool,
    ) -> Self {
        let (header, tab_bar, main_area, input, status_bar) =
            Self::compute_outer_regions(area, input_lines, max_input_height, show_tab_bar);

        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(60),
                Constraint::Percentage(20),
                Constraint::Percentage(20),
            ])
            .split(main_area);

        Self {
            header,
            tab_bar,
            conversation: horizontal[0],
            progress: horizontal[1],
            input,
            status_bar,
            tool_pane: Some(horizontal[2]),
        }
    }

    /// Stacked: vertical split (conversation 70% top, progress 30% bottom).
    fn compute_stacked(
        area: Rect,
        input_lines: u16,
        max_input_height: u16,
        show_tab_bar: bool,
    ) -> Self {
        let (header, tab_bar, main_area, input, status_bar) =
            Self::compute_outer_regions(area, input_lines, max_input_height, show_tab_bar);

        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(main_area);

        Self {
            header,
            tab_bar,
            conversation: vertical[0],
            progress: vertical[1],
            input,
            status_bar,
            tool_pane: None,
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
