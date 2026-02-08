//! **Web Tools** — `web_fetch` and `web_search`
//!
//! This module gives agents the ability to access external web information,
//! gated behind the `web-tools` Cargo feature flag.
//!
//! # Tools
//!
//! | Tool | Description | Risk | Key Dependency |
//! |------|-----------|------|----------------|
//! | `web_fetch` | Fetch a URL, extract readable text from HTML | Low | `reqwest` + `scraper` |
//! | `web_search` | Search the web via DuckDuckGo Instant Answer API | Low | `reqwest` |
//!
//! Both tools are [`RiskLevel::Low`](quorum_domain::tool::entities::RiskLevel::Low)
//! (read-only), so they execute directly without Quorum review.
//!
//! # Feature Gate
//!
//! These tools are only available when the `web-tools` feature is enabled:
//!
//! ```toml
//! # infrastructure/Cargo.toml
//! [features]
//! web-tools = ["dep:reqwest", "dep:scraper"]
//!
//! # cli/Cargo.toml (enabled by default for end users)
//! [features]
//! default = ["web-tools"]
//! web-tools = ["quorum-infrastructure/web-tools"]
//! ```
//!
//! # Alias Support
//!
//! Registered in [`default_tool_spec()`](super::default_tool_spec):
//! - `fetch`, `browse` → `web_fetch`
//! - `web` → `web_search`
//!
//! # Architecture
//!
//! Web tools require an async runtime (they use `reqwest`). The
//! [`LocalToolExecutor`](super::LocalToolExecutor) routes them through
//! `execute_async()` in the `execute()` path and uses
//! `tokio::task::block_in_place` in the `execute_sync()` path.

mod fetch;
mod search;

pub use fetch::{WEB_FETCH, execute_web_fetch, web_fetch_definition};
pub use search::{WEB_SEARCH, execute_web_search, web_search_definition};
