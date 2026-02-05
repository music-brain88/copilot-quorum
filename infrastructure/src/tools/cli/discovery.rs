//! Tool discovery and suggestion
//!
//! This module provides functionality to detect enhanced CLI tools (rg, fd, bat)
//! installed on the user's system and suggest upgrades from standard tools.
//!
//! # Overview
//!
//! The discovery system helps users benefit from faster, more feature-rich
//! tools without requiring manual configuration:
//!
//! ```text
//! $ quorum init
//! 📦 Tool configuration...
//!
//! Default tools (always available):
//!   ✓ grep  → file content search
//!   ✓ find  → file pattern search
//!
//! 🔍 Enhanced tools detected:
//!   • rg (ripgrep) - 10x faster than grep
//!   • fd           - 5x faster than find
//!
//! Would you like to use these enhanced tools? [Y/n]
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use quorum_infrastructure::tools::cli::{discover_enhanced_tools, DiscoveryResult};
//! use quorum_infrastructure::config::FileCliToolsConfig;
//!
//! let config = FileCliToolsConfig::default();
//! let result = discover_enhanced_tools(&config);
//!
//! if result.has_upgrades() {
//!     println!("{}", result.format_upgrade_message());
//! }
//! ```
//!
//! # Detected Tools
//!
//! | Standard | Enhanced | Speedup | Features |
//! |----------|----------|---------|----------|
//! | grep     | rg       | ~10x    | Better regex, .gitignore support |
//! | find     | fd       | ~5x     | Simpler syntax, colorized output |
//! | cat      | bat      | -       | Syntax highlighting, git integration |
//!
//! # Additional Tools
//!
//! The `discover_additional_tools()` function also detects:
//! - `gh`: GitHub CLI for PR/Issue operations
//! - `jq`: JSON processor

use std::collections::HashMap;

use crate::config::{EnhancedToolConfig, FileCliToolsConfig};

/// Information about a detected enhanced tool
#[derive(Debug, Clone)]
pub struct DetectedTool {
    /// Tool name in Quorum (e.g., "grep_search")
    pub tool_name: String,
    /// CLI command that was detected (e.g., "rg")
    pub command: String,
    /// Current command in use (e.g., "grep")
    pub current_command: String,
    /// Description of the enhancement
    pub description: String,
}

/// Result of tool discovery
#[derive(Debug, Clone, Default)]
pub struct DiscoveryResult {
    /// Enhanced tools that are available but not configured
    pub available_upgrades: Vec<DetectedTool>,
    /// Tools that are already using enhanced commands
    pub already_enhanced: Vec<String>,
    /// Tools using standard commands
    pub using_standard: Vec<String>,
}

impl DiscoveryResult {
    /// Check if there are any upgrades available
    pub fn has_upgrades(&self) -> bool {
        !self.available_upgrades.is_empty()
    }

    /// Format a user-friendly message about available upgrades
    pub fn format_upgrade_message(&self) -> String {
        if self.available_upgrades.is_empty() {
            return String::new();
        }

        let mut msg = String::from("🔍 Enhanced tools detected on your system:\n");

        for tool in &self.available_upgrades {
            msg.push_str(&format!(
                "  • {} ({}) - {}\n",
                tool.command, tool.tool_name, tool.description
            ));
        }

        msg.push_str("\nCurrent configuration uses standard tools:\n");
        for tool in &self.using_standard {
            msg.push_str(&format!("  • {}\n", tool));
        }

        msg
    }
}

/// Discover available enhanced tools
///
/// Checks if enhanced tools (rg, fd, bat, gh) are installed and
/// compares against the current configuration.
pub fn discover_enhanced_tools(config: &FileCliToolsConfig) -> DiscoveryResult {
    let mut result = DiscoveryResult::default();

    for (tool_name, enhanced) in &config.enhanced {
        let current_cmd = config.aliases.get(tool_name);

        // Check if enhanced tool is available
        let enhanced_available = is_command_available(&enhanced.command);

        match current_cmd {
            Some(cmd) if cmd == &enhanced.command => {
                // Already using enhanced tool
                result.already_enhanced.push(format!(
                    "{} ({})",
                    tool_name, enhanced.command
                ));
            }
            Some(cmd) => {
                // Using standard tool
                result.using_standard.push(format!("{} ({})", tool_name, cmd));

                if enhanced_available {
                    // Enhanced tool is available but not configured
                    result.available_upgrades.push(DetectedTool {
                        tool_name: tool_name.clone(),
                        command: enhanced.command.clone(),
                        current_command: cmd.clone(),
                        description: enhanced.description.clone(),
                    });
                }
            }
            None => {
                // No alias configured
                if enhanced_available {
                    result.available_upgrades.push(DetectedTool {
                        tool_name: tool_name.clone(),
                        command: enhanced.command.clone(),
                        current_command: String::new(),
                        description: enhanced.description.clone(),
                    });
                }
            }
        }
    }

    result
}

