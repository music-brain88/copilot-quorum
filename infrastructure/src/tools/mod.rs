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
#[cfg(feature = "web-tools")]
pub mod web;

mod executor;
mod registry;

pub use builtin::BuiltinProvider;
pub use cli::CliToolProvider;
pub use executor::LocalToolExecutor;
pub use registry::{RegistryStats, ToolRegistry};

use quorum_domain::tool::entities::ToolSpec;

/// Create the default tool specification with all available tools
#[allow(unused_mut)]
pub fn default_tool_spec() -> ToolSpec {
    let mut spec = ToolSpec::new()
        .register(file::read_file_definition())
        .register(file::write_file_definition())
        .register(command::run_command_definition())
        .register(search::glob_search_definition())
        .register(search::grep_search_definition())
        .register_aliases([
            // run_command aliases
            ("bash", "run_command"),
            ("shell", "run_command"),
            ("execute", "run_command"),
            // read_file aliases
            ("view", "read_file"),
            ("cat", "read_file"),
            ("open", "read_file"),
            // write_file aliases
            ("edit", "write_file"),
            ("save", "write_file"),
            // glob_search aliases
            ("glob", "glob_search"),
            ("find", "glob_search"),
            ("find_files", "glob_search"),
            // grep_search aliases
            ("grep", "grep_search"),
            ("rg", "grep_search"),
            ("search", "grep_search"),
            ("ripgrep", "grep_search"),
        ]);

    #[cfg(feature = "web-tools")]
    {
        spec = spec
            .register(web::web_fetch_definition())
            .register(web::web_search_definition())
            .register_aliases([
                ("fetch", "web_fetch"),
                ("browse", "web_fetch"),
                ("web", "web_search"),
            ]);
    }

    spec
}

/// Get definitions for low-risk (read-only) tools only
#[allow(unused_mut)]
pub fn read_only_tool_spec() -> ToolSpec {
    let mut spec = ToolSpec::new()
        .register(file::read_file_definition())
        .register(search::glob_search_definition())
        .register(search::grep_search_definition())
        .register_aliases([
            // read_file aliases
            ("view", "read_file"),
            ("cat", "read_file"),
            ("open", "read_file"),
            // glob_search aliases
            ("glob", "glob_search"),
            ("find", "glob_search"),
            ("find_files", "glob_search"),
            // grep_search aliases
            ("grep", "grep_search"),
            ("rg", "grep_search"),
            ("search", "grep_search"),
            ("ripgrep", "grep_search"),
        ]);

    #[cfg(feature = "web-tools")]
    {
        spec = spec
            .register(web::web_fetch_definition())
            .register(web::web_search_definition())
            .register_aliases([
                ("fetch", "web_fetch"),
                ("browse", "web_fetch"),
                ("web", "web_search"),
            ]);
    }

    spec
}
