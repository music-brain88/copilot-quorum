//! TUI (Text User Interface) module for copilot-quorum
//!
//! Replaces the rustyline-based REPL with a full ratatui TUI.
//! Architecture: Actor pattern separates the AgentController into a background task,
//! with channels for communication between the TUI event loop and the controller.

mod app;
pub mod content;
pub mod editor;
mod event;
mod human_intervention;
pub mod layout;
mod mode;
mod presenter;
mod progress;
mod route;
mod state;
mod surface;
pub mod tab;
mod widgets;

pub use app::TuiApp;
pub use content::{
    ContentRegistry, ContentRenderer, ContentSlot, ConversationContent, ProgressContent,
};
pub use event::TuiEvent;
pub use human_intervention::TuiHumanIntervention;
pub use layout::{
    LayoutPreset, TuiLayoutConfig, content_slot_to_string, parse_content_slot, parse_surface_id,
    surface_id_to_string,
};
pub use mode::{InputMode, KeyAction};
pub use presenter::TuiPresenter;
pub use progress::TuiProgressBridge;
pub use route::RouteTable;
pub use state::{DisplayMessage, MessageRole, ProgressState, TuiInputConfig, TuiState};
pub use tab::{Pane, PaneId, PaneKind, Tab, TabId, TabManager};
