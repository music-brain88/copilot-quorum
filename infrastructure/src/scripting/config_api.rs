//! `quorum.config` Lua API — get/set/keys with metatable proxy.
//!
//! Bridges the Lua scripting layer to `ConfigAccessorPort` for runtime
//! config access. Supports both function-style and metatable-style access:
//!
//! ```lua
//! quorum.config.get("agent.strategy")        -- function style
//! quorum.config["agent.strategy"]             -- metatable shortcut
//! quorum.config.set("agent.strategy", "debate")
//! quorum.config["agent.strategy"] = "debate"  -- metatable shortcut
//! quorum.config.keys()                        -- list all keys
//! ```

use mlua::prelude::*;
use quorum_application::{ConfigAccessorPort, ConfigValue};
use std::sync::{Arc, Mutex};

use super::event_bus::EventBus;

/// Register the `quorum.config` table on the given `quorum` global.
///
/// The config table uses a metatable with `__index` and `__newindex`
/// to support both `quorum.config.get(k)` and `quorum.config[k]` syntax.
pub fn register_config_api(
    lua: &Lua,
    quorum: &LuaTable,
    config: Arc<Mutex<dyn ConfigAccessorPort>>,
    event_bus: Arc<Mutex<EventBus>>,
) -> LuaResult<()> {
    let config_table = lua.create_table()?;

    // quorum.config.get(key) -> value
    {
        let config = Arc::clone(&config);
        let get_fn = lua.create_function(move |lua, key: String| {
            let guard = config.lock().map_err(|e| {
                LuaError::external(format!("config lock poisoned: {}", e))
            })?;
            match guard.config_get(&key) {
                Ok(value) => push_config_value(lua, value),
                Err(e) => Err(LuaError::external(e.to_string())),
            }
        })?;
        config_table.set("get", get_fn)?;
    }

    // quorum.config.set(key, value) -> nil (fires ConfigChanged event)
    {
        let config = Arc::clone(&config);
        let event_bus = Arc::clone(&event_bus);
        let set_fn = lua.create_function(move |lua, (key, value): (String, LuaValue)| {
            let lua_value = lua_to_config_value(value)?;

            // Get old value for event data
            let old_value = {
                let guard = config.lock().map_err(|e| {
                    LuaError::external(format!("config lock poisoned: {}", e))
                })?;
                guard.config_get(&key).ok()
            };

            // Set new value
            {
                let mut guard = config.lock().map_err(|e| {
                    LuaError::external(format!("config lock poisoned: {}", e))
                })?;
                guard.config_set(&key, lua_value.clone()).map_err(|e| {
                    LuaError::external(e.to_string())
                })?;
            }

            // Fire ConfigChanged event
            let data = lua.create_table()?;
            data.set("key", key.clone())?;
            if let Some(old) = old_value {
                data.set("old_value", push_config_value(lua, old)?)?;
            }
            data.set("new_value", push_config_value(lua, lua_value)?)?;

            let bus = event_bus.lock().map_err(|e| {
                LuaError::external(format!("event_bus lock poisoned: {}", e))
            })?;
            // ConfigChanged is not cancellable, ignore result
            let _ = bus.fire(lua, "ConfigChanged", &data, false);

            Ok(())
        })?;
        config_table.set("set", set_fn)?;
    }

    // quorum.config.keys() -> table of key strings
    {
        let config = Arc::clone(&config);
        let keys_fn = lua.create_function(move |lua, ()| {
            let guard = config.lock().map_err(|e| {
                LuaError::external(format!("config lock poisoned: {}", e))
            })?;
            let keys = guard.config_keys();
            let table = lua.create_table()?;
            for (i, key) in keys.iter().enumerate() {
                table.set(i + 1, key.as_str())?;
            }
            Ok(table)
        })?;
        config_table.set("keys", keys_fn)?;
    }

    // Metatable for config[key] / config[key] = value shortcut
    let meta = lua.create_table()?;

    // __index: config["agent.strategy"] → config.get("agent.strategy")
    {
        let config = Arc::clone(&config);
        let index_fn = lua.create_function(move |lua, (_table, key): (LuaTable, String)| {
            // Skip method names so config.get/set/keys still work
            if key == "get" || key == "set" || key == "keys" {
                return Ok(LuaValue::Nil);
            }
            let guard = config.lock().map_err(|e| {
                LuaError::external(format!("config lock poisoned: {}", e))
            })?;
            match guard.config_get(&key) {
                Ok(value) => push_config_value(lua, value),
                Err(_) => Ok(LuaValue::Nil),
            }
        })?;
        meta.set("__index", index_fn)?;
    }

    // __newindex: config["agent.strategy"] = "debate" → config.set(...)
    {
        let config_for_newindex = Arc::clone(&config);
        let event_bus_for_newindex = Arc::clone(&event_bus);
        let newindex_fn =
            lua.create_function(move |lua, (_table, key, value): (LuaTable, String, LuaValue)| {
                let lua_value = lua_to_config_value(value)?;

                let old_value = {
                    let guard = config_for_newindex.lock().map_err(|e| {
                        LuaError::external(format!("config lock poisoned: {}", e))
                    })?;
                    guard.config_get(&key).ok()
                };

                {
                    let mut guard = config_for_newindex.lock().map_err(|e| {
                        LuaError::external(format!("config lock poisoned: {}", e))
                    })?;
                    guard
                        .config_set(&key, lua_value.clone())
                        .map_err(|e| LuaError::external(e.to_string()))?;
                }

                // Fire ConfigChanged event
                let data = lua.create_table()?;
                data.set("key", key.clone())?;
                if let Some(old) = old_value {
                    data.set("old_value", push_config_value(lua, old)?)?;
                }
                data.set("new_value", push_config_value(lua, lua_value)?)?;

                let bus = event_bus_for_newindex.lock().map_err(|e| {
                    LuaError::external(format!("event_bus lock poisoned: {}", e))
                })?;
                let _ = bus.fire(lua, "ConfigChanged", &data, false);

                Ok(())
            })?;
        meta.set("__newindex", newindex_fn)?;
    }

    config_table.set_metatable(Some(meta));
    quorum.set("config", config_table)?;

    Ok(())
}

