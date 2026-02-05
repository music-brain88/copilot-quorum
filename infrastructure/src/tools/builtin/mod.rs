//! Built-in tool provider module
//!
//! Provides the BuiltinProvider which wraps existing tool implementations.

mod provider;

pub use provider::{BuiltinProvider, BUILTIN_PRIORITY};
