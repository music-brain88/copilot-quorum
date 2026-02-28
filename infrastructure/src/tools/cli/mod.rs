//! CLI tool provider module
//!
//! Wraps external CLI tools (grep, find, rg, fd, etc.) as Quorum tools.
//! Includes discovery for suggesting enhanced tools.

pub mod config;
mod discovery;
mod provider;

pub use discovery::{
    DetectedTool, DiscoveryResult, discover_additional_tools, discover_enhanced_tools,
    generate_enhanced_aliases,
};
pub use provider::{CLI_PRIORITY, CliToolProvider};
