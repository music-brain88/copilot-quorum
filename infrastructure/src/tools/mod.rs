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
//! ├── mod.rs              ← Tool registry setup (this file)
//! ├── executor.rs         ← LocalToolExecutor (implements ToolExecutorPort)
//! ├── registry.rs         ← ToolRegistry for dynamic provider management
//! ├── builtin.rs          ← BuiltinProvider (wraps all built-in tools)
//! ├── cli.rs              ← CliToolProvider (external CLI tools)
//! ├── custom_provider.rs  ← CustomToolProvider (user-defined tools)
//! ├── file/               ← read_file, write_file
//! ├── command/            ← run_command
//! ├── search/             ← glob_search, grep_search
//! └── web/                ← web_fetch, web_search (feature-gated: `web-tools`)
//! ```
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
//! - **custom**: User-defined tools via `quorum.toml` — wraps shell commands
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

pub mod custom_provider;
pub mod schema;

pub use builtin::BuiltinProvider;
pub use cli::CliToolProvider;
pub use custom_provider::CustomToolProvider;
pub use executor::LocalToolExecutor;
pub use registry::{RegistryStats, ToolRegistry};
pub use schema::JsonSchemaToolConverter;

use quorum_domain::tool::entities::ToolSpec;

/// Create the default tool specification with all available tools.
///
/// Registers all 5 core tools (+ 2 web tools with `web-tools` feature).
/// This is the full-capability spec used by `LocalToolExecutor::new()`.
#[allow(unused_mut)]
pub fn default_tool_spec() -> ToolSpec {
    let mut spec = ToolSpec::new()
        .register(file::read_file_definition())
        .register(file::write_file_definition())
        .register(command::run_command_definition())
        .register(search::glob_search_definition())
        .register(search::grep_search_definition());

    #[cfg(feature = "web-tools")]
    {
        spec = spec
            .register(web::web_fetch_definition())
            .register(web::web_search_definition());
    }

    spec
}

/// Create a read-only tool specification — no `write_file` or `run_command`.
///
/// Used by `LocalToolExecutor::read_only()` for contexts where state modification
/// is not allowed (e.g. context gathering phase).
///
/// Web tools (`web_fetch`, `web_search`) are included when the `web-tools`
/// feature is enabled, since they are read-only ([`RiskLevel::Low`](quorum_domain::tool::entities::RiskLevel::Low)).
#[allow(unused_mut)]
pub fn read_only_tool_spec() -> ToolSpec {
    let mut spec = ToolSpec::new()
        .register(file::read_file_definition())
        .register(search::glob_search_definition())
        .register(search::grep_search_definition());

    #[cfg(feature = "web-tools")]
    {
        spec = spec
            .register(web::web_fetch_definition())
            .register(web::web_search_definition());
    }

    spec
}
