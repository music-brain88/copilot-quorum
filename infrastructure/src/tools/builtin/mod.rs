//! Built-in tool provider module
//!
//! Provides the BuiltinProvider which wraps existing tool implementations.

mod provider;

pub use provider::{BUILTIN_PRIORITY, BuiltinProvider};
