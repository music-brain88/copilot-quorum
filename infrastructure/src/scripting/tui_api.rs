//! `quorum.tui` Lua API — route, layout, and content manipulation.
//!
//! Bridges the Lua scripting layer to `TuiAccessorPort` for runtime TUI
//! manipulation. Provides three sub-tables:
//!
//! ```lua
//! -- Routes: map content slots to surfaces
//! quorum.tui.routes.set("progress", "main_pane")
//! quorum.tui.routes.get("progress")            --> "sidebar"
//! quorum.tui.routes.list()                      --> {{content="...", surface="..."}, ...}
//!
//! -- Layout: preset management
//! quorum.tui.layout.current()                   --> "default"
//! quorum.tui.layout.switch("wide")
//! quorum.tui.layout.register_preset("my_layout", {
//!     splits = {40, 30, 30},
//!     direction = "horizontal",
//! })
//! quorum.tui.layout.presets()                   --> {"default", "minimal", ...}
//!
//! -- Content: Lua text-based content slots
//! quorum.tui.content.register("my_panel")
//! quorum.tui.content.set_text("my_panel", "Hello from Lua!")
//! quorum.tui.content.slots()                    --> {"my_panel"}
//! ```

use mlua::prelude::*;
use quorum_application::{CustomPresetConfig, TuiAccessorPort};
use std::sync::{Arc, Mutex};

use super::event_bus::EventBus;

/// Register the `quorum.tui` table with routes/layout/content sub-APIs.
pub fn register_tui_api(
    lua: &Lua,
    quorum: &LuaTable,
    tui_accessor: Arc<Mutex<dyn TuiAccessorPort>>,
    event_bus: Arc<Mutex<EventBus>>,
) -> LuaResult<()> {
    let tui_table = lua.create_table()?;

    register_routes_api(lua, &tui_table, Arc::clone(&tui_accessor))?;
    register_layout_api(
        lua,
        &tui_table,
        Arc::clone(&tui_accessor),
        Arc::clone(&event_bus),
    )?;
    register_content_api(
        lua,
        &tui_table,
        Arc::clone(&tui_accessor),
        Arc::clone(&event_bus),
    )?;

    quorum.set("tui", tui_table)?;
    Ok(())
}

/// Register `quorum.tui.routes` sub-table.
fn register_routes_api(
    lua: &Lua,
    tui_table: &LuaTable,
    tui_accessor: Arc<Mutex<dyn TuiAccessorPort>>,
) -> LuaResult<()> {
    let routes_table = lua.create_table()?;

    // quorum.tui.routes.set(content, surface)
    {
        let accessor = Arc::clone(&tui_accessor);
        let set_fn = lua.create_function(move |_lua, (content, surface): (String, String)| {
            let mut guard = accessor
                .lock()
                .map_err(|e| LuaError::external(format!("tui_accessor lock poisoned: {}", e)))?;
            guard
                .route_set(&content, &surface)
                .map_err(|e| LuaError::external(e.to_string()))?;
            Ok(())
        })?;
        routes_table.set("set", set_fn)?;
    }

    // quorum.tui.routes.get(content) -> surface | nil
    {
        let accessor = Arc::clone(&tui_accessor);
        let get_fn = lua.create_function(move |_lua, content: String| {
            let guard = accessor
                .lock()
                .map_err(|e| LuaError::external(format!("tui_accessor lock poisoned: {}", e)))?;
            Ok(guard.route_get(&content))
        })?;
        routes_table.set("get", get_fn)?;
    }

    // quorum.tui.routes.list() -> table of {content=..., surface=...}
    {
        let accessor = Arc::clone(&tui_accessor);
        let list_fn = lua.create_function(move |lua, ()| {
            let guard = accessor
                .lock()
                .map_err(|e| LuaError::external(format!("tui_accessor lock poisoned: {}", e)))?;
            let entries = guard.route_entries();
            let table = lua.create_table()?;
            for (i, (content, surface)) in entries.iter().enumerate() {
                let entry = lua.create_table()?;
                entry.set("content", content.as_str())?;
                entry.set("surface", surface.as_str())?;
                table.set(i + 1, entry)?;
            }
            Ok(table)
        })?;
        routes_table.set("list", list_fn)?;
    }

    tui_table.set("routes", routes_table)?;
    Ok(())
}

