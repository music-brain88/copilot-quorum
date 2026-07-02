//! Off-screen rendering helpers for the Remote Control API.
//!
//! Backs `screen.capture` and `layout.get`: renders the live `TuiState`
//! into a `TestBackend` buffer (so a remote agent sees exactly what the
//! user sees) and dumps the computed layout geometry as JSON.

use super::app_render;
use super::content::ContentRegistry;
use super::state::TuiState;
use super::surface::{SurfaceId, SurfaceLayout};
use super::widgets::MainLayout;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use serde_json::{Value, json};
use std::cell::RefCell;
use unicode_width::UnicodeWidthStr;

/// One run of identically-styled cells within a captured line.
///
/// `start`/`end` are cell-column coordinates (end-exclusive) — independent
/// of the trimmed line string, which may be shorter.
#[derive(Debug, PartialEq)]
pub(super) struct StyleRun {
    pub start: u16,
    pub end: u16,
    pub fg: String,
    pub bg: String,
    pub mods: Vec<String>,
}

impl StyleRun {
    pub fn to_json(&self) -> Value {
        json!({
            "start": self.start,
            "end": self.end,
            "fg": self.fg,
            "bg": self.bg,
            "mods": self.mods,
        })
    }
}

/// Captured screen: rendered lines + optional per-line style runs.
pub(super) type Captured = (Vec<String>, Option<Vec<Vec<StyleRun>>>);

/// Render `state` off-screen at `width`×`height`.
///
/// Returns the rendered lines (trailing whitespace trimmed) and, when
/// `with_styles` is set, per-line style runs covering columns `0..width`.
pub(super) fn capture_screen(
    state: &TuiState,
    content_registry: &RefCell<ContentRegistry>,
    width: u16,
    height: u16,
    with_styles: bool,
) -> std::io::Result<Captured> {
    let backend = TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend)?;
    terminal.draw(|frame| app_render::render(frame, state, content_registry))?;
    let buf = terminal.backend().buffer();
    Ok((
        buffer_lines(buf),
        with_styles.then(|| buffer_style_runs(buf)),
    ))
}

/// Convert a rendered buffer to one string per row.
///
/// Cells hidden behind a wide grapheme (e.g. CJK) are skipped using the
/// same algorithm as ratatui's own `Buffer` Debug impl — without this,
/// the reset cells following a double-width char would shift the rest of
/// the line right of the real grid.
fn buffer_lines(buf: &Buffer) -> Vec<String> {
    let width = buf.area.width as usize;
    if width == 0 {
        return Vec::new();
    }
    buf.content
        .chunks(width)
        .map(|row| {
            let mut line = String::new();
            let mut skip = 0usize;
            for cell in row {
                if skip == 0 {
                    line.push_str(cell.symbol());
                }
                skip = skip
                    .max(UnicodeWidthStr::width(cell.symbol()))
                    .saturating_sub(1);
            }
            line.trim_end().to_string()
        })
        .collect()
}

/// Run-length encode each row's `(fg, bg, modifiers)` into [`StyleRun`]s.
///
/// Runs are contiguous and gap-free: the first run of each line starts at 0
/// and the last ends at the buffer width, so consumers get full coverage.
fn buffer_style_runs(buf: &Buffer) -> Vec<Vec<StyleRun>> {
    let width = buf.area.width as usize;
    if width == 0 {
        return Vec::new();
    }
    buf.content
        .chunks(width)
        .map(|row| {
            let mut runs: Vec<StyleRun> = Vec::new();
            for (x, cell) in row.iter().enumerate() {
                let fg = cell.fg.to_string();
                let bg = cell.bg.to_string();
                let mods: Vec<String> = cell
                    .modifier
                    .iter_names()
                    .map(|(name, _)| name.to_string())
                    .collect();
                match runs.last_mut() {
                    Some(run) if run.fg == fg && run.bg == bg && run.mods == mods => {
                        run.end = (x + 1) as u16;
                    }
                    _ => runs.push(StyleRun {
                        start: x as u16,
                        end: (x + 1) as u16,
                        fg,
                        bg,
                        mods,
                    }),
                }
            }
            runs
        })
        .collect()
}

pub(super) fn rect_json(r: Rect) -> Value {
    json!({"x": r.x, "y": r.y, "width": r.width, "height": r.height})
}

/// Route table entries as JSON, with per-entry visibility for the given
/// surface layout (a route is invisible when its surface has no area in
/// the current preset, e.g. sidebar under Minimal).
pub(super) fn routes_json(state: &TuiState, surfaces: &SurfaceLayout) -> Value {
    let entries: Vec<Value> = state
        .route
        .entries()
        .iter()
        .map(|entry| {
            json!({
                "content": super::layout::content_slot_to_string(&entry.content),
                "surface": super::layout::surface_id_to_string(&entry.surface),
                "visible": surfaces.area_for(&entry.surface).is_some(),
            })
        })
        .collect();
    Value::Array(entries)
}

