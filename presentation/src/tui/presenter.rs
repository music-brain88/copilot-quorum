//! TUI Presenter — converts UiEvent → TuiState mutations
//!
//! Pure state-update logic with no terminal I/O.
//! Each UiEvent is mapped to one or more TuiState changes and/or TuiEvent emissions.

use super::event::TuiEvent;
use super::state::{DisplayMessage, TuiState};
use quorum_application::{
    AgentErrorEvent, AgentResultEvent, ConfigSnapshot, ContextInitResultEvent, QuorumResultEvent,
    UiEvent, WelcomeInfo,
};
use tokio::sync::mpsc;

/// Stateless presenter: applies UiEvents to TuiState and emits TuiEvents for rendering updates
pub struct TuiPresenter {
    event_tx: mpsc::UnboundedSender<TuiEvent>,
}

impl TuiPresenter {
    pub fn new(event_tx: mpsc::UnboundedSender<TuiEvent>) -> Self {
        Self { event_tx }
    }

    /// Apply a UiEvent: update state and emit rendering events
    pub fn apply(&self, state: &mut TuiState, event: &UiEvent) {
        match event {
            UiEvent::Welcome(info) => self.handle_welcome(state, info),
            UiEvent::Help => self.emit(TuiEvent::Flash("Type :help or ? for commands".into())),
            UiEvent::ConfigDisplay(snapshot) => self.handle_config(state, snapshot),
            UiEvent::ModeChanged { level, description } => {
                state.consensus_level = *level;
                self.emit(TuiEvent::ModeChanged {
                    level: *level,
                    description: description.clone(),
                });
                state.set_flash(format!("Mode: {}", level));
            }
            UiEvent::ScopeChanged { scope, description } => {
                state.phase_scope = *scope;
                self.emit(TuiEvent::ScopeChanged(description.clone()));
                state.set_flash(format!("Scope: {:?}", scope));
            }
            UiEvent::StrategyChanged {
                strategy,
                description,
            } => {
                self.emit(TuiEvent::StrategyChanged(description.clone()));
                state.set_flash(format!("Strategy: {}", strategy));
            }
            UiEvent::HistoryCleared => {
                state.messages.clear();
                state.streaming_text.clear();
                self.emit(TuiEvent::HistoryCleared);
                state.set_flash("History cleared");
            }
            UiEvent::VerboseStatus { enabled } => {
                state.set_flash(format!(
                    "Verbose: {}",
                    if *enabled { "ON" } else { "OFF" }
                ));
            }
            UiEvent::AgentStarting { mode } => {
                state.progress.is_running = true;
                state.consensus_level = *mode;
                self.emit(TuiEvent::AgentStarting);
            }
            UiEvent::AgentResult(result) => self.handle_agent_result(state, result),
            UiEvent::AgentError(error) => self.handle_agent_error(state, error),
            UiEvent::QuorumStarting => {
                state
                    .messages
                    .push(DisplayMessage::system("Quorum Discussion starting..."));
            }
            UiEvent::QuorumResult(result) => self.handle_quorum_result(state, result),
            UiEvent::QuorumError { error } => {
                state
                    .messages
                    .push(DisplayMessage::system(format!("Quorum error: {}", error)));
                self.emit(TuiEvent::AgentError(error.clone()));
            }
            UiEvent::ContextInitStarting { model_count } => {
                state.messages.push(DisplayMessage::system(format!(
                    "Initializing context with {} models...",
                    model_count
                )));
            }
            UiEvent::ContextInitResult(result) => {
                self.handle_context_init_result(state, result);
            }
            UiEvent::ContextInitError { error } => {
                state.messages.push(DisplayMessage::system(format!(
                    "Context init failed: {}",
                    error
                )));
            }
            UiEvent::ContextAlreadyExists => {
                state.set_flash("Context file already exists. Use /init --force to regenerate.");
            }
            UiEvent::CommandError { message } => {
                self.emit(TuiEvent::CommandError(message.clone()));
                state.set_flash(format!("Error: {}", message));
            }
            UiEvent::UnknownCommand { command } => {
                state.set_flash(format!("Unknown command: {}. Type ? for help", command));
            }
            UiEvent::Exit => {
                state.should_quit = true;
                self.emit(TuiEvent::Exit);
            }
        }
    }

    fn handle_welcome(&self, state: &mut TuiState, info: &WelcomeInfo) {
        state.model_name = info.decision_model.to_string();
        state.consensus_level = info.consensus_level;
        state.messages.push(DisplayMessage::system(format!(
            "Welcome! Model: {} | Mode: {}",
            info.decision_model, info.consensus_level
        )));
        self.emit(TuiEvent::Welcome {
            decision_model: info.decision_model.to_string(),
            consensus_level: info.consensus_level,
        });
    }

