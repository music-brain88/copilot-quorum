//! `quorum.keymap` Lua API — custom keybinding registration.
//!
//! Allows users to remap keys or bind Lua callbacks to key combinations:
//!
//! ```lua
//! -- Remap to a built-in action
//! quorum.keymap.set("normal", "Ctrl+s", "submit_input")
//!
//! -- Bind a Lua callback
//! quorum.keymap.set("normal", "Ctrl+p", function()
//!     quorum.config.set("agent.strategy", "debate")
//! end)
//! ```
//!
//! Key descriptors are stored as strings. The presentation layer is
//! responsible for parsing them into crossterm `(KeyCode, KeyModifiers)`.

use mlua::prelude::*;
use std::sync::{Arc, Mutex};

/// A registered keymap entry from Lua (string-based, no crossterm dependency).
#[derive(Debug, Clone)]
pub struct KeymapEntry {
    pub mode: String,
    pub key_descriptor: String,
    pub action: KeymapBinding,
}

/// What happens when a custom-bound key is pressed.
#[derive(Debug, Clone)]
pub enum KeymapBinding {
    /// Maps to a built-in KeyAction by name.
    Builtin(String),
    /// Invokes a Lua callback stored in the registry.
    Callback(u64),
}

/// Storage for custom keymaps registered from Lua.
pub struct KeymapRegistry {
    entries: Vec<KeymapEntry>,
    next_callback_id: u64,
}