/// Check if additional tools are available (gh, etc.)
pub fn discover_additional_tools() -> Vec<DetectedTool> {
    let mut tools = Vec::new();

    // GitHub CLI
    if is_command_available("gh") {
        tools.push(DetectedTool {
            tool_name: "github".to_string(),
            command: "gh".to_string(),
            current_command: String::new(),
            description: "GitHub CLI for PR/Issue operations".to_string(),
        });
    }

    // jq for JSON processing
    if is_command_available("jq") {
        tools.push(DetectedTool {
            tool_name: "json_query".to_string(),
            command: "jq".to_string(),
            current_command: String::new(),
            description: "JSON processor".to_string(),
        });
    }

    tools
}

/// Generate updated aliases with enhanced tools
pub fn generate_enhanced_aliases(
    current: &HashMap<String, String>,
    enhanced: &HashMap<String, EnhancedToolConfig>,
) -> HashMap<String, String> {
    let mut aliases = current.clone();

    for (tool_name, config) in enhanced {
        if is_command_available(&config.command) {
            aliases.insert(tool_name.clone(), config.command.clone());
        }
    }

    aliases
}

/// Check if a command is available on the system
fn is_command_available(command: &str) -> bool {
    which::which(command).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovery_result_default() {
        let result = DiscoveryResult::default();
        assert!(!result.has_upgrades());
        assert!(result.format_upgrade_message().is_empty());
    }

    #[test]
    fn test_discovery_result_with_upgrades() {
        let mut result = DiscoveryResult::default();
        result.available_upgrades.push(DetectedTool {
            tool_name: "grep_search".to_string(),
            command: "rg".to_string(),
            current_command: "grep".to_string(),
            description: "10x faster".to_string(),
        });
        result.using_standard.push("grep_search (grep)".to_string());

        assert!(result.has_upgrades());
        let msg = result.format_upgrade_message();
        assert!(msg.contains("rg"));
        assert!(msg.contains("grep_search"));
        assert!(msg.contains("10x faster"));
    }

    #[test]
    fn test_discover_enhanced_tools() {
        let config = FileCliToolsConfig::default();
        let result = discover_enhanced_tools(&config);

        // Result depends on what's installed
        // At minimum, we should have some tools categorized
        let total = result.available_upgrades.len()
            + result.already_enhanced.len()
            + result.using_standard.len();

        // If config has defaults, we should have some categorization
        if !config.aliases.is_empty() {
            assert!(total > 0 || config.enhanced.is_empty());
        }
    }

    #[test]
    fn test_discover_additional_tools() {
        let tools = discover_additional_tools();
        // Just ensure it doesn't panic
        // Actual results depend on what's installed
        for tool in &tools {
            assert!(!tool.tool_name.is_empty());
            assert!(!tool.command.is_empty());
        }
    }

    #[test]
    fn test_generate_enhanced_aliases() {
        let mut current = HashMap::new();
        current.insert("grep_search".to_string(), "grep".to_string());

        let mut enhanced = HashMap::new();
        enhanced.insert(
            "grep_search".to_string(),
            EnhancedToolConfig {
                command: "definitely_not_installed_xyz".to_string(),
                args: vec![],
                description: "test".to_string(),
            },
        );

        // Should not change since the command doesn't exist
        let result = generate_enhanced_aliases(&current, &enhanced);
        assert_eq!(result.get("grep_search"), Some(&"grep".to_string()));
    }

    #[test]
    fn test_is_command_available() {
        // This should exist on most systems
        #[cfg(unix)]
        assert!(is_command_available("ls"));

        // This should not exist
        assert!(!is_command_available("definitely_not_a_command_123xyz"));
    }
}
