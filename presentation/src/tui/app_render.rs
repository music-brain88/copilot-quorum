//! TUI rendering — all rendering logic extracted from TuiApp.

use super::content::ContentRegistry;
use super::state::TuiState;
use super::surface::SurfaceId;
use super::widgets::{
    MainLayout, header::HeaderWidget, input::InputWidget, status_bar::StatusBarWidget,
    tab_bar::TabBarWidget,
};
use std::cell::RefCell;

/// Build the default content registry with all built-in renderers.
pub(super) fn build_default_registry() -> ContentRegistry {
    use super::widgets::{
        conversation::ConversationRenderer, progress_panel::ProgressRenderer,
        tool_log::ToolLogRenderer,
    };

    ContentRegistry::new()
        .register(Box::new(ConversationRenderer))
        .register(Box::new(ProgressRenderer))
        .register(Box::new(ToolLogRenderer))
}

/// Render all widgets via registry-driven dispatch.
pub(super) fn render(
    frame: &mut ratatui::Frame,
    state: &TuiState,
    content_registry: &RefCell<ContentRegistry>,
) {
    use super::surface::SurfaceLayout;

    let pane_surfaces = state.route.required_pane_surfaces();
    let show_tab_bar = state.tabs.len() > 1;
    let layout = if state.layout_config.preset.is_builtin() {
        MainLayout::compute_with_layout(
            frame.area(),
            state.input_line_count() as u16,
            state.tui_config.max_input_height,
            show_tab_bar,
            state.layout_config.preset.clone(),
            state.layout_config.flex_threshold,
            pane_surfaces.len(),
        )
    } else {
        let splits = state.layout_config.resolve_splits(pane_surfaces.len());
        let direction = state.layout_config.resolve_direction();
        MainLayout::compute_with_splits(
            frame.area(),
            state.input_line_count() as u16,
            state.tui_config.max_input_height,
            show_tab_bar,
            &splits,
            direction,
        )
    };

    // Build surface layout from the computed main layout + pane surfaces
    let surfaces = SurfaceLayout::from_main_layout(&layout, &pane_surfaces);

    // Fixed chrome
    frame.render_widget(HeaderWidget::new(state), layout.header);
    if let Some(tab_bar_area) = surfaces.area_for(&SurfaceId::TabBar) {
        frame.render_widget(TabBarWidget::new(&state.tabs), tab_bar_area);
    }
    frame.render_widget(InputWidget::new(state), layout.input);
    frame.render_widget(StatusBarWidget::new(state), layout.status_bar);

    // Registry-driven content dispatch
    let registry = content_registry.borrow();
    for entry in state.route.entries() {
        if let Some(area) = surfaces.area_for(&entry.surface)
            && let Some(renderer) = registry.get(&entry.content)
        {
            renderer.render_content(state, area, frame.buffer_mut());
        }
    }

    // Dynamic overlays (rendered on top, only when visible)
    if state.show_help {
        let help_area = MainLayout::centered_overlay(70, 70, frame.area());
        frame.render_widget(ratatui::widgets::Clear, help_area);
        render_help(frame, help_area);
    }

    if state.hil_prompt.is_some() {
        let modal_area = MainLayout::centered_overlay(60, 50, frame.area());
        frame.render_widget(ratatui::widgets::Clear, modal_area);
        render_hil_modal(frame, modal_area, state);
    }
}

fn render_help(frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

    let lines = vec![
        Line::from(Span::styled(
            "Keyboard Shortcuts",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("Normal Mode:"),
        Line::from("  i      Enter Insert mode"),
        Line::from("  :      Enter Command mode"),
        Line::from("  s      Switch to Solo mode"),
        Line::from("  e      Switch to Ensemble mode"),
        Line::from("  f      Toggle Fast scope"),
        Line::from("  a      Ask (prefill :ask )"),
        Line::from("  d      Discuss (prefill :discuss )"),
        Line::from("  j/k    Scroll down/up"),
        Line::from("  gg/G   Scroll to top/bottom"),
        Line::from("  gt/gT  Next/prev tab"),
        Line::from("  ?      Toggle this help"),
        Line::from("  Ctrl+C Quit"),
        Line::from(""),
        Line::from("  I      Open $EDITOR (with current input)"),
        Line::from(""),
        Line::from("Insert Mode:"),
        Line::from("  Enter        Send message"),
        Line::from("  Shift+Enter  Insert newline (multiline)"),
        Line::from("  Esc        Return to Normal"),
        Line::from(""),
        Line::from("Commands (:command):"),
        Line::from("  :q       Quit"),
        Line::from("  :help    Show help"),
        Line::from("  :solo    Switch to Solo mode"),
        Line::from("  :ens     Switch to Ensemble mode"),
        Line::from("  :fast    Toggle fast mode"),
        Line::from("  :ask <question>   Ask (lightweight Q&A)"),
        Line::from("  :discuss <question> Discuss (quorum discussion)"),
        Line::from("  :config  Show configuration"),
        Line::from("  :clear   Clear history"),
        Line::from("  :tabnew [form]  New tab (agent/ask/discuss)"),
        Line::from("  :tabclose       Close tab"),
        Line::from("  :tabs           List tabs"),
        Line::from(""),
        Line::from(Span::styled(
            "Press ? or Esc to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Help ")
        .style(Style::default().fg(Color::Cyan));

    frame.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: true }),
        area,
    );
}

fn render_hil_modal(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &TuiState,
) {
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

    let hil = state.hil_prompt.as_ref().unwrap();
    let mut lines = vec![
        Line::from(Span::styled(
            &hil.title,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(format!("Objective: {}", hil.objective)),
        Line::from(""),
    ];

    for (i, task) in hil.tasks.iter().enumerate() {
        lines.push(Line::from(format!("  {}. {}", i + 1, task)));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(&*hil.message));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "y: approve  n: reject  Esc: reject",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Human Intervention ")
        .style(Style::default().fg(Color::Yellow));

    frame.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: true }),
        area,
    );
}
