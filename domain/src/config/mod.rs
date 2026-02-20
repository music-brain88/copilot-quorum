//! Configuration value objects for the domain layer
//!
//! These are domain concepts related to configuration that are
//! used across multiple layers.

pub mod config_key;
mod output_format;

pub use config_key::{ConfigKeyInfo, Mutability, known_keys, lookup_key};
pub use output_format::OutputFormat;
