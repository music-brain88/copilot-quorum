//! CLI tool configuration types (infrastructure-local)
//!
//! These types were originally in `file_config/tools.rs` with serde derives
//! for TOML deserialization. Now that configuration comes from Lua (init.lua),
//! they live here as plain Rust types used by the CLI tool discovery and provider.

use std::collections::HashMap;

/// Enhanced tool definition (for suggesting upgrades)
#[derive(Debug, Clone, Default)]
pub struct EnhancedToolConfig {
    /// CLI command to use
    pub command: String,
    /// Additional arguments
    pub args: Vec<String>,
    /// Description for user prompt
    pub description: String,
}

/// CLI tool provider configuration
#[derive(Debug, Clone)]
pub struct CliToolsConfig {
    /// Whether CLI tools are enabled
    pub enabled: bool,
    /// Tool aliases (tool_name -> CLI command)
    /// e.g., grep_search = "grep" or grep_search = "rg"
    pub aliases: HashMap<String, String>,
    /// Enhanced tool definitions (for suggesting upgrades)
    pub enhanced: HashMap<String, EnhancedToolConfig>,
}

impl Default for CliToolsConfig {
    fn default() -> Self {
        let mut aliases = HashMap::new();
        // Default to standard tools (available everywhere)
        aliases.insert("grep_search".to_string(), "grep".to_string());
        aliases.insert("glob_search".to_string(), "find".to_string());
        aliases.insert("read_file".to_string(), "cat".to_string());

        let mut enhanced = HashMap::new();
        enhanced.insert(
            "grep_search".to_string(),
            EnhancedToolConfig {
                command: "rg".to_string(),
                args: vec![],
                description: "10x faster than grep".to_string(),
            },
        );
        enhanced.insert(
            "glob_search".to_string(),
            EnhancedToolConfig {
                command: "fd".to_string(),
                args: vec![],
                description: "5x faster than find".to_string(),
            },
        );
        enhanced.insert(
            "read_file".to_string(),
            EnhancedToolConfig {
                command: "bat".to_string(),
                args: vec!["--plain".to_string(), "--paging=never".to_string()],
                description: "syntax highlighting".to_string(),
            },
        );

        Self {
            enabled: true,
            aliases,
            enhanced,
        }
    }
}
