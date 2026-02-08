//! Tool implementations for the **Agent Tool System**.
//!
//! This module provides the concrete infrastructure-layer implementations
//! that bring the domain's tool abstractions to life — file I/O, command
//! execution, content search, and web access.
//!
//! # Architecture
//!
//! ```text
//! infrastructure/src/tools/
//! ├── mod.rs          ← Tool registry setup + alias registration (this file)
//! ├── executor.rs     ← LocalToolExecutor (implements ToolExecutorPort)
//! ├── registry.rs     ← ToolRegistry for dynamic provider management
//! ├── builtin.rs      ← BuiltinProvider (wraps all built-in tools)
//! ├── cli.rs          ← CliToolProvider (external CLI tools)
//! ├── file/           ← read_file, write_file
//! ├── command/        ← run_command
//! ├── search/         ← glob_search, grep_search
//! └── web/            ← web_fetch, web_search (feature-gated: `web-tools`)
//! ```
//!
//! # Tool Name Alias System
//!
//! [`default_tool_spec()`] and [`read_only_tool_spec()`] register the canonical
//! tools **and** their aliases. The alias mappings enable the domain-layer
//! [`ToolSpec`] alias system to resolve
//! LLM-hallucinated names at zero cost.
//!
//! Default aliases include:
//!
//! | Alias | Canonical | Available in read-only? |
//! |-------|-----------|:---:|
//! | `bash`, `shell`, `execute` | `run_command` | No |
//! | `view`, `cat`, `open` | `read_file` | Yes |
//! | `edit`, `save` | `write_file` | No |
//! | `glob`, `find`, `find_files` | `glob_search` | Yes |
//! | `grep`, `rg`, `search`, `ripgrep` | `grep_search` | Yes |
//! | `fetch`, `browse` | `web_fetch` | Yes (with `web-tools`) |
//! | `web` | `web_search` | Yes (with `web-tools`) |
//!
//! # Web Tools (Feature-Gated)
//!
//! The `web-tools` feature enables [`web_fetch`](web) and [`web_search`](web)
//! tools, giving agents the ability to access external information.
//! Controlled via `Cargo.toml`:
//!
//! ```toml
//! [features]
//! web-tools = ["dep:reqwest", "dep:scraper"]
//! ```
//!
//! # Providers
//!
//! Tools are organized into providers for extensibility:
//! - **builtin**: Built-in tools (read_file, write_file, etc.) — always available
//! - **cli**: CLI tool wrappers (coming soon)
//! - **mcp**: MCP server tools (coming soon)

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

/// Create the default tool specification with all available tools and aliases.
///
/// Registers all 5 core tools (+ 2 web tools with `web-tools` feature) along with
/// their **Tool Name Alias System** mappings. This is the full-capability spec
/// used by `LocalToolExecutor::new()`.
///
/// See the [module-level documentation](self) for the complete alias table.
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

/// Create a read-only tool specification — no `write_file` or `run_command`.
///
/// Used by `LocalToolExecutor::read_only()` for contexts where state modification
/// is not allowed (e.g. context gathering phase). Excludes high-risk tools
/// and their aliases (`bash`, `shell`, `edit`, `save`, etc.).
///
/// Web tools (`web_fetch`, `web_search`) are included when the `web-tools`
/// feature is enabled, since they are read-only ([`RiskLevel::Low`](quorum_domain::tool::entities::RiskLevel::Low)).
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
