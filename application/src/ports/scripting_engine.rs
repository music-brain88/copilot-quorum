//! Scripting engine port — interface for the Lua runtime.
//!
//! This port abstracts the scripting engine so that:
//! - The application/presentation layers don't depend on mlua
//! - A no-op implementation (`NoScriptingEngine`) is always available
//! - The `scripting` feature gate only affects infrastructure + CLI

use quorum_domain::scripting::{ScriptEventData, ScriptEventType};
use std::path::Path;

/// Outcome of firing an event through the scripting engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventOutcome {
    /// All listeners ran; none requested cancellation.
    Continue,
    /// A listener returned `false`, requesting cancellation (only for cancellable events).
    Cancelled,
}

/// Error from a scripting engine operation.
#[derive(Debug, Clone)]
pub struct ScriptError {
    pub message: String,
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "script error: {}", self.message)
    }
}

impl std::error::Error for ScriptError {}

/// Port for the scripting engine.
///
/// The presentation and application layers interact with the scripting
/// engine exclusively through this trait. The infrastructure layer
/// provides the real `LuaScriptingEngine` implementation; when the
/// `scripting` feature is disabled, `NoScriptingEngine` is used instead.
pub trait ScriptingEnginePort: Send + Sync {
    /// Fire an event, invoking all registered listeners.
    ///
    /// Returns `EventOutcome::Cancelled` if any listener returns `false`
    /// for a cancellable event. Non-cancellable events always return `Continue`.
    fn emit_event(
        &self,
        event: ScriptEventType,
        data: ScriptEventData,
    ) -> Result<EventOutcome, ScriptError>;

    /// Load and execute a Lua script file (e.g. init.lua).
    fn load_script(&self, path: &Path) -> Result<(), ScriptError>;

    /// Whether the engine is actually available (i.e. not `NoScriptingEngine`).
    fn is_available(&self) -> bool;

    /// Retrieve registered custom keymaps.
    ///
    /// Returns a list of `(mode, key_descriptor, action_or_callback_id)` tuples.
    /// The presentation layer uses this to build the custom keymap table.
    fn registered_keymaps(&self) -> Vec<(String, String, KeymapAction)>;

    /// Execute a Lua callback by its registry ID.
    ///
    /// Called by the presentation layer when `KeyAction::LuaCallback(id)` is triggered.
    fn execute_callback(&self, callback_id: u64) -> Result<(), ScriptError>;
}

/// Action bound to a custom keymap entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeymapAction {
    /// A built-in action name (e.g. "submit_input", "quit").
    Builtin(String),
    /// A Lua callback identified by a registry key ID.
    LuaCallback(u64),
}

/// No-op scripting engine used when the `scripting` feature is disabled.
///
/// All operations are safe no-ops: events return `Continue`, script
/// loading is silently ignored, and the engine reports itself as unavailable.
pub struct NoScriptingEngine;

impl ScriptingEnginePort for NoScriptingEngine {
    fn emit_event(
        &self,
        _event: ScriptEventType,
        _data: ScriptEventData,
    ) -> Result<EventOutcome, ScriptError> {
        Ok(EventOutcome::Continue)
    }

    fn load_script(&self, _path: &Path) -> Result<(), ScriptError> {
        Ok(())
    }

    fn is_available(&self) -> bool {
        false
    }

    fn registered_keymaps(&self) -> Vec<(String, String, KeymapAction)> {
        Vec::new()
    }

    fn execute_callback(&self, _callback_id: u64) -> Result<(), ScriptError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_scripting_engine_is_noop() {
        let engine = NoScriptingEngine;
        assert!(!engine.is_available());

        let result = engine
            .emit_event(ScriptEventType::SessionStarted, ScriptEventData::new())
            .unwrap();
        assert_eq!(result, EventOutcome::Continue);

        assert!(engine.registered_keymaps().is_empty());
    }

    #[test]
    fn test_no_scripting_engine_load_script_is_ok() {
        let engine = NoScriptingEngine;
        assert!(engine.load_script(Path::new("/nonexistent")).is_ok());
    }
}
