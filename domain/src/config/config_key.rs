//! Config key registry for runtime config access.
//!
//! Defines metadata for known configuration keys: name, description,
//! mutability, and valid values. Used by [`ConfigAccessorPort`] and
//! future `:config set` / Lua `quorum.config` API.

/// Whether a config key can be changed at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mutability {
    /// Can be changed at runtime (e.g., via TUI commands).
    Mutable,
    /// Set at initialization; cannot be changed during a session.
    ReadOnly,
}

/// Metadata for a single config key.
#[derive(Debug, Clone)]
pub struct ConfigKeyInfo {
    /// Dotted key path (e.g., `"agent.consensus_level"`).
    pub key: &'static str,
    /// Human-readable description.
    pub description: &'static str,
    /// Whether this key can be changed at runtime.
    pub mutability: Mutability,
    /// Valid string values (empty if freeform).
    pub valid_values: &'static [&'static str],
}

/// All known config keys with their metadata.
pub fn known_keys() -> &'static [ConfigKeyInfo] {
    &KNOWN_KEYS
}

/// Look up a config key by its dotted path.
pub fn lookup_key(key: &str) -> Option<&'static ConfigKeyInfo> {
    KNOWN_KEYS.iter().find(|k| k.key == key)
}

static KNOWN_KEYS: [ConfigKeyInfo; 10] = [
    // ==================== Mutable (SessionMode) ====================
    ConfigKeyInfo {
        key: "agent.consensus_level",
        description: "Consensus level: solo or ensemble",
        mutability: Mutability::Mutable,
        valid_values: &["solo", "ensemble"],
    },
    ConfigKeyInfo {
        key: "agent.phase_scope",
        description: "Phase scope: full, fast, or plan-only",
        mutability: Mutability::Mutable,
        valid_values: &["full", "fast", "plan-only"],
    },
    ConfigKeyInfo {
        key: "agent.strategy",
        description: "Orchestration strategy: quorum or debate",
        mutability: Mutability::Mutable,
        valid_values: &["quorum", "debate"],
    },
    // ==================== ReadOnly ====================
    ConfigKeyInfo {
        key: "agent.hil_mode",
        description: "Human-in-the-loop mode",
        mutability: Mutability::ReadOnly,
        valid_values: &["interactive", "auto_reject", "auto_approve"],
    },
    ConfigKeyInfo {
        key: "agent.max_plan_revisions",
        description: "Maximum plan revision attempts",
        mutability: Mutability::ReadOnly,
        valid_values: &[],
    },
    ConfigKeyInfo {
        key: "models.exploration",
        description: "Model for context gathering",
        mutability: Mutability::ReadOnly,
        valid_values: &[],
    },
    ConfigKeyInfo {
        key: "models.decision",
        description: "Model for planning and decisions",
        mutability: Mutability::ReadOnly,
        valid_values: &[],
    },
    ConfigKeyInfo {
        key: "models.review",
        description: "Models for review phases",
        mutability: Mutability::ReadOnly,
        valid_values: &[],
    },
    ConfigKeyInfo {
        key: "execution.max_iterations",
        description: "Maximum agent iterations",
        mutability: Mutability::ReadOnly,
        valid_values: &[],
    },
    ConfigKeyInfo {
        key: "execution.max_tool_turns",
        description: "Maximum tool call turns per iteration",
        mutability: Mutability::ReadOnly,
        valid_values: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_keys_not_empty() {
        assert!(!known_keys().is_empty());
    }

    #[test]
    fn test_lookup_existing_key() {
        let key = lookup_key("agent.consensus_level");
        assert!(key.is_some());
        let info = key.unwrap();
        assert_eq!(info.mutability, Mutability::Mutable);
        assert!(info.valid_values.contains(&"solo"));
        assert!(info.valid_values.contains(&"ensemble"));
    }

    #[test]
    fn test_lookup_nonexistent_key() {
        assert!(lookup_key("nonexistent.key").is_none());
    }

    #[test]
    fn test_mutable_keys() {
        let mutable: Vec<_> = known_keys()
            .iter()
            .filter(|k| k.mutability == Mutability::Mutable)
            .collect();
        assert_eq!(mutable.len(), 3);
    }

    #[test]
    fn test_readonly_keys() {
        let readonly: Vec<_> = known_keys()
            .iter()
            .filter(|k| k.mutability == Mutability::ReadOnly)
            .collect();
        assert_eq!(readonly.len(), 7);
    }
}
