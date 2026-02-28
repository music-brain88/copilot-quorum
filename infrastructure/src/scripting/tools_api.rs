//! `quorum.tools` Lua API — user-defined tool registration.
//!
//! Allows users to register custom tools from Lua:
//!
//! ```lua
//! quorum.tools.register("gh_issue", {
//!     description = "Create a GitHub issue",
//!     command = "gh issue create --title {title} --body {body}",
//!     risk_level = "high",
//!     parameters = {
//!         title = { type = "string", description = "Issue title", required = true },
//!         body = { type = "string", description = "Issue body", required = true },
//!     }
//! })
//! ```
//!
//! Registered tools are discovered by `ScriptingEnginePort::registered_custom_tools()`
//! and merged into the executor's tool spec at startup.

use mlua::prelude::*;
use std::sync::{Arc, Mutex};

use quorum_application::ports::scripting_engine::{CustomToolDef, CustomToolParam};

/// Register the `quorum.tools` table on the given `quorum` global.
pub fn register_tools_api(
    lua: &Lua,
    quorum_table: &LuaTable,
    pending_tools: Arc<Mutex<Vec<CustomToolDef>>>,
) -> LuaResult<()> {
    let tools = lua.create_table()?;

    let pt = pending_tools.clone();
    tools.set(
        "register",
        lua.create_function(move |_, (name, opts): (String, LuaTable)| {
            if name.is_empty() {
                return Err(LuaError::external("tool name cannot be empty"));
            }

            let description: String = opts.get("description")?;
            let command: String = opts.get("command")?;
            let risk_level: String = opts
                .get("risk_level")
                .unwrap_or_else(|_| "high".to_string());

            let mut params = Vec::new();
            if let Ok(params_table) = opts.get::<LuaTable>("parameters") {
                for pair in params_table.pairs::<String, LuaTable>() {
                    let (param_name, param_opts) = pair?;
                    let param_type: String = param_opts
                        .get("type")
                        .unwrap_or_else(|_| "string".to_string());
                    let param_desc: String = param_opts.get("description").unwrap_or_default();
                    let required: bool = param_opts.get("required").unwrap_or(false);
                    params.push(CustomToolParam {
                        name: param_name,
                        param_type,
                        description: param_desc,
                        required,
                    });
                }
            }

            let tool_def = CustomToolDef {
                name,
                description,
                command,
                risk_level,
                parameters: params,
            };

            pt.lock()
                .map_err(|e| LuaError::external(format!("pending_tools lock poisoned: {}", e)))?
                .push(tool_def);
            Ok(())
        })?,
    )?;

    quorum_table.set("tools", tools)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pending() -> Arc<Mutex<Vec<CustomToolDef>>> {
        Arc::new(Mutex::new(Vec::new()))
    }

    #[test]
    fn test_register_simple_tool() {
        let lua = Lua::new();
        let quorum = lua.create_table().unwrap();
        let pending = make_pending();

        register_tools_api(&lua, &quorum, Arc::clone(&pending)).unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        lua.load(
            r#"
            quorum.tools.register("echo_tool", {
                description = "Echo a message",
                command = "echo {message}",
                risk_level = "low",
                parameters = {
                    message = { type = "string", description = "The message", required = true }
                }
            })
        "#,
        )
        .exec()
        .unwrap();

        let tools = pending.lock().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo_tool");
        assert_eq!(tools[0].description, "Echo a message");
        assert_eq!(tools[0].command, "echo {message}");
        assert_eq!(tools[0].risk_level, "low");
        assert_eq!(tools[0].parameters.len(), 1);
        assert_eq!(tools[0].parameters[0].name, "message");
        assert_eq!(tools[0].parameters[0].param_type, "string");
        assert!(tools[0].parameters[0].required);
    }

    #[test]
    fn test_register_tool_defaults_to_high_risk() {
        let lua = Lua::new();
        let quorum = lua.create_table().unwrap();
        let pending = make_pending();

        register_tools_api(&lua, &quorum, Arc::clone(&pending)).unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        lua.load(
            r#"
            quorum.tools.register("risky", {
                description = "A risky tool",
                command = "rm -rf /",
            })
        "#,
        )
        .exec()
        .unwrap();

        let tools = pending.lock().unwrap();
        assert_eq!(tools[0].risk_level, "high");
        assert!(tools[0].parameters.is_empty());
    }

    #[test]
    fn test_register_tool_empty_name_errors() {
        let lua = Lua::new();
        let quorum = lua.create_table().unwrap();
        let pending = make_pending();

        register_tools_api(&lua, &quorum, pending).unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        let result = lua
            .load(
                r#"
            quorum.tools.register("", {
                description = "Bad",
                command = "echo bad",
            })
        "#,
            )
            .exec();
        assert!(result.is_err());
    }

    #[test]
    fn test_register_multiple_tools() {
        let lua = Lua::new();
        let quorum = lua.create_table().unwrap();
        let pending = make_pending();

        register_tools_api(&lua, &quorum, Arc::clone(&pending)).unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        lua.load(
            r#"
            quorum.tools.register("tool_a", {
                description = "Tool A",
                command = "echo a",
            })
            quorum.tools.register("tool_b", {
                description = "Tool B",
                command = "echo b",
                risk_level = "low",
            })
        "#,
        )
        .exec()
        .unwrap();

        let tools = pending.lock().unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "tool_a");
        assert_eq!(tools[1].name, "tool_b");
    }

    #[test]
    fn test_register_tool_with_multiple_params() {
        let lua = Lua::new();
        let quorum = lua.create_table().unwrap();
        let pending = make_pending();

        register_tools_api(&lua, &quorum, Arc::clone(&pending)).unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        lua.load(
            r#"
            quorum.tools.register("gh_issue", {
                description = "Create a GitHub issue",
                command = "gh issue create --title {title} --body {body}",
                risk_level = "high",
                parameters = {
                    title = { type = "string", description = "Issue title", required = true },
                    body = { type = "string", description = "Issue body", required = false },
                }
            })
        "#,
        )
        .exec()
        .unwrap();

        let tools = pending.lock().unwrap();
        assert_eq!(tools[0].parameters.len(), 2);

        // Sort params by name for deterministic assertions
        let mut params = tools[0].parameters.clone();
        params.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(params[0].name, "body");
        assert!(!params[0].required);
        assert_eq!(params[1].name, "title");
        assert!(params[1].required);
    }

    #[test]
    fn test_register_tool_param_type_defaults() {
        let lua = Lua::new();
        let quorum = lua.create_table().unwrap();
        let pending = make_pending();

        register_tools_api(&lua, &quorum, Arc::clone(&pending)).unwrap();
        lua.globals().set("quorum", &quorum).unwrap();

        lua.load(
            r#"
            quorum.tools.register("test", {
                description = "Test",
                command = "echo {x}",
                parameters = {
                    x = { description = "No type specified" }
                }
            })
        "#,
        )
        .exec()
        .unwrap();

        let tools = pending.lock().unwrap();
        assert_eq!(tools[0].parameters[0].param_type, "string");
        assert!(!tools[0].parameters[0].required);
    }
}
