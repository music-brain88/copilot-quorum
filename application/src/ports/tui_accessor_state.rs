//! Default implementation of [`TuiAccessorPort`].
//!
//! `TuiAccessorState` accumulates string-based route/layout/content changes
//! from Lua scripts and buffers them as [`TuiPendingChanges`] for the
//! presentation layer to consume each frame.

use std::collections::HashMap;

use super::tui_accessor::*;

/// Default state-holder implementation of [`TuiAccessorPort`].
///
/// Shared via `Arc<Mutex<TuiAccessorState>>` between:
/// - `LuaScriptingEngine` (writer, via `quorum.tui.*` APIs)
/// - `TuiApp` (reader, via `take_pending_changes()` each frame)
pub struct TuiAccessorState {
    // Current state mirror (for reads)
    routes: HashMap<String, String>,
    current_preset: String,
    custom_presets: HashMap<String, CustomPresetConfig>,
    registered_slots: Vec<String>,
    slot_text: HashMap<String, String>,

    // Pending changes (for writes → presentation consumption)
    pending: TuiPendingChanges,
}

impl TuiAccessorState {
    /// Create with empty state.
    pub fn new() -> Self {
        Self {
            routes: HashMap::new(),
            current_preset: "default".to_string(),
            custom_presets: HashMap::new(),
            registered_slots: Vec::new(),
            slot_text: HashMap::new(),
            pending: TuiPendingChanges::default(),
        }
    }

    /// Create with default routes matching `RouteTable::default_layout()`.
    pub fn with_default_routes() -> Self {
        let mut routes = HashMap::new();
        routes.insert("conversation".to_string(), "main_pane".to_string());
        routes.insert("progress".to_string(), "sidebar".to_string());
        routes.insert("hil_prompt".to_string(), "overlay".to_string());
        routes.insert("help".to_string(), "overlay".to_string());
        routes.insert("notification".to_string(), "status_bar".to_string());

        Self {
            routes,
            current_preset: "default".to_string(),
            custom_presets: HashMap::new(),
            registered_slots: Vec::new(),
            slot_text: HashMap::new(),
            pending: TuiPendingChanges::default(),
        }
    }
}

impl Default for TuiAccessorState {
    fn default() -> Self {
        Self::new()
    }
}

impl TuiAccessorPort for TuiAccessorState {
    // -- Routes --

    fn route_set(&mut self, content: &str, surface: &str) -> Result<(), TuiAccessError> {
        if !is_valid_content_name(content) {
            return Err(TuiAccessError::UnknownContent {
                name: content.to_string(),
            });
        }
        if !is_valid_surface_name(surface) {
            return Err(TuiAccessError::UnknownSurface {
                name: surface.to_string(),
            });
        }

        // Update mirror
        self.routes
            .insert(content.to_string(), surface.to_string());

        // Buffer change
        self.pending
            .route_changes
            .push((content.to_string(), surface.to_string()));

        Ok(())
    }

    fn route_get(&self, content: &str) -> Option<String> {
        self.routes.get(content).cloned()
    }

