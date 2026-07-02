//! TUI Presenter — converts UiEvent → TuiState mutations
//!
//! Pure state-update logic with no terminal I/O.
//! Each UiEvent is mapped to one or more TuiState changes and/or TuiEvent emissions.

use super::event::{RoutedTuiEvent, TuiEvent};
use super::state::{DisplayMessage, TuiState};
use super::tab::PaneKind;
use quorum_application::{
    AgentErrorEvent, AgentResultEvent, AskResultEvent, ConfigSnapshot, ContextInitResultEvent,
    QuorumResultEvent, UiEvent, WelcomeInfo,
};
use tokio::sync::mpsc;

/// Stateless presenter: applies UiEvents to TuiState and emits TuiEvents for rendering updates
pub struct TuiPresenter {
    event_tx: mpsc::UnboundedSender<RoutedTuiEvent>,
}

impl TuiPresenter {
    pub fn new(event_tx: mpsc::UnboundedSender<RoutedTuiEvent>) -> Self {
        Self { event_tx }
    }

    /// Apply a UiEvent: update state and emit rendering events
    pub fn apply(&self, state: &mut TuiState, event: &UiEvent) {
        match event {
            UiEvent::Welcome(info) => self.handle_welcome(state, info),
            UiEvent::Help => {
                state.show_help = true;
                state.set_flash("Press ? or Esc to close");
            }
            UiEvent::ConfigDisplay(snapshot) => self.handle_config(state, snapshot),
            UiEvent::ModeChanged { level, description } => {
                state.consensus_level = *level;
                state.push_message(DisplayMessage::system(format!(
                    "Mode changed: {} ({})",
                    level, description
                )));
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
                let pane = state.tabs.active_pane_mut();
                pane.conversation.messages.clear();
                pane.conversation.streaming_text.clear();
                self.emit(TuiEvent::HistoryCleared);
                state.set_flash("History cleared");
            }
            UiEvent::VerboseStatus { enabled } => {
                state.set_flash(format!("Verbose: {}", if *enabled { "ON" } else { "OFF" }));
            }
            UiEvent::AgentStarting { mode } => {
                state.tabs.active_pane_mut().progress.is_running = true;
                state.consensus_level = *mode;
                self.emit(TuiEvent::AgentStarting);
            }
            UiEvent::AgentResult(result) => self.handle_agent_result(state, result),
            UiEvent::AgentError(error) => self.handle_agent_error(state, error),
            UiEvent::AskStarting => {
                state.push_message(DisplayMessage::system("Ask starting..."));
            }
            UiEvent::AskResult(result) => self.handle_ask_result(state, result),
            UiEvent::AskError { error } => {
                state.push_message(DisplayMessage::system(format!("Ask error: {}", error)));
                self.emit(TuiEvent::AgentError(error.clone()));
            }
            UiEvent::InteractionSpawned(event) => {
                // Fix A: Try to bind the interaction_id to an existing placeholder tab
                // (created immediately by handle_tab_command). If no placeholder exists
                // (e.g., programmatic spawn), create a new tab as before.
                if !state.tabs.bind_interaction_id(event.form, event.id) {
                    let kind = PaneKind::Interaction(event.form, Some(event.id));
                    state.tabs.create_tab(kind);
                }
                state
                    .tabs
                    .active_pane_mut()
                    .set_title_if_empty(&event.query);
                // Echo the query as a user message so the conversation shows
                // what the interaction is answering (issue #274). The root
                // interaction is spawned with an empty query — skip it.
                if !event.query.is_empty() {
                    state.push_message_to(event.id, DisplayMessage::user(event.query.clone()));
                }
            }
            UiEvent::InteractionCompleted(event) => {
                // Root interaction completions (parent_id = None) are not propagated;
                // only child completions need to notify their parent's tab.
                if let Some(parent_id) = event.parent_id {
                    let _ = self.event_tx.send(RoutedTuiEvent::for_interaction(
                        parent_id,
                        TuiEvent::InteractionCompleted {
                            parent_id: Some(parent_id),
                            result_text: event.result_text.clone(),
                        },
                    ));
                }
            }
            UiEvent::InteractionSpawnError { error } => {
                let message = format!("Interaction spawn error: {}", error);
                state.push_message(DisplayMessage::system(message.clone()));
                self.emit(TuiEvent::Flash(message));
            }
            UiEvent::QuorumStarting => {
                state.push_message(DisplayMessage::system("Quorum Discussion starting..."));
            }
            UiEvent::QuorumResult(result) => self.handle_quorum_result(state, result),
            UiEvent::QuorumError { error } => {
                state.push_message(DisplayMessage::system(format!("Quorum error: {}", error)));
                self.emit(TuiEvent::AgentError(error.clone()));
            }
            UiEvent::ContextInitStarting { model_count } => {
                // Mark the pane as running so the header shows the live phase
                // instead of "Ready" while context generation is in flight.
                {
                    let progress = &mut state.tabs.active_pane_mut().progress;
                    progress.is_running = true;
                    progress.phase_name = "Gathering Context".to_string();
                }
                state.push_message(DisplayMessage::system(format!(
                    "Initializing context with {} models...",
                    model_count
                )));
            }
            UiEvent::ContextInitProgress { message } => {
                state.push_message(DisplayMessage::system(message.clone()));
            }
            UiEvent::ContextInitResult(result) => {
                self.handle_context_init_result(state, result);
            }
            UiEvent::ContextInitError { error } => {
                state.push_message(DisplayMessage::system(format!(
                    "Context init failed: {}",
                    error
                )));
                Self::reset_progress(state);
            }
            UiEvent::ContextAlreadyExists => {
                state.set_flash("Context file already exists. Use :init! to regenerate.");
            }
            UiEvent::CommandError { message } => {
                self.emit(TuiEvent::CommandError(message.clone()));
                state.set_flash(format!("Error: {}", message));
            }
            UiEvent::UnknownCommand { command } => {
                state.set_flash(format!("Unknown command: :{}. Type ? for help", command));
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
        state.push_message(DisplayMessage::system(format!(
            "Welcome! Model: {}",
            info.decision_model
        )));
        self.emit(TuiEvent::Welcome {
            decision_model: info.decision_model.to_string(),
            consensus_level: info.consensus_level,
        });
    }

    fn handle_config(&self, state: &mut TuiState, snapshot: &ConfigSnapshot) {
        let config_text = format_config_snapshot(snapshot);
        self.emit(TuiEvent::ConfigDisplay(config_text.clone()));
        state.push_message(DisplayMessage::system(config_text));
    }

    fn handle_agent_result(&self, state: &mut TuiState, result: &AgentResultEvent) {
        state.finalize_stream();
        {
            let progress = &mut state.tabs.active_pane_mut().progress;
            progress.is_running = false;
            progress.current_phase = None;
        }

        let status = if result.success {
            "completed"
        } else {
            "failed"
        };
        state.push_message(DisplayMessage::system(format!("Agent {}", status)));

        if !result.summary.is_empty() {
            state.push_message(DisplayMessage::assistant(result.summary.clone()));
        }

        self.emit(TuiEvent::AgentResult {
            success: result.success,
            summary: result.summary.clone(),
        });
    }

    fn handle_agent_error(&self, state: &mut TuiState, error: &AgentErrorEvent) {
        state.finalize_stream();
        {
            let progress = &mut state.tabs.active_pane_mut().progress;
            progress.is_running = false;
            progress.current_phase = None;
        }

        let msg = if error.cancelled {
            "Operation cancelled".to_string()
        } else {
            format!("Error: {}", error.error)
        };
        state.push_message(DisplayMessage::system(msg.clone()));
        self.emit(TuiEvent::AgentError(msg));
    }

    fn handle_ask_result(&self, _state: &mut TuiState, _result: &AskResultEvent) {
        // The answer is displayed via streaming: StreamChunk accumulates into
        // streaming_text and StreamEnd finalizes it as an assistant message
        // (routed to the correct interaction pane). Pushing result.answer here
        // would duplicate it (#267), so only emit the completion event.
        self.emit(TuiEvent::AgentResult {
            success: true,
            summary: "Ask completed".into(),
        });
    }

    fn handle_quorum_result(&self, state: &mut TuiState, result: &QuorumResultEvent) {
        state.push_message(DisplayMessage::assistant(result.formatted_output.clone()));
        self.emit(TuiEvent::AgentResult {
            success: true,
            summary: "Quorum discussion complete".into(),
        });
    }

    fn handle_context_init_result(&self, state: &mut TuiState, result: &ContextInitResultEvent) {
        state.push_message(DisplayMessage::system(format!(
            "Context saved to: {}",
            result.path
        )));
        state.set_flash("Context initialized successfully");
        Self::reset_progress(state);
    }

    /// Return the Progress pane to its idle state after context init finishes.
    fn reset_progress(state: &mut TuiState) {
        let progress = &mut state.tabs.active_pane_mut().progress;
        progress.current_phase = None;
        progress.quorum_status = None;
        progress.is_running = false;
    }

    fn emit(&self, event: TuiEvent) {
        let _ = self.event_tx.send(RoutedTuiEvent::global(event));
    }
}

/// Format a config snapshot grouped by section, one line per section:
///
/// ```text
/// [agent] consensus_level=solo  phase_scope=full  strategy=quorum  ...
/// [models] exploration=...  decision=...  review=[...]  ...
/// [runtime] working_dir=(current)  verbose=false  history=0
/// ```
fn format_config_snapshot(snapshot: &ConfigSnapshot) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut current_section = "";
    for entry in &snapshot.entries {
        if entry.section() != current_section {
            current_section = entry.section();
            lines.push(format!("[{}]", current_section));
        }
        let line = lines.last_mut().expect("section header pushed above");
        line.push_str(&format!("  {}={}", entry.name(), entry.value));
    }
    // Runtime info is not part of the key registry — only shown unfiltered
    if snapshot.section_filter.is_none() {
        lines.push(format!(
            "[runtime]  working_dir={}  verbose={}  history={}",
            snapshot.working_dir.as_deref().unwrap_or("(current)"),
            snapshot.verbose,
            snapshot.history_count,
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use quorum_domain::ConsensusLevel;

    fn setup() -> (
        TuiPresenter,
        mpsc::UnboundedReceiver<RoutedTuiEvent>,
        TuiState,
    ) {
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
        assert_eq!(state.tabs.active_pane().conversation.messages.len(), 1);
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
        assert_eq!(state.tabs.active_pane().conversation.messages.len(), 1);
        assert!(
            state.tabs.active_pane().conversation.messages[0]
                .content
                .contains("ensemble")
        );
    }

    #[test]
    fn test_history_cleared() {
        let (presenter, _rx, mut state) = setup();
        state
            .tabs
            .active_pane_mut()
            .conversation
            .messages
            .push(DisplayMessage::user("test"));
        state.tabs.active_pane_mut().conversation.streaming_text = "streaming".into();

        presenter.apply(&mut state, &UiEvent::HistoryCleared);
        assert!(state.tabs.active_pane().conversation.messages.is_empty());
        assert!(
            state
                .tabs
                .active_pane()
                .conversation
                .streaming_text
                .is_empty()
        );
    }

    #[test]
    fn test_unknown_command_uses_tui_prefix() {
        let (presenter, _rx, mut state) = setup();
        presenter.apply(
            &mut state,
            &UiEvent::UnknownCommand {
                command: "nonexistent".into(),
            },
        );
        let (flash, _) = state.flash_message.expect("flash should be set");
        assert!(flash.contains(":nonexistent"), "flash was: {}", flash);
        assert!(!flash.contains("/nonexistent"), "flash was: {}", flash);
    }

    #[test]
    fn test_help_opens_overlay_without_circular_flash() {
        let (presenter, _rx, mut state) = setup();
        presenter.apply(&mut state, &UiEvent::Help);
        assert!(state.show_help);
        let (flash, _) = state.flash_message.expect("flash should be set");
        assert!(!flash.contains(":help"), "flash was: {}", flash);
    }

    #[test]
    fn test_interaction_spawned_echoes_query_as_user_message() {
        use super::super::state::MessageRole;
        use quorum_application::InteractionSpawnedEvent;
        use quorum_domain::interaction::{InteractionForm, InteractionId};

        let (presenter, _rx, mut state) = setup();
        presenter.apply(
            &mut state,
            &UiEvent::InteractionSpawned(InteractionSpawnedEvent {
                id: InteractionId(1),
                form: InteractionForm::Ask,
                parent_id: Some(InteractionId(0)),
                query: "What is 2+2?".into(),
            }),
        );

        let pane = state
            .tabs
            .pane_for_interaction_mut(InteractionId(1))
            .expect("spawned pane missing");
        assert_eq!(pane.conversation.messages.len(), 1);
        assert_eq!(pane.conversation.messages[0].role, MessageRole::User);
        assert_eq!(pane.conversation.messages[0].content, "What is 2+2?");
    }

    #[test]
    fn test_interaction_spawned_with_empty_query_does_not_echo() {
        use quorum_application::InteractionSpawnedEvent;
        use quorum_domain::interaction::{InteractionForm, InteractionId};

        let (presenter, _rx, mut state) = setup();
        presenter.apply(
            &mut state,
            &UiEvent::InteractionSpawned(InteractionSpawnedEvent {
                id: InteractionId(0),
                form: InteractionForm::Agent,
                parent_id: None,
                query: String::new(),
            }),
        );

        let pane = state
            .tabs
            .pane_for_interaction_mut(InteractionId(0))
            .expect("root pane missing");
        assert!(pane.conversation.messages.is_empty());
    }

    #[test]
    fn test_config_display_groups_by_section() {
        use quorum_application::ConfigEntry;

        let (presenter, _rx, mut state) = setup();
        let snapshot = ConfigSnapshot {
            entries: vec![
                ConfigEntry {
                    key: "agent.consensus_level".into(),
                    value: "solo".into(),
                },
                ConfigEntry {
                    key: "agent.phase_scope".into(),
                    value: "full".into(),
                },
                ConfigEntry {
                    key: "models.exploration".into(),
                    value: "gpt-5.3-codex".into(),
                },
                ConfigEntry {
                    key: "tui.input.submit_key".into(),
                    value: "enter".into(),
                },
            ],
            section_filter: None,
            working_dir: None,
            verbose: false,
            history_count: 0,
        };

        presenter.apply(&mut state, &UiEvent::ConfigDisplay(snapshot));
        let content = &state.tabs.active_pane().conversation.messages[0].content;
        assert!(content.contains("[agent]  consensus_level=solo  phase_scope=full"));
        assert!(content.contains("[models]  exploration=gpt-5.3-codex"));
        assert!(content.contains("[tui.input]  submit_key=enter"));
        assert!(content.contains("[runtime]"));
    }

    #[test]
    fn test_config_display_filtered_omits_runtime() {
        use quorum_application::ConfigEntry;

        let (presenter, _rx, mut state) = setup();
        let snapshot = ConfigSnapshot {
            entries: vec![ConfigEntry {
                key: "models.decision".into(),
                value: "claude-sonnet-4.5".into(),
            }],
            section_filter: Some("models".into()),
            working_dir: None,
            verbose: false,
            history_count: 0,
        };

        presenter.apply(&mut state, &UiEvent::ConfigDisplay(snapshot));
        let content = &state.tabs.active_pane().conversation.messages[0].content;
        assert!(content.contains("[models]  decision=claude-sonnet-4.5"));
        assert!(!content.contains("[runtime]"));
    }

    #[test]
    fn test_exit_sets_quit() {
        let (presenter, _rx, mut state) = setup();
        presenter.apply(&mut state, &UiEvent::Exit);
        assert!(state.should_quit);
    }

    #[test]
    fn test_ask_result_does_not_duplicate_streamed_answer() {
        // Regression test for #267: the answer is finalized from streaming
        // (StreamEnd → finalize_stream), so AskResult must not push it again.
        let (presenter, mut rx, mut state) = setup();

        // Simulate streaming of the answer followed by StreamEnd finalization
        state.tabs.active_pane_mut().conversation.streaming_text = "4".into();
        state.finalize_stream();

        presenter.apply(
            &mut state,
            &UiEvent::AskResult(AskResultEvent { answer: "4".into() }),
        );

        let messages = &state.tabs.active_pane().conversation.messages;
        let assistant_count = messages
            .iter()
            .filter(|m| m.role == super::super::state::MessageRole::Assistant)
            .count();
        assert_eq!(assistant_count, 1, "answer must appear exactly once");

        // Completion event is still emitted for progress/remote consumers
        assert!(matches!(
            rx.try_recv().unwrap().event,
            TuiEvent::AgentResult { success: true, .. }
        ));
    }

    #[test]
    fn test_ask_result_before_stream_end_does_not_duplicate() {
        // ui_rx is prioritized over tui_event_rx (biased select!), so
        // AskResult can be applied before StreamEnd. The answer must still
        // appear exactly once after the stream is finalized.
        let (presenter, _rx, mut state) = setup();

        state.tabs.active_pane_mut().conversation.streaming_text = "4".into();

        presenter.apply(
            &mut state,
            &UiEvent::AskResult(AskResultEvent { answer: "4".into() }),
        );

        // StreamEnd arrives afterwards
        state.finalize_stream();

        let messages = &state.tabs.active_pane().conversation.messages;
        let assistant_count = messages
            .iter()
            .filter(|m| m.role == super::super::state::MessageRole::Assistant)
            .count();
        assert_eq!(assistant_count, 1, "answer must appear exactly once");
    }

    #[test]
    fn test_agent_error_finalizes_stream() {
        let (presenter, _rx, mut state) = setup();
        state.tabs.active_pane_mut().conversation.streaming_text = "partial".into();
        state.tabs.active_pane_mut().progress.is_running = true;

        presenter.apply(
            &mut state,
            &UiEvent::AgentError(AgentErrorEvent {
                cancelled: false,
                error: "test error".into(),
            }),
        );

        assert!(!state.tabs.active_pane().progress.is_running);
        // streaming text finalized + error message
        assert!(state.tabs.active_pane().conversation.messages.len() >= 2);
    }

    #[test]
    fn test_context_init_marks_header_running_then_idle() {
        // Regression test for #261: while /init runs, the header must show the
        // live phase (is_running = true) instead of "Ready"; once the context
        // is saved it returns to idle.
        let (presenter, _rx, mut state) = setup();

        presenter.apply(&mut state, &UiEvent::ContextInitStarting { model_count: 2 });
        assert!(
            state.tabs.active_pane().progress.is_running,
            "header must report a running phase during init"
        );
        assert_eq!(
            state.tabs.active_pane().progress.phase_name,
            "Gathering Context"
        );

        presenter.apply(
            &mut state,
            &UiEvent::ContextInitResult(ContextInitResultEvent {
                path: ".quorum/context.md".into(),
                content: "ctx".into(),
                contributing_models: vec!["m".into()],
            }),
        );
        assert!(
            !state.tabs.active_pane().progress.is_running,
            "header must return to idle after init completes"
        );
    }

    #[test]
    fn test_context_init_error_resets_running() {
        // A failed /init must also clear the running flag so the header does
        // not stay stuck on a phase.
        let (presenter, _rx, mut state) = setup();

        presenter.apply(&mut state, &UiEvent::ContextInitStarting { model_count: 2 });
        presenter.apply(
            &mut state,
            &UiEvent::ContextInitError {
                error: "boom".into(),
            },
        );
        assert!(!state.tabs.active_pane().progress.is_running);
    }
}
