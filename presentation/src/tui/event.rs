//! TUI event system
//!
//! Handles all events that can occur in the TUI:
//! - Terminal events (key presses, resize)
//! - Application events (from AgentController via UiEvent)
//! - Internal TUI events (for state updates)

use crossterm::event::{KeyEvent, MouseEvent};
use quorum_application::{ConfigSnapshot, UiEvent as AppUiEvent};
use quorum_domain::{AgentPhase, ConsensusLevel, PhaseScope};

/// Events that can occur in the TUI
#[derive(Debug, Clone)]
pub enum Event {
    /// Terminal key event
    Key(KeyEvent),
    /// Terminal mouse event
    Mouse(MouseEvent),
    /// Terminal resize event
    Resize(u16, u16),
    /// Application UI event (from AgentController)
    UiEvent(AppUiEvent),
    /// Tick event for periodic updates
    Tick,
    /// Error occurred
    Error(String),
}

impl From<crossterm::event::Event> for Event {
    fn from(event: crossterm::event::Event) -> Self {
        match event {
            crossterm::event::Event::Key(key) => Event::Key(key),
            crossterm::event::Event::Mouse(mouse) => Event::Mouse(mouse),
            crossterm::event::Event::Resize(width, height) => Event::Resize(width, height),
            _ => Event::Tick,
        }
    }
}

impl From<AppUiEvent> for Event {
    fn from(event: AppUiEvent) -> Self {
        Event::UiEvent(event)
    }
}

/// Internal TUI events for state updates
///
/// These events are emitted by TUI adapters (presenter, progress, HIL)
/// to update the TUI state in response to application events.
#[derive(Debug, Clone)]
pub enum TuiEvent {
    // ==================== Welcome/Config ====================
    Welcome {
        decision_model: String,
        review_models: Vec<String>,
        moderator: Option<String>,
        working_dir: Option<String>,
        consensus_level: ConsensusLevel,
    },
    HelpRequested,
    ConfigDisplay {
        snapshot: ConfigSnapshot,
    },
    
    // ==================== Mode Changes ====================
    ModeChanged {
        level: ConsensusLevel,
        description: String,
    },
    ScopeChanged {
        scope: PhaseScope,
        description: String,
    },
    StrategyChanged {
        strategy: String,
        description: String,
    },
    
    // ==================== Agent Events ====================
    AgentStarting {
        mode: ConsensusLevel,
    },
    AgentResult {
        success: bool,
        summary: String,
        verbose: bool,
    },
    AgentError {
        cancelled: bool,
        error: String,
    },
    
    // ==================== Quorum Events ====================
    QuorumStarting,
    QuorumResult {
        output: String,
    },
    QuorumError {
        error: String,
    },
    
    // ==================== Context Init ====================
    ContextInitStarting {
        model_count: usize,
    },
    ContextInitResult {
        path: String,
        contributing_models: Vec<String>,
    },
    ContextInitError {
        error: String,
    },
    ContextAlreadyExists,
    
    // ==================== Progress Events ====================
    PhaseChange {
        phase: AgentPhase,
        name: String,
    },
    Thought {
        thought_type: String,
        content: String,
    },
    TaskStart {
        description: String,
    },
    TaskComplete {
        description: String,
        success: bool,
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
        category: String,
        message: String,
    },
    ToolRetry {
        tool_name: String,
        attempt: usize,
        max_retries: usize,
        error: String,
    },
    QuorumStart {
        phase: String,
        model_count: usize,
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
    
    // ==================== Human Intervention ====================
    HumanInterventionRequired {
        max_revisions: usize,
    },
    HumanInterventionPrompt {
        request: String,
        objective: String,
        tasks: Vec<String>,
        review_count: usize,
    },
    HumanDecision {
        decision: String,
    },
    
    // ==================== Other ====================
    HistoryCleared,
    VerboseStatus {
        enabled: bool,
    },
    CommandError {
        message: String,
    },
    UnknownCommand {
        command: String,
    },
    Exit,
}
