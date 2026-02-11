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

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Compute the main layout regions from a terminal area
pub struct MainLayout {
    pub header: Rect,
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
    /// The input area grows from 3 (1 line + borders) up to max_input_height + 2 (borders).
    pub fn compute_with_input_config(
        area: Rect,
        input_lines: u16,
        max_input_height: u16,
    ) -> Self {
        // height = lines + 2 (top/bottom border), clamped to max + 2
        let input_h = (input_lines + 2).clamp(3, max_input_height + 2);

        // Vertical split: header | main | input | status_bar
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),        // Header
                Constraint::Min(8),           // Main (conversation + progress)
                Constraint::Length(input_h),  // Input (dynamic)
                Constraint::Length(1),        // Status bar
            ])
            .split(area);

        // Horizontal split of main area: conversation (70%) | progress (30%)
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(vertical[1]);

        Self {
            header: vertical[0],
            conversation: horizontal[0],
            progress: horizontal[1],
            input: vertical[2],
            status_bar: vertical[3],
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
