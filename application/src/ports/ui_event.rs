//! UI event types emitted by AgentController for presentation layer rendering
//!
//! These events form the output port from the application layer to the presentation layer.
//! The presentation layer receives these events and renders them appropriately
//! (e.g., ReplPresenter for CLI, TuiPresenter for TUI in Phase 2).

use quorum_domain::{
    AgentState, ConsensusLevel, InteractionForm, InteractionId, InteractionResult, Model,
    OutputFormat, PhaseScope, Thought,
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
    /// Context initialization progress log (file loading, per-model analysis, synthesis)
    ContextInitProgress { message: String },
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

    // === Review Interaction (#300) ===
    /// Review interaction failed (no quorum reached, gateway error, etc.)
    ReviewError { error: String },

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
    /// Unknown command entered.
    /// `command` is the bare command name without any prefix — each presenter
    /// renders its own prefix convention (`/` for REPL, `:` for TUI).
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

/// A single config key-value pair for display
#[derive(Debug, Clone)]
pub struct ConfigEntry {
    /// Full dotted key path (e.g., `"agent.consensus_level"`)
    pub key: String,
    /// Rendered value (via `ConfigValue` Display)
    pub value: String,
}

impl ConfigEntry {
    /// Section prefix: everything before the last `.` (e.g., `"tui.input"`).
    pub fn section(&self) -> &str {
        self.key.rsplit_once('.').map(|(s, _)| s).unwrap_or("")
    }

    /// Leaf name: everything after the last `.` (e.g., `"submit_key"`).
    pub fn name(&self) -> &str {
        self.key
            .rsplit_once('.')
            .map(|(_, n)| n)
            .unwrap_or(&self.key)
    }
}

/// Snapshot of current configuration for display
#[derive(Debug, Clone)]
pub struct ConfigSnapshot {
    /// All config entries in registry order (filtered when `section_filter` is set)
    pub entries: Vec<ConfigEntry>,
    /// Section filter that produced these entries (e.g., `:config models`)
    pub section_filter: Option<String>,
    /// Runtime info not part of the config key registry
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
    /// The structured result, for consumers that need more than the
    /// notification text (e.g. headless callers awaiting a specific
    /// interaction via `TuiApp::run_headless_until`, #300).
    pub result: Option<InteractionResult>,
}
