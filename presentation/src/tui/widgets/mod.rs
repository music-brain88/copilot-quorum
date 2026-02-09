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
    pub fn compute(area: Rect) -> Self {
        // Vertical split: header | main | input | status_bar
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // Header
                Constraint::Min(8),    // Main (conversation + progress)
                Constraint::Length(3), // Input
                Constraint::Length(1), // Status bar
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