/// Register `quorum.tui.layout` sub-table.
fn register_layout_api(
    lua: &Lua,
    tui_table: &LuaTable,
    tui_accessor: Arc<Mutex<dyn TuiAccessorPort>>,
    event_bus: Arc<Mutex<EventBus>>,
) -> LuaResult<()> {
    let layout_table = lua.create_table()?;

    // quorum.tui.layout.current() -> preset name
    {
        let accessor = Arc::clone(&tui_accessor);
        let current_fn = lua.create_function(move |_lua, ()| {
            let guard = accessor
                .lock()
                .map_err(|e| LuaError::external(format!("tui_accessor lock poisoned: {}", e)))?;
            Ok(guard.layout_current_preset())
        })?;
        layout_table.set("current", current_fn)?;
    }

    // quorum.tui.layout.switch(preset_name)
    {
        let accessor = Arc::clone(&tui_accessor);
        let event_bus = Arc::clone(&event_bus);
        let switch_fn = lua.create_function(move |lua, name: String| {
            {
                let mut guard = accessor.lock().map_err(|e| {
                    LuaError::external(format!("tui_accessor lock poisoned: {}", e))
                })?;
                guard
                    .layout_switch_preset(&name)
                    .map_err(|e| LuaError::external(e.to_string()))?;
            }

            // Fire LayoutChanged event
            let data = lua.create_table()?;
            data.set("preset", name.as_str())?;
            let bus = event_bus
                .lock()
                .map_err(|e| LuaError::external(format!("event_bus lock poisoned: {}", e)))?;
            let _ = bus.fire(lua, "LayoutChanged", &data, false);

            Ok(())
        })?;
        layout_table.set("switch", switch_fn)?;
    }

    // quorum.tui.layout.register_preset(name, config_table)
    {
        let accessor = Arc::clone(&tui_accessor);
        let register_fn =
            lua.create_function(move |_lua, (name, config_table): (String, LuaTable)| {
                let splits = parse_splits(&config_table)?;
                let direction: String = config_table
                    .get("direction")
                    .unwrap_or_else(|_| "horizontal".to_string());

                let config = CustomPresetConfig { splits, direction };

                let mut guard = accessor.lock().map_err(|e| {
                    LuaError::external(format!("tui_accessor lock poisoned: {}", e))
                })?;
                guard
                    .layout_register_preset(&name, config)
                    .map_err(|e| LuaError::external(e.to_string()))?;
                Ok(())
            })?;
        layout_table.set("register_preset", register_fn)?;
    }

    // quorum.tui.layout.presets() -> table of preset names
    {
        let accessor = Arc::clone(&tui_accessor);
        let presets_fn = lua.create_function(move |lua, ()| {
            let guard = accessor
                .lock()
                .map_err(|e| LuaError::external(format!("tui_accessor lock poisoned: {}", e)))?;
            let presets = guard.layout_presets();
            let table = lua.create_table()?;
            for (i, name) in presets.iter().enumerate() {
                table.set(i + 1, name.as_str())?;
            }
            Ok(table)
        })?;
        layout_table.set("presets", presets_fn)?;
    }

    tui_table.set("layout", layout_table)?;
    Ok(())
}