/// Convert a `ConfigValue` into a Lua value within a Lua context.
pub(crate) fn push_config_value(lua: &Lua, value: ConfigValue) -> LuaResult<LuaValue> {
    match value {
        ConfigValue::String(s) => Ok(LuaValue::String(lua.create_string(&s)?)),
        ConfigValue::Integer(n) => Ok(LuaValue::Integer(n)),
        ConfigValue::Boolean(b) => Ok(LuaValue::Boolean(b)),
        ConfigValue::StringList(list) => {
            let table = lua.create_table()?;
            for (i, s) in list.iter().enumerate() {
                table.set(i + 1, s.as_str())?;
            }
            Ok(LuaValue::Table(table))
        }
    }
}

/// Convert a Lua value to `ConfigValue`.
fn lua_to_config_value(value: LuaValue) -> LuaResult<ConfigValue> {
    match value {
        LuaValue::String(s) => Ok(ConfigValue::String(s.to_str()?.to_string())),
        LuaValue::Integer(n) => Ok(ConfigValue::Integer(n)),
        LuaValue::Boolean(b) => Ok(ConfigValue::Boolean(b)),
        LuaValue::Number(n) => Ok(ConfigValue::Integer(n as i64)),
        other => Err(LuaError::external(format!(
            "unsupported config value type: {:?}",
            other
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quorum_application::ConfigAccessError;
    use quorum_domain::ConfigIssue;

    /// Test-only ConfigAccessor that stores values in a HashMap.
    struct MockConfig {
        data: std::collections::HashMap<String, ConfigValue>,
    }

    impl MockConfig {
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

    impl ConfigAccessorPort for MockConfig {
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

    #[test]
    fn test_config_get_function() {
        let lua = Lua::new();
        let quorum = lua.create_table().unwrap();
        let config: Arc<Mutex<dyn ConfigAccessorPort>> =
            Arc::new(Mutex::new(MockConfig::new()));
        let event_bus = Arc::new(Mutex::new(EventBus::new()));

        register_config_api(&lua, &quorum, config, event_bus).unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        let result: String = lua
            .load(r#"quorum.config.get("agent.strategy")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "quorum");
    }

    #[test]
    fn test_config_set_function() {
        let lua = Lua::new();
        let quorum = lua.create_table().unwrap();
        let config: Arc<Mutex<dyn ConfigAccessorPort>> =
            Arc::new(Mutex::new(MockConfig::new()));
        let event_bus = Arc::new(Mutex::new(EventBus::new()));

        register_config_api(&lua, &quorum, config, event_bus).unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        lua.load(r#"quorum.config.set("agent.strategy", "debate")"#)
            .exec()
            .unwrap();

        let result: String = lua
            .load(r#"quorum.config.get("agent.strategy")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "debate");
    }

    #[test]
    fn test_config_keys_function() {
        let lua = Lua::new();
        let quorum = lua.create_table().unwrap();
        let config: Arc<Mutex<dyn ConfigAccessorPort>> =
            Arc::new(Mutex::new(MockConfig::new()));
        let event_bus = Arc::new(Mutex::new(EventBus::new()));

        register_config_api(&lua, &quorum, config, event_bus).unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        let keys: Vec<String> = lua
            .load(
                r#"
            local k = quorum.config.keys()
            local result = {}
            for i = 1, #k do
                result[i] = k[i]
            end
            return result
        "#,
            )
            .eval::<LuaTable>()
            .unwrap()
            .sequence_values::<String>()
            .collect::<LuaResult<Vec<_>>>()
            .unwrap();

        assert!(keys.contains(&"agent.strategy".to_string()));
        assert!(keys.contains(&"agent.consensus_level".to_string()));
    }

    #[test]
    fn test_config_set_fires_event() {
        let lua = Lua::new();
        let quorum = lua.create_table().unwrap();
        let config: Arc<Mutex<dyn ConfigAccessorPort>> =
            Arc::new(Mutex::new(MockConfig::new()));
        let event_bus = Arc::new(Mutex::new(EventBus::new()));

        // Register a listener for ConfigChanged
        let callback = lua
            .load(
                r#"
            function(data)
                _G.changed_key = data.key
                _G.changed_new = data.new_value
            end
        "#,
            )
            .eval::<LuaFunction>()
            .unwrap();
        let key = lua.create_registry_value(callback).unwrap();
        event_bus.lock().unwrap().register("ConfigChanged", key);

        register_config_api(&lua, &quorum, config, event_bus).unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        lua.load(r#"quorum.config.set("agent.strategy", "debate")"#)
            .exec()
            .unwrap();

        let changed_key: String = lua.globals().get("changed_key").unwrap();
        assert_eq!(changed_key, "agent.strategy");
        let changed_new: String = lua.globals().get("changed_new").unwrap();
        assert_eq!(changed_new, "debate");
    }
}
