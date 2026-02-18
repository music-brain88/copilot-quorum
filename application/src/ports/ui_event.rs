//! UI event types emitted by AgentController for presentation layer rendering
//!
//! These events form the output port from the application layer to the presentation layer.
//! The presentation layer receives these events and renders them appropriately
//! (e.g., ReplPresenter for CLI, TuiPresenter for TUI in Phase 2).

use quorum_domain::{
    AgentState, ConsensusLevel, HilMode, InteractionForm, InteractionId, Model, OutputFormat,
    PhaseScope, Thought,
};

/// Events emitted by AgentController for presentation layer to render
#[derive(Debug, Clone)]
pub enum UiEvent {
    // === Welcome & Info ===
    /// Display welcome screen with current configuration
    Welcome(WelcomeInfo),
    /// Display help text for all available commands
    Help,
    /// Display current configuration snapshot
    ConfigDisplay(ConfigSnapshot),

    // === Mode/Config Changes ===
    /// Consensus level changed (Solo ↔ Ensemble)
    ModeChanged {
        level: ConsensusLevel,
        description: String,
    },
    /// Phase scope changed
    ScopeChanged {
        scope: PhaseScope,
        description: String,
    },
    /// Orchestration strategy changed
    StrategyChanged {
        strategy: String,
        description: String,
    },
    /// Conversation history cleared
    HistoryCleared,
    /// Verbose mode status display
    VerboseStatus { enabled: bool },

    // === Agent Execution ===
    /// Agent execution starting
    AgentStarting { mode: ConsensusLevel },
    /// Agent execution completed with results
    AgentResult(Box<AgentResultEvent>),
    /// Agent execution failed
    AgentError(AgentErrorEvent),

    // === Quorum Discussion ===
    /// Quorum discussion starting
    QuorumStarting,
    /// Quorum discussion completed
    QuorumResult(QuorumResultEvent),
    /// Quorum discussion failed
    QuorumError { error: String },

    // === Context Initialization ===
    /// Context initialization starting
    ContextInitStarting { model_count: usize },
    /// Context initialization completed
    ContextInitResult(ContextInitResultEvent),
    /// Context initialization failed
    ContextInitError { error: String },
    /// Context file already exists
    ContextAlreadyExists,

    // === Ask Interaction ===
    /// Ask interaction starting
    AskStarting,
    /// Ask interaction completed
    AskResult(AskResultEvent),
    /// Ask interaction failed
    AskError { error: String },

    // === Interaction Lifecycle ===
    /// Interaction spawned (root or child)
    InteractionSpawned(InteractionSpawnedEvent),
    /// Interaction completed
    InteractionCompleted(InteractionCompletedEvent),
    /// Interaction spawn failed
    InteractionSpawnError { error: String },

    // === Errors & Control ===
    /// Command usage/validation error
    CommandError { message: String },
    /// Unknown command entered
    UnknownCommand { command: String },
    /// Exit message
    Exit,
}

// === Supporting Types ===

/// Information for rendering the welcome screen
#[derive(Debug, Clone)]
pub struct WelcomeInfo {
    pub decision_model: Model,
    pub review_models: Vec<Model>,
    pub moderator: Option<Model>,
    pub working_dir: Option<String>,
    pub consensus_level: ConsensusLevel,
}

/// Snapshot of current configuration for display
#[derive(Debug, Clone)]
pub struct ConfigSnapshot {
    pub exploration_model: Model,
    pub decision_model: Model,
    pub review_models: Vec<Model>,
    pub consensus_level: ConsensusLevel,
    pub phase_scope: PhaseScope,
    pub orchestration_strategy: String,
    pub require_final_review: bool,
    pub max_iterations: usize,
    pub max_plan_revisions: usize,
    pub hil_mode: HilMode,
    pub working_dir: Option<String>,
    pub verbose: bool,
    pub history_count: usize,
}

/// Agent execution result for display
#[derive(Debug, Clone)]
pub struct AgentResultEvent {
    pub success: bool,
    pub summary: String,
    pub state: AgentState,
    pub verbose: bool,
    pub thoughts: Vec<Thought>,
}

/// Agent execution error for display
#[derive(Debug, Clone)]
pub struct AgentErrorEvent {
    pub error: String,
    pub cancelled: bool,
}

/// Quorum discussion result for display
#[derive(Debug, Clone)]
pub struct QuorumResultEvent {
    pub formatted_output: String,
    pub output_format: OutputFormat,
}

/// Context initialization result for display
#[derive(Debug, Clone)]
pub struct ContextInitResultEvent {
    pub path: String,
    pub content: String,
    pub contributing_models: Vec<String>,
}

/// Ask interaction result for display
#[derive(Debug, Clone)]
pub struct AskResultEvent {
    pub answer: String,
}

/// Interaction spawn event for display
#[derive(Debug, Clone)]
pub struct InteractionSpawnedEvent {
    pub id: InteractionId,
    pub form: InteractionForm,
    pub parent_id: Option<InteractionId>,
    pub query: String,
}

/// Interaction completion event for display
#[derive(Debug, Clone)]
pub struct InteractionCompletedEvent {
    pub id: InteractionId,
    pub form: InteractionForm,
    pub parent_id: Option<InteractionId>,
    pub result_text: String,
}
