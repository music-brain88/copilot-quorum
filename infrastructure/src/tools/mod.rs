//! Tool implementations for the agent system
//!
//! This module provides concrete implementations of tools that
//! can be used by the agent to interact with the local file system
//! and execute commands.
//!
//! ## Providers
//!
//! Tools are organized into providers:
//! - `builtin`: Built-in tools (read_file, write_file, etc.) - always available
//! - `cli`: CLI tool wrappers (coming soon)
//! - `mcp`: MCP server tools (coming soon)

pub mod builtin;
pub mod cli;
pub mod command;
pub mod file;
pub mod search;

mod executor;
mod registry;

pub use builtin::BuiltinProvider;
pub use cli::CliToolProvider;
pub use executor::LocalToolExecutor;
pub use registry::{RegistryStats, ToolRegistry};

use quorum_domain::tool::entities::ToolSpec;

/// Create the default tool specification with all available tools
pub fn default_tool_spec() -> ToolSpec {
    ToolSpec::new()
        .register(file::read_file_definition())
        .register(file::write_file_definition())
        .register(command::run_command_definition())
        .register(search::glob_search_definition())
        .register(search::grep_search_definition())
}

/// Get definitions for low-risk (read-only) tools only
pub fn read_only_tool_spec() -> ToolSpec {
    ToolSpec::new()
        .register(file::read_file_definition())
        .register(search::glob_search_definition())
        .register(search::grep_search_definition())
}
