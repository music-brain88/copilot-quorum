//! Clipboard port — abstract interface for writing text to the OS clipboard.
//!
//! Infrastructure adapters implement this trait (e.g. `ArboardClipboard`).
//! A `NoClipboard` fallback is provided for environments where clipboard
//! access is unavailable or intentionally disabled.

/// Error from a clipboard write operation.
#[derive(Debug, Clone)]
pub struct ClipboardError {
    pub message: String,
}

impl std::fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "clipboard error: {}", self.message)
    }
}

impl std::error::Error for ClipboardError {}

/// Port for writing text to the system clipboard.
pub trait ClipboardPort: Send + Sync {
    /// Write `text` to the system clipboard, replacing any existing content.
    fn write(&self, text: &str) -> Result<(), ClipboardError>;
}

/// No-op clipboard used when no real adapter is wired in. All writes fail.
pub struct NoClipboard;

impl ClipboardPort for NoClipboard {
    fn write(&self, _text: &str) -> Result<(), ClipboardError> {
        Err(ClipboardError {
            message: "clipboard unavailable".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_clipboard_write_errors() {
        let cb = NoClipboard;
        let err = cb.write("hello").unwrap_err();
        assert_eq!(err.message, "clipboard unavailable");
    }
}
