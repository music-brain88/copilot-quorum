//! Configuration file loading for copilot-quorum
//!
//! This module handles file I/O and merging of configuration from multiple sources.
//! The priority order (highest to lowest):
//!
//! 1. `--config <path>` specified file
//! 2. Project root: `./quorum.toml` or `./.quorum.toml`
//! 3. XDG config: `$XDG_CONFIG_HOME/copilot-quorum/config.toml`
//! 4. Fallback: `~/.config/copilot-quorum/config.toml`
//! 5. Default values

mod file_config;
mod loader;

pub use file_config::{
    EnhancedToolConfig, FileAgentConfig, FileBuiltinToolsConfig, FileCliToolsConfig, FileConfig,
    FileContextBudgetConfig, FileCustomToolConfig, FileCustomToolParameter, FileMcpServerConfig,
    FileMcpToolsConfig, FileModelsConfig, FileOutputConfig, FileOutputFormat, FileProvidersConfig,
    FileReplConfig, FileToolsConfig, FileTuiConfig, FileTuiInputConfig, FileTuiLayoutConfig,
    FileTuiRoutesConfig, FileTuiSurfaceConfig, FileTuiSurfacesConfig, ToolProviderType,
};
pub use loader::ConfigLoader;
