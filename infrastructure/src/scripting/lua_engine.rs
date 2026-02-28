//! Main Lua scripting engine — ties together EventBus, Sandbox, Config API, Keymap API.
//!
//! `LuaScriptingEngine` implements `ScriptingEnginePort` from the application layer,
//! providing the concrete Lua 5.4 runtime backed by mlua.

use mlua::prelude::*;
use quorum_application::{
    ConfigAccessorPort, CustomToolDef, EventOutcome, KeymapAction, ScriptError,
    ScriptingEnginePort, TuiAccessorPort,
};
use quorum_domain::scripting::{ScriptEventData, ScriptEventType, ScriptValue};
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::command_api::{CommandRegistry, register_command_api};
use super::config_api::register_config_api;
use super::event_bus::EventBus;
use super::keymap_api::{KeymapBinding, KeymapRegistry, register_keymap_api};
use super::sandbox::apply_sandbox;
use super::tools_api::register_tools_api;
use super::tui_api::register_tui_api;

/// Lua 5.4 scripting engine implementing `ScriptingEnginePort`.
///
/// Owns the Lua VM and all associated registries (events, keymaps).
/// Thread-safe via internal `Mutex` wrapping of the Lua state.
pub struct LuaScriptingEngine {
    lua: Mutex<Lua>,
    event_bus: Arc<Mutex<EventBus>>,
    keymap_registry: Arc<Mutex<KeymapRegistry>>,
    command_registry: Arc<Mutex<CommandRegistry>>,
    callback_store: Arc<Mutex<Vec<(u64, LuaRegistryKey)>>>,
    pending_custom_tools: Arc<Mutex<Vec<CustomToolDef>>>,
    provider_config: Arc<Mutex<quorum_domain::ProviderConfig>>,
}

impl LuaScriptingEngine {
    /// Create a new Lua scripting engine with the given config and TUI accessors.
    ///
    /// Sets up the VM with:
    /// - Sandbox (C module blocking)
    /// - `quorum.on(event, callback)` event registration
    /// - `quorum.config.{get,set,keys}` + metatable proxy
    /// - `quorum.keymap.set(mode, key, action)` keybinding API
    /// - `quorum.tui.{routes,layout,content}` TUI manipulation API
    /// - `quorum.command.register(name, opts)` custom command registration
    pub fn new(
        config: Arc<Mutex<dyn ConfigAccessorPort>>,
        tui_accessor: Arc<Mutex<dyn TuiAccessorPort>>,
    ) -> Result<Self, ScriptError> {
        let lua = Lua::new();
        let event_bus = Arc::new(Mutex::new(EventBus::new()));
        let keymap_registry = Arc::new(Mutex::new(KeymapRegistry::new()));
        let command_registry = Arc::new(Mutex::new(CommandRegistry::new()));
        let callback_store: Arc<Mutex<Vec<(u64, LuaRegistryKey)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let pending_custom_tools: Arc<Mutex<Vec<CustomToolDef>>> = Arc::new(Mutex::new(Vec::new()));
        let provider_config: Arc<Mutex<quorum_domain::ProviderConfig>> =
            Arc::new(Mutex::new(quorum_domain::ProviderConfig::default()));

        // Apply sandbox
        apply_sandbox(&lua).map_err(|e| ScriptError {
            message: format!("sandbox setup failed: {}", e),
        })?;

        // Create quorum global table
        let quorum = lua.create_table().map_err(lua_to_script_error)?;

        // Register quorum.on(event_name, callback)
        {
            let bus = Arc::clone(&event_bus);
            let on_fn = lua
                .create_function(move |lua, (event_name, callback): (String, LuaFunction)| {
                    // Validate event name
                    if event_name.parse::<ScriptEventType>().is_err() {
                        return Err(LuaError::external(format!(
                            "unknown event: '{}'. Valid events: ScriptLoading, ScriptLoaded, ConfigChanged, ModeChanged, SessionStarted, PaneCreated, LayoutChanged, ToolCallBefore, ToolCallAfter, PhaseChanged, PlanCreated",
                            event_name
                        )));
                    }

                    let key = lua.create_registry_value(callback)?;
                    let mut bus = bus.lock().map_err(|e| {
                        LuaError::external(format!("event_bus lock poisoned: {}", e))
                    })?;
                    bus.register(&event_name, key);
                    Ok(())
                })
                .map_err(lua_to_script_error)?;
            quorum.set("on", on_fn).map_err(lua_to_script_error)?;
        }

        // Register quorum.config API
        register_config_api(&lua, &quorum, config, Arc::clone(&event_bus))
            .map_err(lua_to_script_error)?;

        // Register quorum.keymap API
        register_keymap_api(
            &lua,
            &quorum,
            Arc::clone(&keymap_registry),
            Arc::clone(&callback_store),
        )
        .map_err(lua_to_script_error)?;

        // Register quorum.tui API
        register_tui_api(&lua, &quorum, tui_accessor, Arc::clone(&event_bus))
            .map_err(lua_to_script_error)?;

        // Register quorum.command API
        register_command_api(
            &lua,
            &quorum,
            Arc::clone(&command_registry),
            Arc::clone(&callback_store),
        )
        .map_err(lua_to_script_error)?;

        // Register quorum.tools API
        register_tools_api(&lua, &quorum, Arc::clone(&pending_custom_tools))
            .map_err(lua_to_script_error)?;

        // Register quorum.providers API
        super::providers_api::register_providers_api(&lua, &quorum, Arc::clone(&provider_config))
            .map_err(lua_to_script_error)?;

        // Set quorum as global
        lua.globals()
            .set("quorum", quorum)
            .map_err(lua_to_script_error)?;

        Ok(Self {
            lua: Mutex::new(lua),
            event_bus,
            keymap_registry,
            command_registry,
            callback_store,
            pending_custom_tools,
            provider_config,
        })
    }

