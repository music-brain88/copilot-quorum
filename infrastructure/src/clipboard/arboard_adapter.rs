//! `arboard` backed adapter for `ClipboardPort`.
//!
//! The underlying `arboard::Clipboard` handle is `!Sync`, so we wrap it in a
//! `Mutex` to get a `Send + Sync` type. Initialization can fail on headless
//! Linux systems lacking X11/Wayland libraries — in that case the adapter is
//! still constructed, and every `write()` call returns a `ClipboardError`
//! instead of panicking.

use std::sync::Mutex;

use quorum_application::{ClipboardError, ClipboardPort};

/// Clipboard adapter backed by the `arboard` crate.
pub struct ArboardClipboard {
    // `None` when `arboard::Clipboard::new()` failed at construction time.
    inner: Mutex<Option<arboard::Clipboard>>,
    init_error: Option<String>,
}

impl ArboardClipboard {
    /// Construct a new adapter. Never panics — initialization errors are
    /// deferred to the first `write()` call.
    pub fn new() -> Self {
        match arboard::Clipboard::new() {
            Ok(cb) => Self {
                inner: Mutex::new(Some(cb)),
                init_error: None,
            },
            Err(e) => Self {
                inner: Mutex::new(None),
                init_error: Some(e.to_string()),
            },
        }
    }
}

impl Default for ArboardClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardPort for ArboardClipboard {
    fn write(&self, text: &str) -> Result<(), ClipboardError> {
        let mut guard = self.inner.lock().map_err(|e| ClipboardError {
            message: format!("clipboard mutex poisoned: {}", e),
        })?;

        let cb = match guard.as_mut() {
            Some(cb) => cb,
            None => {
                return Err(ClipboardError {
                    message: self
                        .init_error
                        .clone()
                        .unwrap_or_else(|| "clipboard unavailable".to_string()),
                });
            }
        };

        cb.set_text(text.to_string()).map_err(|e| ClipboardError {
            message: e.to_string(),
        })
    }
}
