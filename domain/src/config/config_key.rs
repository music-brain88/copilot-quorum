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

static KNOWN_KEYS: [ConfigKeyInfo; 20] = [
    // ==================== agent.* (SessionMode + AgentPolicy) ====================
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
    ConfigKeyInfo {
        key: "agent.hil_mode",
        description: "Human-in-the-loop mode",
        mutability: Mutability::Mutable,
        valid_values: &["interactive", "auto_reject", "auto_approve"],
    },
    ConfigKeyInfo {
        key: "agent.max_plan_revisions",
        description: "Maximum plan revision attempts",
        mutability: Mutability::Mutable,
        valid_values: &[],
    },
    // ==================== models.* (ModelConfig) ====================
    ConfigKeyInfo {
        key: "models.exploration",
        description: "Model for context gathering",
        mutability: Mutability::Mutable,
        valid_values: &[],
    },
    ConfigKeyInfo {
        key: "models.decision",
        description: "Model for planning and decisions",
        mutability: Mutability::Mutable,
        valid_values: &[],
    },
    ConfigKeyInfo {
        key: "models.review",
        description: "Models for review phases",
        mutability: Mutability::Mutable,
        valid_values: &[],
    },
    ConfigKeyInfo {
        key: "models.participants",
        description: "Models for Quorum Discussion",
        mutability: Mutability::Mutable,
        valid_values: &[],
    },
    ConfigKeyInfo {
        key: "models.moderator",
        description: "Model for Quorum Synthesis",
        mutability: Mutability::Mutable,
        valid_values: &[],
    },
    ConfigKeyInfo {
        key: "models.ask",
        description: "Model for lightweight Q&A",
        mutability: Mutability::Mutable,
        valid_values: &[],
    },
    // ==================== execution.* (ExecutionParams) ====================
    ConfigKeyInfo {
        key: "execution.max_iterations",
        description: "Maximum agent iterations",
        mutability: Mutability::Mutable,
        valid_values: &[],
    },
    ConfigKeyInfo {
        key: "execution.max_tool_turns",
        description: "Maximum tool call turns per iteration",
        mutability: Mutability::Mutable,
        valid_values: &[],
    },
    // ==================== output.* ====================
    ConfigKeyInfo {
        key: "output.format",
        description: "Output format: full, synthesis, or json",
        mutability: Mutability::Mutable,
        valid_values: &["full", "synthesis", "json"],
    },
    ConfigKeyInfo {
        key: "output.color",
        description: "Enable colored terminal output",
        mutability: Mutability::Mutable,
        valid_values: &[],
    },
    // ==================== repl.* ====================
    ConfigKeyInfo {
        key: "repl.show_progress",
        description: "Show progress indicators",
        mutability: Mutability::Mutable,
        valid_values: &[],
    },
    ConfigKeyInfo {
        key: "repl.history_file",
        description: "Path to REPL history file",
        mutability: Mutability::Mutable,
        valid_values: &[],
    },
    // ==================== context_budget.* ====================
    ConfigKeyInfo {
        key: "context_budget.max_entry_bytes",
        description: "Maximum bytes for a single task result",
        mutability: Mutability::Mutable,
        valid_values: &[],
    },
    ConfigKeyInfo {
        key: "context_budget.max_total_bytes",
        description: "Maximum bytes for entire results buffer",
        mutability: Mutability::Mutable,
        valid_values: &[],
    },
    ConfigKeyInfo {
        key: "context_budget.recent_full_count",
        description: "Number of recent results to keep in full",
        mutability: Mutability::Mutable,
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
    fn test_all_keys_mutable() {
        // Phase 1.5: all 20 keys are mutable
        let mutable: Vec<_> = known_keys()
            .iter()
            .filter(|k| k.mutability == Mutability::Mutable)
            .collect();
        assert_eq!(mutable.len(), 20);
    }

    #[test]
    fn test_no_readonly_keys() {
        let readonly: Vec<_> = known_keys()
            .iter()
            .filter(|k| k.mutability == Mutability::ReadOnly)
            .collect();
        assert_eq!(readonly.len(), 0);
    }

    #[test]
    fn test_new_model_keys() {
        assert!(lookup_key("models.participants").is_some());
        assert!(lookup_key("models.moderator").is_some());
        assert!(lookup_key("models.ask").is_some());
    }

    #[test]
    fn test_output_keys() {
        let format = lookup_key("output.format").unwrap();
        assert!(format.valid_values.contains(&"synthesis"));
        assert!(lookup_key("output.color").is_some());
    }

    #[test]
    fn test_context_budget_keys() {
        assert!(lookup_key("context_budget.max_entry_bytes").is_some());
        assert!(lookup_key("context_budget.max_total_bytes").is_some());
        assert!(lookup_key("context_budget.recent_full_count").is_some());
    }
}
