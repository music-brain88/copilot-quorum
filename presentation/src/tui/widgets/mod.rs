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
pub mod lua_content;
pub mod model_stream;
pub mod progress_panel;
pub mod status_bar;
pub mod tab_bar;
pub mod tool_log;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders};

use super::content::ContentSlot;
use super::layout::LayoutPreset;
use super::mode::InputMode;
use super::state::TuiState;

/// Build a bordered `Block` for a content pane with focus-aware styling.
///
/// All pane widgets should route through this helper so pane focus is
/// visualized consistently across the TUI. Given the current `TuiState`
/// and the pane's own `ContentSlot`, this decides border color + title
/// style based on whether the pane is focused and whether Visual mode
/// is active.
pub(super) fn focus_block<'a>(state: &TuiState, slot: &ContentSlot, title: &'a str) -> Block<'a> {
    let is_focused = &state.focused_slot == slot;
    let in_visual = state.mode == InputMode::Visual;
    let (border_style, title_style) = focus_styles(is_focused, in_visual);

    Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(title, title_style))
}

/// Resolve (border_style, title_style) for a pane given focus + mode flags.
///
/// Visual language (tmux-style — only the frame signals focus, content stays
/// readable at full brightness):
/// - Unfocused: dark-gray border + dark-gray plain title so the frame recedes.
/// - Focused (Normal/Insert/Command): cyan border + bold cyan title.
/// - Focused + Visual: magenta — matches `InputMode::Visual.color()` so the
///   selected pane and the mode indicator agree.
fn focus_styles(is_focused: bool, in_visual: bool) -> (Style, Style) {
    if !is_focused {
        let muted = Style::default().fg(Color::DarkGray);
        return (muted, muted);
    }
    let accent = if in_visual {
        Color::Magenta
    } else {
        Color::Cyan
    };
    let border = Style::default().fg(accent);
    let title = Style::default()
        .fg(accent)
        .add_modifier(Modifier::BOLD);
    (border, title)
}

/// Reverse-video the lines in the active Visual selection, and prefix the
/// cursor line with a ▸ marker so anchor and cursor are distinguishable.
///
/// No-op if the selection is not active, the focused slot differs, or
/// `lines` is empty. Indices are clamped to `lines.len()` so callers can
/// stay ignorant of wrap state / bounds.
pub(super) fn apply_visual_highlight(
    lines: &mut [ratatui::text::Line<'_>],
    state: &TuiState,
    slot: &ContentSlot,
) {
    if state.mode != InputMode::Visual || &state.focused_slot != slot {
        return;
    }
    let Some(sel) = state.visual_selection else {
        return;
    };
    if lines.is_empty() {
        return;
    }
    let max_idx = lines.len() - 1;
    let (start, end_raw) = sel.range();
    if start > max_idx {
        return;
    }
    let end = end_raw.min(max_idx);
    let cursor = sel.cursor_line.min(max_idx);

    for (i, line) in lines.iter_mut().enumerate() {
        if i < start || i > end {
            continue;
        }
        line.style = line.style.add_modifier(Modifier::REVERSED);
        if i == cursor {
            line.spans.insert(
                0,
                Span::styled(
                    "▸",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
            );
        }
    }
}

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
    /// For custom presets, use [`compute_with_splits`] instead.
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

    /// Compute layout with explicit splits and direction (for custom presets).
    pub fn compute_with_splits(
        area: Rect,
        input_lines: u16,
        max_input_height: u16,
        show_tab_bar: bool,
        splits: &[u16],
        direction: Direction,
    ) -> Self {
        let (header, tab_bar, main_area, input, status_bar) =
            Self::compute_outer_regions(area, input_lines, max_input_height, show_tab_bar);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::state::{TuiState, VisualSelection};
    use ratatui::text::{Line, Span};

    fn make_lines(n: usize) -> Vec<Line<'static>> {
        (0..n)
            .map(|i| Line::from(Span::raw(format!("line {}", i))))
            .collect()
    }

    fn visual_state(
        slot: ContentSlot,
        anchor: usize,
        cursor: usize,
    ) -> TuiState {
        let mut state = TuiState::default();
        state.mode = InputMode::Visual;
        state.focused_slot = slot;
        state.visual_selection = Some(VisualSelection {
            anchor_line: anchor,
            cursor_line: cursor,
        });
        state
    }

    #[test]
    fn visual_highlight_noop_when_not_visual() {
        let mut lines = make_lines(3);
        let state = TuiState::default();
        let before = format!("{:?}", lines);
        apply_visual_highlight(&mut lines, &state, &ContentSlot::Conversation);
        assert_eq!(before, format!("{:?}", lines));
    }

    #[test]
    fn visual_highlight_noop_when_slot_mismatch() {
        let mut lines = make_lines(3);
        let state = visual_state(ContentSlot::Conversation, 0, 2);
        let before = format!("{:?}", lines);
        apply_visual_highlight(&mut lines, &state, &ContentSlot::Progress);
        assert_eq!(before, format!("{:?}", lines));
    }

    #[test]
    fn visual_highlight_applies_reversed_and_cursor_marker() {
        let mut lines = make_lines(4);
        let state = visual_state(ContentSlot::Progress, 1, 2);
        apply_visual_highlight(&mut lines, &state, &ContentSlot::Progress);

        // Line 0: untouched
        assert!(!lines[0].style.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(lines[0].spans[0].content.as_ref(), "line 0");

        // Line 1 (anchor): reversed, no marker
        assert!(lines[1].style.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(lines[1].spans[0].content.as_ref(), "line 1");

        // Line 2 (cursor): reversed + ▸ prepended
        assert!(lines[2].style.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(lines[2].spans[0].content.as_ref(), "▸");
        assert_eq!(lines[2].spans[1].content.as_ref(), "line 2");

        // Line 3: untouched
        assert!(!lines[3].style.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn visual_highlight_clamps_out_of_range() {
        let mut lines = make_lines(3);
        // cursor way past end — should clamp to last line without panic
        let state = visual_state(ContentSlot::Conversation, 0, 999);
        apply_visual_highlight(&mut lines, &state, &ContentSlot::Conversation);

        // All three lines reversed (end clamped to 2)
        for line in &lines {
            assert!(line.style.add_modifier.contains(Modifier::REVERSED));
        }
        // Last line gets the cursor marker
        assert_eq!(lines[2].spans[0].content.as_ref(), "▸");
    }
}
