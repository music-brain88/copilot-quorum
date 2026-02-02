//! Configuration file loader with multi-source merging

use super::file_config::FileConfig;
use figment::{
    providers::{Format, Serialized, Toml},
    Figment,
};
use std::path::PathBuf;

/// Configuration loader that handles file discovery and merging
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
    pub fn load(config_path: Option<&PathBuf>) -> Result<FileConfig, Box<figment::Error>> {
        let mut figment = Figment::new().merge(Serialized::defaults(FileConfig::default()));

        // Add global config (XDG or fallback)
        if let Some(global_path) = Self::global_config_path() {
            if global_path.exists() {
                figment = figment.merge(Toml::file(&global_path).nested());
            }
        }

        // Add project-level config files (check both names)
        for filename in &["quorum.toml", ".quorum.toml"] {
            let path = PathBuf::from(filename);
            if path.exists() {
                // Remove nested() to merge at root level
                figment = figment.merge(Toml::file(&path));
                break;
            }
        }

        // Add explicit config path (highest priority for files)
        if let Some(path) = config_path {
            figment = figment.merge(Toml::file(path).nested());
        }

        figment.extract().map_err(Box::new)
    }

    /// Load only default configuration (for --no-config)
    pub fn load_defaults() -> FileConfig {
        FileConfig::default()
    }

    /// Get the global config file path
    ///
    /// Returns XDG_CONFIG_HOME/copilot-quorum/config.toml if set,
    /// otherwise falls back to ~/.config/copilot-quorum/config.toml
    pub fn global_config_path() -> Option<PathBuf> {
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
    fn test_load_defaults() {
        let config = ConfigLoader::load_defaults();
        assert!(config.council.models.is_empty());
        assert!(config.behavior.enable_review);
    }

    #[test]
    fn test_global_config_path_returns_some() {
        // Should return a path (even if file doesn't exist)
        let path = ConfigLoader::global_config_path();
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.to_string_lossy().contains("copilot-quorum"));
    }
}