/// Register `quorum.tui.content` sub-table.
fn register_content_api(
    lua: &Lua,
    tui_table: &LuaTable,
    tui_accessor: Arc<Mutex<dyn TuiAccessorPort>>,
    event_bus: Arc<Mutex<EventBus>>,
) -> LuaResult<()> {
    let content_table = lua.create_table()?;

    // quorum.tui.content.register(slot_name)
    {
        let accessor = Arc::clone(&tui_accessor);
        let event_bus = Arc::clone(&event_bus);
        let register_fn = lua.create_function(move |lua, slot_name: String| {
            {
                let mut guard = accessor.lock().map_err(|e| {
                    LuaError::external(format!("tui_accessor lock poisoned: {}", e))
                })?;
                guard
                    .content_register(&slot_name)
                    .map_err(|e| LuaError::external(e.to_string()))?;
            }

            // Fire PaneCreated event
            let data = lua.create_table()?;
            data.set("slot", slot_name.as_str())?;
            let bus = event_bus
                .lock()
                .map_err(|e| LuaError::external(format!("event_bus lock poisoned: {}", e)))?;
            let _ = bus.fire(lua, "PaneCreated", &data, false);

            Ok(())
        })?;
        content_table.set("register", register_fn)?;
    }

    // quorum.tui.content.set_text(slot_name, text)
    {
        let accessor = Arc::clone(&tui_accessor);
        let set_text_fn =
            lua.create_function(move |_lua, (slot_name, text): (String, String)| {
                let mut guard = accessor.lock().map_err(|e| {
                    LuaError::external(format!("tui_accessor lock poisoned: {}", e))
                })?;
                guard
                    .content_set_text(&slot_name, &text)
                    .map_err(|e| LuaError::external(e.to_string()))?;
                Ok(())
            })?;
        content_table.set("set_text", set_text_fn)?;
    }

    // quorum.tui.content.slots() -> table of slot names
    {
        let accessor = Arc::clone(&tui_accessor);
        let slots_fn = lua.create_function(move |lua, ()| {
            let guard = accessor
                .lock()
                .map_err(|e| LuaError::external(format!("tui_accessor lock poisoned: {}", e)))?;
            let slots = guard.content_slots();
            let table = lua.create_table()?;
            for (i, name) in slots.iter().enumerate() {
                table.set(i + 1, name.as_str())?;
            }
            Ok(table)
        })?;
        content_table.set("slots", slots_fn)?;
    }

    tui_table.set("content", content_table)?;
    Ok(())
}

