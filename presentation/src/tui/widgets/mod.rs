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

/// Compute the main layout regions from a terminal area
pub struct MainLayout {
    pub header: Rect,
    pub tab_bar: Option<Rect>,
    pub conversation: Rect,
    pub progress: Rect,
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
    pub fn compute_with_input_config(
        area: Rect,
        input_lines: u16,
        max_input_height: u16,
        show_tab_bar: bool,
    ) -> Self {
        let header_h: u16 = 3;
        let tab_bar_h: u16 = if show_tab_bar { 1 } else { 0 };
        let status_h: u16 = 1;

        // Cap input height so header + tab_bar + input + status never exceeds terminal height.
        let max_for_input = area.height.saturating_sub(header_h + tab_bar_h + status_h);
        let desired_h = (input_lines + 2).clamp(3, max_input_height + 2);
        let input_h = desired_h.min(max_for_input).max(1);

        if show_tab_bar {
            // Vertical split: header | tab_bar | main | input | status_bar
            let vertical = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(header_h),  // Header
                    Constraint::Length(tab_bar_h), // Tab bar
                    Constraint::Fill(1),           // Main (conversation + progress)
                    Constraint::Length(input_h),   // Input (dynamic)
                    Constraint::Length(status_h),  // Status bar
                ])
                .split(area);

            let horizontal = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                .split(vertical[2]);

            Self {
                header: vertical[0],
                tab_bar: Some(vertical[1]),
                conversation: horizontal[0],
                progress: horizontal[1],
                input: vertical[3],
                status_bar: vertical[4],
            }
        } else {
            // Vertical split: header | main | input | status_bar (no tab bar)
            let vertical = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(header_h), // Header
                    Constraint::Fill(1),          // Main (conversation + progress)
                    Constraint::Length(input_h),  // Input (dynamic)
                    Constraint::Length(status_h), // Status bar
                ])
                .split(area);

            let horizontal = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                .split(vertical[1]);

            Self {
                header: vertical[0],
                tab_bar: None,
                conversation: horizontal[0],
                progress: horizontal[1],
                input: vertical[2],
                status_bar: vertical[3],
            }
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
