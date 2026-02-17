//! TUI (Text User Interface) module for copilot-quorum
//!
//! Replaces the rustyline-based REPL with a full ratatui TUI.
//! Architecture: Actor pattern separates the AgentController into a background task,
//! with channels for communication between the TUI event loop and the controller.

mod app;
pub mod editor;
mod event;
mod human_intervention;
mod mode;
mod presenter;
mod progress;
mod state;
pub mod tab;
mod widgets;

pub use app::TuiApp;
pub use event::TuiEvent;
pub use human_intervention::TuiHumanIntervention;
pub use mode::{InputMode, KeyAction};
pub use presenter::TuiPresenter;
pub use progress::TuiProgressBridge;
pub use state::{DisplayMessage, MessageRole, ProgressState, TuiInputConfig, TuiState};
pub use tab::{Pane, PaneId, PaneKind, Tab, TabId, TabManager};
