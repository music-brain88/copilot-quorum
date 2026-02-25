//! Scripting domain types
//!
//! Defines event types and value objects for the Lua scripting platform.
//! These types are infrastructure-agnostic — the actual Lua runtime lives
//! in the infrastructure layer behind `ScriptingEnginePort`.

use std::collections::HashMap;

/// Events emitted during scripting lifecycle.
///
/// Phase 1 events cover script loading, config changes, mode switches,
/// and session lifecycle. Future phases will add TUI, Agent, and
/// Knowledge events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptEventType {
    /// Fired before init.lua is loaded. Returning `false` from a listener cancels loading.
    ScriptLoading,
    /// Fired after init.lua has been successfully loaded.
    ScriptLoaded,
    /// Fired after a config key is changed via `quorum.config.set()`.
    ConfigChanged,
    /// Fired when the TUI input mode changes (Normal ↔ Insert ↔ Command).
    ModeChanged,
    /// Fired when a new session starts.
    SessionStarted,
}

impl ScriptEventType {
    /// Event name used in Lua API (`quorum.on("ConfigChanged", fn)`)
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ScriptLoading => "ScriptLoading",
            Self::ScriptLoaded => "ScriptLoaded",
            Self::ConfigChanged => "ConfigChanged",
            Self::ModeChanged => "ModeChanged",
            Self::SessionStarted => "SessionStarted",
        }
    }

    /// Whether this event type supports cancellation (returning `false`).
    pub fn is_cancellable(&self) -> bool {
        matches!(self, Self::ScriptLoading)
    }

}

impl std::str::FromStr for ScriptEventType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ScriptLoading" => Ok(Self::ScriptLoading),
            "ScriptLoaded" => Ok(Self::ScriptLoaded),
            "ConfigChanged" => Ok(Self::ConfigChanged),
            "ModeChanged" => Ok(Self::ModeChanged),
            "SessionStarted" => Ok(Self::SessionStarted),
            other => Err(format!("unknown event: '{}'", other)),
        }
    }
}

impl std::fmt::Display for ScriptEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Data payload for a script event.
///
/// Each event type carries a map of string key-value pairs.
/// This keeps the domain layer free of Lua-specific types while
/// providing enough structure for the infrastructure layer to
/// convert into Lua tables.
#[derive(Debug, Clone, Default)]
pub struct ScriptEventData {
    fields: HashMap<String, ScriptValue>,
}

/// A simple value type that can be passed to/from scripts.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptValue {
    String(String),
    Integer(i64),
    Boolean(bool),
    Nil,
}

impl std::fmt::Display for ScriptValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String(s) => write!(f, "{}", s),
            Self::Integer(n) => write!(f, "{}", n),
            Self::Boolean(b) => write!(f, "{}", b),
            Self::Nil => write!(f, "nil"),
        }
    }
}

impl ScriptEventData {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_field(mut self, key: impl Into<String>, value: ScriptValue) -> Self {
        self.fields.insert(key.into(), value);
        self
    }

    pub fn fields(&self) -> &HashMap<String, ScriptValue> {
        &self.fields
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type_roundtrip() {
        let events = [
            ScriptEventType::ScriptLoading,
            ScriptEventType::ScriptLoaded,
            ScriptEventType::ConfigChanged,
            ScriptEventType::ModeChanged,
            ScriptEventType::SessionStarted,
        ];
        for event in &events {
            let name = event.as_str();
            let parsed: ScriptEventType = name.parse().unwrap();
            assert_eq!(&parsed, event);
        }
    }

    #[test]
    fn test_unknown_event_returns_none() {
        assert!("UnknownEvent".parse::<ScriptEventType>().is_err());
    }

    #[test]
    fn test_only_script_loading_is_cancellable() {
        assert!(ScriptEventType::ScriptLoading.is_cancellable());
        assert!(!ScriptEventType::ScriptLoaded.is_cancellable());
        assert!(!ScriptEventType::ConfigChanged.is_cancellable());
        assert!(!ScriptEventType::ModeChanged.is_cancellable());
        assert!(!ScriptEventType::SessionStarted.is_cancellable());
    }

    #[test]
    fn test_event_data_builder() {
        let data = ScriptEventData::new()
            .with_field("key", ScriptValue::String("agent.strategy".into()))
            .with_field("old_value", ScriptValue::String("quorum".into()))
            .with_field("new_value", ScriptValue::String("debate".into()));

        assert_eq!(data.fields().len(), 3);
        assert_eq!(
            data.fields().get("key"),
            Some(&ScriptValue::String("agent.strategy".into()))
        );
    }
}
