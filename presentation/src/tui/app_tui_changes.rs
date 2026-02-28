//! Apply pending TUI changes from Lua scripting accessor.

use super::content::ContentRegistry;
use super::layout::LayoutPreset;
use super::state::TuiState;
use std::cell::RefCell;

/// Apply pending TUI changes from the Lua scripting accessor to state and registry.
///
/// Extracted as a free function for testability — avoids needing to construct a full `TuiApp`.
pub(super) fn apply_pending_tui_changes(
    changes: quorum_application::TuiPendingChanges,
    state: &mut TuiState,
    content_registry: &RefCell<ContentRegistry>,
) {
    if changes.is_empty() {
        return;
    }

    // 1. Register custom presets (before preset_switch may reference them)
    for (name, config) in changes.new_presets {
        state.layout_config.custom_presets.insert(name, config);
    }

    // 2. Switch preset (changes the route table base)
    if let Some(preset_name) = changes.preset_switch {
        state.layout_config.preset = match preset_name.as_str() {
            "default" => LayoutPreset::Default,
            "minimal" => LayoutPreset::Minimal,
            "wide" => LayoutPreset::Wide,
            "stacked" => LayoutPreset::Stacked,
            _ => LayoutPreset::Custom(preset_name),
        };
        state.route = super::route::RouteTable::from_preset_and_overrides(
            state.layout_config.preset.clone(),
            &state.layout_config.route_overrides,
        );
    }

    // 3. Apply route overrides (on top of the current preset)
    if !changes.route_changes.is_empty() {
        for (content_name, surface_name) in changes.route_changes {
            if let (Some(content), Some(surface)) = (
                super::layout::parse_content_slot(&content_name),
                super::layout::parse_surface_id(&surface_name),
            ) {
                state
                    .layout_config
                    .route_overrides
                    .push(super::layout::RouteOverride { content, surface });
            }
        }
        state.route = super::route::RouteTable::from_preset_and_overrides(
            state.layout_config.preset.clone(),
            &state.layout_config.route_overrides,
        );
    }

    // 4. Register new Lua content slots
    for slot_name in changes.new_content_slots {
        content_registry.borrow_mut().register_mut(Box::new(
            super::widgets::lua_content::LuaContentRenderer::new(slot_name),
        ));
    }

    // 5. Update Lua content text
    for (slot_name, text) in changes.content_text_updates {
        state.lua_content.insert(slot_name, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::content::ContentSlot;
    use crate::tui::surface::SurfaceId;
    use quorum_application::{CustomPresetConfig, TuiPendingChanges};

    fn empty_changes() -> TuiPendingChanges {
        TuiPendingChanges {
            route_changes: vec![],
            preset_switch: None,
            new_presets: vec![],
            new_content_slots: vec![],
            content_text_updates: vec![],
        }
    }

    #[test]
    fn test_apply_empty_changes_is_noop() {
        let mut state = TuiState::new();
        let registry = RefCell::new(ContentRegistry::new());
        let preset_before = state.layout_config.preset.clone();
        apply_pending_tui_changes(empty_changes(), &mut state, &registry);
        assert_eq!(state.layout_config.preset, preset_before);
    }

    #[test]
    fn test_apply_preset_switch() {
        let mut state = TuiState::new();
        let registry = RefCell::new(ContentRegistry::new());
        let changes = TuiPendingChanges {
            preset_switch: Some("minimal".to_string()),
            ..empty_changes()
        };
        apply_pending_tui_changes(changes, &mut state, &registry);
        assert_eq!(state.layout_config.preset, LayoutPreset::Minimal);
    }

    #[test]
    fn test_apply_custom_preset_switch() {
        let mut state = TuiState::new();
        let registry = RefCell::new(ContentRegistry::new());
        let changes = TuiPendingChanges {
            new_presets: vec![(
                "my_layout".to_string(),
                CustomPresetConfig {
                    splits: vec![70, 30],
                    direction: "horizontal".to_string(),
                },
            )],
            preset_switch: Some("my_layout".to_string()),
            ..empty_changes()
        };
        apply_pending_tui_changes(changes, &mut state, &registry);
        assert_eq!(
            state.layout_config.preset,
            LayoutPreset::Custom("my_layout".to_string())
        );
        assert!(state.layout_config.custom_presets.contains_key("my_layout"));
    }

    #[test]
    fn test_apply_content_text_updates() {
        let mut state = TuiState::new();
        let registry = RefCell::new(ContentRegistry::new());
        let changes = TuiPendingChanges {
            content_text_updates: vec![("status".to_string(), "Hello from Lua".to_string())],
            ..empty_changes()
        };
        apply_pending_tui_changes(changes, &mut state, &registry);
        assert_eq!(
            state.lua_content.get("status"),
            Some(&"Hello from Lua".to_string())
        );
    }

    #[test]
    fn test_apply_new_content_slots() {
        let mut state = TuiState::new();
        let registry = RefCell::new(ContentRegistry::new());
        let changes = TuiPendingChanges {
            new_content_slots: vec!["my_panel".to_string()],
            ..empty_changes()
        };
        apply_pending_tui_changes(changes, &mut state, &registry);
        let reg = registry.borrow();
        assert!(
            reg.get(&ContentSlot::LuaSlot("my_panel".to_string()))
                .is_some()
        );
    }

    #[test]
    fn test_apply_route_changes() {
        let mut state = TuiState::new();
        let registry = RefCell::new(ContentRegistry::new());
        let changes = TuiPendingChanges {
            route_changes: vec![("progress".to_string(), "main_pane".to_string())],
            ..empty_changes()
        };
        apply_pending_tui_changes(changes, &mut state, &registry);
        assert_eq!(state.layout_config.route_overrides.len(), 1);
        assert_eq!(
            state.route.surface_for(&ContentSlot::Progress),
            Some(SurfaceId::MainPane),
        );
    }
}
