//! Clipboard adapters — implementations of `ClipboardPort`.

pub mod arboard_adapter;
pub mod fallback;
pub mod osc52_adapter;

pub use arboard_adapter::ArboardClipboard;
pub use fallback::FallbackClipboard;
pub use osc52_adapter::Osc52Clipboard;
