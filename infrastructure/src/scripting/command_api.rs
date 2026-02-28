//! `quorum.command` Lua API — user-defined command registration.
//!
//! Allows users to register custom slash commands from Lua:
//!
//! ```lua
//! quorum.command.register("hello", {
//!     fn = function(args) print("Hello " .. args) end,
//!     description = "Say hello to someone",
//!     usage = "/hello <name>"
//! })
//! ```
//!
//! Registered commands are discovered by `AgentController` at runtime.
//! When an unknown `/command` is entered, the controller checks
//! `ScriptingEnginePort::registered_commands()` before emitting
//! `UiEvent::UnknownCommand`.

use mlua::prelude::*;
use std::sync::{Arc, Mutex};

/// A registered command entry from Lua.
#[derive(Debug, Clone)]
pub struct CommandEntry {
    pub name: String,
    pub description: String,
    pub usage: String,
    pub callback_id: u64,
}

/// Storage for custom commands registered from Lua.
pub struct CommandRegistry {
    entries: Vec<CommandEntry>,
    next_callback_id: u64,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_callback_id: 1,
        }
    }

    pub fn register(&mut self, entry: CommandEntry) {
        // Remove any existing command with the same name (last-write-wins)
        self.entries.retain(|e| e.name != entry.name);
        self.entries.push(entry);
    }

    pub fn next_id(&mut self) -> u64 {
        let id = self.next_callback_id;
        self.next_callback_id += 1;
        id
    }

    pub fn entries(&self) -> &[CommandEntry] {
        &self.entries
    }

    #[allow(dead_code)]
    pub fn find_by_name(&self, name: &str) -> Option<&CommandEntry> {
        self.entries.iter().find(|e| e.name == name)
    }
}

/// Register the `quorum.command` table on the given `quorum` global.
pub fn register_command_api(
    lua: &Lua,
    quorum: &LuaTable,
    registry: Arc<Mutex<CommandRegistry>>,
    callback_store: Arc<Mutex<Vec<(u64, LuaRegistryKey)>>>,
) -> LuaResult<()> {
    let command_table = lua.create_table()?;

    // quorum.command.register(name, opts)
    // opts = { fn = function(args), description = "...", usage = "/name ..." }
    let reg = Arc::clone(&registry);
    let store = Arc::clone(&callback_store);
    let register_fn = lua.create_function(move |lua, (name, opts): (String, LuaTable)| {
        // Validate command name (no leading slash, no spaces)
        if name.is_empty() {
            return Err(LuaError::external("command name cannot be empty"));
        }
        if name.contains(' ') {
            return Err(LuaError::external("command name cannot contain spaces"));
        }
        let name = name.strip_prefix('/').unwrap_or(&name).to_string();

        // Extract callback function (required)
        let callback: LuaFunction = opts
            .get("fn")
            .map_err(|_| LuaError::external("command opts must include 'fn' (a function)"))?;

        // Extract optional metadata
        let description: String = opts.get("description").unwrap_or_default();
        let usage: String = opts.get("usage").unwrap_or_else(|_| format!("/{}", name));

        let mut reg = reg
            .lock()
            .map_err(|e| LuaError::external(format!("command registry lock poisoned: {}", e)))?;

        let id = reg.next_id();
        let key = lua.create_registry_value(callback)?;
        let mut store = store
            .lock()
            .map_err(|e| LuaError::external(format!("callback store lock poisoned: {}", e)))?;
        store.push((id, key));

        reg.register(CommandEntry {
            name,
            description,
            usage,
            callback_id: id,
        });

        Ok(())
    })?;

    command_table.set("register", register_fn)?;
    quorum.set("command", command_table)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_registry_register_and_find() {
        let mut reg = CommandRegistry::new();

        reg.register(CommandEntry {
            name: "hello".to_string(),
            description: "Say hello".to_string(),
            usage: "/hello <name>".to_string(),
            callback_id: 1,
        });

        assert_eq!(reg.entries().len(), 1);
        let entry = reg.find_by_name("hello").unwrap();
        assert_eq!(entry.description, "Say hello");
    }

    #[test]
    fn test_command_registry_replaces_duplicate() {
        let mut reg = CommandRegistry::new();

        reg.register(CommandEntry {
            name: "hello".to_string(),
            description: "Old".to_string(),
            usage: "/hello".to_string(),
            callback_id: 1,
        });

        reg.register(CommandEntry {
            name: "hello".to_string(),
            description: "New".to_string(),
            usage: "/hello".to_string(),
            callback_id: 2,
        });

        assert_eq!(reg.entries().len(), 1);
        assert_eq!(reg.find_by_name("hello").unwrap().description, "New");
    }

    #[test]
    fn test_command_registry_callback_ids_increment() {
        let mut reg = CommandRegistry::new();
        assert_eq!(reg.next_id(), 1);
        assert_eq!(reg.next_id(), 2);
        assert_eq!(reg.next_id(), 3);
    }

    #[test]
    fn test_command_registry_find_nonexistent() {
        let reg = CommandRegistry::new();
        assert!(reg.find_by_name("nonexistent").is_none());
    }

    #[test]
    fn test_lua_command_register() {
        let lua = Lua::new();
        let quorum = lua.create_table().unwrap();
        let registry = Arc::new(Mutex::new(CommandRegistry::new()));
        let callback_store = Arc::new(Mutex::new(Vec::new()));

        register_command_api(&lua, &quorum, Arc::clone(&registry), callback_store).unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        lua.load(
            r#"
            quorum.command.register("hello", {
                fn = function(args) end,
                description = "Say hello",
                usage = "/hello <name>"
            })
        "#,
        )
        .exec()
        .unwrap();

        let reg = registry.lock().unwrap();
        assert_eq!(reg.entries().len(), 1);
        let entry = reg.find_by_name("hello").unwrap();
        assert_eq!(entry.description, "Say hello");
        assert_eq!(entry.usage, "/hello <name>");
    }

    #[test]
    fn test_lua_command_register_strips_leading_slash() {
        let lua = Lua::new();
        let quorum = lua.create_table().unwrap();
        let registry = Arc::new(Mutex::new(CommandRegistry::new()));
        let callback_store = Arc::new(Mutex::new(Vec::new()));

        register_command_api(&lua, &quorum, Arc::clone(&registry), callback_store).unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        lua.load(
            r#"
            quorum.command.register("/test", {
                fn = function() end
            })
        "#,
        )
        .exec()
        .unwrap();

        let reg = registry.lock().unwrap();
        assert!(reg.find_by_name("test").is_some());
    }

    #[test]
    fn test_lua_command_register_missing_fn_errors() {
        let lua = Lua::new();
        let quorum = lua.create_table().unwrap();
        let registry = Arc::new(Mutex::new(CommandRegistry::new()));
        let callback_store = Arc::new(Mutex::new(Vec::new()));

        register_command_api(&lua, &quorum, registry, callback_store).unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        let result = lua
            .load(
                r#"
            quorum.command.register("test", {
                description = "no fn field"
            })
        "#,
            )
            .exec();
        assert!(result.is_err());
    }

    #[test]
    fn test_lua_command_register_empty_name_errors() {
        let lua = Lua::new();
        let quorum = lua.create_table().unwrap();
        let registry = Arc::new(Mutex::new(CommandRegistry::new()));
        let callback_store = Arc::new(Mutex::new(Vec::new()));

        register_command_api(&lua, &quorum, registry, callback_store).unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        let result = lua
            .load(
                r#"
            quorum.command.register("", { fn = function() end })
        "#,
            )
            .exec();
        assert!(result.is_err());
    }
}
