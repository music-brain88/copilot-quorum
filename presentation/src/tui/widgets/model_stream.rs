//! Model stream widget — per-model streaming output during Ensemble planning.
//!
//! Each Ensemble participant model gets its own pane showing live streaming
//! text, status indicator, and optional vote score.

use crate::tui::content::{ContentRenderer, ContentSlot};
use crate::tui::state::{ModelStreamStatus, TuiState};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

/// ContentRenderer for a specific model's streaming output.
///
/// Registered dynamically when Ensemble mode starts.
pub struct ModelStreamRenderer {
    model_name: String,
}

impl ModelStreamRenderer {
    pub fn new(model_name: impl Into<String>) -> Self {
        Self {
            model_name: model_name.into(),
        }
    }
}

impl ContentRenderer for ModelStreamRenderer {
    fn slot(&self) -> ContentSlot {
        ContentSlot::ModelStream(self.model_name.clone())
    }

    fn render_content(&self, state: &TuiState, area: Rect, buf: &mut Buffer) {
        ModelStreamWidget::new(state, &self.model_name).render(area, buf);
    }
}

struct ModelStreamWidget<'a> {
    state: &'a TuiState,
    model_name: &'a str,
}

impl<'a> ModelStreamWidget<'a> {
    fn new(state: &'a TuiState, model_name: &'a str) -> Self {
        Self { state, model_name }
    }
}

impl<'a> Widget for ModelStreamWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let pane = self.state.tabs.active_pane();
        let mut lines: Vec<Line> = Vec::new();

        if let Some(ref ep) = pane.progress.ensemble_progress
            && let Some(ms) = ep.model_streams.get(self.model_name)
        {
            // Status indicator
            let (status_icon, status_color) = match &ms.status {
                ModelStreamStatus::Pending => ("◯", Color::DarkGray),
                ModelStreamStatus::Streaming => ("▸", Color::Yellow),
                ModelStreamStatus::Complete => ("✓", Color::Green),
                ModelStreamStatus::Error(_) => ("✗", Color::Red),
            };

            let mut status_spans = vec![
                Span::styled(
                    format!("{} ", status_icon),
                    Style::default().fg(status_color),
                ),
                Span::styled(
                    format!("{:?}", ms.status),
                    Style::default().fg(status_color),
                ),
            ];

            if let Some(score) = ms.score {
                status_spans.push(Span::styled(
                    format!("  Score: {:.1}/10", score),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ));
            }

            lines.push(Line::from(status_spans));
            lines.push(Line::from(""));

            // Streaming text
            if ms.streaming_text.is_empty() {
                lines.push(Line::from(Span::styled(
                    "Waiting for response...",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                for text_line in ms.streaming_text.lines() {
                    lines.push(Line::from(format!("  {}", text_line)));
                }
                if ms.status == ModelStreamStatus::Streaming {
                    lines.push(Line::from(Span::styled(
                        "  ▌",
                        Style::default().fg(Color::Green),
                    )));
                }
            }

            if let ModelStreamStatus::Error(ref err) = ms.status {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!("Error: {}", err),
                    Style::default().fg(Color::Red),
                )));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "No stream data",
                Style::default().fg(Color::DarkGray),
            )));
        }

        // Short model name for title (take last segment after /)
        let short_name = self
            .model_name
            .rsplit('/')
            .next()
            .unwrap_or(self.model_name);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", short_name))
            .style(Style::default().fg(Color::White));

        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}