/// Parse the `splits` field from a Lua config table into a `Vec<u16>`.
fn parse_splits(config_table: &LuaTable) -> LuaResult<Vec<u16>> {
    let splits_table: LuaTable = config_table
        .get("splits")
        .map_err(|_| LuaError::external("config table must have a 'splits' field"))?;

    let mut splits = Vec::new();
    for value in splits_table.sequence_values::<i64>() {
        let v = value?;
        if v < 0 || v > 100 {
            return Err(LuaError::external(format!(
                "split value must be 0..100, got {}",
                v
            )));
        }
        splits.push(v as u16);
    }

    if splits.is_empty() {
        return Err(LuaError::external("splits must have at least one entry"));
    }

    Ok(splits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quorum_application::TuiAccessorState;

    fn setup() -> (Lua, Arc<Mutex<dyn TuiAccessorPort>>) {
        let lua = Lua::new();
        let accessor: Arc<Mutex<dyn TuiAccessorPort>> =
            Arc::new(Mutex::new(TuiAccessorState::with_default_routes()));
        (lua, accessor)
    }

    fn register(lua: &Lua, accessor: Arc<Mutex<dyn TuiAccessorPort>>) -> LuaResult<()> {
        let quorum = lua.create_table()?;
        let event_bus = Arc::new(Mutex::new(EventBus::new()));
        register_tui_api(lua, &quorum, accessor, event_bus)?;
        lua.globals().set("quorum", quorum)?;
        Ok(())
    }

    // -- Routes tests --

    #[test]
    fn test_routes_get_default() {
        let (lua, accessor) = setup();
        register(&lua, accessor).unwrap();

        let result: String = lua
            .load(r#"quorum.tui.routes.get("conversation")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "main_pane");
    }

    #[test]
    fn test_routes_set_and_get() {
        let (lua, accessor) = setup();
        register(&lua, accessor).unwrap();

        lua.load(r#"quorum.tui.routes.set("progress", "main_pane")"#)
            .exec()
            .unwrap();

        let result: String = lua
            .load(r#"quorum.tui.routes.get("progress")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "main_pane");
    }

    #[test]
    fn test_routes_set_invalid_content() {
        let (lua, accessor) = setup();
        register(&lua, accessor).unwrap();

        let result = lua
            .load(r#"quorum.tui.routes.set("invalid_slot", "sidebar")"#)
            .exec();
        assert!(result.is_err());
    }

    #[test]
    fn test_routes_set_invalid_surface() {
        let (lua, accessor) = setup();
        register(&lua, accessor).unwrap();

        let result = lua
            .load(r#"quorum.tui.routes.set("conversation", "nonexistent")"#)
            .exec();
        assert!(result.is_err());
    }

    #[test]
    fn test_routes_list() {
        let (lua, accessor) = setup();
        register(&lua, accessor).unwrap();

        let count: i64 = lua
            .load(
                r#"
                local entries = quorum.tui.routes.list()
                return #entries
            "#,
            )
            .eval()
            .unwrap();
        assert!(count >= 5); // default routes
    }

    #[test]
    fn test_routes_set_dynamic_names() {
        let (lua, accessor) = setup();
        register(&lua, accessor).unwrap();

        lua.load(r#"quorum.tui.routes.set("model_stream:claude", "dynamic_pane:claude")"#)
            .exec()
            .unwrap();

        let result: String = lua
            .load(r#"quorum.tui.routes.get("model_stream:claude")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "dynamic_pane:claude");
    }

    // -- Layout tests --

    #[test]
    fn test_layout_current_default() {
        let (lua, accessor) = setup();
        register(&lua, accessor).unwrap();

        let result: String = lua.load(r#"quorum.tui.layout.current()"#).eval().unwrap();
        assert_eq!(result, "default");
    }

    #[test]
    fn test_layout_switch() {
        let (lua, accessor) = setup();
        register(&lua, accessor).unwrap();

        lua.load(r#"quorum.tui.layout.switch("wide")"#)
            .exec()
            .unwrap();

        let result: String = lua.load(r#"quorum.tui.layout.current()"#).eval().unwrap();
        assert_eq!(result, "wide");
    }

    #[test]
    fn test_layout_switch_unknown_fails() {
        let (lua, accessor) = setup();
        register(&lua, accessor).unwrap();

        let result = lua
            .load(r#"quorum.tui.layout.switch("nonexistent")"#)
            .exec();
        assert!(result.is_err());
    }

    #[test]
    fn test_layout_register_and_switch() {
        let (lua, accessor) = setup();
        register(&lua, accessor).unwrap();

        lua.load(
            r#"
            quorum.tui.layout.register_preset("triple", {
                splits = {40, 30, 30},
                direction = "horizontal",
            })
            quorum.tui.layout.switch("triple")
        "#,
        )
        .exec()
        .unwrap();

        let result: String = lua.load(r#"quorum.tui.layout.current()"#).eval().unwrap();
        assert_eq!(result, "triple");
    }

    #[test]
    fn test_layout_presets_includes_custom() {
        let (lua, accessor) = setup();
        register(&lua, accessor).unwrap();

        lua.load(
            r#"
            quorum.tui.layout.register_preset("custom1", {
                splits = {50, 50},
                direction = "vertical",
            })
        "#,
        )
        .exec()
        .unwrap();

        let has_custom: bool = lua
            .load(
                r#"
                local presets = quorum.tui.layout.presets()
                for _, name in ipairs(presets) do
                    if name == "custom1" then return true end
                end
                return false
            "#,
            )
            .eval()
            .unwrap();
        assert!(has_custom);
    }

    #[test]
    fn test_layout_register_builtin_name_fails() {
        let (lua, accessor) = setup();
        register(&lua, accessor).unwrap();

        let result = lua
            .load(
                r#"
            quorum.tui.layout.register_preset("default", {
                splits = {100},
                direction = "horizontal",
            })
        "#,
            )
            .exec();
        assert!(result.is_err());
    }

    // -- Content tests --

    #[test]
    fn test_content_register_and_set_text() {
        let (lua, accessor) = setup();
        register(&lua, Arc::clone(&accessor)).unwrap();

        lua.load(
            r#"
            quorum.tui.content.register("my_panel")
            quorum.tui.content.set_text("my_panel", "Hello from Lua!")
        "#,
        )
        .exec()
        .unwrap();

        // Verify pending changes
        let mut guard = accessor.lock().unwrap();
        let changes = guard.take_pending_changes();
        assert_eq!(changes.new_content_slots, vec!["my_panel".to_string()]);
        assert_eq!(
            changes.content_text_updates,
            vec![("my_panel".to_string(), "Hello from Lua!".to_string())]
        );
    }

    #[test]
    fn test_content_set_text_unregistered_fails() {
        let (lua, accessor) = setup();
        register(&lua, accessor).unwrap();

        let result = lua
            .load(r#"quorum.tui.content.set_text("nonexistent", "text")"#)
            .exec();
        assert!(result.is_err());
    }

    #[test]
    fn test_content_register_duplicate_fails() {
        let (lua, accessor) = setup();
        register(&lua, accessor).unwrap();

        let result = lua
            .load(
                r#"
            quorum.tui.content.register("panel1")
            quorum.tui.content.register("panel1")
        "#,
            )
            .exec();
        assert!(result.is_err());
    }

    #[test]
    fn test_content_slots() {
        let (lua, accessor) = setup();
        register(&lua, accessor).unwrap();

        lua.load(
            r#"
            quorum.tui.content.register("a")
            quorum.tui.content.register("b")
        "#,
        )
        .exec()
        .unwrap();

        let count: i64 = lua
            .load(r#"return #quorum.tui.content.slots()"#)
            .eval()
            .unwrap();
        assert_eq!(count, 2);
    }

    // -- Event tests --

    #[test]
    fn test_layout_switch_fires_layout_changed_event() {
        let lua = Lua::new();
        let accessor: Arc<Mutex<dyn TuiAccessorPort>> =
            Arc::new(Mutex::new(TuiAccessorState::with_default_routes()));
        let event_bus = Arc::new(Mutex::new(EventBus::new()));

        // Register listener
        let callback = lua
            .load(
                r#"
                function(data)
                    _G.layout_preset = data.preset
                end
            "#,
            )
            .eval::<LuaFunction>()
            .unwrap();
        let key = lua.create_registry_value(callback).unwrap();
        event_bus.lock().unwrap().register("LayoutChanged", key);

        let quorum = lua.create_table().unwrap();
        register_tui_api(&lua, &quorum, accessor, event_bus).unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        lua.load(r#"quorum.tui.layout.switch("wide")"#)
            .exec()
            .unwrap();

        let preset: String = lua.globals().get("layout_preset").unwrap();
        assert_eq!(preset, "wide");
    }

    #[test]
    fn test_content_register_fires_pane_created_event() {
        let lua = Lua::new();
        let accessor: Arc<Mutex<dyn TuiAccessorPort>> =
            Arc::new(Mutex::new(TuiAccessorState::with_default_routes()));
        let event_bus = Arc::new(Mutex::new(EventBus::new()));

        // Register listener
        let callback = lua
            .load(
                r#"
                function(data)
                    _G.created_slot = data.slot
                end
            "#,
            )
            .eval::<LuaFunction>()
            .unwrap();
        let key = lua.create_registry_value(callback).unwrap();
        event_bus.lock().unwrap().register("PaneCreated", key);

        let quorum = lua.create_table().unwrap();
        register_tui_api(&lua, &quorum, accessor, event_bus).unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        lua.load(r#"quorum.tui.content.register("status_widget")"#)
            .exec()
            .unwrap();

        let slot: String = lua.globals().get("created_slot").unwrap();
        assert_eq!(slot, "status_widget");
    }

    // -- Pending changes integration --

    #[test]
    fn test_route_change_appears_in_pending() {
        let (lua, accessor) = setup();
        register(&lua, Arc::clone(&accessor)).unwrap();

        lua.load(r#"quorum.tui.routes.set("progress", "main_pane")"#)
            .exec()
            .unwrap();

        let mut guard = accessor.lock().unwrap();
        let changes = guard.take_pending_changes();
        assert_eq!(changes.route_changes.len(), 1);
        assert_eq!(
            changes.route_changes[0],
            ("progress".to_string(), "main_pane".to_string())
        );
    }

    #[test]
    fn test_layout_switch_appears_in_pending() {
        let (lua, accessor) = setup();
        register(&lua, Arc::clone(&accessor)).unwrap();

        lua.load(r#"quorum.tui.layout.switch("minimal")"#)
            .exec()
            .unwrap();

        let mut guard = accessor.lock().unwrap();
        let changes = guard.take_pending_changes();
        assert_eq!(changes.preset_switch, Some("minimal".to_string()));
    }
}
