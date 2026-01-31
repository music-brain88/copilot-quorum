//! Configuration loader with multi-source merging

use figment::{
    providers::{Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Council-related configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CouncilConfig {
    /// Models to include in the council
    pub models: Vec<String>,
    /// Model to use as moderator for synthesis
    pub moderator: Option<String>,
}

impl Default for CouncilConfig {
    fn default() -> Self {
        Self {
            models: vec![],
            moderator: None,
        }
    }
}

/// Behavior-related configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BehaviorConfig {
    /// Enable peer review phase
    pub enable_review: bool,
    /// Timeout in seconds for API calls
    pub timeout_seconds: Option<u64>,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            enable_review: true,
            timeout_seconds: None,
        }
    }
}

/// Output-related configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    /// Output format: "full", "synthesis", or "json"
    pub format: Option<String>,
    /// Enable colored output
    pub color: bool,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            format: None,
            color: true,
        }
    }
}

/// REPL-related configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReplConfig {
    /// Show progress indicators
    pub show_progress: bool,
    /// Path to history file
    pub history_file: Option<String>,
}

impl Default for ReplConfig {
    fn default() -> Self {
        Self {
            show_progress: true,
            history_file: None,
        }
    }
}

/// Main application configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Council settings
    pub council: CouncilConfig,
    /// Behavior settings
    pub behavior: BehaviorConfig,
    /// Output settings
    pub output: OutputConfig,
    /// REPL settings
    pub repl: ReplConfig,
}

/// Configuration loader that merges multiple sources
pub struct ConfigLoader;

impl ConfigLoader {
    /// Load configuration from all sources with proper priority
    ///
    /// Priority (highest to lowest):
    /// 1. Explicit config path (if provided)
    /// 2. Project root: `./quorum.toml` or `./.quorum.toml`
    /// 3. XDG config: `$XDG_CONFIG_HOME/copilot-quorum/config.toml`
    /// 4. Fallback: `~/.config/copilot-quorum/config.toml`
    /// 5. Default values
    pub fn load(config_path: Option<&PathBuf>) -> Result<AppConfig, figment::Error> {
        let mut figment = Figment::new().merge(Serialized::defaults(AppConfig::default()));

        // Add global config (XDG or fallback)
        if let Some(global_path) = Self::global_config_path() {
            figment = figment.merge(Toml::file(&global_path).nested());
        }

        // Add project-level config files (check both names)
        for filename in &["quorum.toml", ".quorum.toml"] {
            let path = PathBuf::from(filename);
            if path.exists() {
                figment = figment.merge(Toml::file(&path).nested());
                break;
            }
        }

        // Add explicit config path (highest priority for files)
        if let Some(path) = config_path {
            figment = figment.merge(Toml::file(path).nested());
        }

        figment.extract()
    }

    /// Load only default configuration (for --no-config)
    pub fn load_defaults() -> AppConfig {
        AppConfig::default()
    }

    /// Get the global config file path
    ///
    /// Returns XDG_CONFIG_HOME/copilot-quorum/config.toml if it exists,
    /// otherwise falls back to ~/.config/copilot-quorum/config.toml
    pub fn global_config_path() -> Option<PathBuf> {
        // Try XDG config directory first
        if let Some(config_dir) = dirs::config_dir() {
            let xdg_path = config_dir.join("copilot-quorum").join("config.toml");
            if xdg_path.exists() {
                return Some(xdg_path);
            }
        }

        // Return the expected path even if it doesn't exist yet
        // (so users know where to create it)
        dirs::config_dir().map(|d| d.join("copilot-quorum").join("config.toml"))
    }

    /// Get the project-level config file path (if it exists)
    pub fn project_config_path() -> Option<PathBuf> {
        for filename in &["quorum.toml", ".quorum.toml"] {
            let path = PathBuf::from(filename);
            if path.exists() {
                return Some(path);
            }
        }
        None
    }

    /// Print the config file locations being used (for debugging)
    pub fn print_config_sources() {
        println!("Configuration sources (in priority order):");

        // Project config
        if let Some(path) = Self::project_config_path() {
            println!("  [FOUND] Project: {}", path.display());
        } else {
            println!("  [     ] Project: ./quorum.toml or ./.quorum.toml");
        }

        // Global config
        if let Some(path) = Self::global_config_path() {
            if path.exists() {
                println!("  [FOUND] Global:  {}", path.display());
            } else {
                println!("  [     ] Global:  {}", path.display());
            }
        }

        println!("  [     ] Default: built-in defaults");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert!(config.council.models.is_empty());
        assert!(config.council.moderator.is_none());
        assert!(config.behavior.enable_review);
        assert!(config.output.color);
        assert!(config.repl.show_progress);
    }

    #[test]
    fn test_load_defaults() {
        let config = ConfigLoader::load_defaults();
        assert!(config.council.models.is_empty());
        assert!(config.behavior.enable_review);
    }

    #[test]
    fn test_deserialize_toml() {
        let toml_str = r#"
[council]
models = ["claude-sonnet-4.5", "gpt-5.2-codex"]
moderator = "claude-sonnet-4.5"

[behavior]
enable_review = false
timeout_seconds = 120

[output]
format = "full"
color = false

[repl]
show_progress = false
"#;

        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.council.models.len(), 2);
        assert_eq!(
            config.council.moderator,
            Some("claude-sonnet-4.5".to_string())
        );
        assert!(!config.behavior.enable_review);
        assert_eq!(config.behavior.timeout_seconds, Some(120));
        assert_eq!(config.output.format, Some("full".to_string()));
        assert!(!config.output.color);
        assert!(!config.repl.show_progress);
    }
}
