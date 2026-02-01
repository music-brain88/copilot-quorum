//! Application-level configuration.
//!
//! This module provides configuration types that control how use cases behave,
//! such as API timeouts and retry policies.

use std::time::Duration;

/// Application behavior configuration.
///
/// Controls runtime behavior of use cases like timeout limits for LLM API calls.
#[derive(Debug, Clone, Default)]
pub struct BehaviorConfig {
    /// Maximum time to wait for an API response before timing out.
    pub timeout: Option<Duration>,
}

impl BehaviorConfig {
    /// Creates a BehaviorConfig with a timeout specified in seconds.
    pub fn with_timeout_seconds(seconds: u64) -> Self {
        Self {
            timeout: Some(Duration::from_secs(seconds)),
        }
    }

    /// Creates a BehaviorConfig from an optional timeout in seconds.
    ///
    /// If `seconds` is `None`, no timeout is applied.
    pub fn from_timeout_seconds(seconds: Option<u64>) -> Self {
        Self {
            timeout: seconds.map(Duration::from_secs),
        }
    }
}
