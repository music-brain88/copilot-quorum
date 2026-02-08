//! Web tools: web_fetch, web_search
//!
//! These tools provide web access capabilities for the agent.
//! Gated behind the `web-tools` feature flag.

mod fetch;
mod search;

pub use fetch::{WEB_FETCH, execute_web_fetch, web_fetch_definition};
pub use search::{WEB_SEARCH, execute_web_search, web_search_definition};
