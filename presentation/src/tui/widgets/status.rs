//! Status bar widget

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use quorum_domain::{ConsensusLevel, OrchestrationStrategy, PhaseScope};

/// Widget for rendering status bar (mode, scope, strategy)
pub struct StatusWidget {
    mode: ConsensusLevel,
    scope: PhaseScope,
    strategy: OrchestrationStrategy,
}

impl StatusWidget {
    pub fn new(_state: &crate::tui::state::TuiState) -> Self {
        // For now, use defaults - these will be populated by the app state later
        use quorum_domain::orchestration::entities::QuorumConfig;
        Self {
            mode: ConsensusLevel::Solo,
            scope: PhaseScope::Full,
            strategy: OrchestrationStrategy::Quorum(QuorumConfig::default()),
        }
    }

    pub fn with_config(
        mode: ConsensusLevel,
        scope: PhaseScope,
        strategy: OrchestrationStrategy,
    ) -> Self {
        Self {
            mode,
            scope,
            strategy,
        }
    }
}

impl Widget for StatusWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mode_text = format!("Mode: {:?}", self.mode);
        let scope_text = format!("Scope: {:?}", self.scope);
        let strategy_text = match &self.strategy {
            OrchestrationStrategy::Quorum(_) => "Strategy: Quorum".to_string(),
            OrchestrationStrategy::Debate(_) => "Strategy: Debate".to_string(),
        };

        let line = Line::from(vec![
            Span::styled(
                mode_text,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | "),
            Span::styled(scope_text, Style::default().fg(Color::Yellow)),
            Span::raw(" | "),
            Span::styled(strategy_text, Style::default().fg(Color::Magenta)),
        ]);

        let paragraph = Paragraph::new(line).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Status ")
                .style(Style::default().fg(Color::White)),
        );

        paragraph.render(area, buf);
    }
}