impl KeymapRegistry {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_callback_id: 1,
        }
    }

    pub fn register(&mut self, entry: KeymapEntry) {
        // Remove any existing binding for the same mode + key descriptor
        self.entries
            .retain(|e| !(e.mode == entry.mode && e.key_descriptor == entry.key_descriptor));
        self.entries.push(entry);
    }

    pub fn next_id(&mut self) -> u64 {
        let id = self.next_callback_id;
        self.next_callback_id += 1;
        id
    }

    pub fn entries(&self) -> &[KeymapEntry] {
        &self.entries
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Register the `quorum.keymap` table on the given `quorum` global.
pub fn register_keymap_api(
    lua: &Lua,
    quorum: &LuaTable,
    registry: Arc<Mutex<KeymapRegistry>>,
    callback_store: Arc<Mutex<Vec<(u64, LuaRegistryKey)>>>,
) -> LuaResult<()> {
    let keymap_table = lua.create_table()?;

    // quorum.keymap.set(mode, key_descriptor, action_or_callback)
    let reg = Arc::clone(&registry);
    let store = Arc::clone(&callback_store);
    let set_fn = lua.create_function(
        move |lua, (mode, key_desc, action): (String, String, LuaValue)| {
            // Basic validation of key descriptor format
            if key_desc.is_empty() {
                return Err(LuaError::external("key descriptor cannot be empty"));
            }

            let mut reg = reg
                .lock()
                .map_err(|e| LuaError::external(format!("keymap registry lock poisoned: {}", e)))?;

            let binding = match action {
                LuaValue::String(s) => KeymapBinding::Builtin(s.to_str()?.to_string()),
                LuaValue::Function(f) => {
                    let id = reg.next_id();
                    let key = lua.create_registry_value(f)?;
                    let mut store = store.lock().map_err(|e| {
                        LuaError::external(format!("callback store lock poisoned: {}", e))
                    })?;
                    store.push((id, key));
                    KeymapBinding::Callback(id)
                }
                other => {
                    return Err(LuaError::external(format!(
                        "keymap action must be a string or function, got: {:?}",
                        other
                    )));
                }
            };

            // Validate mode name
            match mode.as_str() {
                "normal" | "insert" | "command" => {}
                _ => {
                    return Err(LuaError::external(format!(
                        "unknown input mode: '{}' (expected: normal, insert, command)",
                        mode
                    )));
                }
            }

            reg.register(KeymapEntry {
                mode: mode.clone(),
                key_descriptor: key_desc,
                action: binding,
            });

            Ok(())
        },
    )?;

    keymap_table.set("set", set_fn)?;
    quorum.set("keymap", keymap_table)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keymap_registry_replaces_duplicate() {
        let mut reg = KeymapRegistry::new();

        reg.register(KeymapEntry {
            mode: "normal".to_string(),
            key_descriptor: "j".to_string(),
            action: KeymapBinding::Builtin("scroll_down".to_string()),
        });

        reg.register(KeymapEntry {
            mode: "normal".to_string(),
            key_descriptor: "j".to_string(),
            action: KeymapBinding::Builtin("quit".to_string()),
        });

        assert_eq!(reg.entries().len(), 1);
        match &reg.entries()[0].action {
            KeymapBinding::Builtin(name) => assert_eq!(name, "quit"),
            _ => panic!("expected builtin action"),
        }
    }

    #[test]
    fn test_keymap_registry_different_modes_coexist() {
        let mut reg = KeymapRegistry::new();

        reg.register(KeymapEntry {
            mode: "normal".to_string(),
            key_descriptor: "j".to_string(),
            action: KeymapBinding::Builtin("scroll_down".to_string()),
        });

        reg.register(KeymapEntry {
            mode: "insert".to_string(),
            key_descriptor: "j".to_string(),
            action: KeymapBinding::Builtin("insert_char".to_string()),
        });

        assert_eq!(reg.entries().len(), 2);
    }

    #[test]
    fn test_keymap_registry_callback_ids_increment() {
        let mut reg = KeymapRegistry::new();
        assert_eq!(reg.next_id(), 1);
        assert_eq!(reg.next_id(), 2);
        assert_eq!(reg.next_id(), 3);
    }

    #[test]
    fn test_keymap_registry_clear() {
        let mut reg = KeymapRegistry::new();
        reg.register(KeymapEntry {
            mode: "normal".to_string(),
            key_descriptor: "j".to_string(),
            action: KeymapBinding::Builtin("scroll_down".to_string()),
        });
        assert_eq!(reg.entries().len(), 1);
        reg.clear();
        assert_eq!(reg.entries().len(), 0);
    }

    #[test]
    fn test_lua_keymap_set_builtin() {
        let lua = Lua::new();
        let quorum = lua.create_table().unwrap();
        let registry = Arc::new(Mutex::new(KeymapRegistry::new()));
        let callback_store = Arc::new(Mutex::new(Vec::new()));

        register_keymap_api(&lua, &quorum, Arc::clone(&registry), callback_store).unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        lua.load(r#"quorum.keymap.set("normal", "Ctrl+s", "submit_input")"#)
            .exec()
            .unwrap();

        let reg = registry.lock().unwrap();
        assert_eq!(reg.entries().len(), 1);
        assert_eq!(reg.entries()[0].mode, "normal");
        assert_eq!(reg.entries()[0].key_descriptor, "Ctrl+s");
        match &reg.entries()[0].action {
            KeymapBinding::Builtin(name) => assert_eq!(name, "submit_input"),
            _ => panic!("expected builtin"),
        }
    }

    #[test]
    fn test_lua_keymap_set_callback() {
        let lua = Lua::new();
        let quorum = lua.create_table().unwrap();
        let registry = Arc::new(Mutex::new(KeymapRegistry::new()));
        let callback_store = Arc::new(Mutex::new(Vec::new()));

        register_keymap_api(
            &lua,
            &quorum,
            Arc::clone(&registry),
            Arc::clone(&callback_store),
        )
        .unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        lua.load(r#"quorum.keymap.set("normal", "Ctrl+p", function() end)"#)
            .exec()
            .unwrap();

        let reg = registry.lock().unwrap();
        assert_eq!(reg.entries().len(), 1);
        match &reg.entries()[0].action {
            KeymapBinding::Callback(id) => assert_eq!(*id, 1),
            _ => panic!("expected callback"),
        }

        let store = callback_store.lock().unwrap();
        assert_eq!(store.len(), 1);
        assert_eq!(store[0].0, 1);
    }

    #[test]
    fn test_lua_keymap_invalid_mode_errors() {
        let lua = Lua::new();
        let quorum = lua.create_table().unwrap();
        let registry = Arc::new(Mutex::new(KeymapRegistry::new()));
        let callback_store = Arc::new(Mutex::new(Vec::new()));

        register_keymap_api(&lua, &quorum, registry, callback_store).unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        let result = lua
            .load(r#"quorum.keymap.set("visual", "j", "scroll_down")"#)
            .exec();
        assert!(result.is_err());
    }
}