    /// Execute a Lua callback by its registry ID.
    ///
    /// Used by the presentation layer when a custom keymap with
    /// `KeyAction::LuaCallback(id)` is triggered.
    pub fn execute_callback(&self, callback_id: u64) -> Result<(), ScriptError> {
        let lua = self.lua.lock().map_err(|e| ScriptError {
            message: format!("lua lock poisoned: {}", e),
        })?;
        let store = self.callback_store.lock().map_err(|e| ScriptError {
            message: format!("callback store lock poisoned: {}", e),
        })?;

        let registry_key = store
            .iter()
            .find(|(id, _)| *id == callback_id)
            .map(|(_, key)| key);

        if let Some(key) = registry_key {
            let func: LuaFunction = lua.registry_value(key).map_err(lua_to_script_error)?;
            func.call::<()>(()).map_err(lua_to_script_error)?;
        } else {
            return Err(ScriptError {
                message: format!("callback not found: {}", callback_id),
            });
        }

        Ok(())
    }
}

impl ScriptingEnginePort for LuaScriptingEngine {
    fn emit_event(
        &self,
        event: ScriptEventType,
        data: ScriptEventData,
    ) -> Result<EventOutcome, ScriptError> {
        let lua = self.lua.lock().map_err(|e| ScriptError {
            message: format!("lua lock poisoned: {}", e),
        })?;
        let bus = self.event_bus.lock().map_err(|e| ScriptError {
            message: format!("event_bus lock poisoned: {}", e),
        })?;

        // Convert ScriptEventData to a Lua table
        let data_table = lua.create_table().map_err(lua_to_script_error)?;
        for (key, value) in data.fields() {
            match value {
                ScriptValue::String(s) => {
                    data_table
                        .set(key.as_str(), s.as_str())
                        .map_err(lua_to_script_error)?;
                }
                ScriptValue::Integer(n) => {
                    data_table
                        .set(key.as_str(), *n)
                        .map_err(lua_to_script_error)?;
                }
                ScriptValue::Boolean(b) => {
                    data_table
                        .set(key.as_str(), *b)
                        .map_err(lua_to_script_error)?;
                }
                ScriptValue::Nil => {
                    data_table
                        .set(key.as_str(), LuaValue::Nil)
                        .map_err(lua_to_script_error)?;
                }
            }
        }

        let continued = bus
            .fire(&lua, event.as_str(), &data_table, event.is_cancellable())
            .map_err(lua_to_script_error)?;

        if continued {
            Ok(EventOutcome::Continue)
        } else {
            Ok(EventOutcome::Cancelled)
        }
    }

