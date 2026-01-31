//! Configuration file support for copilot-quorum
//!
//! Provides loading and merging of configuration from multiple sources
//! with the following priority (highest to lowest):
//!
//! 1. CLI arguments
//! 2. `--config <path>` specified file
//! 3. Project root: `./quorum.toml` or `./.quorum.toml`
//! 4. XDG config: `$XDG_CONFIG_HOME/copilot-quorum/config.toml`
//! 5. Fallback: `~/.config/copilot-quorum/config.toml`
//! 6. Default values

mod loader;

pub use loader::{AppConfig, ConfigLoader};
