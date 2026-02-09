//! TUI (Text User Interface) module for copilot-quorum
//!
//! This module provides a terminal-based interactive UI using ratatui.
//! It integrates agent REPL, progress reporting, and human intervention
//! into a unified multi-pane interface.

mod app;
mod event;
mod human_intervention;
mod mode;
mod presenter;
mod progress;
mod state;
mod widgets;

pub use app::TuiApp;
pub use event::{Event, TuiEvent};
pub use human_intervention::TuiHumanIntervention;
pub use mode::{Action, KeyHandler, Mode, ReplMode};
pub use presenter::TuiPresenter;
pub use progress::TuiProgressReporter;
pub use state::{
    AgentStatus, AppState, Message, MessageEntry, MessageRole, ProgressState, TuiMode, TuiState,
};
pub use widgets::{
    ConversationWidget, HelpWidget, InputWidget, ProgressWidget, StatusWidget, TuiLayout,
};
