//! OSC 52 based clipboard adapter.
//!
//! Writes an OSC 52 escape sequence to the terminal, letting the terminal
//! emulator perform the clipboard operation. Works over SSH where
//! `arboard` (X11/Wayland/AppKit/Win32 backed) cannot reach a display
//! server. Requires a terminal that honors OSC 52 (iTerm2, WezTerm,
//! kitty, Windows Terminal, Alacritty with config, tmux with
//! `set -g set-clipboard on`, etc.).
//!
//! The sequence format is:
//!
//! ```text
//! ESC ] 52 ; c ; <base64-payload> BEL
//! ```
//!
//! Inside tmux (detected via `$TMUX`), the sequence is wrapped in
//! tmux's passthrough:
//!
//! ```text
//! ESC P tmux ; ESC <payload> ESC \
//! ```

use std::io::{self, Write};
use std::sync::Mutex;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use quorum_application::{ClipboardError, ClipboardPort};

/// Clipboard adapter that emits OSC 52 escape sequences.
///
/// The output writer is captured once at construction time. Defaults to
/// stdout, which is the right target while a ratatui alt-screen TUI is
/// active — the terminal emulator intercepts the escape before it hits
/// the alternate screen buffer.
pub struct Osc52Clipboard {
    writer: Mutex<Box<dyn Write + Send>>,
    inside_tmux: bool,
}

impl Osc52Clipboard {
    /// Construct a new adapter that writes to stdout.
    pub fn new() -> Self {
        Self::with_writer(Box::new(io::stdout()))
    }

    /// Construct with a custom writer (used in tests).
    pub fn with_writer(writer: Box<dyn Write + Send>) -> Self {
        Self {
            writer: Mutex::new(writer),
            inside_tmux: std::env::var("TMUX").is_ok(),
        }
    }

    /// Construct with an explicit tmux-passthrough flag (used in tests).
    pub fn with_writer_tmux(writer: Box<dyn Write + Send>, inside_tmux: bool) -> Self {
        Self {
            writer: Mutex::new(writer),
            inside_tmux,
        }
    }

    fn build_sequence(&self, text: &str) -> Vec<u8> {
        let encoded = STANDARD.encode(text.as_bytes());
        if self.inside_tmux {
            // tmux passthrough: \ePtmux;\e <payload> \e\\
            format!("\x1bPtmux;\x1b\x1b]52;c;{}\x07\x1b\\", encoded).into_bytes()
        } else {
            format!("\x1b]52;c;{}\x07", encoded).into_bytes()
        }
    }
}

impl Default for Osc52Clipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardPort for Osc52Clipboard {
    fn write(&self, text: &str) -> Result<(), ClipboardError> {
        let seq = self.build_sequence(text);
        let mut guard = self.writer.lock().map_err(|e| ClipboardError {
            message: format!("osc52 writer mutex poisoned: {}", e),
        })?;
        guard.write_all(&seq).map_err(|e| ClipboardError {
            message: format!("osc52 write failed: {}", e),
        })?;
        guard.flush().map_err(|e| ClipboardError {
            message: format!("osc52 flush failed: {}", e),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};

    #[derive(Clone, Default)]
    struct SharedBuffer(Arc<StdMutex<Vec<u8>>>);

    impl SharedBuffer {
        fn snapshot(&self) -> Vec<u8> {
            self.0.lock().unwrap().clone()
        }
    }

    impl Write for SharedBuffer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn osc52_encodes_ascii_as_base64_payload() {
        let buf = SharedBuffer::default();
        let cb = Osc52Clipboard::with_writer_tmux(Box::new(buf.clone()), false);
        cb.write("hello").unwrap();
        let out = buf.snapshot();
        // "hello" base64 → "aGVsbG8="
        assert_eq!(out, b"\x1b]52;c;aGVsbG8=\x07");
    }

    #[test]
    fn osc52_encodes_utf8_correctly() {
        let buf = SharedBuffer::default();
        let cb = Osc52Clipboard::with_writer_tmux(Box::new(buf.clone()), false);
        cb.write("こんにちは").unwrap();
        let out = buf.snapshot();
        // UTF-8 "こんにちは" → base64 "44GT44KT44Gr44Gh44Gv"
        assert_eq!(out, b"\x1b]52;c;44GT44KT44Gr44Gh44Gv\x07");
    }

    #[test]
    fn osc52_wraps_sequence_in_tmux_passthrough_when_inside_tmux() {
        let buf = SharedBuffer::default();
        let cb = Osc52Clipboard::with_writer_tmux(Box::new(buf.clone()), true);
        cb.write("hi").unwrap();
        let out = buf.snapshot();
        // tmux passthrough: ESC P tmux ; ESC ESC ]52;c;aGk= BEL ESC \
        assert_eq!(out, b"\x1bPtmux;\x1b\x1b]52;c;aGk=\x07\x1b\\");
    }

    #[test]
    fn osc52_handles_empty_string() {
        let buf = SharedBuffer::default();
        let cb = Osc52Clipboard::with_writer_tmux(Box::new(buf.clone()), false);
        cb.write("").unwrap();
        let out = buf.snapshot();
        assert_eq!(out, b"\x1b]52;c;\x07");
    }
}
