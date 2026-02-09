//! TUI Widgets - Ratatui components for rendering Agent REPL
//!
//! This module contains all the widget components for the TUI:
//! - Conversation history rendering
//! - Progress/status indicators
//! - Input box with mode display
//! - Help panel

mod conversation;
mod help;
mod input;
mod progress;
mod status;

pub use conversation::ConversationWidget;
pub use help::HelpWidget;
pub use input::InputWidget;
pub use progress::ProgressWidget;
pub use status::StatusWidget;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    Frame,
};

use super::state::TuiState;

/// Main layout builder for the TUI
pub struct TuiLayout;

impl TuiLayout {
    /// Create the main layout split (conversation | status, progress, input)
    pub fn build(area: Rect) -> (Rect, Rect, Rect, Rect) {
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(10),      // Conversation area (flexible)
                Constraint::Length(3),    // Status bar
                Constraint::Length(5),    // Progress area
                Constraint::Length(3),    // Input box
            ])
            .split(area);

        (main_chunks[0], main_chunks[1], main_chunks[2], main_chunks[3])
    }

    /// Render all widgets for the current state
    pub fn render(frame: &mut Frame, state: &TuiState) {
        let area = frame.area();
        let (conv_area, status_area, progress_area, input_area) = Self::build(area);

        // Render conversation history
        let conversation = ConversationWidget::new(&state.messages);
        frame.render_widget(conversation, conv_area);

        // Render status bar
        let status = StatusWidget::new(state);
        frame.render_widget(status, status_area);

        // Render progress
        let progress = ProgressWidget::new(&state.progress);
        frame.render_widget(progress, progress_area);

        // Render input box
        let input = InputWidget::new(&state.input, state.mode());
        frame.render_widget(input, input_area);

        // If help is visible, render help overlay
        if state.show_help {
            let help = HelpWidget::new();
            let help_area = Self::centered_rect(80, 80, area);
            frame.render_widget(ratatui::widgets::Clear, help_area);
            frame.render_widget(help, help_area);
        }
    }

    /// Create a centered rectangle for overlays (e.g., help dialog)
    fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(r);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(popup_layout[1])[1]
    }
}
