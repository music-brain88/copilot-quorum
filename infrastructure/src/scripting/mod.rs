//! Lua scripting platform (feature-gated: `scripting`)
//!
//! Provides the `LuaScriptingEngine` that implements `ScriptingEnginePort`
//! from the application layer, backed by mlua (Lua 5.4).
//!
//! # Modules
//!
//! - `event_bus` — Callback registry for `quorum.on(event, fn)`
//! - `sandbox` — C module blocking for safety
//! - `config_api` — `quorum.config` get/set/keys + metatable proxy
//! - `keymap_api` — `quorum.keymap.set(mode, key, action)`
//! - `command_api` — `quorum.command.register(name, opts)`
//! - `lua_engine` — Main engine struct tying everything together

mod command_api;
mod config_api;
mod event_bus;
mod keymap_api;
mod lua_engine;
mod sandbox;
mod tui_api;

pub use lua_engine::LuaScriptingEngine;