    fn load_script(&self, path: &Path) -> Result<(), ScriptError> {
        let lua = self.lua.lock().map_err(|e| ScriptError {
            message: format!("lua lock poisoned: {}", e),
        })?;

        let content = std::fs::read_to_string(path).map_err(|e| ScriptError {
            message: format!("failed to read {}: {}", path.display(), e),
        })?;

        lua.load(&content)
            .set_name(path.to_string_lossy())
            .exec()
            .map_err(lua_to_script_error)?;

        Ok(())
    }

    fn is_available(&self) -> bool {
        true
    }

    fn registered_keymaps(&self) -> Vec<(String, String, KeymapAction)> {
        let registry = match self.keymap_registry.lock() {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };

        registry
            .entries()
            .iter()
            .map(|entry| {
                let action = match &entry.action {
                    KeymapBinding::Builtin(name) => KeymapAction::Builtin(name.clone()),
                    KeymapBinding::Callback(id) => KeymapAction::LuaCallback(*id),
                };
                (entry.mode.clone(), entry.key_descriptor.clone(), action)
            })
            .collect()
    }

    fn execute_callback(&self, callback_id: u64) -> Result<(), ScriptError> {
        self.execute_callback(callback_id)
    }

    fn on_tool_call_before(&self, tool_name: &str, args_json: &str) -> bool {
        let data = ScriptEventData::new()
            .with_field("tool_name", ScriptValue::String(tool_name.to_string()))
            .with_field("args", ScriptValue::String(args_json.to_string()));

        match self.emit_event(ScriptEventType::ToolCallBefore, data) {
            Ok(EventOutcome::Continue) => true,
            Ok(EventOutcome::Cancelled) => false,
            Err(_) => true, // On error, allow the tool call to proceed
        }
    }

    fn registered_commands(&self) -> Vec<(String, String, String, u64)> {
        let registry = match self.command_registry.lock() {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };

        registry
            .entries()
            .iter()
            .map(|entry| {
                (
                    entry.name.clone(),
                    entry.description.clone(),
                    entry.usage.clone(),
                    entry.callback_id,
                )
            })
            .collect()
    }

    fn execute_command_callback(&self, callback_id: u64, args: &str) -> Result<(), ScriptError> {
        let lua = self.lua.lock().map_err(|e| ScriptError {
            message: format!("lua lock poisoned: {}", e),
        })?;
        let store = self.callback_store.lock().map_err(|e| ScriptError {
            message: format!("callback store lock poisoned: {}", e),
        })?;

        let registry_key = store
            .iter()
            .find(|(id, _)| *id == callback_id)
            .map(|(_, key)| key);

        if let Some(key) = registry_key {
            let func: LuaFunction = lua.registry_value(key).map_err(lua_to_script_error)?;
            func.call::<()>(args.to_string())
                .map_err(lua_to_script_error)?;
        } else {
            return Err(ScriptError {
                message: format!("command callback not found: {}", callback_id),
            });
        }

        Ok(())
    }

    fn registered_custom_tools(&self) -> Vec<CustomToolDef> {
        self.pending_custom_tools
            .lock()
            .map(|tools| tools.clone())
            .unwrap_or_default()
    }

    fn provider_config(&self) -> Option<quorum_domain::ProviderConfig> {
        self.provider_config.lock().ok().map(|cfg| cfg.clone())
    }
}

