//! TUI event types
//!
//! Defines the commands sent TO the controller task and the events
//! coming FROM it (via UiEvent channel and progress bridge).

use quorum_domain::{
    AgentPhase, ConsensusLevel, ContextMode, HumanDecision, InteractionForm, InteractionId, Plan,
    ReviewRound,
};
use tokio::sync::oneshot;

/// Commands sent from the TUI event loop to the controller task (Actor inbox)
pub enum TuiCommand {
    /// User submitted text from Insert mode
    ProcessRequest(String),
    /// User issued a slash-command from Command mode (e.g. "q", "help", "solo")
    HandleCommand(String),
    /// Set verbose mode
    SetVerbose(bool),
    /// Set cancellation token
    SetCancellation(tokio_util::sync::CancellationToken),
    /// Set reference resolver for automatic reference resolution
    SetReferenceResolver(std::sync::Arc<dyn quorum_application::ReferenceResolverPort>),
    /// Spawn a new interaction
    SpawnInteraction {
        form: InteractionForm,
        query: String,
        context_mode_override: Option<ContextMode>,
    },
    /// Activate an existing interaction
    ActivateInteraction(InteractionId),
    /// Graceful shutdown
    #[allow(dead_code)]
    Quit,
}

/// Events emitted by TuiPresenter / TuiProgressBridge for rendering
#[derive(Debug, Clone)]
pub enum TuiEvent {
    // -- Welcome / Config --
    Welcome {
        decision_model: String,
        consensus_level: ConsensusLevel,
    },
    ConfigDisplay(String),

    // -- Mode / Scope changes --
    ModeChanged {
        level: ConsensusLevel,
        description: String,
    },
    ScopeChanged(String),
    StrategyChanged(String),

    // -- Agent lifecycle --
    AgentStarting,
    AgentResult {
        success: bool,
        summary: String,
    },
    AgentError(String),

    // -- Interaction lifecycle --
    // Note: InteractionSpawned is handled directly in presenter.apply() to avoid
    // a select! loop race condition with subsequent UiEvents.
    InteractionCompleted {
        parent_id: Option<InteractionId>,
        result_text: String,
    },

    // -- Streaming text --
    StreamChunk(String),
    StreamEnd,

    // -- Progress --
    PhaseChange {
        phase: AgentPhase,
        name: String,
    },
    TaskStart {
        description: String,
        index: usize,
        total: usize,
    },
    TaskComplete {
        description: String,
        success: bool,
        index: usize,
        total: usize,
        output: Option<String>,
    },
    ToolCall {
        tool_name: String,
        args: String,
    },
    ToolResult {
        tool_name: String,
        success: bool,
    },
    ToolError {
        tool_name: String,
        message: String,
    },

    // -- Quorum --
    QuorumStart {
        phase: String,
        model_count: usize,
    },
    QuorumModelVote {
        model: String,
        approved: bool,
    },
    QuorumComplete {
        phase: String,
        approved: bool,
        feedback: Option<String>,
    },
    PlanRevision {
        revision: usize,
        feedback: String,
    },

    // -- Ensemble --
    EnsembleStart(usize),
    EnsemblePlanGenerated(String),
    EnsembleVotingStart(usize),
    EnsembleModelFailed {
        model: String,
        error: String,
    },
    EnsembleComplete {
        selected_model: String,
        score: f64,
    },
    EnsembleFallback(String),

    // -- Tool execution lifecycle --
    ToolExecutionUpdate {
        task_index: usize,
        execution_id: String,
        tool_name: String,
        state: ToolExecutionDisplayState,
        duration_ms: Option<u64>,
    },

    // -- Other --
    HistoryCleared,
    CommandError(String),
    Flash(String),
    Exit,
}

/// Display-friendly state for a tool execution
#[derive(Debug, Clone)]
pub enum ToolExecutionDisplayState {
    Pending,
    Running,
    Completed { preview: String },
    Error { message: String },
}

/// Request for human intervention, sent from HumanIntervention port to TUI
pub struct HilRequest {
    pub kind: HilKind,
    pub response_tx: oneshot::Sender<HumanDecision>,
}

pub enum HilKind {
    PlanIntervention {
        request: String,
        plan: Plan,
        review_history: Vec<ReviewRound>,
    },
    ExecutionConfirmation {
        request: String,
        plan: Plan,
    },
}
