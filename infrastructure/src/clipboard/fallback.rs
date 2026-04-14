//! Chained clipboard adapter that delegates to a primary backend and
//! falls back to a secondary one when the primary cannot serve the
//! request.
//!
//! Typical wiring:
//!
//! - **Primary**: `ArboardClipboard` (OS-native — fast, synchronous,
//!   survives terminal restart).
//! - **Fallback**: `Osc52Clipboard` (works over SSH, no display server
//!   required).

use std::sync::Arc;

use quorum_application::{ClipboardError, ClipboardPort};
use tracing::debug;

/// Two-stage clipboard adapter. Tries `primary` first; on failure,
/// delegates the same write to `fallback`.
pub struct FallbackClipboard {
    primary: Arc<dyn ClipboardPort>,
    fallback: Arc<dyn ClipboardPort>,
}

impl FallbackClipboard {
    pub fn new(primary: Arc<dyn ClipboardPort>, fallback: Arc<dyn ClipboardPort>) -> Self {
        Self { primary, fallback }
    }
}

impl ClipboardPort for FallbackClipboard {
    fn write(&self, text: &str) -> Result<(), ClipboardError> {
        match self.primary.write(text) {
            Ok(()) => Ok(()),
            Err(primary_err) => {
                debug!(
                    primary_error = %primary_err,
                    "primary clipboard failed, delegating to fallback"
                );
                self.fallback.write(text)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct RecordingClipboard {
        name: &'static str,
        fail: bool,
        calls: Mutex<Vec<String>>,
    }

    impl RecordingClipboard {
        fn new(name: &'static str, fail: bool) -> Self {
            Self {
                name,
                fail,
                calls: Mutex::new(Vec::new()),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    impl ClipboardPort for RecordingClipboard {
        fn write(&self, text: &str) -> Result<(), ClipboardError> {
            self.calls.lock().unwrap().push(text.to_string());
            if self.fail {
                Err(ClipboardError {
                    message: format!("{} failed", self.name),
                })
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn primary_success_does_not_call_fallback() {
        let primary = Arc::new(RecordingClipboard::new("primary", false));
        let fallback = Arc::new(RecordingClipboard::new("fallback", false));
        let cb = FallbackClipboard::new(primary.clone(), fallback.clone());
        cb.write("hello").unwrap();
        assert_eq!(primary.call_count(), 1);
        assert_eq!(fallback.call_count(), 0);
    }

    #[test]
    fn primary_failure_delegates_to_fallback() {
        let primary = Arc::new(RecordingClipboard::new("primary", true));
        let fallback = Arc::new(RecordingClipboard::new("fallback", false));
        let cb = FallbackClipboard::new(primary.clone(), fallback.clone());
        cb.write("hello").unwrap();
        assert_eq!(primary.call_count(), 1);
        assert_eq!(fallback.call_count(), 1);
    }

    #[test]
    fn both_failing_surfaces_error() {
        let primary = Arc::new(RecordingClipboard::new("primary", true));
        let fallback = Arc::new(RecordingClipboard::new("fallback", true));
        let cb = FallbackClipboard::new(primary, fallback);
        assert!(cb.write("hello").is_err());
    }
}