/// Convert an mlua error to a ScriptError.
fn lua_to_script_error(e: LuaError) -> ScriptError {
    ScriptError {
        message: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quorum_application::{ConfigAccessError, ConfigValue, TuiAccessorState};
    use quorum_domain::ConfigIssue;

    struct TestConfig {
        data: std::collections::HashMap<String, ConfigValue>,
    }

    impl TestConfig {
        fn new() -> Self {
            let mut data = std::collections::HashMap::new();
            data.insert(
                "agent.strategy".to_string(),
                ConfigValue::String("quorum".to_string()),
            );
            data.insert(
                "agent.consensus_level".to_string(),
                ConfigValue::String("solo".to_string()),
            );
            Self { data }
        }
    }

    impl ConfigAccessorPort for TestConfig {
        fn config_get(&self, key: &str) -> Result<ConfigValue, ConfigAccessError> {
            self.data
                .get(key)
                .cloned()
                .ok_or(ConfigAccessError::UnknownKey {
                    key: key.to_string(),
                })
        }

        fn config_set(
            &mut self,
            key: &str,
            value: ConfigValue,
        ) -> Result<Vec<ConfigIssue>, ConfigAccessError> {
            if !self.data.contains_key(key) {
                return Err(ConfigAccessError::UnknownKey {
                    key: key.to_string(),
                });
            }
            self.data.insert(key.to_string(), value);
            Ok(vec![])
        }

        fn config_keys(&self) -> Vec<String> {
            let mut keys: Vec<_> = self.data.keys().cloned().collect();
            keys.sort();
            keys
        }
    }

    fn make_engine() -> LuaScriptingEngine {
        let config: Arc<Mutex<dyn ConfigAccessorPort>> = Arc::new(Mutex::new(TestConfig::new()));
        let tui: Arc<Mutex<dyn TuiAccessorPort>> =
            Arc::new(Mutex::new(TuiAccessorState::with_default_routes()));
        LuaScriptingEngine::new(config, tui).unwrap()
    }

    #[test]
    fn test_engine_is_available() {
        let engine = make_engine();
        assert!(engine.is_available());
    }

    #[test]
    fn test_engine_emit_event() {
        let engine = make_engine();
        let result = engine
            .emit_event(ScriptEventType::SessionStarted, ScriptEventData::new())
            .unwrap();
        assert_eq!(result, EventOutcome::Continue);
    }

    #[test]
    fn test_engine_load_script() {
        let engine = make_engine();

        // Create a temp script
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("test.lua");
        std::fs::write(
            &script_path,
            r#"quorum.config.set("agent.strategy", "debate")"#,
        )
        .unwrap();

        engine.load_script(&script_path).unwrap();

        // Verify the config was changed
        let lua = engine.lua.lock().unwrap();
        let result: String = lua
            .load(r#"quorum.config.get("agent.strategy")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "debate");
    }

    #[test]
    fn test_engine_load_nonexistent_script() {
        let engine = make_engine();
        let result = engine.load_script(Path::new("/nonexistent/init.lua"));
        assert!(result.is_err());
    }

    #[test]
    fn test_engine_event_with_listener() {
        let engine = make_engine();

        // Register a listener via script
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("test.lua");
        std::fs::write(
            &script_path,
            r#"
            quorum.on("SessionStarted", function(data)
                _G.session_mode = data.mode
            end)
        "#,
        )
        .unwrap();
        engine.load_script(&script_path).unwrap();

        // Fire the event
        let data = ScriptEventData::new().with_field("mode", ScriptValue::String("solo".into()));
        let result = engine
            .emit_event(ScriptEventType::SessionStarted, data)
            .unwrap();
        assert_eq!(result, EventOutcome::Continue);

        // Check the global was set
        let lua = engine.lua.lock().unwrap();
        let mode: String = lua.globals().get("session_mode").unwrap();
        assert_eq!(mode, "solo");
    }

    #[test]
    fn test_engine_cancellable_event() {
        let engine = make_engine();

        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("test.lua");
        std::fs::write(
            &script_path,
            r#"
            quorum.on("ScriptLoading", function(data)
                return false
            end)
        "#,
        )
        .unwrap();
        engine.load_script(&script_path).unwrap();

        let data = ScriptEventData::new().with_field("path", ScriptValue::String("/test".into()));
        let result = engine
            .emit_event(ScriptEventType::ScriptLoading, data)
            .unwrap();
        assert_eq!(result, EventOutcome::Cancelled);
    }

    #[test]
    fn test_engine_keymap_registration() {
        let engine = make_engine();

        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("test.lua");
        std::fs::write(
            &script_path,
            r#"
            quorum.keymap.set("normal", "Ctrl+s", "submit_input")
            quorum.keymap.set("normal", "Ctrl+p", function()
                quorum.config.set("agent.strategy", "debate")
            end)
        "#,
        )
        .unwrap();
        engine.load_script(&script_path).unwrap();

        let keymaps = engine.registered_keymaps();
        assert_eq!(keymaps.len(), 2);

        // First: builtin
        assert_eq!(keymaps[0].0, "normal");
        assert_eq!(keymaps[0].1, "Ctrl+s");
        assert_eq!(keymaps[0].2, KeymapAction::Builtin("submit_input".into()));

        // Second: callback
        assert_eq!(keymaps[1].0, "normal");
        assert_eq!(keymaps[1].1, "Ctrl+p");
        assert!(matches!(keymaps[1].2, KeymapAction::LuaCallback(_)));
    }

    #[test]
    fn test_engine_execute_callback() {
        let engine = make_engine();

        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("test.lua");
        std::fs::write(
            &script_path,
            r#"
            quorum.keymap.set("normal", "Ctrl+p", function()
                quorum.config.set("agent.strategy", "debate")
            end)
        "#,
        )
        .unwrap();
        engine.load_script(&script_path).unwrap();

        // Get the callback ID
        let keymaps = engine.registered_keymaps();
        let callback_id = match &keymaps[0].2 {
            KeymapAction::LuaCallback(id) => *id,
            _ => panic!("expected lua callback"),
        };

        // Execute it
        engine.execute_callback(callback_id).unwrap();

        // Verify config changed
        let lua = engine.lua.lock().unwrap();
        let result: String = lua
            .load(r#"quorum.config.get("agent.strategy")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "debate");
    }

    #[test]
    fn test_engine_sandbox_active() {
        let engine = make_engine();

        // Verify sandbox is applied - package.loadlib should be nil
        let lua = engine.lua.lock().unwrap();
        let result: LuaValue = lua
            .globals()
            .get::<LuaTable>("package")
            .unwrap()
            .get("loadlib")
            .unwrap();
        assert_eq!(result, LuaValue::Nil);
    }

    #[test]
    fn test_engine_script_syntax_error() {
        let engine = make_engine();

        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("bad.lua");
        std::fs::write(&script_path, "this is not valid lua {{{{").unwrap();

        let result = engine.load_script(&script_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("bad.lua"));
    }

    #[test]
    fn test_plugins_load_in_alphabetical_order() {
        let engine = make_engine();

        // Create a plugins directory with numbered lua files
        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join("plugins");
        std::fs::create_dir(&plugins_dir).unwrap();

        // Write plugins that append to a global list (intentionally out of order)
        std::fs::write(
            plugins_dir.join("02_second.lua"),
            "_G.load_order = (_G.load_order or '') .. 'second,'",
        )
        .unwrap();
        std::fs::write(
            plugins_dir.join("01_first.lua"),
            "_G.load_order = (_G.load_order or '') .. 'first,'",
        )
        .unwrap();
        std::fs::write(
            plugins_dir.join("03_third.lua"),
            "_G.load_order = (_G.load_order or '') .. 'third,'",
        )
        .unwrap();
        // Non-lua file should be ignored
        std::fs::write(plugins_dir.join("README.md"), "# Plugins").unwrap();

        // Load plugins in sorted order (simulating main.rs logic)
        let mut plugin_files: Vec<std::path::PathBuf> = std::fs::read_dir(&plugins_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "lua"))
            .collect();
        plugin_files.sort();
        for path in &plugin_files {
            engine.load_script(path).unwrap();
        }

        // Verify load order
        let lua = engine.lua.lock().unwrap();
        let order: String = lua.globals().get("load_order").unwrap();
        assert_eq!(order, "first,second,third,");
    }

    #[test]
    fn test_plugin_failure_does_not_block_others() {
        let engine = make_engine();

        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join("plugins");
        std::fs::create_dir(&plugins_dir).unwrap();

        std::fs::write(
            plugins_dir.join("01_good.lua"),
            "_G.loaded_plugins = (_G.loaded_plugins or '') .. 'good,'",
        )
        .unwrap();
        std::fs::write(plugins_dir.join("02_bad.lua"), "this is invalid lua {{{{").unwrap();
        std::fs::write(
            plugins_dir.join("03_also_good.lua"),
            "_G.loaded_plugins = (_G.loaded_plugins or '') .. 'also_good,'",
        )
        .unwrap();

        let mut plugin_files: Vec<std::path::PathBuf> = std::fs::read_dir(&plugins_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "lua"))
            .collect();
        plugin_files.sort();

        // Load each plugin, skipping failures (simulating main.rs behavior)
        for path in &plugin_files {
            let _ = engine.load_script(path); // Errors are logged, not propagated
        }

        // Good plugins should still have loaded
        let lua = engine.lua.lock().unwrap();
        let loaded: Option<String> = lua.globals().get("loaded_plugins").unwrap();
        assert_eq!(loaded.unwrap(), "good,also_good,");
    }
}
