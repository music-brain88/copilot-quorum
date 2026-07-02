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

/// Compute the frame layout for `state` at `area`.
///
/// Single source of truth shared by [`render`] (with `frame.area()`) and the
/// remote `layout.get` / `screen.capture` methods (with an arbitrary size).
pub(super) fn compute_layout(
    state: &TuiState,
    area: ratatui::layout::Rect,
) -> (MainLayout, Vec<super::surface::SurfaceId>) {
    let pane_surfaces = state.route.required_pane_surfaces();
    let show_tab_bar = state.tabs.len() > 1;
    let layout = if state.layout_config.preset.is_builtin() {
        MainLayout::compute_with_layout(
            area,
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
            area,
            state.input_line_count() as u16,
            state.tui_config.max_input_height,
            show_tab_bar,
            &splits,
            direction,
        )
    };
    (layout, pane_surfaces)
}

/// Render all widgets via registry-driven dispatch.
pub(super) fn render(
    frame: &mut ratatui::Frame,
    state: &TuiState,
    content_registry: &RefCell<ContentRegistry>,
) {
    use super::surface::SurfaceLayout;

    let (layout, pane_surfaces) = compute_layout(state, frame.area());

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
        render_help(frame, help_area, state);
    }

    if state.hil_prompt.is_some() {
        let modal_area = MainLayout::centered_overlay(60, 50, frame.area());
        frame.render_widget(ratatui::widgets::Clear, modal_area);
        render_hil_modal(frame, modal_area, state);
    }
}

/// Help overlay content — shared by the renderer and the scroll clamp logic.
fn help_lines() -> Vec<ratatui::text::Line<'static>> {
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};

    vec![
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
        Line::from("Yank / Copy:"),
        Line::from("  yy     Yank recent message (focused pane)"),
        Line::from("  ya     Yank all pane content"),
        Line::from("  Y      Yank last Assistant response"),
        Line::from("  v      Enter Visual mode"),
        Line::from("  Ctrl+w Cycle focused pane"),
        Line::from(""),
        Line::from("Visual Mode:"),
        Line::from("  h/j/k/l   Extend selection"),
        Line::from("  w/b       Word-wise extend"),
        Line::from("  Home/End  Jump to line start/end"),
        Line::from("  y/Enter   Yank selection"),
        Line::from("  Esc/v     Exit to Normal"),
        Line::from(""),
        Line::from("Insert Mode:"),
        Line::from("  Enter        Send message"),
        Line::from("  Shift+Enter  Insert newline (multiline)"),
        Line::from("  Esc        Return to Normal"),
        Line::from(""),
        Line::from("Commands (:command):"),
        Line::from("  :q       Close tab (quit on last tab)"),
        Line::from("  :qa      Quit app (all tabs)"),
        Line::from("  :help    Show help"),
        Line::from("  :solo    Switch to Solo mode"),
        Line::from("  :ens     Switch to Ensemble mode"),
        Line::from("  :fast    Toggle fast mode"),
        Line::from("  :ask <question>   Ask (lightweight Q&A)"),
        Line::from("  :discuss <question> Discuss (quorum discussion)"),
        Line::from("  :config [section]  Show configuration (e.g. :config models)"),
        Line::from("  :clear   Clear history"),
        Line::from("  :tabnew [form]  New tab (agent/ask/discuss)"),
        Line::from("  :tabclose       Close tab"),
        Line::from("  :tabs           List tabs"),
        Line::from(""),
        Line::from(Span::styled(
            "j/k scroll · Press ? or Esc to close",
            Style::default().fg(Color::DarkGray),
        )),
    ]
}

/// Max vertical scroll offset for the Help overlay at the given terminal size.
///
/// Mirrors the render path: same `centered_overlay(70, 70)` rect and the same
/// wrapping algorithm (`Paragraph::line_count`), so the key handler can clamp
/// `help_scroll` exactly to the last visible position.
pub(super) fn help_max_scroll(term_size: (u16, u16)) -> u16 {
    use ratatui::widgets::{Paragraph, Wrap};

    let (width, height) = term_size;
    let area =
        MainLayout::centered_overlay(70, 70, ratatui::layout::Rect::new(0, 0, width, height));
    let content_width = area.width.saturating_sub(2); // borders
    let visible_height = area.height.saturating_sub(2); // borders

    // Built without block so line_count returns pure content lines.
    let paragraph = Paragraph::new(help_lines()).wrap(Wrap { trim: true });
    let total_lines = paragraph.line_count(content_width) as u16;
    total_lines.saturating_sub(visible_height)
}

fn render_help(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &TuiState) {
    use ratatui::style::{Color, Style};
    use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

    let lines = help_lines();
    let content_width = area.width.saturating_sub(2); // borders
    let visible_height = area.height.saturating_sub(2); // borders

    // Defensive clamp — the key handler already clamps against the last known
    // terminal size, but the terminal may have shrunk since.
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true });
    let total_lines = paragraph.line_count(content_width) as u16;
    let max_scroll = total_lines.saturating_sub(visible_height);
    let offset = state.help_scroll.min(max_scroll);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Help ")
        .style(Style::default().fg(Color::Cyan));

    frame.render_widget(paragraph.block(block).scroll((offset, 0)), area);
}

fn render_hil_modal(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &TuiState) {
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
        "y: approve  n: reject  Esc: reject  j/k: scroll conversation",
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
