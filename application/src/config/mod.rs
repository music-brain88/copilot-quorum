//! Application-level configuration.
//!
//! This module provides configuration types that control how use cases behave:
//!
//! - [`ExecutionParams`] — execution loop control (iterations, tool turns, timeouts)
//! - [`QuorumConfig`] — 4-type container for buffer controller propagation

pub mod execution_params;
pub mod quorum_config;

pub use execution_params::ExecutionParams;
pub use quorum_config::QuorumConfig;
