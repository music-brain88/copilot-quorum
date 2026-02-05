//! CLI tool provider module
//!
//! Wraps external CLI tools (grep, find, rg, fd, etc.) as Quorum tools.
//! Includes discovery for suggesting enhanced tools.

mod discovery;
mod provider;

pub use discovery::{
    discover_additional_tools, discover_enhanced_tools, generate_enhanced_aliases, DetectedTool,
    DiscoveryResult,
};
pub use provider::{CliToolProvider, CLI_PRIORITY};