/// Computed layout geometry for `state` at `area` — backs `layout.get`.
pub(super) fn layout_snapshot(state: &TuiState, area: Rect) -> Value {
    let (layout, pane_surfaces) = app_render::compute_layout(state, area);
    let surfaces = SurfaceLayout::from_main_layout(&layout, &pane_surfaces);

    // Mirrors the fallback logic in MainLayout::compute_with_layout.
    let flex_fallback_active = state.layout_config.preset.is_builtin()
        && state.layout_config.flex_threshold > 0
        && area.width < state.layout_config.flex_threshold;

    let (splits, direction) = if flex_fallback_active {
        (vec![100u16], ratatui::layout::Direction::Horizontal)
    } else {
        (
            state.layout_config.resolve_splits(pane_surfaces.len()),
            state.layout_config.resolve_direction(),
        )
    };

    let mut surface_map = serde_json::Map::new();
    let chrome = [
        SurfaceId::Header,
        SurfaceId::TabBar,
        SurfaceId::Input,
        SurfaceId::StatusBar,
    ];
    for id in chrome.iter().chain(pane_surfaces.iter()) {
        if let Some(rect) = surfaces.area_for(id) {
            surface_map.insert(super::layout::surface_id_to_string(id), rect_json(rect));
        }
    }

    // Overlay rects only when actually shown — mirrors app_render::render.
    let help_overlay = state
        .show_help
        .then(|| rect_json(MainLayout::centered_overlay(70, 70, area)));
    let hil_overlay = state
        .hil_prompt
        .is_some()
        .then(|| rect_json(MainLayout::centered_overlay(60, 50, area)));

    json!({
        "terminal": {"width": area.width, "height": area.height},
        "preset": state.layout_config.preset.to_string(),
        "is_builtin": state.layout_config.preset.is_builtin(),
        "flex_threshold": state.layout_config.flex_threshold,
        "flex_fallback_active": flex_fallback_active,
        "direction": match direction {
            ratatui::layout::Direction::Horizontal => "horizontal",
            ratatui::layout::Direction::Vertical => "vertical",
        },
        "splits": splits,
        "pane_surfaces": pane_surfaces
            .iter()
            .map(super::layout::surface_id_to_string)
            .collect::<Vec<_>>(),
        "surfaces": Value::Object(surface_map),
        "routes": routes_json(state, &surfaces),
        "overlays": {"help": help_overlay, "hil": hil_overlay},
    })
}

#[cfg(test)]
mod tests {
    use super::super::app_render::build_default_registry;
    use super::super::state::{DisplayMessage, HilPrompt};
    use super::*;

    fn registry() -> RefCell<ContentRegistry> {
        RefCell::new(build_default_registry())
    }

    #[test]
    fn capture_lines_dimensions() {
        let state = TuiState::new();
        let (lines, styles) = capture_screen(&state, &registry(), 80, 24, false).unwrap();
        assert_eq!(lines.len(), 24);
        for line in &lines {
            assert!(UnicodeWidthStr::width(line.as_str()) <= 80);
        }
        assert!(styles.is_none());
    }

    #[test]
    fn capture_wide_chars_no_drift() {
        let mut state = TuiState::new();
        state.push_message(DisplayMessage::user("こんにちは世界"));
        let (lines, _) = capture_screen(&state, &registry(), 80, 24, false).unwrap();
        let hit: Vec<&String> = lines
            .iter()
            .filter(|l| l.contains("こんにちは世界"))
            .collect();
        assert_eq!(hit.len(), 1, "message should appear exactly once");
        assert!(UnicodeWidthStr::width(hit[0].as_str()) <= 80);
    }

    #[test]
    fn capture_help_overlay() {
        let mut state = TuiState::new();
        state.show_help = true;
        let (lines, _) = capture_screen(&state, &registry(), 80, 24, false).unwrap();
        assert!(lines.iter().any(|l| l.contains("Keyboard Shortcuts")));
    }

    #[test]
    fn capture_hil_overlay() {
        let mut state = TuiState::new();
        state.hil_prompt = Some(HilPrompt {
            title: "Plan Review".into(),
            objective: "Fix the bug".into(),
            tasks: vec!["read file".into()],
            message: "Approve?".into(),
        });
        let (lines, _) = capture_screen(&state, &registry(), 80, 24, false).unwrap();
        assert!(lines.iter().any(|l| l.contains("Human Intervention")));
    }

    #[test]
    fn style_runs_cover_full_width() {
        let state = TuiState::new();
        let (_, styles) = capture_screen(&state, &registry(), 80, 24, true).unwrap();
        let styles = styles.unwrap();
        assert_eq!(styles.len(), 24);
        for runs in &styles {
            assert_eq!(runs.first().unwrap().start, 0);
            assert_eq!(runs.last().unwrap().end, 80);
            for pair in runs.windows(2) {
                assert_eq!(pair[0].end, pair[1].start, "runs must be contiguous");
            }
        }
    }

    #[test]
    fn layout_snapshot_default_has_sidebar() {
        let state = TuiState::new();
        let snap = layout_snapshot(&state, Rect::new(0, 0, 190, 45));
        assert_eq!(snap["preset"], "default");
        assert_eq!(snap["flex_fallback_active"], false);
        assert!(snap["surfaces"]["main_pane"].is_object());
        assert!(snap["surfaces"]["sidebar"].is_object());
        // Geometry sanity: main_pane + sidebar tile the full width
        let main = &snap["surfaces"]["main_pane"];
        let side = &snap["surfaces"]["sidebar"];
        assert_eq!(
            main["width"].as_u64().unwrap() + side["width"].as_u64().unwrap(),
            190
        );
    }

    #[test]
    fn layout_snapshot_minimal_has_no_sidebar() {
        let mut state = TuiState::new();
        state.layout_config.preset = super::super::layout::LayoutPreset::Minimal;
        let snap = layout_snapshot(&state, Rect::new(0, 0, 190, 45));
        assert!(snap["surfaces"]["sidebar"].is_null());
    }

    #[test]
    fn layout_snapshot_flex_fallback() {
        let state = TuiState::new(); // flex_threshold = 120
        let snap = layout_snapshot(&state, Rect::new(0, 0, 100, 30));
        assert_eq!(snap["flex_fallback_active"], true);
        assert!(snap["surfaces"]["sidebar"].is_null());
        assert_eq!(snap["splits"][0], 100);
    }
}
