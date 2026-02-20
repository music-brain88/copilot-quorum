//! Runtime configuration access port.
//!
//! Provides a uniform interface for reading and writing config keys at runtime.
//! Used by future TUI `:config` commands and Lua `quorum.config` API.

use quorum_domain::agent::validation::ConfigIssue;

/// A dynamically-typed configuration value.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValue {
    String(String),
    Integer(i64),
    Boolean(bool),
    StringList(Vec<String>),
}

impl std::fmt::Display for ConfigValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigValue::String(s) => write!(f, "{}", s),
            ConfigValue::Integer(n) => write!(f, "{}", n),
            ConfigValue::Boolean(b) => write!(f, "{}", b),
            ConfigValue::StringList(list) => {
                write!(f, "[{}]", list.join(", "))
            }
        }
    }
}

/// Errors from config access operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigAccessError {
    /// The key is not recognized.
    UnknownKey { key: String },
    /// The key exists but cannot be changed at runtime.
    ReadOnly { key: String },
    /// The provided value is invalid for this key.
    InvalidValue { key: String, message: String },
}

impl std::fmt::Display for ConfigAccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigAccessError::UnknownKey { key } => write!(f, "unknown config key: {}", key),
            ConfigAccessError::ReadOnly { key } => {
                write!(f, "config key '{}' is read-only", key)
            }
            ConfigAccessError::InvalidValue { key, message } => {
                write!(f, "invalid value for '{}': {}", key, message)
            }
        }
    }
}

impl std::error::Error for ConfigAccessError {}

/// Port for runtime config access.
///
/// Implementors provide get/set for known config keys, enforcing
/// mutability constraints and returning validation issues on set.
pub trait ConfigAccessorPort: Send + Sync {
    /// Get the current value of a config key.
    fn config_get(&self, key: &str) -> Result<ConfigValue, ConfigAccessError>;

    /// Set a config key to a new value.
    ///
    /// Returns validation warnings (if any) on success.
    /// Errors if the key is unknown, read-only, or the value is invalid.
    fn config_set(
        &mut self,
        key: &str,
        value: ConfigValue,
    ) -> Result<Vec<ConfigIssue>, ConfigAccessError>;

    /// List all known config key names.
    fn config_keys(&self) -> Vec<String>;
}
