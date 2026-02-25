//! Event callback registry for `quorum.on(event_name, callback)`.
//!
//! Stores Lua callbacks as `LuaRegistryKey` references, keyed by event name.
//! Callbacks execute synchronously in registration order.
//! For cancellable events, a callback returning `false` stops further processing.

use mlua::prelude::*;
use std::collections::HashMap;

/// Registry of Lua callbacks organized by event name.
pub struct EventBus {
    /// event_name → list of Lua callback registry keys (in registration order)
    listeners: HashMap<String, Vec<LuaRegistryKey>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            listeners: HashMap::new(),
        }
    }

    /// Register a Lua callback for the given event name.
    pub fn register(&mut self, event_name: &str, key: LuaRegistryKey) {
        self.listeners
            .entry(event_name.to_string())
            .or_default()
            .push(key);
    }

    /// Get the registry keys for listeners of the given event.
    pub fn listeners(&self, event_name: &str) -> &[LuaRegistryKey] {
        self.listeners
            .get(event_name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Clear all registered listeners (used on reload).
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.listeners.clear();
    }

    /// Fire an event, calling all listeners with the given data table.
    ///
    /// For cancellable events, if any callback returns `false`, this returns
    /// `Ok(false)` immediately without calling remaining listeners.
    /// Non-cancellable events ignore callback return values.
    ///
    /// Returns `Ok(true)` if all listeners ran (or event was not cancelled).
    pub fn fire(
        &self,
        lua: &Lua,
        event_name: &str,
        data: &LuaTable,
        cancellable: bool,
    ) -> LuaResult<bool> {
        let keys = self.listeners(event_name);
        for key in keys {
            let callback: LuaFunction = lua.registry_value(key)?;
            let result: LuaValue = callback.call(data.clone())?;

            if cancellable && let LuaValue::Boolean(false) = result {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_bus_register_and_fire() {
        let lua = Lua::new();
        let mut bus = EventBus::new();

        // Register a callback that sets a global
        let func = lua
            .load(
                r#"
            function(data)
                _G.test_fired = true
                _G.test_key = data.key
            end
        "#,
            )
            .eval::<LuaFunction>()
            .unwrap();

        let key = lua.create_registry_value(func).unwrap();
        bus.register("ConfigChanged", key);

        assert_eq!(bus.listeners("ConfigChanged").len(), 1);
        assert_eq!(bus.listeners("Unknown").len(), 0);

        // Fire the event
        let data = lua.create_table().unwrap();
        data.set("key", "agent.strategy").unwrap();

        let result = bus.fire(&lua, "ConfigChanged", &data, false).unwrap();
        assert!(result);

        // Verify the callback ran
        let fired: bool = lua.globals().get("test_fired").unwrap();
        assert!(fired);
        let key_val: String = lua.globals().get("test_key").unwrap();
        assert_eq!(key_val, "agent.strategy");
    }

    #[test]
    fn test_event_bus_cancellation() {
        let lua = Lua::new();
        let mut bus = EventBus::new();

        // First callback returns false (cancel)
        let cancel_func = lua
            .load("function(data) return false end")
            .eval::<LuaFunction>()
            .unwrap();
        let key1 = lua.create_registry_value(cancel_func).unwrap();
        bus.register("ScriptLoading", key1);

        // Second callback should NOT run
        let second_func = lua
            .load("function(data) _G.should_not_run = true end")
            .eval::<LuaFunction>()
            .unwrap();
        let key2 = lua.create_registry_value(second_func).unwrap();
        bus.register("ScriptLoading", key2);

        let data = lua.create_table().unwrap();
        let result = bus.fire(&lua, "ScriptLoading", &data, true).unwrap();
        assert!(!result); // Cancelled

        // Verify second callback did NOT run
        let ran: LuaValue = lua.globals().get("should_not_run").unwrap();
        assert_eq!(ran, LuaValue::Nil);
    }

    #[test]
    fn test_event_bus_non_cancellable_ignores_false() {
        let lua = Lua::new();
        let mut bus = EventBus::new();

        // Callback returns false, but event is not cancellable
        let func = lua
            .load(
                r#"
            function(data)
                _G.ran = true
                return false
            end
        "#,
            )
            .eval::<LuaFunction>()
            .unwrap();
        let key = lua.create_registry_value(func).unwrap();
        bus.register("ConfigChanged", key);

        let data = lua.create_table().unwrap();
        let result = bus.fire(&lua, "ConfigChanged", &data, false).unwrap();
        assert!(result); // NOT cancelled because event is non-cancellable
    }

    #[test]
    fn test_event_bus_clear() {
        let lua = Lua::new();
        let mut bus = EventBus::new();

        let func = lua
            .load("function() end")
            .eval::<LuaFunction>()
            .unwrap();
        let key = lua.create_registry_value(func).unwrap();
        bus.register("Test", key);

        assert_eq!(bus.listeners("Test").len(), 1);
        bus.clear();
        assert_eq!(bus.listeners("Test").len(), 0);
    }
}