    fn handle_config(&self, state: &mut TuiState, snapshot: &ConfigSnapshot) {
        let config_text = format!(
            "Decision: {} | Review: {} | Scope: {} | HiL: {}",
            snapshot.decision_model,
            if snapshot.review_models.is_empty() {
                "None".to_string()
            } else {
                snapshot
                    .review_models
                    .iter()
                    .map(|m| m.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            },
            snapshot.phase_scope,
            snapshot.hil_mode,
        );
        self.emit(TuiEvent::ConfigDisplay(config_text.clone()));
        state.messages.push(DisplayMessage::system(config_text));
    }

    fn handle_agent_result(&self, state: &mut TuiState, result: &AgentResultEvent) {
        state.finalize_stream();
        state.progress.is_running = false;
        state.progress.current_phase = None;
        state.progress.current_tool = None;

        let status = if result.success { "completed" } else { "failed" };
        state
            .messages
            .push(DisplayMessage::system(format!("Agent {}", status)));

        if !result.summary.is_empty() {
            state
                .messages
                .push(DisplayMessage::assistant(result.summary.clone()));
        }

        self.emit(TuiEvent::AgentResult {
            success: result.success,
            summary: result.summary.clone(),
        });
    }

    fn handle_agent_error(&self, state: &mut TuiState, error: &AgentErrorEvent) {
        state.finalize_stream();
        state.progress.is_running = false;
        state.progress.current_phase = None;

        let msg = if error.cancelled {
            "Operation cancelled".to_string()
        } else {
            format!("Error: {}", error.error)
        };
        state.messages.push(DisplayMessage::system(msg.clone()));
        self.emit(TuiEvent::AgentError(msg));
    }

    fn handle_quorum_result(&self, state: &mut TuiState, result: &QuorumResultEvent) {
        state
            .messages
            .push(DisplayMessage::assistant(result.formatted_output.clone()));
        self.emit(TuiEvent::AgentResult {
            success: true,
            summary: "Quorum discussion complete".into(),
        });
    }

    fn handle_context_init_result(&self, state: &mut TuiState, result: &ContextInitResultEvent) {
        state.messages.push(DisplayMessage::system(format!(
            "Context saved to: {}",
            result.path
        )));
        state.set_flash("Context initialized successfully");
    }

    fn emit(&self, event: TuiEvent) {
        let _ = self.event_tx.send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quorum_domain::ConsensusLevel;

    fn setup() -> (TuiPresenter, mpsc::UnboundedReceiver<TuiEvent>, TuiState) {
        let (tx, rx) = mpsc::unbounded_channel();
        let presenter = TuiPresenter::new(tx);
        let state = TuiState::new();
        (presenter, rx, state)
    }

    #[test]
    fn test_welcome_updates_state() {
        let (presenter, _rx, mut state) = setup();
        let info = WelcomeInfo {
            decision_model: quorum_domain::Model::ClaudeSonnet45,
            review_models: vec![],
            moderator: None,
            working_dir: None,
            consensus_level: ConsensusLevel::Solo,
        };

        presenter.apply(&mut state, &UiEvent::Welcome(info));
        assert_eq!(state.consensus_level, ConsensusLevel::Solo);
        assert!(!state.model_name.is_empty());
        assert_eq!(state.messages.len(), 1);
    }

    #[test]
    fn test_mode_changed_updates_state() {
        let (presenter, _rx, mut state) = setup();
        presenter.apply(
            &mut state,
            &UiEvent::ModeChanged {
                level: ConsensusLevel::Ensemble,
                description: "Multi-model".into(),
            },
        );
        assert_eq!(state.consensus_level, ConsensusLevel::Ensemble);
        assert!(state.flash_message.is_some());
    }

    #[test]
    fn test_history_cleared() {
        let (presenter, _rx, mut state) = setup();
        state.messages.push(DisplayMessage::user("test"));
        state.streaming_text = "streaming".into();

        presenter.apply(&mut state, &UiEvent::HistoryCleared);
        assert!(state.messages.is_empty());
        assert!(state.streaming_text.is_empty());
    }

    #[test]
    fn test_exit_sets_quit() {
        let (presenter, _rx, mut state) = setup();
        presenter.apply(&mut state, &UiEvent::Exit);
        assert!(state.should_quit);
    }

    #[test]
    fn test_agent_error_finalizes_stream() {
        let (presenter, _rx, mut state) = setup();
        state.streaming_text = "partial".into();
        state.progress.is_running = true;

        presenter.apply(
            &mut state,
            &UiEvent::AgentError(AgentErrorEvent {
                cancelled: false,
                error: "test error".into(),
            }),
        );

        assert!(!state.progress.is_running);
        // streaming text finalized + error message
        assert!(state.messages.len() >= 2);
    }
}