    fn route_entries(&self) -> Vec<(String, String)> {
        self.routes
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    // -- Layout --

    fn layout_current_preset(&self) -> String {
        self.current_preset.clone()
    }

    fn layout_switch_preset(&mut self, name: &str) -> Result<(), TuiAccessError> {
        if !KNOWN_PRESETS.contains(&name) && !self.custom_presets.contains_key(name) {
            return Err(TuiAccessError::UnknownPreset {
                name: name.to_string(),
            });
        }

        self.current_preset = name.to_string();
        self.pending.preset_switch = Some(name.to_string());
        Ok(())
    }

    fn layout_register_preset(
        &mut self,
        name: &str,
        config: CustomPresetConfig,
    ) -> Result<(), TuiAccessError> {
        // Don't allow overriding built-in presets
        if KNOWN_PRESETS.contains(&name) {
            return Err(TuiAccessError::DuplicatePreset {
                name: name.to_string(),
            });
        }

        // Validate splits
        if config.splits.is_empty() {
            return Err(TuiAccessError::InvalidConfig {
                message: "splits must have at least one entry".to_string(),
            });
        }
        if config.direction != "horizontal" && config.direction != "vertical" {
            return Err(TuiAccessError::InvalidConfig {
                message: format!(
                    "direction must be 'horizontal' or 'vertical', got '{}'",
                    config.direction
                ),
            });
        }

        self.custom_presets
            .insert(name.to_string(), config.clone());
        self.pending
            .new_presets
            .push((name.to_string(), config));
        Ok(())
    }

    fn layout_presets(&self) -> Vec<String> {
        let mut presets: Vec<String> = KNOWN_PRESETS.iter().map(|s| s.to_string()).collect();
        presets.extend(self.custom_presets.keys().cloned());
        presets
    }

    // -- Content --

    fn content_register(&mut self, slot_name: &str) -> Result<(), TuiAccessError> {
        if self.registered_slots.contains(&slot_name.to_string()) {
            return Err(TuiAccessError::DuplicateSlot {
                name: slot_name.to_string(),
            });
        }

        self.registered_slots.push(slot_name.to_string());
        self.pending
            .new_content_slots
            .push(slot_name.to_string());
        Ok(())
    }

    fn content_set_text(&mut self, slot_name: &str, text: &str) -> Result<(), TuiAccessError> {
        if !self.registered_slots.contains(&slot_name.to_string()) {
            return Err(TuiAccessError::UnknownContent {
                name: slot_name.to_string(),
            });
        }

        self.slot_text
            .insert(slot_name.to_string(), text.to_string());
        self.pending
            .content_text_updates
            .push((slot_name.to_string(), text.to_string()));
        Ok(())
    }

    fn content_slots(&self) -> Vec<String> {
        self.registered_slots.clone()
    }

    // -- Change tracking --

    fn take_pending_changes(&mut self) -> TuiPendingChanges {
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_routes_match_default_layout() {
        let state = TuiAccessorState::with_default_routes();
        assert_eq!(
            state.route_get("conversation"),
            Some("main_pane".to_string())
        );
        assert_eq!(state.route_get("progress"), Some("sidebar".to_string()));
        assert_eq!(state.route_get("hil_prompt"), Some("overlay".to_string()));
        assert_eq!(state.route_get("help"), Some("overlay".to_string()));
        assert_eq!(
            state.route_get("notification"),
            Some("status_bar".to_string())
        );
    }

    #[test]
    fn test_route_set_validates_content_name() {
        let mut state = TuiAccessorState::new();
        assert!(state.route_set("conversation", "sidebar").is_ok());
        assert!(state.route_set("model_stream:claude", "dynamic_pane:claude").is_ok());
        assert!(state.route_set("lua:my_panel", "sidebar").is_ok());

        let err = state.route_set("invalid_slot", "sidebar").unwrap_err();
        assert!(matches!(err, TuiAccessError::UnknownContent { .. }));
    }

    #[test]
    fn test_route_set_validates_surface_name() {
        let mut state = TuiAccessorState::new();
        let err = state
            .route_set("conversation", "nonexistent")
            .unwrap_err();
        assert!(matches!(err, TuiAccessError::UnknownSurface { .. }));
    }

    #[test]
    fn test_route_set_buffers_pending_changes() {
        let mut state = TuiAccessorState::new();
        state.route_set("progress", "main_pane").unwrap();

        let changes = state.take_pending_changes();
        assert_eq!(changes.route_changes.len(), 1);
        assert_eq!(
            changes.route_changes[0],
            ("progress".to_string(), "main_pane".to_string())
        );

        // Second take should be empty
        let changes2 = state.take_pending_changes();
        assert!(changes2.is_empty());
    }

    #[test]
    fn test_layout_switch_preset() {
        let mut state = TuiAccessorState::new();
        assert_eq!(state.layout_current_preset(), "default");

        state.layout_switch_preset("wide").unwrap();
        assert_eq!(state.layout_current_preset(), "wide");

        let changes = state.take_pending_changes();
        assert_eq!(changes.preset_switch, Some("wide".to_string()));
    }

    #[test]
    fn test_layout_switch_unknown_preset_fails() {
        let mut state = TuiAccessorState::new();
        let err = state.layout_switch_preset("unknown").unwrap_err();
        assert!(matches!(err, TuiAccessError::UnknownPreset { .. }));
    }

    #[test]
    fn test_layout_register_custom_preset() {
        let mut state = TuiAccessorState::new();
        let config = CustomPresetConfig {
            splits: vec![40, 30, 30],
            direction: "horizontal".to_string(),
        };
        state.layout_register_preset("my_layout", config).unwrap();

        let presets = state.layout_presets();
        assert!(presets.contains(&"my_layout".to_string()));

        // Now can switch to it
        state.layout_switch_preset("my_layout").unwrap();
        assert_eq!(state.layout_current_preset(), "my_layout");
    }

    #[test]
    fn test_layout_register_builtin_name_fails() {
        let mut state = TuiAccessorState::new();
        let config = CustomPresetConfig {
            splits: vec![50, 50],
            direction: "horizontal".to_string(),
        };
        let err = state.layout_register_preset("default", config).unwrap_err();
        assert!(matches!(err, TuiAccessError::DuplicatePreset { .. }));
    }

    #[test]
    fn test_layout_register_invalid_direction_fails() {
        let mut state = TuiAccessorState::new();
        let config = CustomPresetConfig {
            splits: vec![50, 50],
            direction: "diagonal".to_string(),
        };
        let err = state
            .layout_register_preset("broken", config)
            .unwrap_err();
        assert!(matches!(err, TuiAccessError::InvalidConfig { .. }));
    }

    #[test]
    fn test_content_register_and_set_text() {
        let mut state = TuiAccessorState::new();
        state.content_register("my_panel").unwrap();
        state
            .content_set_text("my_panel", "Hello from Lua!")
            .unwrap();

        let changes = state.take_pending_changes();
        assert_eq!(changes.new_content_slots, vec!["my_panel".to_string()]);
        assert_eq!(
            changes.content_text_updates,
            vec![("my_panel".to_string(), "Hello from Lua!".to_string())]
        );
    }

    #[test]
    fn test_content_register_duplicate_fails() {
        let mut state = TuiAccessorState::new();
        state.content_register("my_panel").unwrap();
        let err = state.content_register("my_panel").unwrap_err();
        assert!(matches!(err, TuiAccessError::DuplicateSlot { .. }));
    }

    #[test]
    fn test_content_set_text_unregistered_fails() {
        let mut state = TuiAccessorState::new();
        let err = state
            .content_set_text("nonexistent", "text")
            .unwrap_err();
        assert!(matches!(err, TuiAccessError::UnknownContent { .. }));
    }

    #[test]
    fn test_pending_changes_drain() {
        let mut state = TuiAccessorState::new();
        state.route_set("conversation", "sidebar").unwrap();
        state.content_register("panel1").unwrap();
        state.content_set_text("panel1", "hello").unwrap();

        let changes = state.take_pending_changes();
        assert!(!changes.is_empty());
        assert_eq!(changes.route_changes.len(), 1);
        assert_eq!(changes.new_content_slots.len(), 1);
        assert_eq!(changes.content_text_updates.len(), 1);

        // Drained — second take is empty
        assert!(state.take_pending_changes().is_empty());
    }
}
