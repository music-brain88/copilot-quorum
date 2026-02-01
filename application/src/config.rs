//! Application-level configuration
//!
//! Configuration for use case behavior like timeouts and retries.

use std::time::Duration;

/// Application behavior configuration
#[derive(Debug, Clone)]
pub struct BehaviorConfig {
    /// Timeout for API calls
    pub timeout: Option<Duration>,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self { timeout: None }
    }
}

impl BehaviorConfig {
    /// Create a new BehaviorConfig with the given timeout in seconds
    pub fn with_timeout_seconds(seconds: u64) -> Self {
        Self {
            timeout: Some(Duration::from_secs(seconds)),
        }
    }

    /// Create from optional timeout seconds
    pub fn from_timeout_seconds(seconds: Option<u64>) -> Self {
        Self {
            timeout: seconds.map(Duration::from_secs),
        }
    }
}
